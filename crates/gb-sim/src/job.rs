//! One simulation run: its settings ([`JobSpec`]) and what it measured ([`JobResult`]).

use gb_config::protocol::{
    CountBasedCorrection, DeadlineScope, DifficultyCountRule, DifficultyVariant, EarlyBlockRule,
    EquivocationScope, StrictOffencePenalty, UnreachableRule, WitnessSigning,
};
use gb_config::sim::{CryptoMode, LatencySource};
use gb_protocol::hash::Hash;
use serde::{Deserialize, Serialize};

/// Fork choice used by every node in a run (M3 C2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForkRule {
    /// SPEC v0.6 §3.7: longest chain, then earlier timestamp, then lower hash \[P\].
    V06,
    /// Comparison: longest chain, then the block seen first.
    FirstSeen,
    /// Comparison: longest chain, then the lower hash only.
    LowerHashOnly,
}

impl ForkRule {
    /// Name used in tables.
    pub fn name(self) -> &'static str {
        match self {
            ForkRule::V06 => "v0.6",
            ForkRule::FirstSeen => "first_seen",
            ForkRule::LowerHashOnly => "lower_hash_only",
        }
    }
}

/// How the fast mode draws winning steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sampler {
    /// Next success step over all slots, then the tied winners (the default).
    Gap,
    /// One geometric draw per slot and round (the reference).
    PerMiner,
}

/// What a group of miner slots does.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    /// Follows the protocol.
    Honest,
    /// S5 restart: re-admits whenever its next win is further than `window_s` away. Under the
    /// old design its entropy is fixed per connection, so it knows its future wins; under the
    /// current design it sees only this round's winning step.
    Restart {
        /// The old fixed-entropy design (true) or per-round entropy (false).
        old_design: bool,
        /// Keep window, seconds.
        window_s: f64,
    },
    /// S6 difficulty hopping: the group's slots join at a window boundary of the network
    /// chain and mine for one window of `window_blocks` blocks; with `idle_windows > 0` they
    /// first stay admitted but idle (counted, not mining) for that many windows; then they
    /// leave for `rest_windows` windows.
    Hop {
        /// Blocks per window.
        window_blocks: u32,
        /// Windows spent counted but idle before the active window.
        idle_windows: u32,
        /// Windows between hops.
        rest_windows: u32,
    },
    /// C3 withholder: wins, collects its PoWit, and releases the block only after it sees the
    /// next honest block at that height, to win by the earlier timestamp.
    Withhold,
}

/// A tagged group of miner slots.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupSpec {
    /// Label in tables.
    pub label: String,
    /// Share of all miner slots.
    pub share: f64,
    /// Behaviour.
    pub strategy: Strategy,
    /// Exempt from household outages (attackers in V1–V3 are always online, M1 G-7).
    pub always_online: bool,
}

