//! One run of the simulator: state, events and the event loop.
//!
//! The state is split across modules that all extend [`Sim`]: `world` (households, miners,
//! witnesses, allocation, outages), `chain` (blocks, difficulty, views, the validator panel,
//! confirmations) and `round` (candidate generation, entropy and PoWit timing, decisions).

use crate::chain::{Block, Candidate, Group};
use crate::engine::{Micros, Queue, SECOND, seconds};
use crate::household::Household;
use crate::job::{JobResult, JobSpec};
use crate::network::Network;
use crate::rng::Keyed;
use crate::world::{Counts, Miner, PanelNode, Shared, WcCount, Witness};
use gb_config::Config;
use gb_protocol::allocation::{Allocation, Registry};
use gb_protocol::committee::Committee;
use gb_protocol::validate::QuorumRule;

/// Events of the simulation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Ev {
    /// A household's own outage processes have a transition due.
    Household { h: u32, token: u32 },
    /// A shared outage process (feeder group, provider, region) has a transition due.
    Shared { process: u32, phase: u8, token: u32 },
    /// A miner's admission completes (Globally Active, if the vote succeeds).
    MinerActivate { m: u32, token: u32 },
    /// A miner's post-admission wait ends.
    MinerWaitEnd { m: u32, token: u32 },
    /// A miner that disappeared without announcing it is recorded offline if still away.
    MinerGrace { m: u32, token: u32 },
    /// A miner re-admits after a self-service offline request (restart or switch).
    MinerRejoin { m: u32, token: u32 },
    /// A miner switches KWC (C5 background switches).
    MinerSwitch { m: u32, token: u32 },
    /// One Chance is judged for a witness that disappeared without announcing it.
    WitnessCheck { w: u32, token: u32 },
    /// A witness absent since `token`'s outage start is deactivated if still away.
    WitnessDeactivate { w: u32, token: u32 },
    /// A witness absent longer than L loses its seat if still away.
    WitnessLongAbsence { w: u32, token: u32 },
    /// A round's candidate generator wakes.
    GroupWake { g: u32 },
    /// A round that ran out of draws checks whether to draw its transiently dropped slots
    /// again (SIM-48).
    GroupRefresh { g: u32 },
    /// A candidate's miner decides whether to broadcast (`uses`: which use of the candidate slot it is for).
    Decide { c: u32, uses: u32 },
    /// A withholder releases its block.
    Release { c: u32, uses: u32 },
    /// Start rule (r): a drawn candidate reaches its own found time, where the rest of its
    /// round's checks run.
    Found { d: u32 },
    /// Recompute the panel's tips and the network chain.
    PanelUpdate,
    /// Committee refresh (every 10th PoW-Tx block).
    Committee,
    /// Take a KWC sample (C7).
    Sample,
    /// A new honest miner arrives (A6).
    Arrival,
    /// A scripted witness crash (C5 mechanics).
    ScriptedCrash { index: u32 },
    /// A scripted outage ends.
    ScriptEnd { h: u32 },
    /// A newcomer without an ID runs out of patience (A6).
    Patience { m: u32 },
    /// A partition starts (`start`) or heals: its witnesses are cut off or return.
    Partition { index: u32, start: bool },
    /// Failover: every KWC's quorum is checked, once a round.
    Failover,
    /// A witness's offline period (the one with `token`) reaches the points threshold n.
    PointsDue { w: u32, token: u32 },
    /// An owed points award waits for a KWC able to decide.
    PointsRecord { w: u32 },
    /// A witness's cooldown after a seat loss ends.
    SeatCooldown { w: u32 },
    /// A scripted regional blackout starts or ends.
    ScriptedBlackout { index: u32, start: bool },
    /// A tracked outage's effect on mining is sampled (every round while it lasts).
    OutageSample { index: u32 },
    /// End of the run.
    End,
}

