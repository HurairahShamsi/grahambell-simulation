//! Protocol parameters from SPEC section 2, grouped into sections.
//!
//! Every SPEC row maps to one or more [`Param`]s. Where a row carries a combined status
//! (for example "D / P"), it is split into separate parameters that each carry a single tag;
//! [`crate::registry`] records the mapping from SPEC rows to configuration keys.

use crate::param::{Param, Status, Sweep};
use serde::{Deserialize, Serialize};

/// PoW-ID issuance-rate mode (SPEC §3.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateMode {
    /// Fixed target of one PoW-ID block per target interval.
    #[default]
    Fixed,
    /// Rate may rise with demand, capped at `registered_IDs / (k × T_min)`.
    Adaptive,
}

/// How the number of KWCs relates to the number of WCs (SPEC §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KwcCountRule {
    /// Each WC leads exactly one KWC and is a subordinate in exactly as many KWCs as a KWC
    /// has subordinate WCs.
    #[default]
    EqualsWcCount,
}

/// Which KWC members must sign the entropy aggregate (SPEC §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntropySigners {
    /// Every online member; the online count must be at least the quorum.
    #[default]
    AllOnlineMembers,
}

/// Chain Allocation Committee membership policy (SPEC §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacMembershipPolicy {
    /// First in, first out.
    #[default]
    FirstInFirstOut,
}

/// How a new Chain Allocation Committee member is chosen (SPEC §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacSeatSelection {
    /// Decided rule: for every 10th PoW-Tx block, a lottery over the canonical active list
    /// (SPEC §3.11); a draw that lands on a member moves to the next position.
    #[default]
    Lottery,
    /// Comparison (the v0.2 rule): the miner of every 10th PoW-Tx block joins; a member's win
    /// passes to the next 10th-block miner who is not a member.
    TenthBlockMiner,
}

/// PoW-ID difficulty rule (SPEC §3.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DifficultyVariant {
    /// Variant B (default): count-based, from the exact admitted online count.
    #[default]
    CountBased,
    /// Variant A (comparison): Bitcoin-style retarget from recent block times.
    BitcoinRetarget,
}

/// Variant B's correction \[P\] (SPEC §3.8). The SPEC names no preferred form; M3 proposes
/// these and compares them, and none is used by default.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub enum CountBasedCorrection {
    /// P1 (the architect's example): the count-based target is multiplied by the mean of the
    /// last `window_blocks` intervals ÷ the target interval, bounded to `1 ± bound`, so
    /// difficulty is multiplied by "target interval ÷ recent average interval".
    RecentIntervalRatio {
        /// Blocks in the sliding window.
        window_blocks: u32,
        /// Largest relative change, for example 0.10 for ±10%.
        bound: f64,
    },
    /// P2: an accumulated factor, multiplied at the end of every `window_blocks` blocks by
    /// that window's mean interval ÷ the target interval, bounded to `1 ± bound`.
    Accumulated {
        /// Blocks per update.
        window_blocks: u32,
        /// Largest relative deviation of the factor from 1.
        bound: f64,
    },
    /// B-exact: the per-step success probability p is set so that 1 − (1 − p)^N equals one
    /// over the target interval exactly, for the N counted miners, with no lookback.
    DiscreteExact,
}

/// Which admitted miners Variant B's count includes (SPEC §3.8; architect, 2026-10-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DifficultyCountRule {
    /// Globally Active miners recorded online whose post-admission wait has ended.
    #[default]
    AfterPostAdmissionWait,
    /// Comparison: every Globally Active miner recorded online, including those still in the
    /// post-admission wait.
    AllGloballyActive,
}

/// What counts as miner equivocation (SPEC §3.4, Appendix A; architect, 2026-10-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquivocationScope {
    /// Two different headers at the same height signed by the same key (the SPEC as
    /// written): a miner whose parent loses sits out until the next block.
    #[default]
    SameHeight,
    /// \[P\] comparison: only two different headers on the same parent, so a miner may
    /// switch parent at the same height.
    SameParent,
}