/// Settings of one run. Protocol parameters not listed here come from the configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobSpec {
    /// Identifier, unique within a suite.
    pub id: String,
    /// Section (V0–V4, C1–C7, A6).
    pub section: String,
    /// Seed of this run.
    pub seed: u64,
    /// Simulated days.
    pub days: f64,
    /// Miner slots at the start.
    pub miners: u32,
    /// Registered IDs at the start.
    pub registered_ids: u64,
    /// Crypto mode.
    pub crypto: CryptoMode,
    /// Winning-step sampler in fast mode.
    pub sampler: Sampler,
    /// Zero latency, no loss, no clock skew, no outages.
    pub idealised: bool,
    /// Household outages on.
    pub outages: bool,
    /// Message loss probability.
    pub loss: f64,
    /// Latency source.
    pub latency: LatencySource,
    /// Scheduled partitions: (region, start day, length in seconds).
    pub partitions: Vec<(usize, f64, f64)>,
    /// Random partitions per region per day, each `partition_length_s` long.
    pub partition_rate_per_region_per_day: f64,
    /// Length of random partitions, seconds.
    pub partition_length_s: f64,
    /// A witness cut off by a partition counts as unreachable (One Chance applies).
    pub partition_cuts_witnesses: bool,
    /// Clock-skew standard deviation, seconds.
    pub skew_sd: f64,
    /// Share of nodes in the badly synchronised tail.
    pub skew_tail_fraction: f64,
    /// Tail standard deviation, seconds.
    pub skew_tail_sd: f64,
    /// Clock tolerance δ, seconds.
    pub delta: f64,
    /// When honest witnesses sign the PoWit.
    pub witness_signing: WitnessSigning,
    /// Nodes' handling of early blocks.
    pub early_block: EarlyBlockRule,
    /// PoWit deadline D, seconds.
    pub deadline: Option<f64>,
    /// Nodes' handling of late PoWits.
    pub deadline_scope: DeadlineScope,
    /// Fork choice.
    pub fork_rule: ForkRule,
    /// What counts as miner equivocation.
    pub equivocation: EquivocationScope,
    /// Difficulty variant.
    pub difficulty: DifficultyVariant,
    /// Variant A window K.
    pub window_blocks: u32,
    /// Variant A clamp (None: unclamped).
    pub clamp: Option<f64>,
    /// Variant B correction.
    pub correction: Option<CountBasedCorrection>,
    /// Variant B count rule.
    pub count_rule: DifficultyCountRule,
    /// One Chance wait, rounds.
    pub one_chance_wait_rounds: u32,
    /// Unreachable rule.
    pub unreachable: UnreachableRule,
    /// Strict-rule penalty.
    pub strict_penalty: StrictOffencePenalty,
    /// Tagged groups of miner slots (the remaining slots are untagged and honest).
    pub groups: Vec<GroupSpec>,
    /// V1: honest IDs are active with this probability (static per ID); attacker IDs always.
    pub honest_id_online: Option<f64>,
    /// V1: genesis IDs counted in the active base (the run stops when group 1 reaches
    /// `threshold` of active IDs).
    pub threshold_genesis: u64,
    /// V1: stop when group 1 holds this share of active IDs.
    pub threshold: Option<f64>,
    /// Admission and re-admission take no time (idealised validations).
    pub instant_admission: bool,
    /// A fixed target for every round (V2).
    pub frozen_target: Option<Hash>,
    /// New honest miners per year (A6), as a multiple of R.
    pub arrivals_multiple_of_r: f64,
    /// Median patience, days (A6).
    pub patience_median_days: f64,
    /// Permanent departures per household per year (C7 sweep), overriding the configuration.
    pub departures_per_year: Option<f64>,
    /// Device mix override (always on, desktop, laptop).
    pub device_mix: Option<Vec<f64>>,
    /// Witnesses go offline independently with probability `1 − f` (C7 check), instead of the
    /// household profiles.
    pub independent_witness_online: Option<f64>,
    /// Horizon audit: generate candidates past the normal horizon and count any that matter.
    pub audit: bool,
    /// Sample KWC quorum states every this many seconds (C7); 0 disables.
    pub kwc_sample_interval_s: f64,
    /// KWCs per sample.
    pub kwc_sample_size: u32,
    /// Collect per-round winning-step data (V0).
    pub record_rounds: bool,
    /// Scripted crashes of single witnesses (C5 mechanics): (day, seconds offline).
    pub scripted_crashes: Vec<(f64, f64)>,
    /// Voluntary KWC switches per mining slot per day (C5 switch times).
    pub switches_per_day: f64,
    /// Days excluded from measurement at the start, while household states settle (realistic
    /// runs; idealised runs start measuring at once).
    pub warmup_days: f64,
    /// Trace every this many events (0: no trace), from `sim.trace_sample`.
    pub trace_every: u64,
}

/// One traced event: when it was processed, what it was, and the entities it names.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceRow {
    /// Simulated time, seconds.
    pub t_s: f64,
    /// Event kind.
    pub event: &'static str,
    /// First entity (household, miner, witness, round, candidate or process; empty if none).
    pub a: Option<u32>,
    /// Second entity (a token or phase; empty if none).
    pub b: Option<u32>,
}

/// Rows a run's trace holds at most.
pub const TRACE_ROWS_MAX: usize = 1_000_000;

