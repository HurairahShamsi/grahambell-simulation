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
    /// A candidate's miner decides whether to broadcast.
    Decide { c: u32 },
    /// A withholder releases its block.
    Release { c: u32 },
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
            Ev::Decide { c } => ("candidate_decide", Some(c), None),
            Ev::Release { c } => ("withheld_release", Some(c), None),
            Ev::PanelUpdate => ("panel_update", None, None),
            Ev::Committee => ("committee_refresh", None, None),
            Ev::Sample => ("kwc_sample", None, None),
            Ev::Arrival => ("arrival", None, None),
            Ev::ScriptedCrash { index } => ("scripted_crash", Some(index), None),
            Ev::ScriptEnd { h } => ("scripted_crash_end", Some(h), None),
            Ev::Patience { m } => ("patience_end", Some(m), None),
            Ev::End => ("end", None, None),
        }
    }
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
    pub active: std::collections::BTreeSet<usize>,
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
    pub panel_update_at: Micros,
    pub hop_state: Vec<u8>,
    pub withheld: Vec<u32>,
    pub pending: std::collections::BTreeMap<u32, (u64, Vec<u32>)>,
    pub panel_split: Vec<bool>,
    pub invariants_due: Micros,
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
        let now = sim.end.min(sim.q.now());
        sim.finish(now);
        sim.res.events = sim.q.processed();
        sim.res.wall_s = started.elapsed().as_secs_f64();
        sim.res.sim_s = seconds(now);
        Ok(sim.res)
    }

    fn handle(&mut self, now: Micros, ev: Ev) {
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
            Ev::Decide { c } => self.on_decide(now, c),
            Ev::Release { c } => self.on_release(now, c),
            Ev::PanelUpdate => self.on_panel_update(now),
            Ev::Committee => self.on_committee(now),
            Ev::Sample => self.on_sample(now),
            Ev::Arrival => self.on_arrival(now),
            Ev::ScriptedCrash { index } => self.on_scripted_crash(now, index),
            Ev::ScriptEnd { h } => {
                self.set_cause(now, h, crate::household::bit::SCRIPT, false, false, now)
            }
            Ev::Patience { m } => self.on_patience(now, m),
            Ev::End => self.stopped = true,
        }
    }

    /// The day of simulated time `t` falls in.
    pub(crate) fn day_index(&self, t: Micros) -> usize {
        (t / (86_400 * SECOND)) as usize
    }
}
