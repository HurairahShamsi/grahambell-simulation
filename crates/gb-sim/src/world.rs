//! The world: households, miner slots, witnesses, allocation, shared outages and the
//! admission and One Chance processes (SPEC §3.1–§3.2, §4.2, §4.5–§4.8).

use crate::engine::{Micros, SECOND, micros, seconds};
use crate::household::{self, CauseContext, Device, Household, bit};
use crate::job::Strategy;
use crate::network::{
    GossipTable, Latency, LossModel, Network, OverlaySettings, Partition, RIPE_MATRIX,
    calibrate_gossip, spread_regions, spread_salted,
};
use crate::rng::{Keyed, Purpose};
use crate::samplers::{exponential, lognormal, normal};
use crate::sim::{Ev, Params, Sim, lane};
use gb_config::protocol::OfflinePointsOutcome;
use gb_config::sim::{GridProfile, LatencySource};
use gb_protocol::allocation::{IdStatus, kwc_wcs};
use gb_protocol::chain::step_win_probability;
use gb_protocol::hash::{Hash, tagged};
use gb_protocol::offline::{OfflinePoints, Penalty, one_chance_outcome, penalty_for};
use gb_protocol::validate::{MinerKind, QuorumRule, witness_signing_tolerance};

/// No index.
pub(crate) const NONE: u32 = u32::MAX;

/// The cause index of a partition cut, after the household cause bits (offline points).
pub(crate) const CAUSE_CUT: u8 = bit::COUNT as u8;

/// The index of a household cause bit (offline points by cause).
pub(crate) fn cause_index(cause: u16) -> u8 {
    cause.trailing_zeros().min(bit::COUNT as u32) as u8
}

/// A miner slot's admission state (SPEC §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Not admitted.
    Idle,
    /// Admission in progress.
    Admitting,
    /// Globally Active, in the post-admission wait.
    Waiting,
    /// Mining.
    Mining,
    /// Left for good.
    Gone,
}

/// One unregistered miner slot (a /64).
#[derive(Debug, Clone)]
pub(crate) struct Miner {
    pub household: u32,
    pub group: u8,
    pub kwc: u32,
    pub phase: Phase,
    pub token: u32,
    pub mine_at: Micros,
    pub ghost: bool,
    pub recorded: bool,
    pub idle: bool,
    pub key_epoch: u32,
    pub vote_failures: u8,
    pub decision_at: Micros,
    pub decision_kind: u8,
    /// End of the epoch after its own offline request (restart or switch): it re-admits then,
    /// not earlier, even if its household reconnects meanwhile (SPEC §3.2, SIM-18).
    pub rejoin_at: Micros,
    /// When its household last shut down with an offline request (`u64::MAX` if never).
    pub announced_off_at: Micros,
    /// Valid IDs won by this slot (voided IDs excluded).
    pub ids: u32,
    /// When it won its first ID (`u64::MAX` if it has none).
    pub first_id_at: Micros,
    pub joined_at: Micros,
    pub newcomer: bool,
    pub first_id: bool,
    pub patience_until: Micros,
    pub next_success: u64,
    pub conn: u32,
}

/// One registered ID acting as a witness node.
#[derive(Debug, Clone)]
pub(crate) struct Witness {
    pub household: u32,
    pub reg: u32,
    pub id: Hash,
    pub seat: u32,
    pub recorded: bool,
    pub ghost: bool,
    pub banned: bool,
    pub deactivated: bool,
    pub token: u32,
    pub down_at: Micros,
    pub request_at: Micros,
    pub check_at: Micros,
    pub offences: Vec<u64>,
    pub registered_at: Micros,
    pub short_outage: bool,
    /// Its ID was voided: its PoW-ID block was replaced below confirmation depth. Not a ban.
    pub void: bool,
    /// Online but cut off by a partition, so unreachable for its KWCs (architect,
    /// 2026-10-14), until `cut_until`.
    pub cut_off: bool,
    pub cut_until: Micros,
    /// Offline points (the M3a.1 E4 ladder).
    pub points: OfflinePoints,
    /// What started its current offline period (`cause_index`).
    pub down_cause: u8,
    /// Point awards due but not yet recorded, by cause, while no KWC can decide.
    pub points_owed: Vec<u8>,
    /// Whether every award since its points last reset came from scheduled load-shedding.
    pub points_loadshed_only: bool,
    /// After a seat loss at the points cap, no seat before this time.
    pub seat_cooldown_until: Micros,
}

impl Witness {
    /// No longer a witness: banned, or its ID voided.
    pub(crate) fn removed(&self) -> bool {
        self.banned || self.void
    }
}

/// Counters of one WC's seated members.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WcCount {
    pub seated: u32,
    pub online: u32,
    pub recorded: u32,
    pub ghost: u32,
}

/// Counters over miner slots.
#[derive(Debug, Clone, Default)]
pub(crate) struct Counts {
    pub after_wait: u64,
    pub all: u64,
    pub mining_group: Vec<u64>,
    pub mining_region: Vec<u64>,
    /// Mining slots by group and device (C1 breakdown).
    pub mining_group_device: Vec<Vec<u64>>,
    /// Mining slots by group and region (C1 breakdown).
    pub mining_group_region: Vec<Vec<u64>>,
    pub last: Micros,
    pub hop_mining: u64,
}

/// Seconds a scripted blackout's diagnosis keeps sampling after it ends, so that gaps that
/// run on past its end are covered (E3).
pub(crate) const DIAGNOSIS_TAIL_S: f64 = 6.0 * 3_600.0;

/// The E3 diagnosis of one outage record: a sample a round from the start of a scripted
/// blackout to [`DIAGNOSIS_TAIL_S`] after its end, kept outside the result.
#[derive(Debug, Clone, Default)]
pub(crate) struct OutageDiag {
    /// (time, miners mining with their household online, those of them whose KWC can issue
    /// entropy, the network tip's per-step success probability).
    pub samples: Vec<(Micros, u32, u32, f64)>,
    /// Traced runs (W2), with each sample: the round on the network tip's slots not yet drawn
    /// and those of them whose miner can mine, and its skipped slots and those of them whose
    /// miner can mine ([`Sim::tip_round_skips`]).
    pub skip_samples: Vec<(u32, u32, u32, u32)>,
    /// Failover moves before it started.
    pub moves_before: u64,
}

/// An ideal validator: always online, at a fixed place.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PanelNode {
    pub region: usize,
    pub skew: f64,
    pub access: f64,
}

/// Kinds of shared outage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SharedKind {
    Fault,
    Loadshed,
    Isp,
    Blackout,
}

/// One shared outage process.
#[derive(Debug, Clone)]
pub(crate) struct SharedProcess {
    pub kind: SharedKind,
    pub cause: u16,
    pub members: Vec<u32>,
    pub affected: Vec<u32>,
    pub token: u32,
    pub draws: u32,
    pub rate_per_s: f64,
    pub median_s: f64,
    pub sigma: f64,
    pub blocks: Vec<(f64, f64)>,
    pub ups_runtime: f64,
    pub ends: Micros,
}

/// Shared outage processes.
#[derive(Debug, Clone, Default)]
pub(crate) struct Shared {
    pub processes: Vec<SharedProcess>,
    pub feeder_of: Vec<Vec<u32>>,
    pub isp_of: Vec<Vec<u32>>,
    pub blackout_of: Vec<u32>,
}

/// Status of a miner, for counter updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MinerStatus {
    after_wait: bool,
    all: bool,
    mining: bool,
    hop: bool,
}

/// Status of a witness, for counter updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WitnessStatus {
    wc: Option<usize>,
    online: bool,
    recorded: bool,
    ghost: bool,
}

impl<'a> Sim<'a> {
    // ------------------------------------------------------------------ build

