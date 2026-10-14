//! The hash chain and the winning step (SPEC §3.5, Appendix A).
//!
//! ```text
//! h_0     = SHA256("GB/chain" ‖ prev_hash ‖ height ‖ t0 ‖ nonce=0 ‖ E ‖ header_digest)
//! h_{n+1} = SHA256("GB/chain" ‖ h_n ‖ prev_hash ‖ height ‖ (t0+n+1) ‖ nonce=n+1 ‖ E ‖ header_digest)
//! ```
//!
//! The miner wins at the first `n` with `h_n < target`, and the block timestamp is `t0 + n`.

use crate::encoding::Header;
use crate::hash::{Hash, tagged};

/// Domain tag of chain steps.
pub const CHAIN_TAG: &[u8] = b"GB/chain";

/// Where the chain starts (SPEC §3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartRule {
    /// Rule (a), the protocol rule: `t0 = timestamp(previous PoW-ID block) + 1 s`, whenever the
    /// entropy arrives.
    A,
    /// Rule (b), comparison only (S13): the chain starts at the local arrival time of the
    /// miner's entropy.
    B {
        /// Local arrival time of the entropy, in seconds.
        arrival: u64,
    },
    /// Rule (r), the M3a.1 experiment (architect, 2026-10-14): steps count from the time the
    /// miner's KWC issued its entropy, as the KWC attests it ([`t0_message`]): the clock time
    /// at which the t-th required signature was made, rounded up to a whole second, and never
    /// before `timestamp(previous PoW-ID block) + 1 s`. A miner whose entropy comes late
    /// starts late; it cannot catch up past steps. The entropy depends on the header only.
    R {
        /// Attested entropy issue time, in whole seconds.
        issued: u64,
    },
}

/// The chain's start time `t0` under a start rule.
pub fn chain_start(previous_block_timestamp: u64, rule: StartRule) -> u64 {
    match rule {
        StartRule::A => previous_block_timestamp + 1,
        StartRule::B { arrival } => arrival,
        StartRule::R { issued } => issued.max(previous_block_timestamp + 1),
    }
}

/// Domain tag of start-time attestations (start rule (r)).
pub const T0_TAG: &[u8] = b"GB/t0";

/// Start rule (r): the message a miner's KWC signs (as a threshold signature) to attest its
/// chain start `t0` for the round of `header`. It is separate from the entropy, which signs
/// the header only, so the entropy is the same whatever `t0` is attested.
pub fn t0_message(header: &Header, t0: u64) -> Hash {
    tagged(T0_TAG, &[&header.digest(), &t0.to_be_bytes()])
}

/// Inputs that fix a chain for one round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainInputs {
    /// Previous PoW-ID block hash.
    pub prev_hash: Hash,
    /// Height of the block being mined.
    pub height: u64,
    /// Chain start time.
    pub t0: u64,
    /// Entropy E committed to the header.
    pub entropy: Hash,
    /// `header_digest`.
    pub header_digest: Hash,
}

impl ChainInputs {
    /// Inputs for `header` with entropy `entropy`, starting at `t0`.
    pub fn new(header: &Header, entropy: Hash, t0: u64) -> ChainInputs {
        ChainInputs {
            prev_hash: header.prev_hash,
            height: header.height,
            t0,
            entropy,
            header_digest: header.digest(),
        }
    }

    /// `h_0`.
    pub fn first(&self) -> Hash {
        tagged(
            CHAIN_TAG,
            &[
                &self.prev_hash,
                &self.height.to_be_bytes(),
                &self.t0.to_be_bytes(),
                &0u64.to_be_bytes(),
                &self.entropy,
                &self.header_digest,
            ],
        )
    }

    /// `h_{n}` from `h_{n−1}`, for `n ≥ 1`.
    pub fn next(&self, previous: &Hash, n: u64) -> Hash {
        tagged(
            CHAIN_TAG,
            &[
                previous,
                &self.prev_hash,
                &self.height.to_be_bytes(),
                &(self.t0 + n).to_be_bytes(),
                &n.to_be_bytes(),
                &self.entropy,
                &self.header_digest,
            ],
        )
    }