/// How a member that fails One Chance is treated (SPEC §4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnreachableRule {
    /// The strict rule, the interim default (architect, 2026-10-12): failing One Chance is
    /// refusal and is penalised.
    #[default]
    Strict,
    /// \[P\] "unreachable is not refusal": an unreachable member is recorded offline; a
    /// penalty needs proof that it refused while demonstrably online.
    UnreachableIsNotRefusal,
}

/// The penalty for a failed One Chance under the strict rule (SPEC §4.6, §4.8; architect,
/// 2026-10-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrictOffencePenalty {
    /// The §4.8 ladder for lesser offences (24 h, then 30 days, then permanent), never reset.
    #[default]
    Ladder,
    /// \[P\] comparison: the same ladder, with the offence count reset after a year without
    /// an offence.
    LadderYearlyReset,
    /// Comparison (§4.6 read literally): a permanent ban at the first failed One Chance.
    ImmediateBan,
}

/// When an honest witness signs the PoWit (SPEC §3.6; architect, 2026-10-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WitnessSigning {
    /// When its own clock reaches `t0 + N`; δ applies to validators only.
    #[default]
    OnTime,
    /// Comparison (the earlier reading of ASSUMPTIONS P-4): when its own clock reaches
    /// `t0 + N − δ`.
    EarliestWithinTolerance,
}

/// How a node treats a block whose PoWit reached it after the deadline D (SPEC §3.7;
/// architect, 2026-10-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeadlineScope {
    /// Literal: the block, and any chain containing it, is invalid at that node for good.
    #[default]
    Permanent,
    /// Comparison: the node does not extend the block, but accepts a longer chain that
    /// contains it.
    NotExtendedOnly,
}

/// How a node treats a block it received before the block's timestamp minus δ (SPEC §3.7;
/// architect, 2026-10-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EarlyBlockRule {
    /// Literal: rejected at that node for good, because its receipt time is a fact.
    #[default]
    Permanent,
    /// Comparison: re-evaluated if it arrives again after its timestamp minus δ.
    ReevaluateLater,
}

/// A fraction `numerator / denominator`.
///
/// Used for approval thresholds (SPEC §4.3, §4.6), the adaptive-cap safety factor k
/// (SPEC §3.9) and the quorum fractions of the section G trade-off analysis. Keeping the two
/// integers, rather than a float, makes "two-thirds rounded up" exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fraction {
    /// Numerator.
    pub numerator: u64,
    /// Denominator.
    pub denominator: u64,
}

impl Fraction {
    /// The fraction `numerator / denominator`.
    pub const fn new(numerator: u64, denominator: u64) -> Self {
        Fraction {
            numerator,
            denominator,
        }
    }

    /// Approvals needed out of `members`: `⌈numerator × members / denominator⌉`.
    ///
    /// Returns `None` when the denominator is zero.
    pub fn approvals_needed(&self, members: u64) -> Option<u64> {
        if self.denominator == 0 {
            return None;
        }
        let scaled = u128::from(self.numerator) * u128::from(members);
        let denominator = u128::from(self.denominator);
        u64::try_from(scaled.div_ceil(denominator)).ok()
    }

    /// The value as `f64`, or `None` when the denominator is zero.
    pub fn to_f64(&self) -> Option<f64> {
        (self.denominator != 0).then(|| self.numerator as f64 / self.denominator as f64)
    }

    /// True when the fraction is strictly greater than one half.
    pub fn exceeds_half(&self) -> bool {
        2 * u128::from(self.numerator) > u128::from(self.denominator)
    }

    /// True when the fraction lies in `(0, 1]`.
    pub fn is_proper(&self) -> bool {
        self.denominator > 0 && self.numerator > 0 && self.numerator <= self.denominator
    }
}

/// How genesis IDs are distributed (SPEC §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenesisDistribution {
    /// KYC, one ID per verified person.
    #[default]
    KycOnePerVerifiedPerson,
}

/// Last step of the penalty ladder for lesser offences (SPEC §4.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PenaltyFinalStep {
    /// Permanent ban.
    #[default]
    Permanent,
}