    pub(crate) fn build(
        spec: &'a crate::job::JobSpec,
        cfg: &'a gb_config::Config,
    ) -> Result<Sim<'a>, crate::SimError> {
        let interval_s = cfg.issuance.pow_id_target_interval_s.value.round().max(1.0) as u64;
        let grace = micros((u64::from(cfg.timing.grace_epoch_rounds.value) * interval_s) as f64);
        let ladder = cfg.penalties.suspension_ladder_s.value.clone();
        let p = Params {
            interval_s,
            grace,
            convergence: micros(
                cfg.admission.convergence_interval_epochs.value * (grace as f64 / SECOND as f64),
            ),
            post_wait: micros(
                (u64::from(
                    spec.post_wait_rounds
                        .unwrap_or(cfg.admission.post_admission_wait_rounds.value),
                ) * interval_s) as f64,
            ),
            one_chance: micros((u64::from(spec.one_chance_wait_rounds) * interval_s) as f64),
            deactivation: micros(cfg.sim.world.deactivation_threshold_s),
            long_absence: micros(cfg.offline.long_absence_threshold_s.value),
            wc_size: cfg.witness.wc_size.value as usize,
            offsets: cfg
                .witness
                .ring_offsets
                .value
                .iter()
                .map(|o| u64::from(*o))
                .collect(),
            unregistered: QuorumRule::from_config(cfg, MinerKind::Unregistered),
            registered: QuorumRule::from_config(cfg, MinerKind::Registered),
            quorum_unregistered: cfg.quorum.unregistered_total_min.value as usize,
            capacity: cfg.capacity.unregistered_per_leader_wc.value,
            confirmation_depth: u64::from(cfg.issuance.confirmation_depth_blocks.value),
            beacon_delay: u64::from(cfg.witness.allocation_beacon_delay_blocks.value),
            pow_tx: micros(cfg.transactions.pow_tx_interval_s.value),
            cac_size: cfg.cac.size.value as usize,
            cac_every: u64::from(cfg.cac.join_every_n_tx_blocks.value),
            ladder,
            seconds_per_year: cfg.model.seconds_per_year as f64,
            warmup: if spec.idealised || !spec.outages {
                0
            } else {
                micros(spec.warmup_days * 86_400.0)
            },
            settle_depth: 12,
            signing_tolerance: witness_signing_tolerance(spec.witness_signing, spec.delta),
            fork_storm_unsettled: cfg.sim.world.fork_storm_unsettled as usize,
            cache_entries: cfg.sim.acceptance_cache_entries as usize,
        };
        let mut keyed = Keyed::new(spec.seed, "gb-sim");
        let net_cfg = &cfg.sim.network;
        let regions = net_cfg.regions.len();
        let latency = match spec.latency {
            LatencySource::RipeAtlas => Latency::from_ripe(RIPE_MATRIX, &net_cfg.regions)
                .map_err(|e| crate::SimError::Setup(e.to_string()))?,
            LatencySource::Stylised => Latency::stylised(net_cfg),
        };
        let loss = LossModel {
            loss: if spec.idealised { 0.0 } else { spec.loss },
            rto_initial: net_cfg.rto_initial_s,
            max_retries: net_cfg.rto_max_retries,
        };
        let access_median = net_cfg.access_delay_median_ms / 1_000.0;
        let gossip = if spec.idealised {
            GossipTable::zero(regions)
        } else {
            let settings = OverlaySettings {
                nodes: net_cfg.overlay_nodes as usize,
                degree: cfg.admission.peer_bootstrap_count.value as usize,
                sources_per_region: net_cfg.overlay_sources_per_region as usize,
                hop_processing: net_cfg.hop_processing_ms / 1_000.0,
                access_median,
                access_sigma: net_cfg.access_delay_sigma,
            };
            calibrate_gossip(
                &mut keyed,
                &latency,
                &loss,
                &net_cfg.region_shares,
                &settings,
            )
        };
        let end = micros(spec.days * 86_400.0);
        let mut partitions = Vec::new();
        for (region, day, length) in &spec.partitions {
            let start = micros(day * 86_400.0);
            partitions.push(Partition {
                region: *region,
                start,
                end: start + micros(*length),
            });
        }
        if spec.partition_rate_per_region_per_day > 0.0 && !spec.idealised {
            for region in 0..regions {
                let mut t = 0u64;
                let mut i = 0;
                loop {
                    let u = keyed.uniforms(Purpose::Setup, 900 + region as u64, i);
                    i += 1;
                    t += micros(exponential(
                        1.0 - u[0],
                        spec.partition_rate_per_region_per_day / 86_400.0,
                    ));
                    if t >= end {
                        break;
                    }
                    partitions.push(Partition {
                        region,
                        start: t,
                        end: t + micros(spec.partition_length_s),
                    });
                }
            }
        }
        let net = Network {
            latency,
            loss,
            gossip,
            partitions,
            processing: net_cfg.hop_processing_ms / 1_000.0,
            idealised: spec.idealised,
        };
        let groups = spec.groups.len() + 1;
        let mut sim = Sim {
            spec,
            cfg,
            p,
            keyed,
            q: crate::engine::Queue::default(),
            net,
            hh: Vec::new(),
            miners: Vec::new(),
            witnesses: Vec::new(),
            seat_owner: Vec::new(),
            alloc: gb_protocol::allocation::Allocation::new(
                cfg.witness.wc_size.value as usize,
                &cfg.witness
                    .ring_offsets
                    .value
                    .iter()
                    .map(|o| u64::from(*o))
                    .collect::<Vec<_>>(),
            ),
            registry: gb_protocol::allocation::Registry::default(),
            committee: gb_protocol::committee::Committee::new(cfg.cac.size.value as usize),
            active: crate::active::ActiveSet::default(),
            wc: Vec::new(),
            kwc_load: Vec::new(),
            counts: Counts {
                mining_group: vec![0; groups],
                mining_region: vec![0; regions],
                mining_group_device: vec![vec![0; 3]; groups],
                mining_group_region: vec![vec![0; regions]; groups],
                ..Counts::default()
            },
            blocks: Vec::new(),
            unsettled: Vec::new(),
            settled_ids: Vec::new(),
            groups: Vec::new(),
            candidates: Vec::new(),
            panel: Vec::new(),
            network_tip: 0,
            confirmed_height: 0,
            confirmed: Vec::new(),
            placements: Vec::new(),
            shared: Shared::default(),
            res: crate::job::JobResult {
                id: spec.id.clone(),
                ids_by_group: vec![0; groups],
                slot_s_by_group: vec![0.0; groups],
                ids_by_region: vec![0; regions],
                slot_s_by_region: vec![0.0; regions],
                penalties: vec![vec![0; 3]; regions],
                bans: vec![vec![0; 3]; regions],
                late_majority_by_region: vec![0; regions],
                blocks_by_region: vec![0; regions],
                witness_id_s: vec![vec![0.0; 3]; regions],
                offline_records: vec![vec![0; 3]; regions],
                point_awards: vec![vec![0; 3]; regions],
                point_awards_by_cause: vec![0; bit::COUNT + 1],
                points_capped: vec![vec![0; 3]; regions],
                points_capped_loadshed_only: vec![vec![0; 3]; regions],
                seats_lost: vec![vec![0; 3]; regions],
                ids_blacklisted: vec![vec![0; 3]; regions],
                ids_by_group_device: vec![vec![0; 3]; groups],
                slot_s_by_group_device: vec![vec![0.0; 3]; groups],
                ids_by_group_region: vec![vec![0; regions]; groups],
                slot_s_by_group_region: vec![vec![0.0; regions]; groups],
                ..crate::job::JobResult::default()
            },
            end,
            max_skew: 0.0,
            max_access: 0.0,
            d_max: 0,
            committee_ticks: 0,
            v1_attacker: 0,
            stopped: false,
            stop_at: u64::MAX,
            panel_update_at: u64::MAX,
            hop_state: vec![0; groups],
            withheld: Vec::new(),
            pending: std::collections::BTreeMap::new(),
            panel_split: Vec::new(),
            invariants_due: 0,
            availability_due: 86_400 * SECOND,
            own_entries: 0,
            deferred: Vec::new(),
            deferred_free: Vec::new(),
            free_candidates: Vec::new(),
            failover_below: Vec::new(),
            outage_of_process: Vec::new(),
            outage_of_script: Vec::new(),
            outage_diag: Vec::new(),
            trace_state: Vec::new(),
        };
        if spec.start_rule == gb_config::protocol::StartRule::B
            || (spec.start_rule == gb_config::protocol::StartRule::R
                && spec.crypto != gb_config::sim::CryptoMode::Fast)
        {
            return Err(crate::SimError::Setup(
                "start rule (b) is not simulated, and rule (r) runs in fast mode only".to_string(),
            ));
        }
        sim.build_population();
        Ok(sim)
    }

    fn new_household(&mut self, region: usize, key: u64, always_online: bool) -> u32 {
        let h = self.hh.len() as u32;
        let cfg = &self.cfg.sim;
        let w = self.keyed.uniforms(Purpose::NodeAttributes, key, 0);
        let mix = self
            .spec
            .device_mix
            .clone()
            .unwrap_or_else(|| cfg.households.device_mix.clone());
        let device = Device::from_index(pick_weighted(&mix, w[0]));
        let unreliable = cfg
            .network
            .unreliable_grid_regions
            .iter()
            .any(|r| cfg.network.regions.get(region) == Some(r));
        let grid: &GridProfile = if unreliable {
            &cfg.households.unreliable_grid
        } else {
            &cfg.households.reliable_grid
        };
        // Feeder group and provider are drawn uniformly from the region's (SIM-12–SIM-14).
        // Until 2026-10-14 they were drawn from the bit pattern of a uniform float, whose top
        // bits barely vary, so every household of a region shared one feeder group and one
        // provider, and each feeder fault, load-shedding block and provider outage took the
        // whole region offline (ASSUMPTIONS SIM-35).
        let feeders = grid.feeder_groups as usize;
        let grid_group = (region * feeders + household::uniform_index(w[1], feeders)) as u32;
        let isps = cfg.households.isps_per_region.max(1) as usize;
        let isp = (region * isps + household::uniform_index(w[2], isps)) as u32;
        let ups = w[3] < grid.ups_share;
        let (skew, access) = if self.spec.idealised {
            (0.0, 0.0)
        } else {
            let w2 = self.keyed.uniforms(Purpose::NodeAttributes, key, 1);
            let z = normal(1.0 - w2[0], w2[1]);
            let sd = if w2[2] < self.spec.skew_tail_fraction {
                self.spec.skew_tail_sd
            } else {
                self.spec.skew_sd
            };
            let access = lognormal(
                cfg.network.access_delay_median_ms / 1_000.0,
                cfg.network.access_delay_sigma,
                normal(1.0 - w2[3], w2[4]),
            );
            (sd * z, access)
        };
        self.max_skew = self.max_skew.max(skew.abs());
        self.max_access = self.max_access.max(access);
        let independent = self.spec.independent_witness_online;
        let always_online = always_online || !self.spec.outages || self.spec.idealised;
        let mut hh = Household {
            region: region as u8,
            device,
            grid_group,
            isp,
            ups,
            skew,
            access,
            mask: 0,
            announced: 0,
            ends: [u64::MAX; bit::COUNT],
            next: [u64::MAX; 6],
            draws: [0; 6],
            wake_gen: 0,
            miner: None,
            witnesses: Vec::new(),
            always_online,
            independent: None,
        };
        let ctx = self.cause_context(region);
        if let Some(f) = independent.filter(|_| !always_online) {
            // C7 check: every household alternates between exponential up and down periods
            // (mean cycle 4 hours), online with probability f, starting in the stationary state.
            let cycle = 4.0 * 3_600.0;
            hh.always_online = false;
            hh.independent = Some((f * cycle, (1.0 - f) * cycle));
            household::initial_next(&mut self.keyed, u64::from(h), &mut hh, &ctx);
            let u = self.keyed.uniforms(Purpose::NodeAttributes, key, 2);
            if u[0] < 1.0 - f {
                let ends = micros(exponential(1.0 - u[1], 1.0 / ((1.0 - f) * cycle)).max(1.0));
                let c = household::OWN_CAUSES
                    .iter()
                    .position(|x| *x == bit::GLITCH)
                    .unwrap_or(3);
                hh.mask |= bit::GLITCH;
                hh.ends[bit::GLITCH.trailing_zeros() as usize] = ends;
                hh.next[c] = ends;
            }
        } else if !hh.always_online {
            household::initial_next(&mut self.keyed, u64::from(h), &mut hh, &ctx);
        }
        self.hh.push(hh);
        h
    }

    pub(crate) fn cause_context(&self, region: usize) -> CauseContext<'a> {
        CauseContext {
            settings: &self.cfg.sim.households,
            utc_offset_h: self
                .cfg
                .sim
                .network
                .utc_offsets_h
                .get(region)
                .copied()
                .unwrap_or(0.0),
            seconds_per_year: self.p.seconds_per_year,
        }
    }

    fn build_population(&mut self) {
        let spec = self.spec;
        let shares = self.cfg.sim.network.region_shares.clone();
        // Miner slots and their groups.
        let mut miner_regions = spread_regions(spec.miners as usize, &shares, &mut self.keyed);
        let group_shares: Vec<f64> = {
            let tagged: f64 = spec.groups.iter().map(|g| g.share).sum();
            std::iter::once((1.0 - tagged).max(0.0))
                .chain(spec.groups.iter().map(|g| g.share))
                .collect()
        };
        let miner_groups = if let Some(home) = spec.groups_in_region {
            // Every tagged slot in one region; the untagged slots spread over the regions by
            // their shares (W1).
            assert!(
                usize::from(home) < shares.len(),
                "groups_in_region {home} is not a region"
            );
            let groups = spread_salted(spec.miners as usize, &group_shares, &mut self.keyed, 100);
            let untagged = groups.iter().filter(|g| **g == 0).count();
            let spread = spread_salted(untagged, &shares, &mut self.keyed, 101);
            let slots = miner_regions.iter_mut().zip(&groups);
            for ((region, _), spread_region) in slots.filter(|(_, g)| **g == 0).zip(spread) {
                *region = spread_region;
            }
            for (region, _) in miner_regions
                .iter_mut()
                .zip(&groups)
                .filter(|(_, g)| **g > 0)
            {
                *region = usize::from(home);
            }
            groups
        } else if spec.groups_by_region {
            // Each region's miners split into the groups by their shares (SIM-51).
            let mut groups = vec![0usize; spec.miners as usize];
            for region in 0..shares.len() {
                let slots: Vec<usize> = (0..miner_regions.len())
                    .filter(|i| miner_regions[*i] == region)
                    .collect();
                let labels = spread_salted(
                    slots.len(),
                    &group_shares,
                    &mut self.keyed,
                    region as u64 + 1,
                );
                for (k, i) in slots.iter().enumerate() {
                    groups[*i] = labels[k];
                }
            }
            groups
        } else {
            spread_regions(spec.miners as usize, &group_shares, &mut self.keyed)
        };
        for (i, region) in miner_regions.iter().enumerate() {
            let group = miner_groups[i];
            let always = group > 0 && spec.groups[group - 1].always_online;
            self.add_miner(*region, group as u8, always, false, 0);
        }
        // Genesis witnesses.
        let witness_regions =
            spread_regions(spec.registered_ids as usize, &shares, &mut self.keyed);
        let launch = tagged(b"GB/sim-launch", &[&spec.seed.to_be_bytes()]);
        for (i, region) in witness_regions.iter().enumerate() {
            let key = (1u64 << 40) | i as u64;
            let h = self.new_household(*region, key, false);
            let id = tagged(
                b"GB/sim-genesis-id",
                &[&spec.seed.to_be_bytes(), &(i as u64).to_be_bytes()],
            );
            let w = self.add_witness(h, id, 0);
            self.insert_seat(w, &launch);
        }
        // The committee starts full (lottery draws from setup beacons).
        let list: Vec<usize> = self.active.iter().collect();
        for i in 0..self.p.cac_size as u64 {
            let beacon = tagged(
                b"GB/sim-cac-setup",
                &[&spec.seed.to_be_bytes(), &i.to_be_bytes()],
            );
            self.committee.refresh(&list, &beacon);
        }
        // Panel validators.
        let panel_regions = spread_regions(
            self.cfg.sim.world.panel_nodes as usize,
            &shares,
            &mut self.keyed,
        );
        for (i, region) in panel_regions.iter().enumerate() {
            let key = (2u64 << 40) | i as u64;
            let w = self.keyed.uniforms(Purpose::NodeAttributes, key, 1);
            let (skew, access) = if spec.idealised {
                (0.0, 0.0)
            } else {
                let z = normal(1.0 - w[0], w[1]);
                let sd = if w[2] < spec.skew_tail_fraction {
                    spec.skew_tail_sd
                } else {
                    spec.skew_sd
                };
                (
                    sd * z,
                    lognormal(
                        self.cfg.sim.network.access_delay_median_ms / 1_000.0,
                        self.cfg.sim.network.access_delay_sigma,
                        normal(1.0 - w[3], w[4]),
                    ),
                )
            };
            self.max_skew = self.max_skew.max(skew.abs());
            self.max_access = self.max_access.max(access);
            self.panel.push(PanelNode {
                region: *region,
                skew,
                access,
            });
        }
        self.panel_split = vec![false; self.panel.len()];
        self.build_shared();
    }

    /// Adds a miner slot in a new household and returns its index.
    pub(crate) fn add_miner(
        &mut self,
        region: usize,
        group: u8,
        always_online: bool,
        newcomer: bool,
        now: Micros,
    ) -> u32 {
        let m = self.miners.len() as u32;
        let key = u64::from(m);
        let h = self.new_household(region, key, always_online);
        self.hh[h as usize].miner = Some(m);
        let patience_until = if newcomer {
            let w = self.keyed.uniforms(Purpose::Growth, u64::from(m), 1);
            let days = lognormal(
                self.spec.patience_median_days,
                self.cfg.sim.growth.patience_sigma,
                normal(1.0 - w[0], w[1]),
            );
            now + micros(days * 86_400.0)
        } else {
            u64::MAX
        };
        self.miners.push(Miner {
            household: h,
            group,
            kwc: NONE,
            phase: Phase::Idle,
            token: 0,
            mine_at: 0,
            ghost: false,
            recorded: false,
            idle: false,
            key_epoch: 0,
            vote_failures: 0,
            decision_at: 0,
            decision_kind: 0,
            rejoin_at: 0,
            announced_off_at: u64::MAX,
            ids: 0,
            first_id_at: u64::MAX,
            joined_at: now,
            newcomer,
            first_id: false,
            patience_until,
            next_success: u64::MAX,
            conn: 0,
        });
        m
    }

    fn add_witness(&mut self, h: u32, id: Hash, now: Micros) -> u32 {
        let w = self.witnesses.len() as u32;
        let reg = self.registry.register(id) as u32;
        self.active.insert(reg as usize);
        let online = self.hh[h as usize].online();
        self.witnesses.push(Witness {
            household: h,
            reg,
            id,
            seat: NONE,
            recorded: online,
            ghost: false,
            banned: false,
            deactivated: false,
            token: 0,
            down_at: 0,
            request_at: 0,
            check_at: u64::MAX,
            offences: Vec::new(),
            registered_at: now,
            short_outage: false,
            void: false,
            cut_off: false,
            cut_until: 0,
            points: OfflinePoints::default(),
            down_cause: 0,
            points_owed: Vec::new(),
            points_loadshed_only: true,
            seat_cooldown_until: 0,
        });
        self.hh[h as usize].witnesses.push(w);
        w
    }

    fn build_shared(&mut self) {
        let cfg = &self.cfg.sim;
        let regions = cfg.network.regions.len();
        let hcfg = &cfg.households;
        let mut processes = Vec::new();
        let mut feeder_members: Vec<Vec<u32>> = Vec::new();
        let mut isp_members: Vec<Vec<u32>> = Vec::new();
        let mut region_members: Vec<Vec<u32>> = vec![Vec::new(); regions];
        // Group households.
        let max_feeders = hcfg
            .reliable_grid
            .feeder_groups
            .max(hcfg.unreliable_grid.feeder_groups) as usize;
        let isps = hcfg.isps_per_region.max(1) as usize;
        feeder_members.resize(regions * max_feeders, Vec::new());
        isp_members.resize(regions * isps, Vec::new());
        for (h, hh) in self.hh.iter().enumerate() {
            if hh.always_online || hh.independent.is_some() {
                continue;
            }
            feeder_members[hh.grid_group as usize].push(h as u32);
            isp_members[hh.isp as usize].push(h as u32);
            region_members[hh.region as usize].push(h as u32);
        }
        let mut feeder_of = vec![Vec::new(); feeder_members.len()];
        let mut isp_of = vec![Vec::new(); isp_members.len()];
        let mut blackout_of = vec![NONE; regions];
        for region in 0..regions {
            let unreliable = cfg
                .network
                .unreliable_grid_regions
                .iter()
                .any(|r| cfg.network.regions[region] == *r);
            let grid = if unreliable {
                &hcfg.unreliable_grid
            } else {
                &hcfg.reliable_grid
            };
            for g in 0..grid.feeder_groups as usize {
                let index = region * max_feeders + g;
                let members = feeder_members[index].clone();
                let w = self.keyed.uniforms(Purpose::SharedOutage, index as u64, 0);
                let hours = grid.loadshed_hours[pick_weighted(&grid.loadshed_weights, w[0])];
                let phase_h = 24.0 * g as f64 / grid.feeder_groups as f64;
                feeder_of[index].push(processes.len() as u32);
                processes.push(SharedProcess {
                    kind: SharedKind::Fault,
                    cause: bit::GRID,
                    members: members.clone(),
                    affected: Vec::new(),
                    token: 0,
                    draws: 0,
                    rate_per_s: grid.faults_per_year / self.p.seconds_per_year,
                    median_s: grid.fault_median_min * 60.0,
                    sigma: grid.fault_sigma,
                    blocks: Vec::new(),
                    ups_runtime: grid.ups_runtime_h * 3_600.0,
                    ends: 0,
                });
                let blocks = household::loadshed_blocks(hours, grid.loadshed_block_h, phase_h);
                if !blocks.is_empty() {
                    feeder_of[index].push(processes.len() as u32);
                    processes.push(SharedProcess {
                        kind: SharedKind::Loadshed,
                        cause: bit::LOADSHED,
                        members,
                        affected: Vec::new(),
                        token: 0,
                        draws: 0,
                        rate_per_s: 0.0,
                        median_s: 0.0,
                        sigma: 0.0,
                        blocks,
                        ups_runtime: grid.ups_runtime_h * 3_600.0,
                        ends: 0,
                    });
                }
            }
            for i in 0..isps {
                let index = region * isps + i;
                isp_of[index].push(processes.len() as u32);
                processes.push(SharedProcess {
                    kind: SharedKind::Isp,
                    cause: bit::ISP,
                    members: isp_members[index].clone(),
                    affected: Vec::new(),
                    token: 0,
                    draws: 0,
                    rate_per_s: hcfg.isp_outages_per_year / self.p.seconds_per_year,
                    median_s: hcfg.isp_outage_median_min * 60.0,
                    sigma: hcfg.isp_outage_sigma,
                    blocks: Vec::new(),
                    ups_runtime: 0.0,
                    ends: 0,
                });
            }
            blackout_of[region] = processes.len() as u32;
            processes.push(SharedProcess {
                kind: SharedKind::Blackout,
                cause: bit::BLACKOUT,
                members: region_members[region].clone(),
                affected: Vec::new(),
                token: 0,
                draws: 0,
                rate_per_s: hcfg.blackouts_per_year / self.p.seconds_per_year,
                median_s: hcfg.blackout_median_h * 3_600.0,
                sigma: hcfg.blackout_sigma,
                blocks: Vec::new(),
                ups_runtime: 0.0,
                ends: 0,
            });
        }
        self.shared = Shared {
            processes,
            feeder_of,
            isp_of,
            blackout_of,
        };
    }

    /// Registers a newly created household with the shared outage processes of its feeder
    /// group, provider and region.
    pub(crate) fn join_shared(&mut self, h: u32) {
        let hh = &self.hh[h as usize];
        if hh.always_online || hh.independent.is_some() {
            return;
        }
        let mut list: Vec<u32> = Vec::new();
        if let Some(v) = self.shared.feeder_of.get(hh.grid_group as usize) {
            list.extend(v);
        }
        if let Some(v) = self.shared.isp_of.get(hh.isp as usize) {
            list.extend(v);
        }
        if let Some(b) = self.shared.blackout_of.get(hh.region as usize) {
            if *b != NONE {
                list.push(*b);
            }
        }
        for p in list {
            self.shared.processes[p as usize].members.push(h);
        }
    }

    // ------------------------------------------------------------------ start

    pub(crate) fn start(&mut self) {
        // Hopping groups start in their first window's state (away), before anyone mines.
        self.apply_hops(0);
        // Miners start admitted and mining (warm start).
        for m in 0..self.miners.len() as u32 {
            if self.hh[self.miners[m as usize].household as usize].online() {
                self.admit_now(0, m);
            }
        }
        for h in 0..self.hh.len() as u32 {
            self.schedule_household(h);
        }
        for p in 0..self.shared.processes.len() as u32 {
            self.schedule_shared_start(0, p);
        }
        self.q.push(self.end, Ev::End);
        self.q.push(self.p.pow_tx * self.p.cac_every, Ev::Committee);
        if self.spec.kwc_sample_interval_s > 0.0 {
            self.q.push(self.p.warmup.max(micros(60.0)), Ev::Sample);
        }
        if self.spec.arrivals_multiple_of_r > 0.0 {
            self.schedule_arrival(0);
        }
        if !self.spec.scripted_crashes.is_empty() {
            let at = micros(self.spec.scripted_crashes[0].0 * 86_400.0);
            self.q.push(at, Ev::ScriptedCrash { index: 0 });
        }
        if self.spec.partition_cuts_witnesses {
            let parts: Vec<(Micros, Micros)> = self
                .net
                .partitions
                .iter()
                .map(|p| (p.start, p.end))
                .collect();
            for (i, (start, end)) in parts.into_iter().enumerate() {
                let index = i as u32;
                self.q.push(start, Ev::Partition { index, start: true });
                self.q.push(
                    end,
                    Ev::Partition {
                        index,
                        start: false,
                    },
                );
            }
        }
        if self.spec.failover_after_rounds.is_some() {
            self.q.push(micros(self.p.interval_s as f64), Ev::Failover);
        }
        if self.spec.track_outages {
            self.outage_of_process = vec![NONE; self.shared.processes.len()];
        }
        self.outage_of_script = vec![NONE; self.spec.scripted_blackouts.len()];
        for (i, (_, day, _)) in self.spec.scripted_blackouts.iter().enumerate() {
            let index = i as u32;
            self.q.push(
                micros(day * 86_400.0),
                Ev::ScriptedBlackout { index, start: true },
            );
        }
        self.d_max = micros(self.net.max_delay(self.max_access) + 0.2);
        self.start_chain();
    }

    fn schedule_household(&mut self, h: u32) {
        let hh = &mut self.hh[h as usize];
        hh.wake_gen = hh.wake_gen.wrapping_add(1);
        let t = hh.next_wake();
        if t != u64::MAX {
            let token = hh.wake_gen;
            self.q.push(t, Ev::Household { h, token });
        }
    }

    // ------------------------------------------------------------------ households

    pub(crate) fn on_household(&mut self, now: Micros, h: u32, token: u32) {
        if self.hh[h as usize].wake_gen != token {
            return;
        }
        let region = self.hh[h as usize].region as usize;
        let ctx = self.cause_context(region);
        loop {
            let (c, t) = {
                let hh = &self.hh[h as usize];
                let (c, t) = hh
                    .next
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, t)| **t)
                    .map(|(c, t)| (c, *t))
                    .unwrap_or((0, u64::MAX));
                (c, t)
            };
            if t > now {
                break;
            }
            let mut hh = std::mem::replace(&mut self.hh[h as usize], placeholder_household());
            let tr = household::transition(&mut self.keyed, u64::from(h), &mut hh, c, now, &ctx);
            self.hh[h as usize] = hh;
            if let Some(tr) = tr {
                self.set_cause(now, h, tr.cause, tr.start, tr.announced, tr.ends);
            }
        }
        self.schedule_household(h);
    }

    /// Sets or clears an outage cause of a household and applies the protocol effects of a
    /// change between online and offline.
    pub(crate) fn set_cause(
        &mut self,
        now: Micros,
        h: u32,
        cause: u16,
        on: bool,
        announced: bool,
        ends: Micros,
    ) {
        let was_online = self.hh[h as usize].online();
        // Statuses before the change, so counters move from what they counted.
        let miner_old = self.hh[h as usize].miner.map(|m| (m, self.miner_status(m)));
        let witness_olds: Vec<(u32, WitnessStatus)> = self.hh[h as usize]
            .witnesses
            .iter()
            .map(|w| (*w, self.witness_status(*w)))
            .collect();
        {
            let hh = &mut self.hh[h as usize];
            let index = cause.trailing_zeros() as usize;
            if on {
                hh.mask |= cause;
                hh.ends[index] = ends;
                if announced {
                    hh.announced |= cause;
                } else {
                    hh.announced &= !cause;
                }
            } else {
                hh.mask &= !cause;
                hh.announced &= !cause;
            }
        }
        let online = self.hh[h as usize].online();
        // A witness cut off by a partition is already down for its KWCs: the household's
        // changes reach it when the partition heals.
        if was_online && !online {
            if let Some((m, old)) = miner_old {
                self.miner_down(now, m, announced, old);
            }
            for (w, old) in witness_olds {
                if !self.witnesses[w as usize].cut_off {
                    self.witness_down(now, w, announced, old, cause_index(cause));
                } else {
                    self.commit_witness(w, old);
                }
            }
        } else if !was_online && online {
            if let Some((m, old)) = miner_old {
                self.miner_up(now, m, old);
            }
            for (w, old) in witness_olds {
                if !self.witnesses[w as usize].cut_off {
                    self.witness_up(now, w, old);
                } else {
                    self.commit_witness(w, old);
                }
            }
        }
    }

    /// Whether witness `w` can be reached by its KWCs: its household is online and no
    /// partition cuts it off.
    pub(crate) fn witness_reachable(&self, w: u32) -> bool {
        let wit = &self.witnesses[w as usize];
        !wit.cut_off && self.hh[wit.household as usize].online()
    }

    /// When an unreachable witness can next be reached, from `from` on (`u64::MAX` if never).
    pub(crate) fn witness_back_at(&self, w: u32, from: Micros) -> Micros {
        let wit = &self.witnesses[w as usize];
        let hh = self.hh[wit.household as usize].back_at(from);
        if wit.cut_off {
            hh.max(wit.cut_until)
        } else {
            hh
        }
    }

    /// A partition starts or heals (architect, 2026-10-14: a member online but cut off by a
    /// partition counts as unreachable). At the start every reachable witness in the cut
    /// region disappears for its KWCs without announcing it, so One Chance applies; at the
    /// end it returns, if its household is online.
    pub(crate) fn on_partition(&mut self, now: Micros, index: u32, start: bool) {
        let part = self.net.partitions[index as usize];
        for w in 0..self.witnesses.len() as u32 {
            let h = self.witnesses[w as usize].household as usize;
            if self.hh[h].region as usize != part.region || self.witnesses[w as usize].removed() {
                continue;
            }
            if start {
                self.witness_cut(now, w, part.end);
            } else {
                self.witness_uncut(now, w);
            }
        }
    }

    fn witness_cut(&mut self, now: Micros, w: u32, until: Micros) {
        if self.witnesses[w as usize].cut_off {
            let wit = &mut self.witnesses[w as usize];
            wit.cut_until = wit.cut_until.max(until);
            return;
        }
        if !self.witness_reachable(w) {
            return;
        }
        let old = self.witness_status(w);
        let pt = self.p.pow_tx.max(1);
        let (one_chance, grace) = (self.p.one_chance, self.p.grace);
        let wit = &mut self.witnesses[w as usize];
        wit.cut_off = true;
        wit.cut_until = until;
        wit.token = wit.token.wrapping_add(1);
        wit.down_at = now;
        let token = wit.token;
        if wit.recorded {
            // Like an unannounced disappearance, without counting it as a household crash.
            wit.ghost = true;
            wit.short_outage = until.saturating_sub(now) < grace;
            let request = (now / pt + 1) * pt;
            wit.request_at = request;
            wit.check_at = request + one_chance;
            let check = wit.check_at;
            self.res.partition_cuts += 1;
            self.q
                .push_lane(lane::FIRST_CHECK, check, Ev::WitnessCheck { w, token });
        }
        self.commit_witness(w, old);
        self.start_points_period(now, w, CAUSE_CUT);
    }

    fn witness_uncut(&mut self, now: Micros, w: u32) {
        let wit = &self.witnesses[w as usize];
        if !wit.cut_off || now < wit.cut_until {
            return;
        }
        let old = self.witness_status(w);
        self.witnesses[w as usize].cut_off = false;
        if self.hh[self.witnesses[w as usize].household as usize].online() {
            self.witness_up(now, w, old);
        } else {
            self.commit_witness(w, old);
        }
    }

    // ------------------------------------------------------------------ shared outages

    fn schedule_shared_start(&mut self, now: Micros, p: u32) {
        let process = &mut self.shared.processes[p as usize];
        process.token = process.token.wrapping_add(1);
        let token = process.token;
        let start = match process.kind {
            SharedKind::Loadshed => next_loadshed(now, &process.blocks),
            _ => {
                if process.rate_per_s <= 0.0 || process.members.is_empty() {
                    return;
                }
                let n = process.draws;
                process.draws += 1;
                let u =
                    self.keyed
                        .uniforms(Purpose::SharedOutage, 1_000 + u64::from(p), u64::from(n));
                Some(now + micros(exponential(1.0 - u[0], process.rate_per_s)))
            }
        };
        if let Some(start) = start {
            self.q.push(
                start,
                Ev::Shared {
                    process: p,
                    phase: 0,
                    token,
                },
            );
        }
    }

    pub(crate) fn on_shared(&mut self, now: Micros, p: u32, phase: u8, token: u32) {
        if self.shared.processes[p as usize].token != token {
            return;
        }
        match phase {
            0 => {
                let (duration, cause, ups_runtime, grid) = {
                    let process = &mut self.shared.processes[p as usize];
                    let duration = match process.kind {
                        SharedKind::Loadshed => current_block_length(now, &process.blocks),
                        _ => {
                            let n = process.draws;
                            process.draws += 1;
                            let u = self.keyed.uniforms(
                                Purpose::SharedOutage,
                                1_000 + u64::from(p),
                                u64::from(n),
                            );
                            lognormal(process.median_s, process.sigma, normal(1.0 - u[0], u[1]))
                        }
                    };
                    let grid = matches!(process.kind, SharedKind::Fault | SharedKind::Loadshed);
                    (duration.max(1.0), process.cause, process.ups_runtime, grid)
                };
                let ends = now + micros(duration);
                let members = self.shared.processes[p as usize].members.clone();
                let mut affected = Vec::new();
                let mut ups_later = false;
                for h in members {
                    if grid && self.hh[h as usize].ups {
                        if duration > ups_runtime {
                            ups_later = true;
                        }
                        continue;
                    }
                    affected.push(h);
                    self.set_cause(now, h, cause, true, false, ends);
                }
                if self.spec.track_outages {
                    let kind = match self.shared.processes[p as usize].kind {
                        SharedKind::Blackout => Some(0),
                        SharedKind::Isp => Some(1),
                        _ => None,
                    };
                    if let (Some(kind), Some(first)) = (kind, affected.first()) {
                        let region = self.hh[*first as usize].region as usize;
                        let miners = affected
                            .iter()
                            .filter_map(|h| self.hh[*h as usize].miner)
                            .filter(|m| {
                                matches!(
                                    self.miners[*m as usize].phase,
                                    Phase::Mining | Phase::Waiting
                                )
                            })
                            .count() as u64;
                        let r = self.start_outage_record(
                            now,
                            kind,
                            region,
                            duration,
                            affected.len(),
                            miners,
                        );
                        if let Some(x) = self.outage_of_process.get_mut(p as usize) {
                            *x = r;
                        }
                    }
                }
                let process = &mut self.shared.processes[p as usize];
                process.affected = affected;
                process.ends = ends;
                let token = process.token;
                if ups_later {
                    self.q.push(
                        now + micros(ups_runtime),
                        Ev::Shared {
                            process: p,
                            phase: 1,
                            token,
                        },
                    );
                }
                self.q.push(
                    ends,
                    Ev::Shared {
                        process: p,
                        phase: 2,
                        token,
                    },
                );
            }
            1 => {
                let (cause, ends) = {
                    let process = &self.shared.processes[p as usize];
                    (process.cause, process.ends)
                };
                let members = self.shared.processes[p as usize].members.clone();
                let mut extra = Vec::new();
                for h in members {
                    if self.hh[h as usize].ups {
                        extra.push(h);
                        self.set_cause(now, h, cause, true, false, ends);
                    }
                }
                self.shared.processes[p as usize].affected.extend(extra);
            }
            _ => {
                let (cause, affected) = {
                    let process = &mut self.shared.processes[p as usize];
                    (process.cause, std::mem::take(&mut process.affected))
                };
                for h in affected {
                    self.set_cause(now, h, cause, false, false, now);
                }
                if let Some(x) = self.outage_of_process.get_mut(p as usize) {
                    *x = NONE;
                }
                self.schedule_shared_start(now, p);
            }
        }
    }

    // ------------------------------------------------------------------ miners

    pub(crate) fn miner_status(&self, m: u32) -> MinerStatus {
        let miner = &self.miners[m as usize];
        let online = self.hh[miner.household as usize].online();
        MinerStatus {
            after_wait: miner.recorded && miner.phase == Phase::Mining,
            all: miner.recorded && matches!(miner.phase, Phase::Waiting | Phase::Mining),
            mining: miner.phase == Phase::Mining && online && !miner.idle,
            hop: miner.phase == Phase::Mining
                && online
                && !miner.idle
                && self
                    .hop_state
                    .get(miner.group as usize)
                    .copied()
                    .unwrap_or(0)
                    == 2,
        }
    }

    /// Applies the counter changes between `old` and the miner's current status.
    pub(crate) fn commit_miner(&mut self, now: Micros, m: u32, old: MinerStatus) {
        self.advance_area(now);
        let new = self.miner_status(m);
        let miner = &self.miners[m as usize];
        let group = miner.group as usize;
        let region = self.hh[miner.household as usize].region as usize;
        let delta = |a: bool, b: bool| i64::from(b) - i64::from(a);
        self.counts.after_wait =
            (self.counts.after_wait as i64 + delta(old.after_wait, new.after_wait)) as u64;
        self.counts.all = (self.counts.all as i64 + delta(old.all, new.all)) as u64;
        let d = delta(old.mining, new.mining);
        self.counts.mining_group[group] = (self.counts.mining_group[group] as i64 + d) as u64;
        self.counts.mining_region[region] = (self.counts.mining_region[region] as i64 + d) as u64;
        let device = self.hh[miner.household as usize].device as usize;
        let gd = &mut self.counts.mining_group_device[group][device];
        *gd = (*gd as i64 + d) as u64;
        let gr = &mut self.counts.mining_group_region[group][region];
        *gr = (*gr as i64 + d) as u64;
        self.counts.hop_mining = (self.counts.hop_mining as i64 + delta(old.hop, new.hop)) as u64;
    }

    /// Integrates mining slot-seconds up to `now`.
    pub(crate) fn advance_area(&mut self, now: Micros) {
        let from = self.counts.last.max(self.p.warmup);
        if now > from {
            let dt = (now - from) as f64 / SECOND as f64;
            let day = self.day_index(from);
            if self.res.daily_slot_s_by_group.len() <= day {
                self.res
                    .daily_slot_s_by_group
                    .resize(day + 1, vec![0.0; self.counts.mining_group.len()]);
            }
            for (g, n) in self.counts.mining_group.iter().enumerate() {
                self.res.slot_s_by_group[g] += *n as f64 * dt;
                self.res.daily_slot_s_by_group[day][g] += *n as f64 * dt;
            }
            if self.res.daily_slot_s_by_region.len() <= day {
                self.res
                    .daily_slot_s_by_region
                    .resize(day + 1, vec![0.0; self.counts.mining_region.len()]);
            }
            for (r, n) in self.counts.mining_region.iter().enumerate() {
                self.res.slot_s_by_region[r] += *n as f64 * dt;
                self.res.daily_slot_s_by_region[day][r] += *n as f64 * dt;
            }
            self.res.hop_slot_s += self.counts.hop_mining as f64 * dt;
            for (g, row) in self.counts.mining_group_device.iter().enumerate() {
                for (x, n) in row.iter().enumerate() {
                    self.res.slot_s_by_group_device[g][x] += *n as f64 * dt;
                }
            }
            for (g, row) in self.counts.mining_group_region.iter().enumerate() {
                for (x, n) in row.iter().enumerate() {
                    self.res.slot_s_by_group_region[g][x] += *n as f64 * dt;
                }
            }
        }
        self.counts.last = now.max(self.counts.last);
    }

    /// Admits a miner at once (warm start and instant admission).
    pub(crate) fn admit_now(&mut self, now: Micros, m: u32) {
        let old = self.miner_status(m);
        if self.miners[m as usize].kwc == NONE {
            let k = self.choose_kwc(m);
            self.miners[m as usize].kwc = k;
            if k != NONE {
                self.kwc_load[k as usize] += 1;
            }
        }
        let miner = &mut self.miners[m as usize];
        miner.phase = Phase::Mining;
        miner.recorded = true;
        miner.ghost = false;
        miner.mine_at = now;
        miner.token = miner.token.wrapping_add(1);
        self.commit_miner(now, m, old);
        self.on_mining_start(now, m);
    }

    pub(crate) fn choose_kwc(&mut self, m: u32) -> u32 {
        let wcs = self.alloc.active_wcs() as usize;
        if wcs == 0 {
            return NONE;
        }
        if self.kwc_load.len() < wcs {
            self.kwc_load.resize(wcs, 0);
        }
        let epoch = u64::from(self.miners[m as usize].conn);
        for attempt in 0..64u64 {
            let w = self
                .keyed
                .words(Purpose::Admission, u64::from(m), epoch * 64 + attempt);
            for word in w {
                let k = household::pick(word, wcs);
                if self.kwc_load[k] < self.p.capacity
                    && (self.spec.failover_after_rounds.is_none()
                        || !self.kwc_below_quorum(k as u32))
                {
                    return k as u32;
                }
            }
        }
        NONE
    }

    /// Starts admission (SPEC §3.1): bootstrap, local validation, claim broadcast and the
    /// convergence interval, then the MOBu vote.
    pub(crate) fn start_admission(&mut self, now: Micros, m: u32) {
        let miner = &self.miners[m as usize];
        if miner.phase != Phase::Idle {
            return;
        }
        if self.spec.instant_admission {
            self.admit_now(now, m);
            return;
        }
        let old = self.miner_status(m);
        let k = self.choose_kwc(m);
        if k == NONE {
            // With failover, no KWC able to take it now: it tries again an epoch later.
            if self.spec.failover_after_rounds.is_some() {
                self.miners[m as usize].conn += 1;
                let token = self.miners[m as usize].token;
                self.q
                    .push(now + self.p.grace, Ev::MinerRejoin { m, token });
            }
            return;
        }
        self.kwc_load[k as usize] += 1;
        let h = self.miners[m as usize].household;
        let region = self.hh[h as usize].region as usize;
        let access = self.hh[h as usize].access;
        let conn = u64::from(self.miners[m as usize].conn);
        let u = self
            .keyed
            .uniforms(Purpose::Admission, (1 << 32) | u64::from(m), conn);
        // Bootstrap from peers, local validation by the KWC, then the claim's gossip.
        let rtt = |net: &Network, u0: f64, u1: f64| {
            2.0 * net.direct_delay(region, access, region, 0.01, u0, u1)
        };
        let bootstrap = rtt(&self.net, u[0], 1.0 - u[1]);
        let local = rtt(&self.net, u[2], 1.0 - u[3]);
        let claim = self.net.gossip_delay(region, region, 0.01, u[4]);
        let delay = micros(bootstrap + local + claim) + self.p.convergence;
        let miner = &mut self.miners[m as usize];
        miner.kwc = k;
        miner.phase = Phase::Admitting;
        miner.token = miner.token.wrapping_add(1);
        miner.conn += 1;
        let token = miner.token;
        self.commit_miner(now, m, old);
        self.q.push(now + delay, Ev::MinerActivate { m, token });
    }

    pub(crate) fn on_miner_activate(&mut self, now: Micros, m: u32, token: u32) {
        if self.miners[m as usize].token != token
            || self.miners[m as usize].phase != Phase::Admitting
        {
            return;
        }
        let h = self.miners[m as usize].household;
        if !self.hh[h as usize].online() {
            self.release_slot(now, m);
            return;
        }
        // The MOBu needs two-thirds of the KWC (27 of 40) online to vote.
        let k = self.miners[m as usize].kwc;
        let online = self.kwc_counts(k).online as usize;
        if online < self.p.quorum_unregistered {
            let miner = &mut self.miners[m as usize];
            miner.vote_failures += 1;
            if miner.vote_failures >= 2 {
                miner.vote_failures = 0;
                self.release_slot(now, m);
                self.start_admission(now, m);
            } else {
                let token = miner.token;
                self.q
                    .push(now + self.p.grace, Ev::MinerActivate { m, token });
            }
            return;
        }
        let old = self.miner_status(m);
        let miner = &mut self.miners[m as usize];
        miner.phase = Phase::Waiting;
        miner.recorded = true;
        miner.ghost = false;
        miner.vote_failures = 0;
        miner.mine_at = now + self.p.post_wait;
        miner.token = miner.token.wrapping_add(1);
        let token = miner.token;
        self.commit_miner(now, m, old);
        self.q
            .push(now + self.p.post_wait, Ev::MinerWaitEnd { m, token });
    }

    pub(crate) fn on_miner_wait_end(&mut self, now: Micros, m: u32, token: u32) {
        if self.miners[m as usize].token != token || self.miners[m as usize].phase != Phase::Waiting
        {
            return;
        }
        let old = self.miner_status(m);
        self.miners[m as usize].phase = Phase::Mining;
        self.commit_miner(now, m, old);
        self.on_mining_start(now, m);
    }

    /// A miner starts mining: record restart and switch times, and apply its strategy.
    fn on_mining_start(&mut self, now: Micros, m: u32) {
        let miner = &mut self.miners[m as usize];
        if miner.decision_kind != 0 {
            let took = (now - miner.decision_at) as f64 / SECOND as f64;
            if miner.decision_kind == 1 && self.res.restart_s.len() < 10_000 {
                self.res.restart_s.push(took);
            } else if miner.decision_kind == 2 && self.res.switch_s.len() < 10_000 {
                self.res.switch_s.push(took);
            } else if miner.decision_kind == 3 && self.res.failover_s.len() < 10_000 {
                self.res.failover_s.push(took);
            }
            miner.decision_kind = 0;
        }
        if let Some(Strategy::Restart {
            old_design: true,
            window_s,
        }) = self.spec_strategy(m)
        {
            self.new_connection_sequence(now, m, window_s);
        }
        if self.spec.switches_per_day > 0.0 {
            let token = self.miners[m as usize].token;
            let u = self.keyed.uniforms(
                Purpose::Admission,
                (2 << 32) | u64::from(m),
                u64::from(self.miners[m as usize].conn),
            );
            let t = exponential(1.0 - u[0], self.spec.switches_per_day / 86_400.0);
            self.q.push(now + micros(t), Ev::MinerSwitch { m, token });
        }
    }

    /// Old design (S5): entropy is fixed for the connection, so the miner knows when its
    /// next winning step comes; it restarts when that is further than its keep window.
    pub(crate) fn new_connection_sequence(&mut self, now: Micros, m: u32, window_s: f64) {
        let q = self.frozen_step_probability();
        let conn = u64::from(self.miners[m as usize].conn);
        let u = self
            .keyed
            .uniforms(Purpose::Strategy, u64::from(m), conn << 20);
        let now_s = now / SECOND;
        let gap = crate::samplers::geometric_failures(1.0 - u[0], q);
        let next = now_s.saturating_add(gap);
        self.miners[m as usize].next_success = next;
        if (next - now_s) as f64 > window_s {
            self.restart(now, m);
        }
    }

    /// The next success of an old-design miner after `after_s`.
    pub(crate) fn advance_connection_sequence(&mut self, m: u32, after_s: u64, draw: u64) {
        let q = self.frozen_step_probability();
        let conn = u64::from(self.miners[m as usize].conn);
        let u = self
            .keyed
            .uniforms(Purpose::Strategy, u64::from(m), (conn << 20) + 1 + draw);
        let gap = crate::samplers::geometric_failures(1.0 - u[0], q);
        self.miners[m as usize].next_success = after_s.saturating_add(1).saturating_add(gap);
    }

    /// The per-step success probability of the frozen target (V2).
    pub(crate) fn frozen_step_probability(&self) -> f64 {
        self.spec
            .frozen_target
            .as_ref()
            .map_or(0.0, gb_protocol::chain::step_win_probability)
    }

    /// A self-service offline request followed by re-admission after one epoch (SPEC §3.2);
    /// `kind` 1 is a restart (S5), 2 a switch of KWC (C5).
    pub(crate) fn self_offline(&mut self, now: Micros, m: u32, kind: u8) {
        let old = self.miner_status(m);
        if !matches!(
            self.miners[m as usize].phase,
            Phase::Mining | Phase::Waiting
        ) {
            return;
        }
        let k = self.miners[m as usize].kwc;
        if k != NONE && (k as usize) < self.kwc_load.len() {
            self.kwc_load[k as usize] = self.kwc_load[k as usize].saturating_sub(1);
        }
        let miner = &mut self.miners[m as usize];
        miner.phase = Phase::Idle;
        miner.recorded = false;
        miner.ghost = false;
        miner.kwc = NONE;
        miner.decision_at = now;
        miner.decision_kind = kind;
        miner.token = miner.token.wrapping_add(1);
        let token = miner.token;
        self.commit_miner(now, m, old);
        // It queries peers, broadcasts its own offline request and waits one epoch.
        let wait = if self.spec.instant_admission {
            0
        } else {
            self.p.grace + micros(0.5)
        };
        self.miners[m as usize].rejoin_at = now + wait;
        self.q.push(now + wait, Ev::MinerRejoin { m, token });
    }

    pub(crate) fn restart(&mut self, now: Micros, m: u32) {
        self.res.restarts += 1;
        self.self_offline(now, m, 1);
    }

    pub(crate) fn on_miner_rejoin(&mut self, now: Micros, m: u32, token: u32) {
        if self.miners[m as usize].token != token {
            return;
        }
        if self.hh[self.miners[m as usize].household as usize].online() {
            self.start_admission(now, m);
        }
    }

    pub(crate) fn on_miner_switch(&mut self, now: Micros, m: u32, token: u32) {
        if self.miners[m as usize].token != token || self.miners[m as usize].phase != Phase::Mining
        {
            return;
        }
        if self.hh[self.miners[m as usize].household as usize].online() {
            self.self_offline(now, m, 2);
        }
    }

    fn miner_down(&mut self, now: Micros, m: u32, announced: bool, old: MinerStatus) {
        let phase = self.miners[m as usize].phase;
        match phase {
            Phase::Waiting | Phase::Mining => {
                if announced {
                    self.miners[m as usize].announced_off_at = now;
                    self.release_slot_recorded(now, m, old);
                    return;
                }
                let miner = &mut self.miners[m as usize];
                miner.ghost = true;
                let token = miner.token;
                self.commit_miner(now, m, old);
                self.q.push_lane(
                    lane::MINER_GRACE,
                    now + self.p.grace,
                    Ev::MinerGrace { m, token },
                );
            }
            Phase::Admitting => self.release_slot_recorded(now, m, old),
            _ => self.commit_miner(now, m, old),
        }
    }

    fn miner_up(&mut self, now: Micros, m: u32, old: MinerStatus) {
        let miner = &mut self.miners[m as usize];
        if miner.ghost {
            miner.ghost = false;
            self.commit_miner(now, m, old);
            return;
        }
        self.commit_miner(now, m, old);
        // A miner within the epoch after its own offline request waits for its rejoin.
        if self.miners[m as usize].phase == Phase::Idle && now >= self.miners[m as usize].rejoin_at
        {
            // A shutdown announced less than a grace epoch ago re-admits at once (architect,
            // 2026-10-14); counted.
            let off = self.miners[m as usize].announced_off_at;
            if off != u64::MAX && now.saturating_sub(off) < self.p.grace {
                self.res.quick_readmits += 1;
            }
            self.start_admission(now, m);
        }
    }

    /// Voids witness `w`'s ID, won by miner slot `miner`, whose PoW-ID block a reorganisation
    /// below confirmation depth replaced (architect, 2026-10-14). The ID leaves the registry's
    /// active list, the committee, its seat and any pending placement. This is not a ban or a
    /// One Chance penalty: no ban record, no offence; the miner slot simply goes back to being
    /// a candidate (a newcomer without another ID waits again for a first ID).
    pub(crate) fn void_id(&mut self, w: u32, miner: u32) {
        if self.witnesses[w as usize].void {
            return;
        }
        let old = self.witness_status(w);
        self.witnesses[w as usize].void = true;
        self.commit_witness(w, old);
        let reg = self.witnesses[w as usize].reg as usize;
        self.registry.set_status(reg, IdStatus::Void);
        self.active.remove(reg);
        self.committee.depart(reg);
        self.remove_seat(w);
        self.placements.retain(|(_, x)| *x != w);
        self.res.voided_ids += 1;
        if miner == NONE {
            return;
        }
        let slot = &mut self.miners[miner as usize];
        slot.ids = slot.ids.saturating_sub(1);
        if slot.ids == 0 && slot.first_id {
            slot.first_id = false;
            slot.first_id_at = u64::MAX;
            if slot.newcomer {
                self.res.first_ids -= 1;
                // It had given up and was counted as a first ID instead: a dropout again.
                if slot.phase == Phase::Gone {
                    self.res.dropouts += 1;
                }
            }
        }
    }

    pub(crate) fn on_miner_grace(&mut self, now: Micros, m: u32, token: u32) {
        let miner = &self.miners[m as usize];
        if miner.token != token || !miner.ghost {
            return;
        }
        let old = self.miner_status(m);
        self.release_slot_recorded(now, m, old);
    }

    fn release_slot_recorded(&mut self, now: Micros, m: u32, old: MinerStatus) {
        let k = self.miners[m as usize].kwc;
        if k != NONE && (k as usize) < self.kwc_load.len() {
            self.kwc_load[k as usize] = self.kwc_load[k as usize].saturating_sub(1);
        }
        let miner = &mut self.miners[m as usize];
        miner.phase = Phase::Idle;
        miner.recorded = false;
        miner.ghost = false;
        miner.kwc = NONE;
        miner.token = miner.token.wrapping_add(1);
        self.commit_miner(now, m, old);
    }

    /// Frees a miner's slot (offline record, failed admission, or a confirmed win).
    pub(crate) fn release_slot(&mut self, now: Micros, m: u32) {
        let old = self.miner_status(m);
        self.release_slot_recorded(now, m, old);
    }

    /// The strategy of a miner's group.
    pub(crate) fn spec_strategy(&self, m: u32) -> Option<Strategy> {
        let g = self.miners[m as usize].group as usize;
        if g == 0 {
            None
        } else {
            self.spec.groups.get(g - 1).map(|s| s.strategy)
        }
    }

    // ------------------------------------------------------------------ witnesses

    fn witness_status(&self, w: u32) -> WitnessStatus {
        let wit = &self.witnesses[w as usize];
        let seated = wit.seat != NONE && !wit.removed();
        WitnessStatus {
            wc: seated.then(|| wit.seat as usize / self.p.wc_size),
            online: self.witness_reachable(w),
            recorded: wit.recorded,
            ghost: wit.ghost,
        }
    }

    fn commit_witness(&mut self, w: u32, old: WitnessStatus) {
        let new = self.witness_status(w);
        if let Some(wc) = old.wc {
            let c = &mut self.wc[wc];
            c.seated -= 1;
            c.online -= u32::from(old.online);
            c.recorded -= u32::from(old.recorded);
            c.ghost -= u32::from(old.ghost);
        }
        if let Some(wc) = new.wc {
            if self.wc.len() <= wc {
                self.wc.resize(wc + 1, WcCount::default());
            }
            let c = &mut self.wc[wc];
            c.seated += 1;
            c.online += u32::from(new.online);
            c.recorded += u32::from(new.recorded);
            c.ghost += u32::from(new.ghost);
        }
    }

    fn witness_down(
        &mut self,
        now: Micros,
        w: u32,
        announced: bool,
        old: WitnessStatus,
        cause: u8,
    ) {
        if self.witnesses[w as usize].removed() {
            return;
        }
        let back = self.hh[self.witnesses[w as usize].household as usize].back_at(now);
        let wit = &mut self.witnesses[w as usize];
        wit.token = wit.token.wrapping_add(1);
        wit.down_at = now;
        let token = wit.token;
        if announced || !wit.recorded {
            wit.recorded = false;
        } else {
            // Disappeared without an offline request: still recorded online, so every entropy
            // request in its four KWCs waits for it until One Chance fails (SPEC §4.5).
            wit.ghost = true;
            let short = back.saturating_sub(now) < self.p.grace;
            wit.short_outage = short;
            // The first request comes with the next PoW-Tx round of a registered miner in one
            // of its KWCs (every PoW-Tx interval).
            let pt = self.p.pow_tx.max(1);
            let request = (now / pt + 1) * pt;
            wit.request_at = request;
            wit.check_at = request + self.p.one_chance;
            let check = wit.check_at;
            self.res.witness_crashes += 1;
            if short {
                self.res.short_glitches += 1;
            }
            self.q
                .push_lane(lane::FIRST_CHECK, check, Ev::WitnessCheck { w, token });
        }
        self.commit_witness(w, old);
        self.start_points_period(now, w, cause);
        self.q.push_lane(
            lane::DEACTIVATE,
            now + self.p.deactivation,
            Ev::WitnessDeactivate { w, token },
        );
        self.q.push_lane(
            lane::LONG_ABSENCE,
            now + self.p.long_absence,
            Ev::WitnessLongAbsence { w, token },
        );
    }

    fn witness_up(&mut self, now: Micros, w: u32, old: WitnessStatus) {
        let wit = &mut self.witnesses[w as usize];
        if wit.removed() {
            return;
        }
        wit.token = wit.token.wrapping_add(1);
        if wit.ghost {
            // Back before One Chance failed: it signs late and nothing is recorded.
            wit.ghost = false;
            let stall = now.saturating_sub(wit.request_at) as f64 / SECOND as f64;
            if now > wit.request_at {
                self.res.stall_s_sum += stall;
                if self.res.stall_samples.len() < 10_000 {
                    self.res.stall_samples.push(stall);
                }
            }
        }
        let wit = &mut self.witnesses[w as usize];
        wit.recorded = true;
        let reactivate = wit.deactivated;
        wit.deactivated = false;
        let reg = wit.reg as usize;
        let unseated = wit.seat == NONE;
        self.commit_witness(w, old);
        if reactivate {
            self.registry.set_status(reg, IdStatus::Active);
            self.active.insert(reg);
        }
        if unseated && now >= self.witnesses[w as usize].seat_cooldown_until {
            // Its seat was vacated after an absence longer than L: re-inserted with the beacon
            // of the block `beacon delay` after its re-activation is confirmed. After a seat
            // loss at the points cap it waits for the cooldown (`on_seat_cooldown`).
            let target = self.network_height() + self.p.confirmation_depth + self.p.beacon_delay;
            self.placements.push((target, w));
        }
    }

    pub(crate) fn on_witness_check(&mut self, now: Micros, w: u32, token: u32) {
        let wit = &self.witnesses[w as usize];
        if wit.token != token || !wit.ghost {
            return;
        }
        // The record needs a vote of two-thirds of one of its KWCs (27 of 40 online).
        if !self.witness_vote_possible(w) {
            self.q.push_lane(
                lane::RECHECK,
                now + micros(self.p.interval_s as f64),
                Ev::WitnessCheck { w, token },
            );
            return;
        }
        let old = self.witness_status(w);
        let outcome = one_chance_outcome(self.spec.unreachable, false, false);
        let h = self.witnesses[w as usize].household as usize;
        let (region, device) = (self.hh[h].region as usize, self.hh[h].device as usize);
        let stall =
            now.saturating_sub(self.witnesses[w as usize].request_at) as f64 / SECOND as f64;
        self.res.stall_s_sum += stall;
        if self.res.stall_samples.len() < 10_000 {
            self.res.stall_samples.push(stall);
        }
        let wit = &mut self.witnesses[w as usize];
        if outcome.recorded_offline {
            wit.ghost = false;
            wit.recorded = false;
            self.res.offline_records[region][device] += 1;
            if wit.short_outage && !wit.cut_off {
                self.res.short_glitches_recorded += 1;
            }
        }
        let mut ban = false;
        if outcome.penalised {
            let now_s = now / SECOND;
            let penalty = penalty_for(
                self.spec.strict_penalty,
                &self.p.ladder,
                &wit.offences,
                now_s,
                self.p.seconds_per_year as u64,
            );
            wit.offences.push(now_s);
            self.res.penalties[region][device] += 1;
            if penalty == Penalty::Permanent {
                ban = true;
            }
        }
        self.commit_witness(w, old);
        if ban {
            self.ban_witness(w, region, device);
        }
    }

    fn witness_vote_possible(&self, w: u32) -> bool {
        let seat = self.witnesses[w as usize].seat;
        if seat == NONE {
            return true;
        }
        let wc = u64::from(seat) / self.p.wc_size as u64;
        let wcs = self.alloc.active_wcs();
        if wc >= wcs {
            return true;
        }
        // The four KWCs this WC sits in: led by wc, and by wc − offset for each offset.
        let can_vote =
            |k: u64| self.kwc_counts(k as u32).online as usize >= self.p.quorum_unregistered;
        can_vote(wc)
            || self
                .p
                .offsets
                .iter()
                .any(|o| can_vote((wc + wcs - o % wcs) % wcs))
    }

    fn ban_witness(&mut self, w: u32, region: usize, device: usize) {
        self.res.bans[region][device] += 1;
        let old = self.witness_status(w);
        let reg = self.witnesses[w as usize].reg as usize;
        self.witnesses[w as usize].banned = true;
        self.commit_witness(w, old);
        self.registry.set_status(reg, IdStatus::Banned);
        self.active.remove(reg);
        self.committee.depart(reg);
        self.remove_seat(w);
    }

    pub(crate) fn on_witness_deactivate(&mut self, _now: Micros, w: u32, token: u32) {
        let wit = &self.witnesses[w as usize];
        if wit.token != token || wit.removed() || self.hh[wit.household as usize].online() {
            return;
        }
        let reg = wit.reg as usize;
        self.witnesses[w as usize].deactivated = true;
        self.registry.set_status(reg, IdStatus::Deactivated);
        self.active.remove(reg);
        self.committee.depart(reg);
    }

    pub(crate) fn on_witness_long_absence(&mut self, _now: Micros, w: u32, token: u32) {
        let wit = &self.witnesses[w as usize];
        if wit.token != token || wit.removed() || self.hh[wit.household as usize].online() {
            return;
        }
        if wit.seat != NONE {
            self.remove_seat(w);
        }
    }

    // ------------------------------------------------------------------ allocation

    /// Inserts a witness by inside-out Fisher–Yates (SPEC §4.2) and updates the counters of
    /// the WCs whose membership changed.
    pub(crate) fn insert_seat(&mut self, w: u32, beacon: &Hash) {
        let id = self.witnesses[w as usize].id;
        let n = self.alloc.filled();
        let Some(j) = self.alloc.insert(id, beacon) else {
            return;
        };
        if j == n {
            let old = self.witness_status(w);
            self.seat_owner.push(w);
            self.witnesses[w as usize].seat = n as u32;
            self.commit_witness(w, old);
        } else {
            let displaced = self.seat_owner[j];
            let old_d = self.witness_status(displaced);
            self.seat_owner.push(displaced);
            self.witnesses[displaced as usize].seat = n as u32;
            self.commit_witness(displaced, old_d);
            let old = self.witness_status(w);
            self.seat_owner[j] = w;
            self.witnesses[w as usize].seat = j as u32;
            self.commit_witness(w, old);
        }
        let wcs = self.alloc.active_wcs() as usize;
        if self.kwc_load.len() < wcs {
            self.kwc_load.resize(wcs, 0);
        }
    }

    /// Removes a witness's seat by swap-with-last (SPEC §4.2).
    pub(crate) fn remove_seat(&mut self, w: u32) {
        let seat = self.witnesses[w as usize].seat;
        if seat == NONE {
            return;
        }
        let id = self.witnesses[w as usize].id;
        let last = self.alloc.filled() - 1;
        let old = self.witness_status(w);
        self.witnesses[w as usize].seat = NONE;
        self.commit_witness(w, old);
        if !self.alloc.remove(&id) {
            self.res.invariant_failures += 1;
            return;
        }
        if seat as usize != last {
            let moved = self.seat_owner[last];
            let old_m = self.witness_status(moved);
            self.witnesses[moved as usize].seat = seat;
            self.seat_owner[seat as usize] = moved;
            self.commit_witness(moved, old_m);
        }
        self.seat_owner.pop();
        // Miners in a KWC that no longer exists re-admit.
        let wcs = self.alloc.active_wcs() as usize;
        if self.kwc_load.len() > wcs {
            let gone: Vec<u32> = (0..self.miners.len() as u32)
                .filter(|m| {
                    let k = self.miners[*m as usize].kwc;
                    k != NONE && k as usize >= wcs
                })
                .collect();
            self.kwc_load.truncate(wcs);
            for m in gone {
                let old = self.miner_status(m);
                self.miners[m as usize].kwc = NONE;
                self.release_slot_recorded(self.q.now(), m, old);
                self.start_admission(self.q.now(), m);
            }
        }
    }

    /// Members of KWC `k` in seat order (leader WC first, Appendix A bitfield order).
    pub(crate) fn kwc_members(&self, k: u32) -> Vec<u32> {
        let wcs = self.alloc.active_wcs();
        let size = self.p.wc_size;
        kwc_wcs(u64::from(k), wcs, &self.p.offsets)
            .flat_map(|wc| {
                let start = wc as usize * size;
                self.seat_owner[start..start + size].to_vec()
            })
            .collect()
    }

    /// Counters of KWC `k` (the sum over its WCs).
    pub(crate) fn kwc_counts(&self, k: u32) -> WcCount {
        let wcs = self.alloc.active_wcs();
        let mut total = WcCount::default();
        if u64::from(k) >= wcs {
            return total;
        }
        for wc in kwc_wcs(u64::from(k), wcs, &self.p.offsets) {
            if let Some(c) = self.wc.get(wc as usize) {
                total.seated += c.seated;
                total.online += c.online;
                total.recorded += c.recorded;
                total.ghost += c.ghost;
            }
        }
        total
    }

    /// Registers the winner of a confirmed block as a new ID, to be placed at its beacon
    /// block (SPEC §4.2).
    pub(crate) fn register_winner(&mut self, now: Micros, block: u32) {
        let (miner, hash, height) = {
            let b = &self.blocks[block as usize];
            (b.miner, b.hash, b.height)
        };
        if miner == NONE {
            return;
        }
        let h = self.miners[miner as usize].household;
        let w = self.add_witness(h, hash, now);
        self.blocks[block as usize].registered = w;
        self.miners[miner as usize].ids += 1;
        self.placements
            .push((height + self.p.beacon_delay + self.p.confirmation_depth, w));
        // The winner is treated as offline once its block is confirmed (SPEC §3.10).
        let first = !self.miners[miner as usize].first_id;
        if self.miners[miner as usize].newcomer && first {
            self.res.first_ids += 1;
            // A newcomer that gave up while its block awaited confirmation got an ID after
            // all: it is a first ID, not a dropout.
            if self.miners[miner as usize].phase == Phase::Gone {
                self.res.dropouts -= 1;
            }
            let waited =
                (now - self.miners[miner as usize].joined_at) as f64 / (86_400.0 * SECOND as f64);
            if self.res.waits_days.len() < 50_000 {
                self.res.waits_days.push(waited);
            }
        }
        if first {
            self.miners[miner as usize].first_id_at = now;
        }
        self.miners[miner as usize].first_id = true;
        self.miners[miner as usize].key_epoch += 1;
        let readmit = self.cfg.sim.world.winners_readmit;
        if matches!(
            self.miners[miner as usize].phase,
            Phase::Waiting | Phase::Mining
        ) {
            self.release_slot(now, miner);
            if readmit && self.hh[h as usize].online() {
                self.start_admission(now, miner);
            }
        }
    }

    /// Places IDs whose beacon block is now on the network chain.
    pub(crate) fn place_pending(&mut self) {
        let height = self.network_height();
        let mut due: Vec<(u64, u32)> = Vec::new();
        self.placements.retain(|(h, w)| {
            if *h <= height {
                due.push((*h, *w));
                false
            } else {
                true
            }
        });
        due.sort();
        for (h, w) in due {
            if let Some(beacon) = self
                .network_block_at(h)
                .map(|b| self.blocks[b as usize].hash)
            {
                if !self.witnesses[w as usize].removed() {
                    self.insert_seat(w, &beacon);
                }
            }
        }
    }

    // ------------------------------------------------------------------ M3a.1 mechanisms

    /// Whether KWC `k` can issue entropy now: under the all-online-members rule at least the
    /// quorum of members recorded online and none of them unreachable (no stall); under a
    /// threshold, at least t of its seated members reachable.
    pub(crate) fn kwc_can_issue(&self, k: u32) -> bool {
        let c = self.kwc_counts(k);
        match self.threshold_rule() {
            Some(rule) => {
                let t = gb_protocol::entropy::entropy_threshold(c.seated as usize, rule)
                    .unwrap_or(c.seated as usize);
                t > 0 && c.online as usize >= t
            }
            None => c.recorded as usize >= self.p.quorum_unregistered && c.ghost == 0,
        }
    }

    /// Whether KWC `k` is below quorum (failover): fewer reachable members than the quorum of
    /// online members (all-online-members rule) or than t (threshold).
    pub(crate) fn kwc_below_quorum(&self, k: u32) -> bool {
        let c = self.kwc_counts(k);
        let needed = match self.threshold_rule() {
            Some(rule) => gb_protocol::entropy::entropy_threshold(c.seated as usize, rule)
                .unwrap_or(c.seated as usize)
                .max(1),
            None => self.p.quorum_unregistered,
        };
        (c.online as usize) < needed
    }

    /// Failover (the M3a.1 experiment E3): once a round, every KWC with miners is checked; one
    /// below quorum for `failover_after_rounds` rounds in a row sends its online miners to
    /// KWCs that can issue entropy, each by a self-service offline request and re-admission
    /// (SPEC §3.2, about 450 s).
    pub(crate) fn on_failover(&mut self, now: Micros) {
        let Some(after) = self.spec.failover_after_rounds else {
            return;
        };
        let kwcs = self.alloc.active_wcs() as usize;
        self.failover_below.resize(kwcs, 0);
        let mut moving = vec![false; kwcs];
        let mut any = false;
        for (k, moves) in moving.iter_mut().enumerate() {
            if self.kwc_load.get(k).copied().unwrap_or(0) == 0 {
                self.failover_below[k] = 0;
                continue;
            }
            if self.kwc_below_quorum(k as u32) {
                self.failover_below[k] += 1;
                if self.failover_below[k] >= after {
                    self.failover_below[k] = 0;
                    self.res.failover_triggers += 1;
                    *moves = true;
                    any = true;
                }
            } else {
                self.failover_below[k] = 0;
            }
        }
        if any {
            for m in 0..self.miners.len() as u32 {
                let miner = &self.miners[m as usize];
                let k = miner.kwc as usize;
                if miner.kwc != NONE
                    && k < kwcs
                    && moving[k]
                    && matches!(miner.phase, Phase::Mining | Phase::Waiting)
                    && self.hh[miner.household as usize].online()
                {
                    self.res.failover_moves += 1;
                    self.self_offline(now, m, 3);
                }
            }
        }
        self.q
            .push(now + micros(self.p.interval_s as f64), Ev::Failover);
    }

    /// Starts the points clock of an offline period of witness `w` (the M3a.1 E4 ladder).
    fn start_points_period(&mut self, now: Micros, w: u32, cause: u8) {
        let Some(after) = self.spec.offline_points.as_ref().map(|p| p.after_s) else {
            return;
        };
        let wit = &mut self.witnesses[w as usize];
        wit.down_cause = cause;
        let token = wit.token;
        self.q.push_lane(
            lane::POINTS_DUE,
            now + micros(after),
            Ev::PointsDue { w, token },
        );
    }

    /// An offline period of witness `w` reaches n: a point award is due. A KWC records it by a
    /// two-thirds decision; while none of its KWCs can decide, it waits, a round at a time,
    /// even if the member is back meanwhile.
    pub(crate) fn on_points_due(&mut self, now: Micros, w: u32, token: u32) {
        let wit = &self.witnesses[w as usize];
        if wit.token != token || wit.removed() {
            return;
        }
        let cause = wit.down_cause;
        if self.witness_vote_possible(w) {
            self.award_points(now, w, cause);
        } else {
            let first = self.witnesses[w as usize].points_owed.is_empty();
            self.witnesses[w as usize].points_owed.push(cause);
            if first {
                self.q.push_lane(
                    lane::POINTS_RECORD,
                    now + micros(self.p.interval_s as f64),
                    Ev::PointsRecord { w },
                );
            }
        }
    }

    /// Owed point awards of witness `w` are recorded once one of its KWCs can decide.
    pub(crate) fn on_points_record(&mut self, now: Micros, w: u32) {
        if self.witnesses[w as usize].removed() {
            self.witnesses[w as usize].points_owed.clear();
            return;
        }
        if !self.witness_vote_possible(w) {
            self.q.push_lane(
                lane::POINTS_RECORD,
                now + micros(self.p.interval_s as f64),
                Ev::PointsRecord { w },
            );
            return;
        }
        let owed = std::mem::take(&mut self.witnesses[w as usize].points_owed);
        for cause in owed {
            if self.witnesses[w as usize].removed() {
                break;
            }
            self.res.point_awards_delayed += 1;
            self.award_points(now, w, cause);
        }
    }

    /// Records one points award for witness `w`, and applies the outcome at the cap.
    fn award_points(&mut self, now: Micros, w: u32, cause: u8) {
        let Some(spec) = self.spec.offline_points.clone() else {
            return;
        };
        let now_s = now / SECOND;
        let reset = spec.reset_s as u64;
        let h = self.witnesses[w as usize].household as usize;
        let (region, device) = (self.hh[h].region as usize, self.hh[h].device as usize);
        let wit = &mut self.witnesses[w as usize];
        if wit.points.at(now_s, reset) == 0 {
            wit.points_loadshed_only = true;
        }
        if cause != cause_index(bit::LOADSHED) {
            wit.points_loadshed_only = false;
        }
        let total = wit.points.award(now_s, &spec.steps, spec.cap, reset);
        let loadshed_only = wit.points_loadshed_only;
        self.res.point_awards[region][device] += 1;
        self.res.point_awards_by_cause[cause as usize] += 1;
        if total < spec.cap {
            return;
        }
        self.res.points_capped[region][device] += 1;
        if loadshed_only {
            self.res.points_capped_loadshed_only[region][device] += 1;
        }
        self.witnesses[w as usize].points.clear();
        match spec.outcome {
            OfflinePointsOutcome::SeatLoss => {
                let until = now + micros(spec.seat_cooldown_s);
                self.witnesses[w as usize].seat_cooldown_until = until;
                self.placements.retain(|(_, x)| *x != w);
                if self.witnesses[w as usize].seat != NONE {
                    self.res.seats_lost[region][device] += 1;
                    self.remove_seat(w);
                }
                self.q
                    .push_lane(lane::SEAT_COOLDOWN, until, Ev::SeatCooldown { w });
            }
            OfflinePointsOutcome::Blacklist => {
                self.res.ids_blacklisted[region][device] += 1;
                self.remove_banned(w);
            }
        }
    }

    /// A seat-loss cooldown ends: a reachable member may take a seat again.
    pub(crate) fn on_seat_cooldown(&mut self, now: Micros, w: u32) {
        let wit = &self.witnesses[w as usize];
        if wit.removed() || wit.seat != NONE || now < wit.seat_cooldown_until {
            return;
        }
        if self.witness_reachable(w) && !self.placements.iter().any(|(_, x)| *x == w) {
            let target = self.network_height() + self.p.confirmation_depth + self.p.beacon_delay;
            self.placements.push((target, w));
        }
    }

    /// A scripted regional blackout (E3) starts or ends: every household of the region goes
    /// offline, as in a random regional blackout (no UPS covers it).
    pub(crate) fn on_scripted_blackout(&mut self, now: Micros, index: u32, start: bool) {
        let (region, _, seconds_off) = self.spec.scripted_blackouts[index as usize];
        let Some(&p) = self.shared.blackout_of.get(region) else {
            return;
        };
        if p == NONE {
            return;
        }
        let members = self.shared.processes[p as usize].members.clone();
        if start {
            let ends = now + micros(seconds_off);
            let mut miners = 0u64;
            for h in &members {
                self.set_cause(now, *h, bit::SCRIPTED_BLACKOUT, true, false, ends);
                if let Some(m) = self.hh[*h as usize].miner {
                    if matches!(
                        self.miners[m as usize].phase,
                        Phase::Mining | Phase::Waiting
                    ) {
                        miners += 1;
                    }
                }
            }
            self.q.push(
                ends,
                Ev::ScriptedBlackout {
                    index,
                    start: false,
                },
            );
            if self.spec.track_outages {
                let r =
                    self.start_outage_record(now, 2, region, seconds_off, members.len(), miners);
                self.outage_of_script[index as usize] = r;
            }
        } else {
            for h in members {
                self.set_cause(now, h, bit::SCRIPTED_BLACKOUT, false, false, now);
            }
            if let Some(r) = self.outage_of_script.get_mut(index as usize) {
                *r = NONE;
            }
        }
    }

    /// Opens an outage record (E3) and schedules its first sample a round in.
    fn start_outage_record(
        &mut self,
        now: Micros,
        kind: u8,
        region: usize,
        duration_s: f64,
        households: usize,
        miners_lost_power: u64,
    ) -> u32 {
        let index = self.res.outages.len() as u32;
        let mut record = crate::job::OutageRecord {
            kind,
            region: region as u8,
            start_s: seconds(now),
            duration_s,
            households: households as u64,
            miners_lost_power,
            ..crate::job::OutageRecord::default()
        };
        if kind == 2 {
            // The region's shares, the KWCs and the spare capacity at the start (E3 diagnosis).
            let seated: Vec<u32> = (0..self.witnesses.len() as u32)
                .filter(|w| !self.witnesses[*w as usize].removed())
                .collect();
            let in_region = |hh: &Household| usize::from(hh.region) == region;
            let witnesses_here = seated
                .iter()
                .filter(|w| in_region(&self.hh[self.witnesses[**w as usize].household as usize]))
                .count();
            let admitted: Vec<u32> = (0..self.miners.len() as u32)
                .filter(|m| {
                    matches!(
                        self.miners[*m as usize].phase,
                        Phase::Mining | Phase::Waiting
                    )
                })
                .collect();
            let miners_here = admitted
                .iter()
                .filter(|m| in_region(&self.hh[self.miners[**m as usize].household as usize]))
                .count();
            record.region_witness_share = witnesses_here as f64 / seated.len().max(1) as f64;
            record.region_miner_share = miners_here as f64 / admitted.len().max(1) as f64;
            record.kwcs = self.alloc.active_wcs();
            let (_, spare) = self.kwcs_unable_and_spare();
            record.spare_capacity_start = spare;
            record.spare_capacity_min = spare;
        }
        self.res.outages.push(record);
        self.outage_diag.push(OutageDiag {
            samples: Vec::new(),
            skip_samples: Vec::new(),
            moves_before: self.res.failover_moves,
        });
        self.q.push(
            now + micros(self.p.interval_s as f64),
            Ev::OutageSample { index },
        );
        index
    }

    /// Samples a tracked outage's effect on mining: miners mining with their household online
    /// whose KWC cannot issue entropy. Every round while the outage lasts.
    pub(crate) fn on_outage_sample(&mut self, now: Micros, index: u32) {
        let (start, duration, kind) = {
            let r = &self.res.outages[index as usize];
            (r.start_s, r.duration_s, r.kind)
        };
        // A scripted blackout's diagnosis samples on past its end.
        let tail = if kind == 2 { DIAGNOSIS_TAIL_S } else { 0.0 };
        if seconds(now) > start + duration + tail {
            return;
        }
        let within = seconds(now) <= start + duration;
        let kwcs = self.alloc.active_wcs() as usize;
        let mut able: Vec<Option<bool>> = vec![None; kwcs];
        let mut unable_kwcs = 0u64;
        let mut lost = 0u64;
        for m in 0..self.miners.len() {
            let miner = &self.miners[m];
            if miner.phase != Phase::Mining
                || miner.idle
                || miner.kwc == NONE
                || miner.kwc as usize >= kwcs
                || !self.hh[miner.household as usize].online()
            {
                continue;
            }
            let k = miner.kwc as usize;
            let can = match able[k] {
                Some(x) => x,
                None => {
                    let x = self.kwc_can_issue(k as u32);
                    able[k] = Some(x);
                    if !x {
                        unable_kwcs += 1;
                    }
                    x
                }
            };
            if !can {
                lost += 1;
            }
        }
        if within {
            let r = &mut self.res.outages[index as usize];
            r.samples += 1;
            r.witnessing_lost_max = r.witnessing_lost_max.max(lost);
            r.witnessing_lost_miner_rounds += lost;
            r.kwcs_unable_max = r.kwcs_unable_max.max(unable_kwcs);
        }
        if kind == 2 {
            let mining = (0..self.miners.len())
                .filter(|m| {
                    let miner = &self.miners[*m];
                    miner.phase == Phase::Mining
                        && !miner.idle
                        && miner.kwc != NONE
                        && (miner.kwc as usize) < kwcs
                        && self.hh[miner.household as usize].online()
                })
                .count() as u64;
            let p = step_win_probability(&self.blocks[self.network_tip as usize].child_target);
            let (below, spare) = self.kwcs_unable_and_spare();
            let moves = self.res.failover_moves;
            let skips = self.spec.trace_blocks.then(|| self.tip_round_skips(now));
            if let Some(d) = self.outage_diag.get_mut(index as usize) {
                d.samples
                    .push((now, mining as u32, mining.saturating_sub(lost) as u32, p));
                if let Some(s) = skips {
                    d.skip_samples.push(s);
                }
                let before = d.moves_before;
                let r = &mut self.res.outages[index as usize];
                r.failover_moves = moves.saturating_sub(before);
                if within {
                    r.kwcs_below_t_max = r.kwcs_below_t_max.max(below);
                    r.spare_capacity_min = r.spare_capacity_min.min(spare);
                }
            }
        }
        let next = now + micros(self.p.interval_s as f64);
        if seconds(next) <= start + duration + tail {
            self.q.push(next, Ev::OutageSample { index });
        }
    }

    /// One daily-curve sample (E2, `availability_curve`): the shares of miners mining (admitted,
    /// past their wait, not idle, household online), of seated witnesses reachable, of KWCs
    /// able to issue entropy and of miners able to get it, added to its UTC hour.
    pub(crate) fn sample_availability(&mut self, now: Micros) {
        let kwcs = self.alloc.active_wcs() as u32;
        let able: Vec<bool> = (0..kwcs).map(|k| self.kwc_can_issue(k)).collect();
        let (mut mining, mut with_entropy) = (0usize, 0usize);
        for m in &self.miners {
            if m.phase == Phase::Mining
                && !m.idle
                && m.kwc != NONE
                && m.mine_at <= now
                && self.hh[m.household as usize].online()
            {
                mining += 1;
                if able.get(m.kwc as usize).copied().unwrap_or(false) {
                    with_entropy += 1;
                }
            }
        }
        let reachable = (0..self.witnesses.len() as u32)
            .filter(|w| !self.witnesses[*w as usize].removed() && self.witness_reachable(*w))
            .count();
        let n = self.miners.len().max(1) as f64;
        let hour = ((now / (3_600 * SECOND)) % 24) as usize;
        if self.res.availability_by_hour.len() < 24 {
            self.res.availability_by_hour = vec![[0.0; 5]; 24];
        }
        let row = &mut self.res.availability_by_hour[hour];
        row[0] += 1.0;
        row[1] += mining as f64 / n;
        row[2] += reachable as f64 / self.witnesses.len().max(1) as f64;
        row[3] += able.iter().filter(|x| **x).count() as f64 / f64::from(kwcs.max(1));
        row[4] += with_entropy as f64 / n;
    }

    /// Whether miner `m` could mine at `now` (W2): admitted and past its wait, not idle, with a
    /// KWC that can issue entropy and its household online.
    pub(crate) fn can_mine_now(&self, m: u32, now: Micros) -> bool {
        let miner = &self.miners[m as usize];
        matches!(miner.phase, Phase::Mining | Phase::Waiting)
            && miner.mine_at <= now
            && !miner.idle
            && miner.kwc != NONE
            && miner.kwc < self.alloc.active_wcs() as u32
            && self.hh[miner.household as usize].online()
            && self.kwc_can_issue(miner.kwc)
    }

    /// The entropy state the W2 trace records with each block: active KWCs, those able to
    /// issue entropy, miners mining with their household online, and those of them whose KWC
    /// can issue (as the E3 diagnosis counts them).
    pub(crate) fn entropy_state(&self) -> [u32; 4] {
        let kwcs = self.alloc.active_wcs() as usize;
        let able: Vec<bool> = (0..kwcs as u32).map(|k| self.kwc_can_issue(k)).collect();
        let (mut active, mut with_entropy) = (0u32, 0u32);
        for miner in &self.miners {
            if miner.phase != Phase::Mining
                || miner.idle
                || miner.kwc == NONE
                || miner.kwc as usize >= kwcs
                || !self.hh[miner.household as usize].online()
            {
                continue;
            }
            active += 1;
            if able[miner.kwc as usize] {
                with_entropy += 1;
            }
        }
        let able_kwcs = able.iter().filter(|x| **x).count() as u32;
        [kwcs as u32, able_kwcs, active, with_entropy]
    }

    /// KWCs that cannot issue entropy, and the free miner places in those that can (the E3
    /// diagnosis).
    pub(crate) fn kwcs_unable_and_spare(&self) -> (u64, u64) {
        let kwcs = self.alloc.active_wcs() as u32;
        let mut unable = 0u64;
        let mut spare = 0u64;
        for k in 0..kwcs {
            if self.kwc_can_issue(k) {
                let load = self.kwc_load.get(k as usize).copied().unwrap_or(0);
                spare += u64::from(self.p.capacity.saturating_sub(load));
            } else {
                unable += 1;
            }
        }
        (unable, spare)
    }

    /// Removes a witness's ID for good without a One Chance ban record (points blacklist).
    fn remove_banned(&mut self, w: u32) {
        let old = self.witness_status(w);
        let reg = self.witnesses[w as usize].reg as usize;
        self.witnesses[w as usize].banned = true;
        self.commit_witness(w, old);
        self.registry.set_status(reg, IdStatus::Banned);
        self.active.remove(reg);
        self.committee.depart(reg);
        self.remove_seat(w);
        self.placements.retain(|(_, x)| *x != w);
    }

    // ------------------------------------------------------------------ invariants

    /// Checks the incremental caches against a recount from scratch (V0 invariants): the
    /// active list against `Registry::canonical_active_list`, the seat table against the
    /// allocation, the WC counters, the miner counters and the KWC loads. Each invariant that
    /// fails adds one to `invariant_failures`.
    pub(crate) fn check_invariants(&mut self) {
        let mut failures = 0u64;
        let cached: Vec<usize> = self.active.iter().collect();
        failures += u64::from(cached != self.registry.canonical_active_list());
        // Seats: each filled seat's owner holds that seat and that ID, and no other witness
        // holds a seat (together a bijection between seated witnesses and the allocation).
        let filled = self.alloc.filled();
        let seated = self.witnesses.iter().filter(|w| w.seat != NONE).count();
        let seats_agree = self.seat_owner.len() == filled
            && seated == filled
            && self.seat_owner.iter().enumerate().all(|(s, w)| {
                let wit = &self.witnesses[*w as usize];
                wit.seat as usize == s && self.alloc.id_at(s) == Some(&wit.id)
            });
        failures += u64::from(!seats_agree);
        let mut wc: Vec<WcCount> = Vec::new();
        for w in 0..self.witnesses.len() as u32 {
            let s = self.witness_status(w);
            if let Some(i) = s.wc {
                if wc.len() <= i {
                    wc.resize(i + 1, WcCount::default());
                }
                let c = &mut wc[i];
                c.seated += 1;
                c.online += u32::from(s.online);
                c.recorded += u32::from(s.recorded);
                c.ghost += u32::from(s.ghost);
            }
        }
        let mut cached_wc = self.wc.clone();
        let len = wc.len().max(cached_wc.len());
        wc.resize(len, WcCount::default());
        cached_wc.resize(len, WcCount::default());
        failures += u64::from(wc != cached_wc);
        let mut counts = Counts {
            mining_group: vec![0; self.counts.mining_group.len()],
            mining_region: vec![0; self.counts.mining_region.len()],
            ..Counts::default()
        };
        for m in 0..self.miners.len() as u32 {
            let s = self.miner_status(m);
            let miner = &self.miners[m as usize];
            let region = self.hh[miner.household as usize].region as usize;
            counts.after_wait += u64::from(s.after_wait);
            counts.all += u64::from(s.all);
            counts.mining_group[miner.group as usize] += u64::from(s.mining);
            counts.mining_region[region] += u64::from(s.mining);
            counts.hop_mining += u64::from(s.hop);
        }
        failures += u64::from(
            counts.after_wait != self.counts.after_wait
                || counts.all != self.counts.all
                || counts.mining_group != self.counts.mining_group
                || counts.mining_region != self.counts.mining_region
                || counts.hop_mining != self.counts.hop_mining,
        );
        let mut load = vec![0u32; self.kwc_load.len()];
        let mut stray = false;
        for miner in &self.miners {
            if miner.kwc != NONE {
                match load.get_mut(miner.kwc as usize) {
                    Some(l) => *l += 1,
                    None => stray = true,
                }
            }
        }
        failures += u64::from(stray || load != self.kwc_load);
        self.res.invariant_failures += failures;
    }

    // ------------------------------------------------------------------ committee, samples, growth

    pub(crate) fn on_committee(&mut self, now: Micros) {
        self.committee_ticks += 1;
        let beacon = tagged(
            b"GB/sim-powtx",
            &[
                &self.spec.seed.to_be_bytes(),
                &self.committee_ticks.to_be_bytes(),
            ],
        );
        let active = &self.active;
        self.committee
            .refresh_with(active.len(), |i| active.nth(i), &beacon);
        self.q
            .push(now + self.p.pow_tx * self.p.cac_every, Ev::Committee);
    }

    pub(crate) fn on_sample(&mut self, now: Micros) {
        let wcs = self.alloc.active_wcs();
        if wcs > 0 {
            let n = self.spec.kwc_sample_size.max(1);
            let (mut seated, mut online) = (0u64, 0u64);
            for c in self.wc.iter().take(wcs as usize) {
                seated += u64::from(c.seated);
                online += u64::from(c.online);
            }
            let day = self.day_index(now);
            if self.res.daily_kwc.len() <= day {
                self.res.daily_kwc.resize(day + 1, [0; 3]);
            }
            let tick = now / SECOND;
            for i in 0..n {
                let w = self.keyed.words(Purpose::Sampling, tick, u64::from(i))[0];
                let k = household::pick(w, wcs as usize) as u32;
                let members = self.kwc_members(k);
                let on: Vec<bool> = members
                    .iter()
                    .map(|w| !self.witnesses[*w as usize].removed() && self.witness_reachable(*w))
                    .collect();
                let unreg_fail = !self.p.unregistered.is_met(&bitfield_of(&on));
                let reg_fail = !self.p.registered.is_met(&bitfield_of(&on));
                self.res.kwc_samples += 1;
                self.res.kwc_fail_unregistered += u64::from(unreg_fail);
                self.res.kwc_fail_registered += u64::from(reg_fail);
                self.res.daily_kwc[day][0] += u64::from(unreg_fail);
                self.res.daily_kwc[day][1] += u64::from(reg_fail);
                self.res.daily_kwc[day][2] += 1;
            }
            if seated > 0 {
                self.res.online_fraction_sum += online as f64 / seated as f64;
            }
        }
        self.q
            .push(now + micros(self.spec.kwc_sample_interval_s), Ev::Sample);
    }

    fn schedule_arrival(&mut self, now: Micros) {
        let rate = self.spec.arrivals_multiple_of_r * self.p.seconds_per_year
            / self.p.interval_s as f64
            / self.p.seconds_per_year;
        let n = self.res.arrivals;
        let u = self.keyed.uniforms(Purpose::Growth, 0, n);
        self.q
            .push(now + micros(exponential(1.0 - u[0], rate)), Ev::Arrival);
    }

    pub(crate) fn on_arrival(&mut self, now: Micros) {
        self.res.arrivals += 1;
        let u = self.keyed.uniforms(Purpose::Growth, 1, self.res.arrivals);
        let region = pick_weighted(&self.cfg.sim.network.region_shares, u[0]);
        let m = self.add_miner(region, 0, false, true, now);
        let h = self.miners[m as usize].household;
        self.join_shared(h);
        self.schedule_household(h);
        self.start_admission(now, m);
        self.schedule_arrival(now);
        // Patience: the newcomer gives up if it has no ID by then.
        let until = self.miners[m as usize].patience_until;
        if until != u64::MAX {
            self.q.push(until, Ev::Patience { m });
        }
    }

    pub(crate) fn on_patience(&mut self, now: Micros, m: u32) {
        if self.miners[m as usize].first_id || self.miners[m as usize].phase == Phase::Gone {
            return;
        }
        self.res.dropouts += 1;
        let old = self.miner_status(m);
        let k = self.miners[m as usize].kwc;
        if k != NONE && (k as usize) < self.kwc_load.len() {
            self.kwc_load[k as usize] = self.kwc_load[k as usize].saturating_sub(1);
        }
        let miner = &mut self.miners[m as usize];
        miner.phase = Phase::Gone;
        miner.recorded = false;
        miner.ghost = false;
        miner.kwc = NONE;
        miner.token = miner.token.wrapping_add(1);
        self.commit_miner(now, m, old);
    }

    pub(crate) fn on_scripted_crash(&mut self, now: Micros, index: u32) {
        let Some((_, seconds)) = self.spec.scripted_crashes.get(index as usize).copied() else {
            return;
        };
        // A seated, recorded-online witness in an always-online household goes down
        // unannounced for `seconds`.
        let w = (0..self.witnesses.len() as u32).find(|w| {
            let wit = &self.witnesses[*w as usize];
            wit.seat != NONE
                && wit.recorded
                && !wit.ghost
                && self.hh[wit.household as usize].online()
        });
        if let Some(w) = w {
            let h = self.witnesses[w as usize].household;
            let ends = now + micros(seconds);
            self.set_cause(now, h, bit::SCRIPT, true, false, ends);
            self.q.push(ends, Ev::ScriptEnd { h });
        }
        let next = index + 1;
        if let Some((day, _)) = self.spec.scripted_crashes.get(next as usize) {
            self.q
                .push(micros(day * 86_400.0), Ev::ScriptedCrash { index: next });
        }
    }
}

