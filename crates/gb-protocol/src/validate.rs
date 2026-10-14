//! The witness signing condition and global validation (SPEC §3.6–§3.7).
//!
//! [`validate`] checks one PoW-ID block in the order of SPEC §3.7 and returns the first
//! failure as a [`Rejection`]. Time values are seconds on the validator's own clock.

use crate::chain::{ChainInputs, StartRule, below_target, chain_start};
use crate::encoding::{Header, block_hash};
use crate::entropy::{
    Bitfield, PublicKeyBytes, SignatureBytes, entropy_output, powit_message, verify_entropy,
    verify_proof_of_possession, verify_same_message,
};
use crate::error::ProtocolError;
use crate::hash::Hash;
use gb_config::Config;
use gb_config::protocol::{DeadlineScope, EarlyBlockRule, EquivocationScope, WitnessSigning};
use std::cmp::Ordering;

/// Which miners a quorum applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinerKind {
    /// Unregistered miners (PoW-ID): any `unregistered_total_min` of all seats.
    Unregistered,
    /// Registered miners (PoW-Tx): leader-WC and subordinate minimums.
    Registered,
}

/// A PoWit quorum over the KWC's seats in seat order (leader WC first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuorumRule {
    /// Any `quorum` of `seats`.
    Pool {
        /// Seats.
        seats: usize,
        /// Signatures needed.
        quorum: usize,
    },
    /// At least `leader_quorum` of the first `leader_seats` and `subordinate_quorum` of the
    /// rest.
    TwoGroup {
        /// Leader-WC seats.
        leader_seats: usize,
        /// Leader signatures needed.
        leader_quorum: usize,
        /// Subordinate seats.
        subordinate_seats: usize,
        /// Subordinate signatures needed.
        subordinate_quorum: usize,
    },
}

impl QuorumRule {
    /// The SPEC §2 quorum for a 40-node KWC from the configuration.
    pub fn from_config(config: &Config, kind: MinerKind) -> QuorumRule {
        let wc = config.witness.wc_size.value as usize;
        let subordinates = wc * config.witness.subordinate_wcs_per_kwc.value as usize;
        let q = &config.quorum;
        match kind {
            MinerKind::Unregistered => QuorumRule::Pool {
                seats: wc + subordinates,
                quorum: q.unregistered_total_min.value as usize,
            },
            MinerKind::Registered => QuorumRule::TwoGroup {
                leader_seats: wc,
                leader_quorum: q.registered_leader_min.value as usize,
                subordinate_seats: subordinates,
                subordinate_quorum: q.registered_subordinate_min.value as usize,
            },
        }
    }

    /// Seats in the KWC.
    pub fn seats(&self) -> usize {
        match *self {
            QuorumRule::Pool { seats, .. } => seats,
            QuorumRule::TwoGroup {
                leader_seats,
                subordinate_seats,
                ..
            } => leader_seats + subordinate_seats,
        }
    }

    /// True when the signers in `bitfield` meet the quorum.
    pub fn is_met(&self, bitfield: &Bitfield) -> bool {
        if bitfield.seats() != self.seats() {
            return false;
        }
        match *self {
            QuorumRule::Pool { quorum, .. } => bitfield.count() >= quorum,
            QuorumRule::TwoGroup {
                leader_seats,
                leader_quorum,
                subordinate_quorum,
                ..
            } => {
                let signers = bitfield.signers();
                let leader = signers.iter().filter(|s| **s < leader_seats).count();
                leader >= leader_quorum && signers.len() - leader >= subordinate_quorum
            }
        }
    }
}

/// The KWC's member keys in seat order, each with a checked proof of possession.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KwcKeys {
    keys: Vec<PublicKeyBytes>,
}

impl KwcKeys {
    /// Accepts member keys only with valid proofs of possession.
    pub fn new(members: &[(PublicKeyBytes, SignatureBytes)]) -> Result<KwcKeys, ProtocolError> {
        for (pk, pop) in members {
            if !verify_proof_of_possession(pk, pop) {
                return Err(ProtocolError::Bls(
                    "invalid proof of possession".to_string(),
                ));
            }
        }
        Ok(KwcKeys {
            keys: members.iter().map(|(pk, _)| *pk).collect(),
        })
    }

    /// Keys in seat order.
    pub fn keys(&self) -> &[PublicKeyBytes] {
        &self.keys
    }
}

/// A PoW-ID block with its entropy and its PoWit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The header.
    pub header: Header,
    /// The miner's signature on the header.
    pub miner_signature: SignatureBytes,
    /// The entropy aggregate (σ_m and the online members' signatures).
    pub entropy_aggregate: SignatureBytes,
    /// Members whose entropy signatures are in the aggregate.
    pub entropy_signers: Bitfield,
    /// The winning step N.
    pub winning_step: u64,
    /// The block timestamp, which must equal `t0 + N`.
    pub timestamp: u64,
    /// The PoWit aggregate signature on `"GB/powit" ‖ block hash`.
    pub powit_aggregate: SignatureBytes,
    /// PoWit signers.
    pub powit_signers: Bitfield,
}

impl Block {
    /// The entropy E this block commits to.
    pub fn entropy(&self) -> Hash {
        entropy_output(&self.entropy_aggregate, &self.entropy_signers)
    }