/// Signature scheme (SPEC §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureScheme {
    /// BLS12-381 aggregate signatures with signer bitfield and proof of possession.
    #[default]
    #[serde(rename = "bls12_381_aggregate_bitfield_pop")]
    Bls12381AggregateBitfieldPop,
}

/// Hash function (SPEC §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HashFunction {
    /// SHA-256.
    #[default]
    Sha256,
}

/// Where the PoW-ID hash chain starts (SPEC §3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartRule {
    /// Rule (a): always at `t0 = timestamp(previous PoW-ID block) + 1 s`.
    #[default]
    A,
    /// Rule (b), comparison only: when the miner's entropy arrives locally.
    B,
}

/// Choice between two valid PoW-ID blocks at the same height (SPEC §3.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TieBreak {
    /// The SPEC v0.6 order (status P for its last step): keep the block with the earlier
    /// timestamp (smaller winning step N), then the one with the lower block hash.
    #[default]
    LowerBlockHash,
    /// Comparison: keep the block seen first.
    FirstSeen,
}

/// Hash pacing (SPEC §2 "Hash pacing").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pacing {
    /// Hash attempts per second per admitted /64 (unregistered) or per ID (registered).
    pub hashes_per_second: Param<f64>,
}

impl Default for Pacing {
    fn default() -> Self {
        Pacing {
            hashes_per_second: Param::decided(1.0),
        }
    }
}

/// PoW-ID issuance parameters (SPEC §2, §3.5, §3.7, §3.8, §3.9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Issuance {
    /// Target interval between PoW-ID blocks, in seconds.
    pub pow_id_target_interval_s: Param<f64>,
    /// Fixed or adaptive issuance rate.
    pub rate_mode: Param<RateMode>,
    /// Difficulty rule: Variant B (count-based, default) or Variant A (comparison).
    pub difficulty_variant: Param<DifficultyVariant>,
    /// Variant A comparison: retarget window K, in PoW-ID blocks.
    pub retarget_window_blocks: Param<u32>,
    /// Variant A comparison: largest difficulty change per retarget, as a factor.
    pub retarget_clamp_factor: Param<f64>,
    /// Variant B's correction \[P\] ("—" by default: Variant B uses the count alone). M3
    /// compares P1, P2 and B-exact (see [`CountBasedCorrection`]).
    pub count_based_correction: Param<Option<CountBasedCorrection>>,
    /// Which admitted miners Variant B's count includes.
    pub difficulty_count_rule: Param<DifficultyCountRule>,
    /// Minimum attack time floor `T_min` for adaptive issuance, in years.
    pub min_attack_time_floor_years: Param<f64>,
    /// Adaptive-cap checkpoint interval as a fraction of `T_min`.
    pub cap_checkpoint_interval_fraction_of_t_min: Param<f64>,
    /// Adaptive-cap safety factor k in `R ≤ registered_IDs / (k × T_min)`.
    pub cap_safety_factor: Param<Fraction>,
    /// Confirmation depth before a new ID becomes active, in PoW-ID blocks.
    pub confirmation_depth_blocks: Param<u32>,
    /// Hash-chain start rule.
    pub start_rule: Param<StartRule>,
    /// Tie-break between two valid PoW-ID blocks at the same height.
    pub same_height_tie_break: Param<TieBreak>,
}