impl Ev {
    /// The event's name and the entities it names, for traces.
    fn describe(self) -> (&'static str, Option<u32>, Option<u32>) {
        match self {
            Ev::Household { h, token } => ("household", Some(h), Some(token)),
            Ev::Shared { process, phase, .. } => {
                ("shared_outage", Some(process), Some(u32::from(phase)))
            }
            Ev::MinerActivate { m, .. } => ("miner_activate", Some(m), None),
            Ev::MinerWaitEnd { m, .. } => ("miner_wait_end", Some(m), None),
            Ev::MinerGrace { m, .. } => ("miner_grace", Some(m), None),
            Ev::MinerRejoin { m, .. } => ("miner_rejoin", Some(m), None),
            Ev::MinerSwitch { m, .. } => ("miner_switch", Some(m), None),
            Ev::WitnessCheck { w, .. } => ("witness_one_chance", Some(w), None),
            Ev::WitnessDeactivate { w, .. } => ("witness_deactivate", Some(w), None),
            Ev::WitnessLongAbsence { w, .. } => ("witness_long_absence", Some(w), None),
            Ev::GroupWake { g } => ("round_wake", Some(g), None),
            Ev::GroupRefresh { g } => ("round_refresh", Some(g), None),
            Ev::Decide { c, .. } => ("candidate_decide", Some(c), None),
            Ev::Release { c, .. } => ("withheld_release", Some(c), None),
            Ev::Found { d } => ("candidate_found", Some(d), None),
            Ev::PanelUpdate => ("panel_update", None, None),
            Ev::Committee => ("committee_refresh", None, None),
            Ev::Sample => ("kwc_sample", None, None),
            Ev::Arrival => ("arrival", None, None),
            Ev::ScriptedCrash { index } => ("scripted_crash", Some(index), None),
            Ev::ScriptEnd { h } => ("scripted_crash_end", Some(h), None),
            Ev::Patience { m } => ("patience_end", Some(m), None),
            Ev::Partition { index, start } => ("partition", Some(index), Some(u32::from(start))),
            Ev::Failover => ("failover_check", None, None),
            Ev::PointsDue { w, .. } => ("points_due", Some(w), None),
            Ev::PointsRecord { w } => ("points_record", Some(w), None),
            Ev::SeatCooldown { w } => ("seat_cooldown_end", Some(w), None),
            Ev::ScriptedBlackout { index, start } => {
                ("scripted_blackout", Some(index), Some(u32::from(start)))
            }
            Ev::OutageSample { index } => ("outage_sample", Some(index), None),
            Ev::End => ("end", None, None),
        }
    }
}

/// FIFO lanes of the event queue: timers scheduled a constant delay after the current time,
/// so their times never decrease within a lane (`Queue::push_lane`).
pub(crate) mod lane {
    /// The first One Chance check after an unannounced disappearance.
    pub const FIRST_CHECK: usize = 0;
    /// One Chance retried a round later, when no KWC could vote.
    pub const RECHECK: usize = 1;
    /// Deactivation of a witness absent past the threshold.
    pub const DEACTIVATE: usize = 2;
    /// Seat vacated after an absence longer than L.
    pub const LONG_ABSENCE: usize = 3;
    /// A miner recorded offline after the grace epoch.
    pub const MINER_GRACE: usize = 4;
    /// An offline period reaching the points threshold.
    pub const POINTS_DUE: usize = 5;
    /// An owed points award retried a round later.
    pub const POINTS_RECORD: usize = 6;
    /// The end of a seat-loss cooldown.
    pub const SEAT_COOLDOWN: usize = 7;
}

/// Protocol settings a run reads often, resolved once.
#[derive(Debug, Clone)]
pub(crate) struct Params {
    pub interval_s: u64,
    pub grace: Micros,
    pub convergence: Micros,
    pub post_wait: Micros,
    pub one_chance: Micros,
    pub deactivation: Micros,
    pub long_absence: Micros,
    pub wc_size: usize,
    pub offsets: Vec<u64>,
    pub unregistered: QuorumRule,
    pub registered: QuorumRule,
    pub quorum_unregistered: usize,
    pub capacity: u32,
    pub confirmation_depth: u64,
    pub beacon_delay: u64,
    pub pow_tx: Micros,
    pub cac_size: usize,
    pub cac_every: u64,
    pub ladder: Vec<u64>,
    pub seconds_per_year: f64,
    pub warmup: Micros,
    pub settle_depth: u64,
    pub signing_tolerance: f64,
    pub fork_storm_unsettled: usize,
    pub cache_entries: usize,
}

/// One simulation run.
pub(crate) struct Sim<'a> {
    pub spec: &'a JobSpec,
    pub cfg: &'a Config,
    pub p: Params,
    pub keyed: Keyed,
    pub q: Queue<Ev>,
    pub net: Network,
    pub hh: Vec<Household>,
    pub miners: Vec<Miner>,
    pub witnesses: Vec<Witness>,
    pub seat_owner: Vec<u32>,
    pub alloc: Allocation,
    pub registry: Registry,
    pub committee: Committee,
    pub active: crate::active::ActiveSet,
    pub wc: Vec<WcCount>,
    pub kwc_load: Vec<u32>,
    pub counts: Counts,
    pub blocks: Vec<Block>,
    pub unsettled: Vec<u32>,
    pub settled_ids: Vec<u32>,
    pub groups: Vec<Group>,
    pub candidates: Vec<Candidate>,
    pub panel: Vec<PanelNode>,
    pub network_tip: u32,
    pub confirmed_height: u64,
    pub confirmed: Vec<u32>,
    pub placements: Vec<(u64, u32)>,
    pub shared: Shared,
    pub res: JobResult,
    pub end: Micros,
    pub max_skew: f64,
    pub max_access: f64,
    pub d_max: Micros,
    pub committee_ticks: u64,
    pub v1_attacker: u64,
    pub stopped: bool,
    /// When a fork storm stopped the run ([`Micros::MAX`] if none did).
    pub stop_at: Micros,
    pub panel_update_at: Micros,
    pub hop_state: Vec<u8>,
    pub withheld: Vec<u32>,
    pub pending: std::collections::BTreeMap<u32, (u64, Vec<u32>)>,
    pub panel_split: Vec<bool>,
    pub invariants_due: Micros,
    /// Entries in the unsettled blocks' acceptance caches.
    pub own_entries: usize,
    /// Start rule (r): candidates waiting for their own found time (`Ev::Found`).
    pub deferred: Vec<Option<crate::round::Prep>>,
    /// Free slots of `deferred`.
    pub deferred_free: Vec<u32>,
    /// Candidate slots that are done and may be used again.
    pub free_candidates: Vec<u32>,
    /// Failover: consecutive rounds each KWC has been below quorum.
    pub failover_below: Vec<u32>,
    /// The outage record of each shared outage process while it lasts (`NONE` otherwise).
    pub outage_of_process: Vec<u32>,
    /// The outage record of each scripted blackout.
    pub outage_of_script: Vec<u32>,
    /// E3 diagnosis samples of each outage record (scripted blackouts only).
    pub outage_diag: Vec<crate::world::OutageDiag>,
    /// Traced runs (W2): each block's entropy state when it was broadcast
    /// ([`Sim::entropy_state`]).
    pub trace_state: Vec<(u32, [u32; 4])>,
}