/// A bitfield over a KWC's seats from per-seat flags.
pub(crate) fn bitfield_of(on: &[bool]) -> gb_protocol::entropy::Bitfield {
    let set: Vec<usize> = on
        .iter()
        .enumerate()
        .filter(|(_, x)| **x)
        .map(|(i, _)| i)
        .collect();
    gb_protocol::entropy::Bitfield::from_seats(on.len(), &set)
}

/// An index drawn by weights from a uniform.
pub(crate) fn pick_weighted(weights: &[f64], u: f64) -> usize {
    let total: f64 = weights.iter().sum();
    let mut acc = 0.0;
    for (i, w) in weights.iter().enumerate() {
        acc += w / total;
        if u < acc {
            return i;
        }
    }
    weights.len().saturating_sub(1)
}

fn next_loadshed(now: Micros, blocks: &[(f64, f64)]) -> Option<Micros> {
    if blocks.is_empty() {
        return None;
    }
    let day = 86_400 * SECOND;
    let start_of_day = now - now % day;
    let mut best = u64::MAX;
    for d in 0..2u64 {
        for (start, _) in blocks {
            let t = start_of_day + d * day + micros(*start);
            if t > now && t < best {
                best = t;
            }
        }
    }
    (best != u64::MAX).then_some(best)
}