impl Default for Issuance {
    fn default() -> Self {
        Issuance {
            pow_id_target_interval_s: Param::decided(30.0),
            rate_mode: Param::with_status(RateMode::Fixed, Status::P).swept(
                Sweep::values(vec![RateMode::Fixed, RateMode::Adaptive])
                    .with_note("fixed is decided; adaptive is the proposed variant (SPEC §3.9)"),
            ),
            difficulty_variant: Param::decided(DifficultyVariant::CountBased).swept(
                Sweep::values(vec![
                    DifficultyVariant::CountBased,
                    DifficultyVariant::BitcoinRetarget,
                ])
                .with_note("Variant A (Bitcoin-style retarget) is the comparison (SPEC §3.8)"),
            ),
            retarget_window_blocks: Param::decided(144),
            retarget_clamp_factor: Param::decided(4.0),
            count_based_correction: Param::with_status(None, Status::P).swept(
                Sweep::values(vec![
                    Some(CountBasedCorrection::RecentIntervalRatio {
                        window_blocks: 144,
                        bound: 0.10,
                    }),
                    Some(CountBasedCorrection::Accumulated {
                        window_blocks: 144,
                        bound: 0.10,
                    }),
                    Some(CountBasedCorrection::DiscreteExact),
                ])
                .with_note("P1, P2 and B-exact, proposed and compared in M3"),
            ),
            difficulty_count_rule: Param::decided(DifficultyCountRule::AfterPostAdmissionWait)
                .swept(
                    Sweep::values(vec![
                        DifficultyCountRule::AfterPostAdmissionWait,
                        DifficultyCountRule::AllGloballyActive,
                    ])
                    .with_note(
                        "all Globally Active miners is the comparison (architect, 2026-10-12)",
                    ),
                ),
            min_attack_time_floor_years: Param::with_status(2.0, Status::P)
                .swept(Sweep::range(1.0, 10.0)),
            cap_checkpoint_interval_fraction_of_t_min: Param::decided(1.0)
                .swept(Sweep::values(vec![0.25, 0.5, 1.0])),
            cap_safety_factor: Param::decided(Fraction::new(7, 6)),
            confirmation_depth_blocks: Param::with_status(6, Status::O).swept(Sweep::range(3, 12)),
            start_rule: Param::decided(StartRule::A).swept(
                Sweep::values(vec![StartRule::A, StartRule::B])
                    .with_note("rule (b) is the rejected alternative, kept for S13"),
            ),
            same_height_tie_break: Param::with_status(TieBreak::LowerBlockHash, Status::P).swept(
                Sweep::values(vec![TieBreak::LowerBlockHash, TieBreak::FirstSeen]),
            ),
        }
    }
}

/// Transaction-layer parameters (SPEC §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transactions {
    /// Interval between PoW-Tx blocks, in seconds.
    pub pow_tx_interval_s: Param<f64>,
}

impl Default for Transactions {
    fn default() -> Self {
        Transactions {
            pow_tx_interval_s: Param::with_status(10.0, Status::O).swept(Sweep::range(1.0, 10.0)),
        }
    }
}

/// Witness Chain structure and allocation (SPEC §2, §4.1, §4.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Witness {
    /// Witness nodes per WC.
    pub wc_size: Param<u32>,
    /// Subordinate WCs per KWC (3 gives 40-node KWCs; 2 is the 30-node comparison).
    pub subordinate_wcs_per_kwc: Param<u32>,
    /// Relation between the number of KWCs and WCs.
    pub kwc_count_rule: Param<KwcCountRule>,
    /// Ring offsets of the subordinate WCs: KWC `w` has subordinates `(w + offset) mod W`.
    pub ring_offsets: Param<Vec<u32>>,
    /// Ring offsets in the 30-node comparison layout.
    pub ring_offsets_30_node: Param<Vec<u32>>,
    /// Blocks between an ID's confirmation and the block whose hash is its allocation beacon.
    pub allocation_beacon_delay_blocks: Param<u32>,
    /// Registered miners watched per witness node.
    pub watched_registered_per_node: Param<u32>,
    /// Unregistered miners watched per witness node.
    pub watched_unregistered_per_node: Param<u32>,
}

impl Default for Witness {
    fn default() -> Self {
        Witness {
            wc_size: Param::decided(10),
            subordinate_wcs_per_kwc: Param::decided(3)
                .swept(Sweep::values(vec![3, 2]).with_note("1 + 2 (30 nodes) for comparison")),
            kwc_count_rule: Param::decided(KwcCountRule::EqualsWcCount),
            ring_offsets: Param::decided(vec![1, 4, 6]),
            ring_offsets_30_node: Param::decided(vec![1, 3]),
            allocation_beacon_delay_blocks: Param::with_status(6, Status::O)
                .swept(Sweep::values(vec![3, 6, 12])),
            watched_registered_per_node: Param::decided(800),
            watched_unregistered_per_node: Param::decided(200),
        }
    }
}

