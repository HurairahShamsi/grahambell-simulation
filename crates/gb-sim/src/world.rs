//! The world: households, miner slots, witnesses, allocation, shared outages and the
//! admission and One Chance processes (SPEC §3.1–§3.2, §4.2, §4.5–§4.8).

use crate::engine::{Micros, SECOND, micros};
use crate::household::{self, CauseContext, Device, Household, bit};
use crate::job::Strategy;
use crate::network::{
    GossipTable, Latency, LossModel, Network, OverlaySettings, Partition, RIPE_MATRIX,
    calibrate_gossip, spread_regions,
};
use crate::rng::{Keyed, Purpose};
use crate::samplers::{exponential, lognormal, normal};
use crate::sim::{Ev, Params, Sim, lane};
use gb_config::sim::{GridProfile, LatencySource};
use gb_protocol::allocation::{IdStatus, kwc_wcs};
use gb_protocol::hash::{Hash, tagged};
use gb_protocol::offline::{Penalty, one_chance_outcome, penalty_for};
use gb_protocol::validate::{MinerKind, QuorumRule, witness_signing_tolerance};

/// No index.
pub(crate) const NONE: u32 = u32::MAX;

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
    pub last: Micros,
    pub hop_mining: u64,
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
                (u64::from(cfg.admission.post_admission_wait_rounds.value) * interval_s) as f64,
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
                ..crate::job::JobResult::default()
            },
            end,
            max_skew: 0.0,
            max_access: 0.0,
            d_max: 0,
            committee_ticks: 0,
            v1_attacker: 0,
            stopped: false,
            panel_update_at: u64::MAX,
            hop_state: vec![0; groups],
            withheld: Vec::new(),
            pending: std::collections::BTreeMap::new(),
            panel_split: Vec::new(),
            invariants_due: 0,
        };
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
        let feeders = grid.feeder_groups as usize;
        let grid_group = (region * feeders + household::pick(w[1].to_bits(), feeders)) as u32;
        let isps = cfg.households.isps_per_region.max(1) as usize;
        let isp = (region * isps + household::pick(w[2].to_bits(), isps)) as u32;
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
        let miner_regions = spread_regions(spec.miners as usize, &shares, &mut self.keyed);
        let group_shares: Vec<f64> = {
            let tagged: f64 = spec.groups.iter().map(|g| g.share).sum();
            std::iter::once((1.0 - tagged).max(0.0))
                .chain(spec.groups.iter().map(|g| g.share))
                .collect()
        };
        let miner_groups = spread_regions(spec.miners as usize, &group_shares, &mut self.keyed);
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
        if was_online && !online {
            if let Some((m, old)) = miner_old {
                self.miner_down(now, m, announced, old);
            }
            for (w, old) in witness_olds {
                self.witness_down(now, w, announced, old);
            }
        } else if !was_online && online {
            if let Some((m, old)) = miner_old {
                self.miner_up(now, m, old);
            }
            for (w, old) in witness_olds {
                self.witness_up(now, w, old);
            }
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
                if self.kwc_load[k] < self.p.capacity {
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
        if self.miners[m as usize].phase == Phase::Idle {
            self.start_admission(now, m);
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
        let seated = wit.seat != NONE && !wit.banned;
        WitnessStatus {
            wc: seated.then(|| wit.seat as usize / self.p.wc_size),
            online: self.hh[wit.household as usize].online(),
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

    fn witness_down(&mut self, now: Micros, w: u32, announced: bool, old: WitnessStatus) {
        if self.witnesses[w as usize].banned {
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
        if wit.banned {
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
        if unseated {
            // Its seat was vacated after an absence longer than L: re-inserted with the beacon
            // of the block `beacon delay` after its re-activation is confirmed.
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
            if wit.short_outage {
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
        if wit.token != token || wit.banned || self.hh[wit.household as usize].online() {
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
        if wit.token != token || wit.banned || self.hh[wit.household as usize].online() {
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
        self.placements
            .push((height + self.p.beacon_delay + self.p.confirmation_depth, w));
        // The winner is treated as offline once its block is confirmed (SPEC §3.10).
        let first = !self.miners[miner as usize].first_id;
        if self.miners[miner as usize].newcomer && first {
            self.res.first_ids += 1;
            let waited =
                (now - self.miners[miner as usize].joined_at) as f64 / (86_400.0 * SECOND as f64);
            if self.res.waits_days.len() < 50_000 {
                self.res.waits_days.push(waited);
            }
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
                if !self.witnesses[w as usize].banned {
                    self.insert_seat(w, &beacon);
                }
            }
        }
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
                    .map(|w| {
                        let wit = &self.witnesses[*w as usize];
                        !wit.banned && self.hh[wit.household as usize].online()
                    })
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
}