impl<'a> Sim<'a> {
    /// Runs the job to its end and returns what it measured.
    pub(crate) fn run(spec: &'a JobSpec, cfg: &'a Config) -> Result<JobResult, crate::SimError> {
        let started = std::time::Instant::now();
        let mut sim = Sim::build(spec, cfg)?;
        sim.start();
        while let Some((now, ev)) = sim.q.pop() {
            if now >= sim.end || sim.stopped {
                break;
            }
            if spec.trace_every > 0
                && sim.q.processed() % spec.trace_every == 0
                && sim.res.trace.len() < crate::job::TRACE_ROWS_MAX
            {
                let (event, a, b) = ev.describe();
                sim.res.trace.push(crate::job::TraceRow {
                    t_s: seconds(now),
                    event,
                    a,
                    b,
                });
            }
            sim.handle(now, ev);
        }
        let now = sim.end.min(sim.q.now()).min(sim.stop_at);
        sim.finish(now);
        sim.res.events = sim.q.processed();
        sim.res.wall_s = started.elapsed().as_secs_f64();
        sim.res.sim_s = seconds(now);
        Ok(sim.res)
    }

    pub(crate) fn handle(&mut self, now: Micros, ev: Ev) {
        match ev {
            Ev::Household { h, token } => self.on_household(now, h, token),
            Ev::Shared {
                process,
                phase,
                token,
            } => self.on_shared(now, process, phase, token),
            Ev::MinerActivate { m, token } => self.on_miner_activate(now, m, token),
            Ev::MinerWaitEnd { m, token } => self.on_miner_wait_end(now, m, token),
            Ev::MinerGrace { m, token } => self.on_miner_grace(now, m, token),
            Ev::MinerRejoin { m, token } => self.on_miner_rejoin(now, m, token),
            Ev::MinerSwitch { m, token } => self.on_miner_switch(now, m, token),
            Ev::WitnessCheck { w, token } => self.on_witness_check(now, w, token),
            Ev::WitnessDeactivate { w, token } => self.on_witness_deactivate(now, w, token),
            Ev::WitnessLongAbsence { w, token } => self.on_witness_long_absence(now, w, token),
            Ev::GroupWake { g } => self.on_group_wake(now, g),
            Ev::GroupRefresh { g } => self.on_group_refresh(now, g),
            Ev::Decide { c, uses } => self.on_decide(now, c, uses),
            Ev::Release { c, uses } => self.on_release(now, c, uses),
            Ev::Found { d } => self.on_found(now, d),
            Ev::PanelUpdate => self.on_panel_update(now),
            Ev::Committee => self.on_committee(now),
            Ev::Sample => self.on_sample(now),
            Ev::Arrival => self.on_arrival(now),
            Ev::ScriptedCrash { index } => self.on_scripted_crash(now, index),
            Ev::ScriptEnd { h } => {
                self.set_cause(now, h, crate::household::bit::SCRIPT, false, false, now)
            }
            Ev::Patience { m } => self.on_patience(now, m),
            Ev::Partition { index, start } => self.on_partition(now, index, start),
            Ev::Failover => self.on_failover(now),
            Ev::PointsDue { w, token } => self.on_points_due(now, w, token),
            Ev::PointsRecord { w } => self.on_points_record(now, w),
            Ev::SeatCooldown { w } => self.on_seat_cooldown(now, w),
            Ev::ScriptedBlackout { index, start } => self.on_scripted_blackout(now, index, start),
            Ev::OutageSample { index } => self.on_outage_sample(now, index),
            Ev::End => self.stopped = true,
        }
    }

    /// The day of simulated time `t` falls in.
    pub(crate) fn day_index(&self, t: Micros) -> usize {
        (t / (86_400 * SECOND)) as usize
    }
}