/// PoWit quorums (SPEC §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quorum {
    /// Registered miners: minimum signatures from the leader WC.
    pub registered_leader_min: Param<u32>,
    /// Registered miners: minimum signatures from the subordinate WCs (40-node KWC).
    pub registered_subordinate_min: Param<u32>,
    /// Registered miners: minimum subordinate signatures in the 30-node comparison.
    pub registered_subordinate_min_30_node: Param<u32>,
    /// Unregistered miners: minimum signatures from any KWC members (40-node KWC).
    pub unregistered_total_min: Param<u32>,
    /// Unregistered miners: minimum signatures in the 30-node comparison.
    pub unregistered_total_min_30_node: Param<u32>,
}

impl Default for Quorum {
    fn default() -> Self {
        Quorum {
            registered_leader_min: Param::decided(7),
            registered_subordinate_min: Param::decided(21),
            registered_subordinate_min_30_node: Param::decided(14),
            unregistered_total_min: Param::decided(27),
            unregistered_total_min_30_node: Param::decided(20),
        }
    }
}

/// Entropy lock (SPEC §2, §3.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entropy {
    /// Who must sign the entropy aggregate.
    pub signers: Param<EntropySigners>,
    /// What counts as two headers from one miner in one round (equivocation).
    pub equivocation_scope: Param<EquivocationScope>,
}

impl Default for Entropy {
    fn default() -> Self {
        Entropy {
            signers: Param::decided(EntropySigners::AllOnlineMembers),
            equivocation_scope: Param::decided(EquivocationScope::SameHeight).swept(
                Sweep::values(vec![
                    EquivocationScope::SameHeight,
                    EquivocationScope::SameParent,
                ])
                .with_note("same parent is the [P] comparison (architect, 2026-10-12)"),
            ),
        }
    }
}

/// Miner capacity and spare witnessing capacity (SPEC §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capacity {
    /// Total miners (registered + unregistered) per leader WC.
    pub miners_per_leader_wc: Param<u32>,
    /// Unregistered miners per leader WC; the registered share is the remainder.
    pub unregistered_per_leader_wc: Param<u32>,
    /// Spare witnessing capacity target over demand (0.33 means +33%).
    pub spare_target_fraction: Param<f64>,
}

impl Default for Capacity {
    fn default() -> Self {
        Capacity {
            miners_per_leader_wc: Param::decided(250),
            unregistered_per_leader_wc: Param::with_status(50, Status::O).swept(
                Sweep::values(vec![50, 150, 235])
                    .with_note("registered/unregistered split 200/50, 100/150, 15/235"),
            ),
            spare_target_fraction: Param::decided(0.33),
        }
    }
}

impl Capacity {
    /// Registered miners per leader WC implied by the total and the unregistered share.
    pub fn registered_per_leader_wc(&self) -> Option<u32> {
        self.miners_per_leader_wc
            .value
            .checked_sub(self.unregistered_per_leader_wc.value)
    }
}

/// Network limits (SPEC §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    /// Per-node request rate limit, in requests per second.
    pub node_request_rate_limit_per_s: Param<u32>,
}

impl Default for Network {
    fn default() -> Self {
        Network {
            node_request_rate_limit_per_s: Param::decided(1000),
        }
    }
}