    /// The block hash.
    pub fn hash(&self) -> Hash {
        block_hash(&self.header, &self.entropy(), self.winning_step)
    }
}

/// The chain tip the block extends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tip {
    /// Hash of the tip.
    pub hash: Hash,
    /// Height of the tip.
    pub height: u64,
    /// Timestamp of the tip.
    pub timestamp: u64,
}

/// When the validator received the block and its PoWit, on its own clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reception {
    /// Arrival time of the block.
    pub block_at: f64,
    /// Arrival time of the PoWit.
    pub powit_at: f64,
}

/// Validation parameters (SPEC §2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rules {
    /// Clock tolerance δ, seconds.
    pub clock_tolerance_s: f64,
    /// PoWit arrival deadline D, seconds after the block timestamp; `None` disables it.
    pub powit_deadline_s: Option<f64>,
    /// Chain start rule (rule (a) is the protocol; (b) is the S13 comparison).
    pub start_rule: StartRule,
    /// Steps a validator recomputes at most before rejecting.
    pub max_steps: u64,
}

impl Rules {
    /// Rules from the configuration, with start rule (a).
    pub fn from_config(config: &Config) -> Rules {
        Rules {
            clock_tolerance_s: config.timing.clock_tolerance_s.value,
            powit_deadline_s: config.timing.powit_deadline_s.value,
            start_rule: StartRule::A,
            max_steps: 1_000_000,
        }
    }
}

/// Why a block is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    /// Not the next height on the tip, or the previous hash does not match (serialisation).
    NotOnTip,
    /// The candidate key or address is already registered or active (uniqueness).
    NotUnique,
    /// Fewer online members signed the entropy than the quorum.
    EntropyBelowQuorum,
    /// The entropy aggregate does not verify.
    BadEntropy,
    /// `h_N` is not below the target.
    NotWinning,
    /// An earlier step already wins, so N is not the first winning step.
    EarlierWinningStep,
    /// `timestamp ≠ t0 + N`.
    TimestampMismatch,
    /// The block arrived before its own timestamp (beyond δ).
    ReceivedEarly,
    /// The PoWit signers do not meet the quorum.
    PowitBelowQuorum,
    /// The PoWit aggregate does not verify.
    BadPowit,
    /// The PoWit arrived later than D after the block timestamp.
    PowitLate,
}

/// SPEC §3.6: an honest member signs the PoWit only when its own clock has reached
/// `t0 + N`, within tolerance δ.
pub fn may_sign(own_clock: f64, t0: u64, winning_step: u64, clock_tolerance_s: f64) -> bool {
    own_clock >= (t0 + winning_step) as f64 - clock_tolerance_s
}

/// A chain tip as fork choice sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForkCandidate {
    /// Height of the tip.
    pub height: u64,
    /// Timestamp of the tip block (`t0 + N`).
    pub timestamp: u64,
    /// Block hash of the tip.
    pub hash: Hash,
}

/// SPEC §3.7 fork choice between two valid tips, in order: the longer chain; at the same
/// height, the earlier timestamp (smaller winning step N); at the same height and timestamp,
/// the lower block hash \[P\]. Returns `Ordering::Less` when `a` is preferred.
pub fn fork_choice(a: &ForkCandidate, b: &ForkCandidate) -> Ordering {
    b.height
        .cmp(&a.height)
        .then(a.timestamp.cmp(&b.timestamp))
        .then(a.hash.cmp(&b.hash))
}

/// SPEC §3.7 same-height tie-break: the earlier timestamp wins, then the lower block hash
/// \[P\]. Arguments are `(timestamp, block hash)`; returns `Ordering::Less` when `a` is
/// preferred.
pub fn prefer(a: (u64, &Hash), b: (u64, &Hash)) -> Ordering {
    a.0.cmp(&b.0).then(a.1.cmp(b.1))
}

/// First-seen fork choice, the comparison to SPEC §3.7 (M3 C2): the longer chain; at the same
/// height, the tip this node received first (`a_seen`, `b_seen` on its own clock); then the
/// lower block hash. Returns `Ordering::Less` when `a` is preferred.
pub fn fork_choice_first_seen(
    a: &ForkCandidate,
    a_seen: f64,
    b: &ForkCandidate,
    b_seen: f64,
) -> Ordering {
    b.height
        .cmp(&a.height)
        .then(a_seen.total_cmp(&b_seen))
        .then(a.hash.cmp(&b.hash))
}

/// Lower-hash-only fork choice, the comparison of M3 C2 (iii): the longer chain, then the
/// lower block hash, ignoring timestamps. Returns `Ordering::Less` when `a` is preferred.
pub fn fork_choice_lower_hash(a: &ForkCandidate, b: &ForkCandidate) -> Ordering {
    b.height.cmp(&a.height).then(a.hash.cmp(&b.hash))
}

/// The tolerance an honest witness applies in [`may_sign`] (SPEC §3.6; architect,
/// 2026-10-12): zero when it signs on time, at its own clock `t0 + N` (the default); δ under
/// the earlier reading of ASSUMPTIONS P-4, at `t0 + N − δ`.
pub fn witness_signing_tolerance(rule: WitnessSigning, clock_tolerance_s: f64) -> f64 {
    match rule {
        WitnessSigning::OnTime => 0.0,
        WitnessSigning::EarliestWithinTolerance => clock_tolerance_s,
    }
}