    /// The chain `h_0, h_1, …`, step by step.
    pub fn steps(&self) -> impl Iterator<Item = (u64, Hash)> + '_ {
        let mut state: Option<(u64, Hash)> = None;
        std::iter::from_fn(move || {
            let next = match state {
                None => (0, self.first()),
                Some((n, h)) => (n + 1, self.next(&h, n + 1)),
            };
            state = Some(next);
            Some(next)
        })
    }

    /// The first winning step `N` and `h_N`, searching at most `max_steps` steps.
    pub fn winning_step(&self, target: &Hash, max_steps: u64) -> Option<(u64, Hash)> {
        self.steps()
            .take(usize::try_from(max_steps).unwrap_or(usize::MAX))
            .find(|(_, h)| below_target(h, target))
    }

    /// `h_n`, computed from `h_0` (every step must be computed in order).
    pub fn step(&self, n: u64) -> Hash {
        let mut h = self.first();
        for i in 1..=n {
            h = self.next(&h, i);
        }
        h
    }
}

/// True when `hash`, read as a 256-bit big-endian integer, is below `target`.
pub fn below_target(hash: &Hash, target: &Hash) -> bool {
    hash < target
}

/// 2⁻²⁵⁶ as an `f64` (exactly representable: biased exponent 1023 − 256).
const TWO_POW_MINUS_256: f64 = f64::from_bits((1023 - 256) << 52);

/// The probability that one step wins against `target`, `T / 2²⁵⁶`, as an `f64`. A uniformly
/// random 256-bit hash is below `T` with exactly this probability, so a simulator can draw
/// winning steps from it instead of hashing (ARCHITECTURE §3).
pub fn step_win_probability(target: &Hash) -> f64 {
    use num_traits::ToPrimitive;
    num_bigint::BigUint::from_bytes_be(target)
        .to_f64()
        .unwrap_or(f64::MAX)
        * TWO_POW_MINUS_256
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::tests::sample;

    #[test]
    fn rule_a_starts_one_second_after_the_previous_block_and_rule_b_at_arrival() {
        assert_eq!(chain_start(1_000, StartRule::A), 1_001);
        assert_eq!(chain_start(1_000, StartRule::B { arrival: 1_017 }), 1_017);
    }

    #[test]
    fn rule_r_starts_at_the_attested_issue_time_but_never_before_rule_a() {
        assert_eq!(chain_start(1_000, StartRule::R { issued: 1_017 }), 1_017);
        assert_eq!(chain_start(1_000, StartRule::R { issued: 1_001 }), 1_001);
        // An attested time at or before the previous block's timestamp starts as rule (a).
        assert_eq!(chain_start(1_000, StartRule::R { issued: 990 }), 1_001);
        assert_eq!(chain_start(1_000, StartRule::R { issued: 1_000 }), 1_001);
    }

    #[test]
    fn the_t0_attestation_names_the_round_and_the_start() {
        let header = sample();
        assert_ne!(t0_message(&header, 10), t0_message(&header, 11));
        let mut other = header;
        other.height += 1;
        assert_ne!(t0_message(&header, 10), t0_message(&other, 10));
    }

    #[test]
    fn steps_follow_the_recurrence() {
        let inputs = ChainInputs::new(&sample(), [7; 32], 1_001);
        let steps: Vec<(u64, Hash)> = inputs.steps().take(4).collect();
        assert_eq!(steps[0].1, inputs.first());
        for w in steps.windows(2) {
            assert_eq!(w[1].0, w[0].0 + 1);
            assert_eq!(w[1].1, inputs.next(&w[0].1, w[1].0));
        }
        assert_eq!(inputs.step(3), steps[3].1);
    }

    #[test]
    fn winning_step_is_the_first_below_target() {
        let inputs = ChainInputs::new(&sample(), [9; 32], 5);
        // Target 2^252: each step wins with probability 1/16.
        let mut target = [0u8; 32];
        target[0] = 0x10;
        let (n, h) = inputs.winning_step(&target, 10_000).unwrap();
        assert!(below_target(&h, &target));
        assert!(
            inputs
                .steps()
                .take(n as usize)
                .all(|(_, x)| !below_target(&x, &target))
        );
    }

    #[test]
    fn step_win_probability_is_the_target_over_two_to_the_256() {
        let mut target = [0u8; 32];
        target[0] = 0x10;
        assert_eq!(step_win_probability(&target), 1.0 / 16.0);
        let t = crate::issuance::variant_b_target(1_000, 30);
        let p = step_win_probability(&t);
        assert!((p * 30_000.0 - 1.0).abs() < 1e-12, "{p}");
        assert_eq!(step_win_probability(&[0; 32]), 0.0);
    }

    #[test]
    fn a_different_entropy_or_start_gives_a_different_chain() {
        let a = ChainInputs::new(&sample(), [1; 32], 10);
        let b = ChainInputs::new(&sample(), [2; 32], 10);
        let c = ChainInputs::new(&sample(), [1; 32], 11);
        assert_ne!(a.first(), b.first());
        assert_ne!(a.first(), c.first());
    }
}