/// Timing parameters (SPEC §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    /// Grace epoch, in rounds of the miner's own block type.
    pub grace_epoch_rounds: Param<u32>,
    /// Clock tolerance δ, in seconds.
    pub clock_tolerance_s: Param<f64>,
    /// Wait before One Chance is judged to have failed, in rounds (SPEC §4.5; one grace epoch
    /// by default).
    pub one_chance_wait_rounds: Param<u32>,
    /// PoWit arrival deadline D, in seconds after the block's timestamp (SPEC §3.7; "—" until
    /// chosen).
    pub powit_deadline_s: Param<Option<f64>>,
    /// How a node treats a block whose PoWit reached it after D.
    pub powit_deadline_scope: Param<DeadlineScope>,
    /// How a node treats a block it received before its timestamp minus δ.
    pub early_block: Param<EarlyBlockRule>,
    /// When an honest witness signs the PoWit.
    pub witness_signing: Param<WitnessSigning>,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            powit_deadline_scope: Param::decided(DeadlineScope::Permanent).swept(
                Sweep::values(vec![
                    DeadlineScope::Permanent,
                    DeadlineScope::NotExtendedOnly,
                ])
                .with_note("not extended only is the comparison (architect, 2026-10-12)"),
            ),
            early_block: Param::decided(EarlyBlockRule::Permanent).swept(
                Sweep::values(vec![
                    EarlyBlockRule::Permanent,
                    EarlyBlockRule::ReevaluateLater,
                ])
                .with_note("re-evaluation is the comparison (architect, 2026-10-12)"),
            ),
            witness_signing: Param::decided(WitnessSigning::OnTime).swept(
                Sweep::values(vec![
                    WitnessSigning::OnTime,
                    WitnessSigning::EarliestWithinTolerance,
                ])
                .with_note("earliest within δ (the earlier P-4 reading) is the comparison"),
            ),
            grace_epoch_rounds: Param::decided(5),
            clock_tolerance_s: Param::with_status(2.0, Status::O).swept(Sweep::range(0.5, 10.0)),
            one_chance_wait_rounds: Param::with_status(5, Status::O)
                .swept(Sweep::values(vec![1, 2, 5])),
            powit_deadline_s: Param::with_status(None, Status::O).swept(Sweep::values(vec![
                Some(5.0),
                Some(10.0),
                Some(30.0),
                Some(60.0),
            ])),
        }
    }
}

/// Admission lifecycle parameters (SPEC §2, §3.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    /// Wait after admission before mining, in rounds.
    pub post_admission_wait_rounds: Param<u32>,
    /// Convergence interval, in epochs.
    pub convergence_interval_epochs: Param<f64>,
    /// Registered peers a newcomer bootstraps from.
    pub peer_bootstrap_count: Param<u32>,
}

impl Default for Admission {
    fn default() -> Self {
        Admission {
            post_admission_wait_rounds: Param::with_status(5, Status::O).swept(Sweep::range(1, 10)),
            convergence_interval_epochs: Param::with_status(1.0, Status::O)
                .swept(Sweep::range(0.5, 3.0)),
            peer_bootstrap_count: Param::decided(8).swept(Sweep::range(4, 8)),
        }
    }
}

/// Chain Allocation Committee (SPEC §2, §4.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cac {
    /// Committee size.
    pub size: Param<u32>,
    /// Membership policy.
    pub membership_policy: Param<CacMembershipPolicy>,
    /// One new member joins for every n-th PoW-Tx block.
    pub join_every_n_tx_blocks: Param<u32>,
    /// How the new member is chosen.
    pub seat_selection: Param<CacSeatSelection>,
    /// Lottery beacon offset k: the draw for block B uses the hash of block B + k.
    pub lottery_beacon_offset_blocks: Param<u32>,
    /// Approval threshold, rounded up.
    pub approval_threshold: Param<Fraction>,
}

impl Default for Cac {
    fn default() -> Self {
        Cac {
            size: Param::decided(600)
                .swept(Sweep::range(100, 1000).with_note("100–1,000 for comparison")),
            membership_policy: Param::decided(CacMembershipPolicy::FirstInFirstOut),
            join_every_n_tx_blocks: Param::decided(10),
            seat_selection: Param::decided(CacSeatSelection::Lottery).swept(
                Sweep::values(vec![
                    CacSeatSelection::Lottery,
                    CacSeatSelection::TenthBlockMiner,
                ])
                .with_note("the v0.2 rule (miner of every 10th PoW-Tx block) is the comparison"),
            ),
            lottery_beacon_offset_blocks: Param::with_status(3, Status::O)
                .swept(Sweep::values(vec![1, 3, 6, 12])),
            approval_threshold: Param::decided(Fraction::new(2, 3)),
        }
    }
}