/// SPEC §3.4: whether signing `new` after `previous`, with the same candidate key, would be
/// equivocation. Under the SPEC as written any two different headers at one height are; under
/// the \[P\] comparison only two different headers on the same parent are. An honest miner
/// asks this before signing a second header.
pub fn equivocates(scope: EquivocationScope, previous: &Header, new: &Header) -> bool {
    previous != new
        && previous.height == new.height
        && previous.candidate_pk == new.candidate_pk
        && match scope {
            EquivocationScope::SameHeight => true,
            EquivocationScope::SameParent => previous.prev_hash == new.prev_hash,
        }
}

/// SPEC §3.7 PoWit deadline at a node that received a block's PoWit late. The node never
/// extends that block; this returns whether it still accepts a longer chain that contains it
/// (never under the literal rule; yes under the comparison).
pub fn accepts_chain_through_late_block(scope: DeadlineScope) -> bool {
    match scope {
        DeadlineScope::Permanent => false,
        DeadlineScope::NotExtendedOnly => true,
    }
}

/// SPEC §3.7: whether a node that received a block before its timestamp minus δ accepts it
/// when it arrives again later (never under the literal rule; yes under the comparison).
pub fn reevaluates_early_block(rule: EarlyBlockRule) -> bool {
    match rule {
        EarlyBlockRule::Permanent => false,
        EarlyBlockRule::ReevaluateLater => true,
    }
}

/// SPEC §4.6 witness double-signing \[D\]: the seats that signed valid PoWits for two
/// different blocks from the same miner at the same height. Each PoWit aggregate is verified,
/// and proofs of possession (checked by [`KwcKeys::new`]) make every marked signer accountable
/// for its signature. Returns no seats unless both aggregates verify.
pub fn witness_equivocators(a: &Block, b: &Block, kwc: &KwcKeys) -> Vec<usize> {
    let (ha, hb) = (a.hash(), b.hash());
    let same_round = a.header.height == b.header.height
        && a.header.candidate_pk == b.header.candidate_pk
        && ha != hb;
    let valid = |block: &Block, hash: &Hash| {
        verify_same_message(
            kwc.keys(),
            &block.powit_signers,
            &powit_message(hash),
            &block.powit_aggregate,
        )
    };
    if !same_round || !valid(a, &ha) || !valid(b, &hb) {
        return Vec::new();
    }
    a.powit_signers
        .signers()
        .into_iter()
        .filter(|seat| b.powit_signers.get(*seat))
        .collect()
}

/// Serialisation (SPEC §3.7): the header must be the next height on `tip` and name it as its
/// previous block.
pub fn check_on_tip(header: &Header, tip: &Tip) -> Result<(), Rejection> {
    if header.height != tip.height + 1 || header.prev_hash != tip.hash {
        return Err(Rejection::NotOnTip);
    }
    Ok(())
}

/// The entropy signers must meet the miner kind's quorum (SPEC §3.4: online members must
/// number at least the quorum).
pub fn check_entropy_quorum(quorum: &QuorumRule, signers: &Bitfield) -> Result<(), Rejection> {
    if !quorum.is_met(signers) {
        return Err(Rejection::EntropyBelowQuorum);
    }
    Ok(())
}

/// Full chain recomputation (SPEC §3.5, §3.7): `h_N` must be below the target and N must be
/// the first winning step of the chain that starts at `t0` with entropy `entropy`.
pub fn check_chain(
    header: &Header,
    entropy: &Hash,
    t0: u64,
    winning_step: u64,
    max_steps: u64,
) -> Result<(), Rejection> {
    let inputs = ChainInputs::new(header, *entropy, t0);
    if winning_step >= max_steps {
        return Err(Rejection::NotWinning);
    }
    let mut first_win = None;
    let mut last = [0u8; 32];
    for (n, hash) in inputs.steps().take(winning_step as usize + 1) {
        if below_target(&hash, &header.target) && first_win.is_none() {
            first_win = Some(n);
        }
        last = hash;
    }
    if !below_target(&last, &header.target) {
        return Err(Rejection::NotWinning);
    }
    if first_win != Some(winning_step) {
        return Err(Rejection::EarlierWinningStep);
    }
    Ok(())
}

/// The block timestamp must equal `t0 + N` (SPEC §3.5).
pub fn check_timestamp(timestamp: u64, t0: u64, winning_step: u64) -> Result<(), Rejection> {
    if timestamp != t0 + winning_step {
        return Err(Rejection::TimestampMismatch);
    }
    Ok(())
}

/// The block must not be received before its own timestamp, within tolerance δ, on the
/// validator's own clock (SPEC §3.7).
pub fn check_received(
    block_at: f64,
    timestamp: u64,
    clock_tolerance_s: f64,
) -> Result<(), Rejection> {
    if block_at < timestamp as f64 - clock_tolerance_s {
        return Err(Rejection::ReceivedEarly);
    }
    Ok(())
}