/// Counts and sums a run produces. Everything a table needs is here, so tables never rerun a
/// simulation. Results round-trip through JSON exactly (checkpoints), apart from the wall
/// time and the trace, which are not serialised.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JobResult {
    /// Job identifier.
    pub id: String,
    /// Events processed.
    pub events: u64,
    /// Wall time, seconds (machine-dependent; not part of reproducible outputs).
    #[serde(skip)]
    pub wall_s: f64,
    /// Simulated seconds.
    pub sim_s: f64,
    /// Blocks broadcast.
    pub blocks: u64,
    /// Heights on the final chain (excluding the last 12).
    pub canonical: u64,
    /// Broadcast blocks off the final chain (excluding the last 12 heights).
    pub orphans: u64,
    /// Heights with at least two broadcast blocks.
    pub fork_heights: u64,
    /// At heights with two or more blocks: the orphan had the lower hash (it would have won
    /// under lower-hash-only).
    pub loser_lower_hash: u64,
    /// Rounds (groups whose candidates were drawn to the horizon).
    pub rounds: u64,
    /// Rounds whose earliest winning step had two or more eligible winners.
    pub tie_rounds: u64,
    /// Sum of the eligible slot counts at round starts.
    pub round_slots: f64,
    /// Final-chain blocks by group (index 0 = untagged honest).
    pub ids_by_group: Vec<u64>,
    /// Mining slot-seconds by group.
    pub slot_s_by_group: Vec<f64>,
    /// Final-chain blocks by region.
    pub ids_by_region: Vec<u64>,
    /// Mining slot-seconds by region.
    pub slot_s_by_region: Vec<f64>,
    /// Daily batches for confidence intervals: blocks by group per day.
    pub daily_ids_by_group: Vec<Vec<u64>>,
    /// Daily batches: slot-seconds by group per day.
    pub daily_slot_s_by_group: Vec<Vec<f64>>,
    /// Daily final-chain block counts.
    pub daily_ids: Vec<u64>,
    /// Sum and count of final-chain intervals (timestamp differences), seconds.
    pub interval_sum_s: f64,
    /// Number of intervals.
    pub interval_count: u64,
    /// Sum of squared intervals.
    pub interval_sq_sum_s: f64,
    /// Blocks rejected as late by a majority of panel nodes.
    pub late_majority: u64,
    /// Blocks rejected as early by a majority of panel nodes.
    pub early_majority: u64,
    /// Panel node-block pairs rejected as late.
    pub late_pairs: u64,
    /// Panel node-block pairs rejected as early.
    pub early_pairs: u64,
    /// Panel node-block pairs checked.
    pub panel_pairs: u64,
    /// Panel nodes whose final tip is off the final chain.
    pub split_panel_nodes: u64,
    /// Reorgs of the network chain deeper than the confirmation depth.
    pub deep_reorgs: u64,
    /// Deepest reorg of the network chain, blocks.
    pub max_reorg: u64,
    /// Withheld blocks.
    pub withheld: u64,
    /// Withheld blocks that ended on the final chain.
    pub withheld_won: u64,
    /// Honest blocks displaced from the network chain by a released withheld block.
    pub displaced_by_withheld: u64,
    /// Candidates abandoned because a better block was already seen.
    pub abandoned: u64,
    /// Candidates whose miner sat out because its parent lost (literal one-header rule).
    pub sat_out: u64,
    /// Candidates dropped because the KWC could not form entropy or a PoWit.
    pub quorum_failures: u64,
    /// Candidates delayed by an entropy stall.
    pub stalled_candidates: u64,
    /// Unannounced witness disappearances.
    pub witness_crashes: u64,
    /// Sum of entropy-stall durations (from the first request to the record or the return),
    /// seconds.
    pub stall_s_sum: f64,
    /// Stall durations, seconds (sampled up to 10,000).
    pub stall_samples: Vec<f64>,
    /// Witness outages shorter than the grace epoch.
    pub short_glitches: u64,
    /// Of those, recorded offline after a failed One Chance.
    pub short_glitches_recorded: u64,
    /// Honest witnesses penalised, by region and device: `[region][device]`.
    pub penalties: Vec<Vec<u64>>,
    /// Honest witness IDs banned permanently, by region and device.
    pub bans: Vec<Vec<u64>>,
    /// Witness ID-seconds, by region and device.
    pub witness_id_s: Vec<Vec<f64>>,
    /// Records offline after One Chance, by region and device.
    pub offline_records: Vec<Vec<u64>>,
    /// Times from a switch decision to mining in the new KWC, seconds (sampled).
    pub switch_s: Vec<f64>,
    /// Times from a restart decision to mining again, seconds (sampled).
    pub restart_s: Vec<f64>,
    /// KWC samples taken.
    pub kwc_samples: u64,
    /// Sampled KWCs below the unregistered quorum (27 of 40).
    pub kwc_fail_unregistered: u64,
    /// Sampled KWCs below the registered quorum (7 + 21).
    pub kwc_fail_registered: u64,
    /// Sum of the online fraction of seated witnesses over samples.
    pub online_fraction_sum: f64,
    /// Daily batches of sampled failures (unregistered, registered, samples).
    pub daily_kwc: Vec<[u64; 3]>,
    /// New honest miners arrived (A6).
    pub arrivals: u64,
    /// Of those, won a first ID.
    pub first_ids: u64,
    /// Of those, gave up.
    pub dropouts: u64,
    /// Waits to a first ID, days (sampled).
    pub waits_days: Vec<f64>,
    /// Blocks to the V1 threshold (when it was reached).
    pub threshold_blocks: Option<u64>,
    /// V1: final-chain blocks won by group 1 among those counted.
    pub v1_attacker_blocks: u64,
    /// V1: honest blocks whose ID counted as active.
    pub v1_honest_active: u64,
    /// Restart decisions.
    pub restarts: u64,
    /// V0: earliest winning step of each round (sampled up to 50,000).
    pub nmin: Vec<u64>,
    /// V0: eligible slots of each round (same rounds).
    pub round_r: Vec<u64>,
    /// V0: per-step success probability of each round (same rounds).
    pub round_p: Vec<f64>,
    /// V0: winners at the earliest step (same rounds).
    pub tie_size: Vec<u64>,
    /// V0: wins by slot.
    pub wins_by_slot: Vec<u64>,
    /// V0: top 53 bits of block hashes as uniforms (sampled up to 50,000).
    pub hash_uniforms: Vec<f64>,
    /// V0: each slot's first winning step in the rounds checked (stand-in and real modes),
    /// scaled by its round's per-step success probability (1 on average for a geometric law);
    /// chains that did not win within the cap are recorded at the cap.
    pub slot_steps: Vec<f64>,
    /// Real mode: blocks whose `validate()` and `validate_facts` verdicts were compared.
    pub validated: u64,
    /// Real mode: verdict mismatches.
    pub validate_mismatches: u64,
    /// Broadcast blocks that failed `validate_facts` at their own miner (must be 0).
    pub self_invalid: u64,
    /// Horizon audit: candidates past the normal horizon that were broadcast (must be 0).
    pub horizon_hits: u64,
    /// Invariant checks that failed (must be 0).
    pub invariant_failures: u64,
    /// Group-1 blocks in hop or idle windows and the slot-seconds there (V3).
    pub hop_ids: u64,
    /// Group-1 mining slot-seconds while hopping.
    pub hop_slot_s: f64,
    /// Correction factor samples at the end of each window (ppm), V3/C4.
    pub factor_ppm: Vec<u64>,
    /// Final-chain blocks in each half-day, for issuance stability.
    pub halfday_ids: Vec<u64>,
    /// Drawn candidates that did not become blocks, by reason (see `DROP_REASONS`).
    pub drops: Vec<u64>,
    /// Daily batches: final-chain blocks by region per day.
    pub daily_ids_by_region: Vec<Vec<u64>>,
    /// Daily batches: slot-seconds by region per day.
    pub daily_slot_s_by_region: Vec<Vec<f64>>,
    /// Blocks rejected as late by a majority of panel nodes, by the miner's region.
    pub late_majority_by_region: Vec<u64>,
    /// Final-chain blocks broadcast, by the miner's region (denominator of the above).
    pub blocks_by_region: Vec<u64>,
    /// Durations of consecutive windows of `window_blocks` blocks on the final chain, seconds.
    pub window_s: Vec<f64>,
    /// Global issuance pauses: gaps of more than 120 s between consecutive final-chain
    /// broadcasts (count and total seconds).
    pub pauses: u64,
    /// Total length of those pauses, seconds.
    pub pause_s: f64,
    /// Newcomers (A6) still waiting for a first ID at the end.
    pub waiting_newcomers: u64,
    /// The most blocks unsettled at once.
    pub max_unsettled: u64,
    /// The most unsettled blocks at one height.
    pub max_fork_width: u64,
    /// When the run stopped early because more blocks than `sim.world.fork_storm_unsettled`
    /// were unsettled at once (a fork storm), in simulated seconds; `None` when it ran to its
    /// end. Daily series then keep only the complete days before.
    pub fork_storm_s: Option<f64>,
    /// Witnesses cut off by a partition while recorded online (each then went through One
    /// Chance).
    pub partition_cuts: u64,
    /// IDs voided because their PoW-ID block was replaced below confirmation depth.
    pub voided_ids: u64,
    /// Miners whose household announced a shutdown and came back within the grace epoch,
    /// and re-admitted at once.
    pub quick_readmits: u64,
    /// Sampled events (only when `trace_every > 0`; not part of the tables).
    #[serde(skip)]
    pub trace: Vec<TraceRow>,
}