/// Genesis distribution (SPEC §2, §10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Genesis {
    /// Number of genesis IDs, G.
    pub ids: Param<u64>,
    /// Distribution method.
    pub distribution: Param<GenesisDistribution>,
    /// Fraction of genesis IDs secretly controlled by one party.
    pub secretly_controlled_fraction: Param<f64>,
}

impl Default for Genesis {
    fn default() -> Self {
        Genesis {
            ids: Param::decided(2_900_000).swept(Sweep::values(vec![
                1_000_000, 2_100_000, 2_900_000, 5_000_000, 10_000_000,
            ])),
            distribution: Param::decided(GenesisDistribution::KycOnePerVerifiedPerson),
            secretly_controlled_fraction: Param::with_status(0.0, Status::O)
                .swept(Sweep::range(0.0, 0.2)),
        }
    }
}

/// Offline handling (SPEC §2, §4.7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offline {
    /// Absence after which an ID is deactivated, in seconds ("—" until chosen).
    pub deactivation_threshold_s: Param<Option<f64>>,
    /// Wait before a returning ID is re-activated, in seconds ("—" until chosen).
    pub reactivation_wait_s: Param<Option<f64>>,
    /// Long-absence threshold L, in seconds: a seat is vacated only after a continuous absence
    /// longer than this (or a ban).
    pub long_absence_threshold_s: Param<f64>,
    /// How a member that fails One Chance is treated (SPEC §4.6).
    pub unreachable_rule: Param<UnreachableRule>,
}

impl Default for Offline {
    fn default() -> Self {
        const HOUR: f64 = 3_600.0;
        const DAY: f64 = 86_400.0;
        Offline {
            unreachable_rule: Param::with_status(UnreachableRule::Strict, Status::O).swept(
                Sweep::values(vec![
                    UnreachableRule::Strict,
                    UnreachableRule::UnreachableIsNotRefusal,
                ])
                .with_note(
                    "strict is the interim default; unreachable is not refusal is [P] (SPEC §4.6, §12)",
                ),
            ),
            deactivation_threshold_s: Param::with_status(None, Status::O).swept(Sweep::values(
                vec![
                    Some(HOUR),
                    Some(6.0 * HOUR),
                    Some(24.0 * HOUR),
                    Some(3.0 * DAY),
                    Some(7.0 * DAY),
                ],
            )),
            reactivation_wait_s: Param::with_status(None, Status::O).swept(Sweep::values(vec![
                Some(DAY),
                Some(7.0 * DAY),
                Some(14.0 * DAY),
                Some(30.0 * DAY),
            ])),
            long_absence_threshold_s: Param::with_status(30.0 * DAY, Status::O).swept(
                Sweep::values(vec![
                    3.0 * DAY,
                    7.0 * DAY,
                    30.0 * DAY,
                    90.0 * DAY,
                    365.0 * DAY,
                ]),
            ),
        }
    }
}

/// Penalties for lesser offences (SPEC §2, §4.8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Penalties {
    /// Successive suspensions before the final step, in seconds (24 h, then 30 days).
    pub suspension_ladder_s: Param<Vec<u64>>,
    /// Final step of the ladder.
    pub final_step: Param<PenaltyFinalStep>,
    /// The penalty for a failed One Chance under the strict rule.
    pub strict_offence: Param<StrictOffencePenalty>,
}

impl Default for Penalties {
    fn default() -> Self {
        Penalties {
            suspension_ladder_s: Param::decided(vec![86_400, 2_592_000]),
            final_step: Param::decided(PenaltyFinalStep::Permanent),
            strict_offence: Param::decided(StrictOffencePenalty::Ladder).swept(
                Sweep::values(vec![
                    StrictOffencePenalty::Ladder,
                    StrictOffencePenalty::LadderYearlyReset,
                    StrictOffencePenalty::ImmediateBan,
                ])
                .with_note(
                    "yearly reset is [P]; immediate ban is §4.6 read literally (architect, 2026-10-12)",
                ),
            ),
        }
    }
}

/// Cryptographic primitives (SPEC §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crypto {
    /// Signature scheme.
    pub signature_scheme: Param<SignatureScheme>,
    /// Hash function.
    pub hash_function: Param<HashFunction>,
}