/// The PoWit signers must meet the miner kind's quorum (SPEC §2, §3.6).
pub fn check_powit_quorum(quorum: &QuorumRule, signers: &Bitfield) -> Result<(), Rejection> {
    if !quorum.is_met(signers) {
        return Err(Rejection::PowitBelowQuorum);
    }
    Ok(())
}

/// The PoWit must arrive within D seconds of the block timestamp, on the validator's own
/// clock (SPEC §3.7); `None` disables the deadline.
pub fn check_powit_deadline(
    powit_at: f64,
    timestamp: u64,
    deadline_s: Option<f64>,
) -> Result<(), Rejection> {
    if let Some(deadline) = deadline_s {
        if powit_at > timestamp as f64 + deadline {
            return Err(Rejection::PowitLate);
        }
    }
    Ok(())
}

/// Validates `block` on `tip` (SPEC §3.7). `unique` reports whether the candidate key and the
/// address are free. Returns the block hash when valid.
///
/// The checks run in this order and the first failure is returned: [`check_on_tip`],
/// uniqueness, [`check_entropy_quorum`], the entropy aggregate, [`check_chain`],
/// [`check_timestamp`], [`check_received`], [`check_powit_quorum`], the PoWit aggregate and
/// [`check_powit_deadline`]. [`validate_facts`] runs the same checks without the two BLS
/// verifications.
pub fn validate(
    block: &Block,
    tip: &Tip,
    kwc: &KwcKeys,
    quorum: &QuorumRule,
    reception: &Reception,
    rules: &Rules,
    unique: &dyn Fn(&Header) -> bool,
) -> Result<Hash, Rejection> {
    let h = &block.header;
    check_on_tip(h, tip)?;
    if !unique(h) {
        return Err(Rejection::NotUnique);
    }
    check_entropy_quorum(quorum, &block.entropy_signers)?;
    if !verify_entropy(
        h,
        &block.miner_signature,
        kwc.keys(),
        &block.entropy_signers,
        &block.entropy_aggregate,
    ) {
        return Err(Rejection::BadEntropy);
    }
    let t0 = chain_start(tip.timestamp, rules.start_rule);
    check_chain(h, &block.entropy(), t0, block.winning_step, rules.max_steps)?;
    check_timestamp(block.timestamp, t0, block.winning_step)?;
    check_received(reception.block_at, block.timestamp, rules.clock_tolerance_s)?;
    check_powit_quorum(quorum, &block.powit_signers)?;
    let hash = block.hash();
    if !verify_same_message(
        kwc.keys(),
        &block.powit_signers,
        &powit_message(&hash),
        &block.powit_aggregate,
    ) {
        return Err(Rejection::BadPowit);
    }
    check_powit_deadline(reception.powit_at, block.timestamp, rules.powit_deadline_s)?;
    Ok(hash)
}

/// The facts of a block that validation reads, without its signatures. Simulators that stand
/// in for BLS (ARCHITECTURE §3) validate these with [`validate_facts`].
#[derive(Debug, Clone, Copy)]
pub struct BlockFacts<'a> {
    /// The header.
    pub header: &'a Header,
    /// The entropy E the block commits to.
    pub entropy: Hash,
    /// Members whose entropy signatures are in the aggregate.
    pub entropy_signers: &'a Bitfield,
    /// The winning step N.
    pub winning_step: u64,
    /// The block timestamp.
    pub timestamp: u64,
    /// PoWit signers.
    pub powit_signers: &'a Bitfield,
}

/// Whether [`validate_facts`] recomputes the hash chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainCheck {
    /// Recompute the full chain, as [`validate`] does.
    Recompute,
    /// Trust the winning step: only for a simulator that drew N from the per-step success
    /// probability instead of hashing (its equivalence is tested before use).
    Trusted,
}