/// Why a drawn candidate did not become a block, indexing [`JobResult::drops`].
pub const DROP_REASONS: [&str; 10] = [
    "not_mining",
    "parent_not_accepted",
    "mines_on_sibling",
    "offline",
    "entropy_quorum",
    "sat_out",
    "round_already_won",
    "powit_quorum",
    "abandoned_at_broadcast",
    "restarter_slot",
];

impl JobSpec {
    /// A run with every setting taken from the configuration: realistic network and
    /// households, the SPEC rules and defaults, and no tagged groups.
    pub fn from_config(config: &gb_config::Config, id: &str, section: &str, seed: u64) -> JobSpec {
        let sim = &config.sim;
        let issuance = &config.issuance;
        let timing = &config.timing;
        JobSpec {
            id: id.to_string(),
            section: section.to_string(),
            seed,
            days: 1.0,
            miners: 1_000,
            registered_ids: sim.world.registered_ids,
            crypto: sim.crypto_mode,
            sampler: Sampler::Gap,
            idealised: false,
            outages: true,
            loss: sim.network.loss,
            latency: sim.network.latency_source,
            partitions: Vec::new(),
            partition_rate_per_region_per_day: 0.0,
            partition_length_s: 300.0,
            skew_sd: sim.network.skew_sd_s,
            skew_tail_fraction: sim.network.skew_tail_fraction,
            skew_tail_sd: sim.network.skew_tail_sd_s,
            delta: timing.clock_tolerance_s.value,
            witness_signing: timing.witness_signing.value,
            early_block: timing.early_block.value,
            deadline: timing.powit_deadline_s.value,
            deadline_scope: timing.powit_deadline_scope.value,
            fork_rule: ForkRule::V06,
            equivocation: config.entropy.equivocation_scope.value,
            difficulty: issuance.difficulty_variant.value,
            window_blocks: issuance.retarget_window_blocks.value,
            clamp: Some(issuance.retarget_clamp_factor.value),
            correction: issuance.count_based_correction.value,
            count_rule: issuance.difficulty_count_rule.value,
            one_chance_wait_rounds: timing.one_chance_wait_rounds.value,
            unreachable: config.offline.unreachable_rule.value,
            strict_penalty: config.penalties.strict_offence.value,
            groups: Vec::new(),
            honest_id_online: None,
            threshold_genesis: 0,
            threshold: None,
            instant_admission: false,
            frozen_target: None,
            arrivals_multiple_of_r: 0.0,
            patience_median_days: sim
                .growth
                .patience_median_days
                .first()
                .copied()
                .unwrap_or(14.0),
            departures_per_year: None,
            device_mix: None,
            independent_witness_online: None,
            audit: false,
            kwc_sample_interval_s: 0.0,
            kwc_sample_size: 0,
            record_rounds: false,
            scripted_crashes: Vec::new(),
            switches_per_day: 0.0,
            partition_cuts_witnesses: sim.network.partition_cuts_witnesses,
            warmup_days: 1.0,
            trace_every: if sim.trace_sample > 0.0 {
                (1.0 / sim.trace_sample).round().max(1.0) as u64
            } else {
                0
            },
        }
    }

    /// The same run with zero latency, no loss, no clock skew and no outages, and instant
    /// admission.
    pub fn idealised(mut self) -> JobSpec {
        self.idealised = true;
        self.outages = false;
        self.loss = 0.0;
        self.skew_sd = 0.0;
        self.skew_tail_fraction = 0.0;
        self.instant_admission = true;
        self
    }
}