fn current_block_length(now: Micros, blocks: &[(f64, f64)]) -> f64 {
    let day = 86_400 * SECOND;
    let in_day = (now % day) as f64 / SECOND as f64;
    blocks
        .iter()
        .min_by(|a, b| (a.0 - in_day).abs().total_cmp(&(b.0 - in_day).abs()))
        .map_or(0.0, |b| b.1)
}

fn placeholder_household() -> Household {
    Household {
        region: 0,
        device: Device::AlwaysOn,
        grid_group: 0,
        isp: 0,
        ups: false,
        skew: 0.0,
        access: 0.0,
        mask: 0,
        announced: 0,
        ends: [0; bit::COUNT],
        next: [u64::MAX; 6],
        draws: [0; 6],
        wake_gen: 0,
        miner: None,
        witnesses: Vec::new(),
        always_online: true,
        independent: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::JobSpec;

    #[test]
    fn a_round_keeps_its_kwc_members_when_a_placement_changes_the_kwc() {
        let cfg = gb_config::Config::default();
        let mut spec = JobSpec::from_config(&cfg, "round-members", "T", 3).idealised();
        spec.miners = 4;
        spec.registered_ids = 400;
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start_chain();
        let before = sim.round_members(0, 0);
        assert_eq!(before.len(), 40);
        // Place 40 new IDs: KWC 0 changes, but round 0 keeps the list it first used.
        for i in 0..40u64 {
            let h = sim.miners[0].household;
            let id = tagged(b"test-id", &[&i.to_be_bytes()]);
            let w = sim.add_witness(h, id, 0);
            sim.insert_seat(w, &tagged(b"test-beacon", &[&i.to_be_bytes()]));
        }
        assert_ne!(sim.kwc_members(0), before);
        assert_eq!(sim.round_members(0, 0), before);
        // The next round reads the allocation afresh.
        sim.open_group(0, 0);
        assert_eq!(sim.round_members(1, 0), sim.kwc_members(0));
    }

    fn small_sim(cfg: &gb_config::Config) -> JobSpec {
        let mut spec = JobSpec::from_config(cfg, "world-test", "T", 5).idealised();
        spec.miners = 4;
        spec.registered_ids = 400;
        spec
    }

    #[test]
    fn a_household_glitch_during_a_switch_does_not_cut_the_epoch_wait() {
        let cfg = gb_config::Config::default();
        let mut spec = small_sim(&cfg);
        spec.instant_admission = false;
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        assert_eq!(sim.miners[0].phase, Phase::Mining);
        let t = 1_000 * SECOND;
        sim.self_offline(t, 0, 2);
        let rejoin = sim.miners[0].rejoin_at;
        assert_eq!(rejoin, t + sim.p.grace + micros(0.5));
        let token = sim.miners[0].token;
        // The household drops out and comes back within the epoch: no admission yet.
        let h = sim.miners[0].household;
        let (down, up) = (t + 30 * SECOND, t + 60 * SECOND);
        sim.set_cause(down, h, bit::GLITCH, true, false, up);
        sim.set_cause(up, h, bit::GLITCH, false, false, up);
        assert_eq!(sim.miners[0].phase, Phase::Idle);
        assert_eq!(sim.miners[0].token, token);
        // The rejoin at the end of the epoch starts it.
        sim.on_miner_rejoin(rejoin, 0, token);
        assert_eq!(sim.miners[0].phase, Phase::Admitting);
    }

    #[test]
    fn a_newcomer_that_gave_up_before_its_block_confirmed_counts_once() {
        let cfg = gb_config::Config::default();
        let spec = small_sim(&cfg);
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let m = sim.add_miner(0, 0, true, true, 0);
        let mut block = sim.blocks[0].clone();
        block.parent = 0;
        block.height = 1;
        block.miner = m;
        block.hash = tagged(b"test-block", &[]);
        sim.blocks.push(block);
        let b = (sim.blocks.len() - 1) as u32;
        // Patience runs out while the block awaits confirmation...
        sim.on_patience(SECOND, m);
        assert_eq!((sim.res.first_ids, sim.res.dropouts), (0, 1));
        // ...and the block confirms: a first ID, not a dropout.
        sim.register_winner(2 * SECOND, b);
        assert_eq!((sim.res.first_ids, sim.res.dropouts), (1, 0));
        assert!(sim.miners[m as usize].first_id);
    }

    #[test]
    fn tagged_groups_take_each_regions_miners_in_proportion_when_spread_by_region() {
        // SIM-51: regions and groups were spread with the same shuffle, so a 10 % group sat
        // in one region; with `groups_by_region` every region holds its share of each group.
        let cfg = gb_config::Config::default();
        let mut spec = small_sim(&cfg);
        spec.miners = 1_000;
        spec.groups = [0.1, 0.5]
            .iter()
            .map(|share| crate::job::GroupSpec {
                label: format!("x={share}"),
                share: *share,
                strategy: Strategy::Honest,
                always_online: false,
            })
            .collect();
        let count = |sim: &Sim<'_>, region: usize, group: u8| {
            sim.miners
                .iter()
                .filter(|m| {
                    m.group == group && sim.hh[m.household as usize].region as usize == region
                })
                .count()
        };
        let regions = cfg.sim.network.regions.len();
        let old = Sim::build(&spec, &cfg).unwrap();
        let holding_x01 = (0..regions).filter(|r| count(&old, *r, 1) > 0).count();
        assert!(
            holding_x01 <= 2,
            "the old placement put x=0.1 in {holding_x01} regions"
        );
        let mut by_region = spec.clone();
        by_region.groups_by_region = true;
        let sim = Sim::build(&by_region, &cfg).unwrap();
        for region in 0..regions {
            let n: usize = (0..3).map(|g| count(&sim, region, g)).sum();
            assert_eq!(
                n,
                count(&old, region, 0) + count(&old, region, 1) + count(&old, region, 2)
            );
            for (g, share) in [(1u8, 0.1), (2, 0.5)] {
                let c = count(&sim, region, g) as f64;
                assert!(
                    (c - share * n as f64).abs() <= 1.0,
                    "region {region} group {g}: {c} of {n}"
                );
            }
        }
    }

    #[test]
    fn groups_in_one_region_put_every_tagged_slot_there_and_spread_the_rest() {
        // W1: the withholder in one small region; the honest slots keep the regions' shares.
        let cfg = gb_config::Config::default();
        let mut spec = small_sim(&cfg);
        spec.miners = 1_000;
        spec.groups = vec![crate::job::GroupSpec {
            label: "withholder".to_string(),
            share: 0.1,
            strategy: Strategy::Withhold,
            always_online: false,
        }];
        let home = cfg.sim.network.regions.len() - 1;
        spec.groups_in_region = Some(home as u8);
        let sim = Sim::build(&spec, &cfg).unwrap();
        let region_of = |m: &Miner| sim.hh[m.household as usize].region as usize;
        let tagged: Vec<&Miner> = sim.miners.iter().filter(|m| m.group == 1).collect();
        assert_eq!(tagged.len(), 100);
        assert!(tagged.iter().all(|m| region_of(m) == home));
        let untagged: Vec<&Miner> = sim.miners.iter().filter(|m| m.group == 0).collect();
        for (region, share) in cfg.sim.network.region_shares.iter().enumerate() {
            let c = untagged.iter().filter(|m| region_of(m) == region).count() as f64;
            assert!((c - share * 900.0).abs() <= 1.0, "region {region}: {c}");
        }
    }

    #[test]
    fn households_spread_over_every_feeder_group_and_provider_of_their_region() {
        // SIM-12–SIM-14: a feeder fault or provider outage takes one group of a region
        // offline, not the whole region.
        let cfg = gb_config::Config::default();
        let mut spec = small_sim(&cfg);
        spec.idealised = false;
        spec.outages = true;
        spec.registered_ids = 20_000;
        let sim = Sim::build(&spec, &cfg).unwrap();
        let feeders = cfg.sim.households.reliable_grid.feeder_groups as usize;
        let isps = cfg.sim.households.isps_per_region as usize;
        for region in 0..cfg.sim.network.regions.len() {
            let homes: Vec<&Household> = sim
                .hh
                .iter()
                .filter(|h| h.region as usize == region)
                .collect();
            if homes.len() < 1_000 {
                continue;
            }
            let groups: std::collections::BTreeSet<u32> =
                homes.iter().map(|h| h.grid_group).collect();
            let providers: std::collections::BTreeSet<u32> = homes.iter().map(|h| h.isp).collect();
            assert_eq!(groups.len(), feeders, "region {region}");
            assert_eq!(providers.len(), isps, "region {region}");
            // No feeder group holds more than twice its fair share.
            for g in groups {
                let n = homes.iter().filter(|h| h.grid_group == g).count();
                assert!(
                    n * feeders < 2 * homes.len(),
                    "region {region} group {g}: {n}"
                );
            }
        }
    }

    #[test]
    fn a_partition_cuts_its_witnesses_off_and_they_go_through_one_chance() {
        let cfg = gb_config::Config::default();
        let mut spec = small_sim(&cfg);
        spec.idealised = false;
        spec.outages = false;
        spec.partitions = vec![(0, 0.01, 300.0)];
        spec.partition_cuts_witnesses = true;
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let in_region: Vec<u32> = (0..sim.witnesses.len() as u32)
            .filter(|w| sim.hh[sim.witnesses[*w as usize].household as usize].region == 0)
            .collect();
        assert!(!in_region.is_empty());
        let part = sim.net.partitions[0];
        sim.on_partition(part.start, 0, true);
        for w in &in_region {
            let wit = &sim.witnesses[*w as usize];
            assert!(wit.cut_off && wit.ghost && !sim.witness_reachable(*w));
            assert_eq!(wit.cut_until, part.end);
        }
        assert_eq!(sim.res.partition_cuts, in_region.len() as u64);
        // Household crash and glitch counters are for household outages only.
        assert_eq!(sim.res.witness_crashes, 0);
        sim.on_partition(part.end, 0, false);
        for w in &in_region {
            assert!(sim.witness_reachable(*w) && !sim.witnesses[*w as usize].cut_off);
        }
        sim.check_invariants();
        assert_eq!(sim.res.invariant_failures, 0);
    }

    #[test]
    fn an_announced_shutdown_that_returns_within_the_epoch_re_admits_at_once() {
        let cfg = gb_config::Config::default();
        let mut spec = small_sim(&cfg);
        spec.instant_admission = false;
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let h = sim.miners[0].household;
        let t = 1_000 * SECOND;
        sim.set_cause(t, h, bit::USER, true, true, t + 60 * SECOND);
        assert_eq!(sim.miners[0].phase, Phase::Idle);
        sim.set_cause(t + 60 * SECOND, h, bit::USER, false, false, t + 60 * SECOND);
        assert_eq!(sim.miners[0].phase, Phase::Admitting);
        assert_eq!(sim.res.quick_readmits, 1);
    }

    #[test]
    fn failover_moves_miners_off_a_kwc_below_quorum_after_two_rounds() {
        let cfg = gb_config::Config::default();
        let mut spec = small_sim(&cfg);
        spec.instant_admission = false;
        spec.failover_after_rounds = Some(2);
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let k = sim.miners[0].kwc;
        // 14 of its 40 members go offline: 26 online, below the quorum of 27.
        let t = 100 * SECOND;
        for w in sim.kwc_members(k).into_iter().take(14) {
            let h = sim.witnesses[w as usize].household;
            sim.set_cause(t, h, bit::CRASH, true, false, u64::MAX);
        }
        assert!(sim.kwc_below_quorum(k));
        // One round below quorum moves nobody; the second moves its miners.
        sim.on_failover(t);
        assert_eq!((sim.miners[0].kwc, sim.res.failover_moves), (k, 0));
        sim.on_failover(t + 30 * SECOND);
        assert_eq!(sim.miners[0].phase, Phase::Idle);
        assert!(sim.res.failover_moves >= 1 && sim.res.failover_triggers >= 1);
        // It re-admits to a KWC that can issue entropy and mines again about 450 s later.
        while let Some((now, ev)) = sim.q.pop() {
            if now > t + 3_600 * SECOND || !sim.res.failover_s.is_empty() {
                break;
            }
            sim.handle(now, ev);
        }
        let took = sim.res.failover_s.first().copied().unwrap_or(0.0);
        assert!((440.0..470.0).contains(&took), "{took}");
        assert_ne!(sim.miners[0].kwc, k);
        assert!(!sim.kwc_below_quorum(sim.miners[0].kwc));
    }

    fn points_spec() -> crate::job::OfflinePointsSpec {
        crate::job::OfflinePointsSpec {
            after_s: 3_600.0,
            steps: vec![1, 1, 2, 3, 4, 5],
            cap: 20,
            reset_s: 31_536_000.0,
            outcome: OfflinePointsOutcome::SeatLoss,
            seat_cooldown_s: 2_592_000.0,
        }
    }

    #[test]
    fn offline_points_need_a_kwc_decision_and_the_cap_costs_the_seat_for_a_cooldown() {
        let cfg = gb_config::Config::default();
        let mut spec = small_sim(&cfg);
        spec.offline_points = Some(points_spec());
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let w = 0u32;
        assert_ne!(sim.witnesses[0].seat, NONE);
        let h = sim.witnesses[0].household;
        let (hour, day) = (3_600 * SECOND, 86_400 * SECOND);
        // Six offline periods of two hours (n = 1 h): 1, 1, 2, 3, 4 and 5 points.
        for i in 1..=6u64 {
            let t = i * day;
            sim.set_cause(t, h, bit::USER, true, true, t + 2 * hour);
            let token = sim.witnesses[0].token;
            sim.on_points_due(t + hour, w, token);
            sim.set_cause(t + 2 * hour, h, bit::USER, false, false, t + 2 * hour);
        }
        assert_eq!(sim.witnesses[0].points.total, 16);
        // A period shorter than n earns nothing: the member is back when it would be due.
        let t = 7 * day;
        sim.set_cause(t, h, bit::USER, true, true, t + hour / 2);
        let token = sim.witnesses[0].token;
        sim.set_cause(t + hour / 2, h, bit::USER, false, false, t + hour / 2);
        sim.on_points_due(t + hour, w, token);
        assert_eq!(sim.witnesses[0].points.total, 16);
        // Every other witness away: no KWC can decide, so the next award waits.
        let t = 8 * day;
        let others: Vec<u32> = (1..sim.witnesses.len() as u32)
            .map(|x| sim.witnesses[x as usize].household)
            .filter(|x| *x != h)
            .collect();
        for x in &others {
            sim.set_cause(t, *x, bit::USER, true, true, t + 3 * hour);
        }
        sim.set_cause(t, h, bit::USER, true, true, t + 2 * hour);
        let token = sim.witnesses[0].token;
        sim.on_points_due(t + hour, w, token);
        assert_eq!(sim.witnesses[0].points_owed.len(), 1);
        assert_eq!(sim.witnesses[0].points.total, 16);
        for x in &others {
            sim.set_cause(t + 2 * hour, *x, bit::USER, false, false, t + 2 * hour);
        }
        // Recorded once a KWC can decide: 21 points, capped at 20, and the seat is lost.
        sim.on_points_record(t + 2 * hour, w);
        assert_eq!(sim.res.point_awards_delayed, 1);
        assert_eq!(sim.witnesses[0].seat, NONE);
        assert_eq!(sim.res.seats_lost.iter().flatten().sum::<u64>(), 1);
        assert_eq!(sim.res.points_capped.iter().flatten().sum::<u64>(), 1);
        assert_eq!(sim.res.point_awards.iter().flatten().sum::<u64>(), 7);
        // Back during the cooldown it takes no seat; at its end it does.
        sim.set_cause(t + 3 * hour, h, bit::USER, false, false, t + 3 * hour);
        assert!(!sim.placements.iter().any(|(_, x)| *x == w));
        let until = sim.witnesses[0].seat_cooldown_until;
        assert_eq!(until, t + 2 * hour + 30 * day);
        sim.on_seat_cooldown(until, w);
        assert!(sim.placements.iter().any(|(_, x)| *x == w));
        sim.check_invariants();
        assert_eq!(sim.res.invariant_failures, 0);
    }

    #[test]
    fn a_scripted_blackout_takes_its_region_offline_and_is_recorded() {
        let cfg = gb_config::Config::default();
        let mut spec = small_sim(&cfg);
        spec.idealised = false;
        spec.outages = true;
        spec.scripted_blackouts = vec![(0, 0.5, 3_600.0)];
        spec.track_outages = true;
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let region: Vec<u32> = sim.shared.processes[sim.shared.blackout_of[0] as usize]
            .members
            .clone();
        assert!(!region.is_empty());
        let t = micros(0.5 * 86_400.0);
        sim.on_scripted_blackout(t, 0, true);
        let blacked = |sim: &Sim<'_>, h: u32| sim.hh[h as usize].mask & bit::SCRIPTED_BLACKOUT != 0;
        assert!(
            region
                .iter()
                .all(|h| blacked(&sim, *h) && !sim.hh[*h as usize].online())
        );
        assert!(
            (0..sim.hh.len() as u32)
                .filter(|h| !region.contains(h))
                .all(|h| !blacked(&sim, h))
        );
        assert_eq!(sim.res.outages.len(), 1);
        let r = &sim.res.outages[0];
        assert_eq!(
            (r.kind, r.region, r.households),
            (2, 0, region.len() as u64)
        );
        assert_eq!(r.duration_s, 3_600.0);
        sim.on_outage_sample(t + 30 * SECOND, 0);
        assert_eq!(sim.res.outages[0].samples, 1);
        sim.on_scripted_blackout(t + 3_600 * SECOND, 0, false);
        assert!(region.iter().all(|h| !blacked(&sim, *h)));
    }
}