/// [`validate`] without the two BLS verifications (entropy and PoWit aggregates), in the
/// same order, for blocks whose signatures a simulator stands in for. With
/// [`ChainCheck::Recompute`] it agrees with [`validate`] on every block whose two aggregates
/// verify.
pub fn validate_facts(
    facts: &BlockFacts<'_>,
    tip: &Tip,
    quorum: &QuorumRule,
    reception: &Reception,
    rules: &Rules,
    unique: &dyn Fn(&Header) -> bool,
    chain: ChainCheck,
) -> Result<Hash, Rejection> {
    let h = facts.header;
    check_on_tip(h, tip)?;
    if !unique(h) {
        return Err(Rejection::NotUnique);
    }
    check_entropy_quorum(quorum, facts.entropy_signers)?;
    let t0 = chain_start(tip.timestamp, rules.start_rule);
    if chain == ChainCheck::Recompute {
        check_chain(h, &facts.entropy, t0, facts.winning_step, rules.max_steps)?;
    }
    check_timestamp(facts.timestamp, t0, facts.winning_step)?;
    check_received(reception.block_at, facts.timestamp, rules.clock_tolerance_s)?;
    check_powit_quorum(quorum, facts.powit_signers)?;
    let hash = block_hash(h, &facts.entropy, facts.winning_step);
    check_powit_deadline(reception.powit_at, facts.timestamp, rules.powit_deadline_s)?;
    Ok(hash)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::encoding::Address;
    use crate::entropy::{Keypair, aggregate, entropy_aggregate, member_message, miner_message};

    /// A complete valid block on a test KWC of `seats` members (all sign), with a target that
    /// wins about one step in 8.
    pub(crate) struct Fixture {
        pub(crate) block: Block,
        pub(crate) tip: Tip,
        pub(crate) kwc: KwcKeys,
        pub(crate) members: Vec<Keypair>,
        pub(crate) quorum: QuorumRule,
    }

    pub(crate) fn fixture(seats: usize, quorum: QuorumRule) -> Fixture {
        let members: Vec<Keypair> = (0..seats)
            .map(|i| Keypair::from_seed(&[i as u8 + 1; 32]).unwrap())
            .collect();
        let kwc = KwcKeys::new(
            &members
                .iter()
                .map(|k| (k.public, k.proof_of_possession()))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let miner = Keypair::from_seed(&[200; 32]).unwrap();
        let tip = Tip {
            hash: [3; 32],
            height: 9,
            timestamp: 1_000,
        };
        let mut target = [0u8; 32];
        target[0] = 0x20;
        let header = Header {
            version: crate::PROTOCOL_VERSION,
            height: 10,
            prev_hash: tip.hash,
            candidate_pk: miner.public,
            reward_wallet: [4; 32],
            address: Address::Ipv6Prefix64([0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 2]),
            kwc_id: 7,
            target,
        };
        let sig_m = miner.sign(&miner_message(&header));
        let msg = member_message(&header, &sig_m);
        let sigs: Vec<SignatureBytes> = members.iter().map(|k| k.sign(&msg)).collect();
        let all = Bitfield::from_seats(seats, &(0..seats).collect::<Vec<_>>());
        let (agg, e) = entropy_aggregate(&sig_m, &sigs, &all).unwrap();
        let t0 = tip.timestamp + 1;
        let (n, _) = ChainInputs::new(&header, e, t0)
            .winning_step(&target, 10_000)
            .unwrap();
        let mut block = Block {
            header,
            miner_signature: sig_m,
            entropy_aggregate: agg,
            entropy_signers: all.clone(),
            winning_step: n,
            timestamp: t0 + n,
            powit_aggregate: [0; 96],
            powit_signers: all,
        };
        let pmsg = powit_message(&block.hash());
        let psigs: Vec<SignatureBytes> = members.iter().map(|k| k.sign(&pmsg)).collect();
        block.powit_aggregate = aggregate(&psigs).unwrap();
        Fixture {
            block,
            tip,
            kwc,
            members,
            quorum,
        }
    }

    fn on_time(block: &Block) -> Reception {
        Reception {
            block_at: block.timestamp as f64,
            powit_at: block.timestamp as f64 + 1.0,
        }
    }

    fn rules(deadline: Option<f64>) -> Rules {
        Rules {
            clock_tolerance_s: 2.0,
            powit_deadline_s: deadline,
            start_rule: StartRule::A,
            max_steps: 1_000_000,
        }
    }

    fn pool(seats: usize, quorum: usize) -> QuorumRule {
        QuorumRule::Pool { seats, quorum }
    }

    fn run(f: &Fixture, block: &Block, reception: Reception, r: Rules) -> Result<Hash, Rejection> {
        validate(block, &f.tip, &f.kwc, &f.quorum, &reception, &r, &|_| true)
    }

    #[test]
    fn a_complete_block_is_valid() {
        let f = fixture(4, pool(4, 3));
        assert_eq!(
            run(&f, &f.block, on_time(&f.block), rules(Some(5.0))),
            Ok(f.block.hash())
        );
    }

    #[test]
    fn under_rule_r_the_same_header_gives_the_same_entropy_for_any_t0() {
        // Architect, 2026-10-14: the entropy signs the header only and t0 is attested
        // separately, so one header keeps its entropy whatever t0 is attested, and each t0
        // gives its own chain and timestamp.
        let f = fixture(4, pool(4, 3));
        let e = f.block.entropy();
        let mut firsts = Vec::new();
        for issued in [
            f.tip.timestamp + 1,
            f.tip.timestamp + 37,
            f.tip.timestamp + 600,
        ] {
            let mut b = f.block.clone();
            let inputs = ChainInputs::new(&b.header, e, issued);
            let (n, _) = inputs.winning_step(&b.header.target, 10_000).unwrap();
            b.winning_step = n;
            b.timestamp = issued + n;
            assert_eq!(b.entropy(), e);
            let pmsg = powit_message(&b.hash());
            let psigs: Vec<SignatureBytes> = f.members.iter().map(|k| k.sign(&pmsg)).collect();
            b.powit_aggregate = aggregate(&psigs).unwrap();
            let mut r = rules(Some(5.0));
            r.start_rule = StartRule::R { issued };
            assert_eq!(run(&f, &b, on_time(&b), r), Ok(b.hash()));
            // Attested at another time, the same block is invalid.
            r.start_rule = StartRule::R {
                issued: issued + 1_000,
            };
            assert!(run(&f, &b, on_time(&b), r).is_err());
            firsts.push(inputs.first());
        }
        assert!(firsts[0] != firsts[1] && firsts[1] != firsts[2]);
    }

    #[test]
    fn each_rule_rejects_its_violation() {
        let f = fixture(4, pool(4, 3));
        let r = rules(Some(5.0));
        let mut b = f.block.clone();
        b.timestamp += 1;
        assert_eq!(
            run(&f, &b, on_time(&f.block), r),
            Err(Rejection::TimestampMismatch)
        );
        let early = Reception {
            block_at: f.block.timestamp as f64 - 2.5,
            powit_at: f.block.timestamp as f64,
        };
        assert_eq!(run(&f, &f.block, early, r), Err(Rejection::ReceivedEarly));
        let late = Reception {
            block_at: f.block.timestamp as f64,
            powit_at: f.block.timestamp as f64 + 5.5,
        };
        assert_eq!(run(&f, &f.block, late, r), Err(Rejection::PowitLate));
        assert!(
            run(&f, &f.block, late, rules(None)).is_ok(),
            "no deadline set"
        );
        let mut b = f.block.clone();
        b.winning_step += 1;
        b.timestamp += 1;
        let outcome = run(&f, &b, on_time(&b), r);
        assert!(matches!(
            outcome,
            Err(Rejection::NotWinning | Rejection::EarlierWinningStep)
        ));
        let mut b = f.block.clone();
        b.powit_signers = Bitfield::from_seats(4, &[0, 1]);
        assert_eq!(
            run(&f, &b, on_time(&b), r),
            Err(Rejection::PowitBelowQuorum)
        );
        let mut b = f.block.clone();
        b.powit_signers = Bitfield::from_seats(4, &[0, 1, 2]);
        assert_eq!(run(&f, &b, on_time(&b), r), Err(Rejection::BadPowit));
        let mut b = f.block.clone();
        b.header.kwc_id += 1;
        assert_eq!(run(&f, &b, on_time(&b), r), Err(Rejection::BadEntropy));
        let other_tip = Tip {
            height: 10,
            ..f.tip
        };
        assert_eq!(
            validate(
                &f.block,
                &other_tip,
                &f.kwc,
                &f.quorum,
                &on_time(&f.block),
                &r,
                &|_| true
            ),
            Err(Rejection::NotOnTip)
        );
        assert_eq!(
            validate(
                &f.block,
                &f.tip,
                &f.kwc,
                &f.quorum,
                &on_time(&f.block),
                &r,
                &|_| false
            ),
            Err(Rejection::NotUnique)
        );
    }

    fn facts_of(block: &Block) -> (Block, Hash) {
        (block.clone(), block.entropy())
    }

    fn run_facts(
        f: &Fixture,
        block: &Block,
        reception: Reception,
        r: Rules,
        unique: bool,
        tip: &Tip,
    ) -> Result<Hash, Rejection> {
        let (b, entropy) = facts_of(block);
        let facts = BlockFacts {
            header: &b.header,
            entropy,
            entropy_signers: &b.entropy_signers,
            winning_step: b.winning_step,
            timestamp: b.timestamp,
            powit_signers: &b.powit_signers,
        };
        validate_facts(
            &facts,
            tip,
            &f.quorum,
            &reception,
            &r,
            &|_| unique,
            ChainCheck::Recompute,
        )
    }

    #[test]
    fn validate_and_validate_facts_agree_except_on_signatures() {
        let f = fixture(4, pool(4, 3));
        let r = rules(Some(5.0));
        let t0 = f.tip.timestamp + 1;
        let later = ChainInputs::new(&f.block.header, f.block.entropy(), t0)
            .steps()
            .skip(f.block.winning_step as usize + 1)
            .take(100_000)
            .find(|(_, h)| below_target(h, &f.block.header.target))
            .map(|(n, _)| n)
            .unwrap();
        let mut cases: Vec<(Block, Reception, Rules, bool, Tip)> = Vec::new();
        let base = f.block.clone();
        cases.push((base.clone(), on_time(&base), r, true, f.tip));
        cases.push((base.clone(), on_time(&base), rules(None), true, f.tip));
        let mut b = base.clone();
        b.timestamp += 1;
        cases.push((b, on_time(&base), r, true, f.tip));
        let early = Reception {
            block_at: base.timestamp as f64 - 2.5,
            powit_at: base.timestamp as f64,
        };
        cases.push((base.clone(), early, r, true, f.tip));
        let late = Reception {
            block_at: base.timestamp as f64,
            powit_at: base.timestamp as f64 + 5.5,
        };
        cases.push((base.clone(), late, r, true, f.tip));
        cases.push((base.clone(), late, rules(None), true, f.tip));
        let mut b = base.clone();
        b.winning_step += 1;
        b.timestamp += 1;
        cases.push((b.clone(), on_time(&b), r, true, f.tip));
        let mut b = base.clone();
        b.winning_step = later;
        b.timestamp = t0 + later;
        cases.push((b.clone(), on_time(&b), r, true, f.tip));
        let mut b = base.clone();
        b.powit_signers = Bitfield::from_seats(4, &[0, 1]);
        cases.push((b.clone(), on_time(&b), r, true, f.tip));
        let mut b = base.clone();
        b.entropy_signers = Bitfield::from_seats(4, &[0, 1]);
        cases.push((b.clone(), on_time(&b), r, true, f.tip));
        cases.push((base.clone(), on_time(&base), r, false, f.tip));
        let other_tip = Tip {
            height: 10,
            ..f.tip
        };
        cases.push((base.clone(), on_time(&base), r, true, other_tip));
        let mut rejections = Vec::new();
        for (block, reception, rules, unique, tip) in &cases {
            let full = validate(block, tip, &f.kwc, &f.quorum, reception, rules, &|_| {
                *unique
            });
            let facts = run_facts(&f, block, *reception, *rules, *unique, tip);
            assert_eq!(full, facts, "case {block:?}");
            rejections.push(full.err());
        }
        // The cases cover every rejection except the two signature checks.
        for expected in [
            Rejection::NotOnTip,
            Rejection::NotUnique,
            Rejection::EntropyBelowQuorum,
            Rejection::EarlierWinningStep,
            Rejection::TimestampMismatch,
            Rejection::ReceivedEarly,
            Rejection::PowitBelowQuorum,
            Rejection::PowitLate,
        ] {
            assert!(
                rejections.contains(&Some(expected)),
                "{expected:?} not covered"
            );
        }
        assert!(rejections.contains(&None));
    }

    #[test]
    fn trusted_chain_check_skips_only_the_recomputation() {
        let f = fixture(4, pool(4, 3));
        let mut b = f.block.clone();
        b.winning_step += 1;
        b.timestamp += 1;
        let (b, entropy) = facts_of(&b);
        let facts = BlockFacts {
            header: &b.header,
            entropy,
            entropy_signers: &b.entropy_signers,
            winning_step: b.winning_step,
            timestamp: b.timestamp,
            powit_signers: &b.powit_signers,
        };
        let outcome = validate_facts(
            &facts,
            &f.tip,
            &f.quorum,
            &on_time(&b),
            &rules(None),
            &|_| true,
            ChainCheck::Trusted,
        );
        assert_eq!(outcome, Ok(block_hash(&b.header, &entropy, b.winning_step)));
    }

    #[test]
    fn quorum_rules_count_groups_in_seat_order() {
        let two = QuorumRule::TwoGroup {
            leader_seats: 10,
            leader_quorum: 7,
            subordinate_seats: 30,
            subordinate_quorum: 21,
        };
        let seats = |set: Vec<usize>| Bitfield::from_seats(40, &set);
        assert!(two.is_met(&seats((0..7).chain(10..31).collect())));
        assert!(
            !two.is_met(&seats((0..6).chain(10..40).collect())),
            "6 leaders"
        );
        assert!(
            !two.is_met(&seats((0..10).chain(10..30).collect())),
            "20 subordinates"
        );
        let config = Config::default();
        assert_eq!(QuorumRule::from_config(&config, MinerKind::Registered), two);
        assert_eq!(
            QuorumRule::from_config(&config, MinerKind::Unregistered),
            QuorumRule::Pool {
                seats: 40,
                quorum: 27
            }
        );
    }

    #[test]
    fn members_sign_only_from_t0_plus_n_within_tolerance() {
        assert!(!may_sign(1_007.9, 1_001, 9, 2.0));
        assert!(may_sign(1_008.0, 1_001, 9, 2.0));
    }

    #[test]
    fn fork_choice_prefers_length_then_earlier_timestamp_then_lower_hash() {
        let c = |height, timestamp, h: u8| ForkCandidate {
            height,
            timestamp,
            hash: [h; 32],
        };
        // Longer chain wins even with a later timestamp and higher hash.
        assert_eq!(
            fork_choice(&c(11, 2_000, 9), &c(10, 1_000, 1)),
            Ordering::Less
        );
        // Same height: earlier timestamp (smaller N) wins even with a higher hash.
        assert_eq!(
            fork_choice(&c(10, 1_005, 9), &c(10, 1_006, 1)),
            Ordering::Less
        );
        // Same height and timestamp: lower hash wins.
        assert_eq!(
            fork_choice(&c(10, 1_005, 1), &c(10, 1_005, 2)),
            Ordering::Less
        );
        assert_eq!(prefer((1_005, &[9; 32]), (1_006, &[1; 32])), Ordering::Less);
        assert_eq!(prefer((1_005, &[1; 32]), (1_005, &[2; 32])), Ordering::Less);
    }

    #[test]
    fn comparison_fork_rules_order_by_arrival_or_hash_alone() {
        let c = |height, timestamp, h: u8| ForkCandidate {
            height,
            timestamp,
            hash: [h; 32],
        };
        // First-seen: same height, the earlier arrival wins even with a later timestamp.
        assert_eq!(
            fork_choice_first_seen(&c(10, 1_009, 9), 5.0, &c(10, 1_002, 1), 6.0),
            Ordering::Less
        );
        // Length still comes first.
        assert_eq!(
            fork_choice_first_seen(&c(10, 1_002, 1), 1.0, &c(11, 1_009, 9), 9.0),
            Ordering::Greater
        );
        // Lower hash only: timestamps are ignored.
        assert_eq!(
            fork_choice_lower_hash(&c(10, 1_009, 1), &c(10, 1_002, 2)),
            Ordering::Less
        );
        assert_eq!(
            fork_choice_lower_hash(&c(10, 1_009, 9), &c(11, 1_002, 2)),
            Ordering::Greater
        );
    }

    #[test]
    fn equivocation_scope_decides_whether_a_parent_switch_is_allowed() {
        let f = fixture(4, pool(4, 3));
        let first = f.block.header;
        let mut other_parent = first;
        other_parent.prev_hash = [8; 32];
        let mut same_parent = first;
        same_parent.reward_wallet[0] ^= 1;
        let mut next_height = first;
        next_height.height += 1;
        use EquivocationScope::{SameHeight, SameParent};
        assert!(equivocates(SameHeight, &first, &other_parent));
        assert!(!equivocates(SameParent, &first, &other_parent));
        assert!(equivocates(SameParent, &first, &same_parent));
        assert!(!equivocates(SameHeight, &first, &first));
        assert!(!equivocates(SameHeight, &first, &next_height));
    }

    #[test]
    fn honest_signing_tolerance_and_node_rules_follow_the_configuration() {
        assert_eq!(witness_signing_tolerance(WitnessSigning::OnTime, 2.0), 0.0);
        assert_eq!(
            witness_signing_tolerance(WitnessSigning::EarliestWithinTolerance, 2.0),
            2.0
        );
        // On time: not before t0 + N.
        assert!(!may_sign(1_009.9, 1_001, 9, 0.0));
        assert!(may_sign(1_010.0, 1_001, 9, 0.0));
        assert!(!accepts_chain_through_late_block(DeadlineScope::Permanent));
        assert!(accepts_chain_through_late_block(
            DeadlineScope::NotExtendedOnly
        ));
        assert!(!reevaluates_early_block(EarlyBlockRule::Permanent));
        assert!(reevaluates_early_block(EarlyBlockRule::ReevaluateLater));
    }

    #[test]
    fn only_the_first_winning_step_is_valid() {
        // Full recomputation finds the first step below the target; presenting a later step
        // that also meets the target is rejected.
        let f = fixture(4, pool(4, 3));
        let t0 = f.tip.timestamp + 1;
        let inputs = ChainInputs::new(&f.block.header, f.block.entropy(), t0);
        let later = inputs
            .steps()
            .skip(f.block.winning_step as usize + 1)
            .take(100_000)
            .find(|(_, h)| below_target(h, &f.block.header.target))
            .map(|(n, _)| n)
            .unwrap();
        let mut b = f.block.clone();
        b.winning_step = later;
        b.timestamp = t0 + later;
        assert_eq!(
            run(&f, &b, on_time(&b), rules(None)),
            Err(Rejection::EarlierWinningStep)
        );
    }

    /// A second block from the same miner at the same height (a different header), with a
    /// PoWit signed by `signers`.
    fn second_block(f: &Fixture, signers: &[usize]) -> Block {
        let mut b = f.block.clone();
        b.header.reward_wallet[0] ^= 1;
        let msg = powit_message(&b.hash());
        let sigs: Vec<SignatureBytes> = signers.iter().map(|i| f.members[*i].sign(&msg)).collect();
        b.powit_signers = Bitfield::from_seats(f.members.len(), signers);
        b.powit_aggregate = aggregate(&sigs).unwrap();
        b
    }

    #[test]
    fn witnesses_who_sign_two_blocks_of_one_miner_at_one_height_are_found() {
        let f = fixture(4, pool(4, 3));
        // The fixture's PoWit is signed by all four seats.
        let b = second_block(&f, &[1, 2, 3]);
        assert_eq!(witness_equivocators(&f.block, &b, &f.kwc), vec![1, 2, 3]);
        // Nobody is flagged for the same block twice.
        assert!(witness_equivocators(&f.block, &f.block, &f.kwc).is_empty());
        // Different heights or different miners are not equivocation.
        let mut c = b.clone();
        c.header.height += 1;
        assert!(witness_equivocators(&f.block, &c, &f.kwc).is_empty());
        let mut d = b.clone();
        d.header.candidate_pk = f.members[0].public;
        assert!(witness_equivocators(&f.block, &d, &f.kwc).is_empty());
        // A forged aggregate proves nothing.
        let mut e = b.clone();
        e.powit_signers = Bitfield::from_seats(4, &[0, 1, 2, 3]);
        assert!(witness_equivocators(&f.block, &e, &f.kwc).is_empty());
    }

    #[test]
    fn disjoint_signers_are_not_flagged() {
        let mut f = fixture(4, pool(4, 2));
        let msg = powit_message(&f.block.hash());
        let sigs: Vec<SignatureBytes> = f.members[..2].iter().map(|k| k.sign(&msg)).collect();
        f.block.powit_signers = Bitfield::from_seats(4, &[0, 1]);
        f.block.powit_aggregate = aggregate(&sigs).unwrap();
        let b = second_block(&f, &[2, 3]);
        assert!(witness_equivocators(&f.block, &b, &f.kwc).is_empty());
    }

    #[test]
    fn keys_without_a_valid_proof_of_possession_are_refused() {
        let a = Keypair::from_seed(&[1; 32]).unwrap();
        let b = Keypair::from_seed(&[2; 32]).unwrap();
        assert!(KwcKeys::new(&[(a.public, b.proof_of_possession())]).is_err());
    }
}