impl Default for Crypto {
    fn default() -> Self {
        Crypto {
            signature_scheme: Param::decided(SignatureScheme::Bls12381AggregateBitfieldPop),
            hash_function: Param::with_status(HashFunction::Sha256, Status::P),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_thirds_rounded_up_gives_spec_values() {
        let rule = Fraction::new(2, 3);
        assert_eq!(rule.approvals_needed(600), Some(400));
        assert_eq!(rule.approvals_needed(40), Some(27));
        assert_eq!(rule.approvals_needed(30), Some(20));
        assert_eq!(rule.approvals_needed(100), Some(67));
        assert_eq!(rule.approvals_needed(1000), Some(667));
    }

    #[test]
    fn quorum_fractions_round_up_as_in_the_section_g_preview() {
        // Reference: hand-computed ceilings, for example ⌈0.51 × 40⌉ = ⌈20.4⌉ = 21.
        assert_eq!(Fraction::new(51, 100).approvals_needed(40), Some(21));
        assert_eq!(Fraction::new(55, 100).approvals_needed(40), Some(22));
        assert_eq!(Fraction::new(60, 100).approvals_needed(40), Some(24));
        assert_eq!(Fraction::new(75, 100).approvals_needed(40), Some(30));
        assert_eq!(Fraction::new(51, 100).approvals_needed(600), Some(306));
        assert_eq!(Fraction::new(2, 3).approvals_needed(10), Some(7));
    }

    #[test]
    fn zero_denominator_has_no_threshold_or_value() {
        let rule = Fraction::new(2, 0);
        assert_eq!(rule.approvals_needed(600), None);
        assert_eq!(rule.to_f64(), None);
        assert!(!rule.is_proper());
    }

    #[test]
    fn half_is_not_above_half() {
        assert!(!Fraction::new(1, 2).exceeds_half());
        assert!(Fraction::new(51, 100).exceeds_half());
        assert!(Fraction::new(7, 6).exceeds_half());
        assert!(Fraction::new(1, 1).is_proper() && !Fraction::new(7, 6).is_proper());
        assert_eq!(Fraction::new(7, 6).to_f64(), Some(7.0 / 6.0));
    }

    #[test]
    fn registered_capacity_is_total_minus_unregistered() {
        assert_eq!(Capacity::default().registered_per_leader_wc(), Some(200));
    }

    #[test]
    fn genesis_default_is_two_point_nine_million_decided() {
        let genesis = Genesis::default();
        assert_eq!(genesis.ids.value, 2_900_000);
        assert_eq!(genesis.ids.status, Status::D);
    }

    #[test]
    fn long_absence_threshold_defaults_to_thirty_days_and_is_open() {
        let offline = Offline::default();
        assert_eq!(offline.long_absence_threshold_s.value, 2_592_000.0);
        assert_eq!(offline.long_absence_threshold_s.status, Status::O);
        let sweep = offline.long_absence_threshold_s.sweep.unwrap();
        assert_eq!(
            sweep.values,
            vec![259_200.0, 604_800.0, 2_592_000.0, 7_776_000.0, 31_536_000.0]
        );
    }

    #[test]
    fn spec_v0_3_defaults_are_recorded() {
        let issuance = Issuance::default();
        assert_eq!(issuance.cap_safety_factor.value, Fraction::new(7, 6));
        assert_eq!(
            issuance.cap_checkpoint_interval_fraction_of_t_min.value,
            1.0
        );
        assert_eq!(
            issuance.difficulty_variant.value,
            DifficultyVariant::CountBased
        );
        assert_eq!(issuance.count_based_correction.status, Status::P);
        let cac = Cac::default();
        assert_eq!(cac.seat_selection.value, CacSeatSelection::Lottery);
        assert_eq!(cac.lottery_beacon_offset_blocks.value, 3);
        assert_eq!(cac.lottery_beacon_offset_blocks.status, Status::O);
        let witness = Witness::default();
        assert_eq!(witness.ring_offsets.value, vec![1, 4, 6]);
        assert_eq!(witness.allocation_beacon_delay_blocks.value, 6);
    }
}
