//! Difficulty and issuance rate (SPEC §3.8–§3.9).
//!
//! A target `T` makes each hash win with probability `T / 2²⁵⁶`.

use crate::hash::Hash;
use gb_config::protocol::{CountBasedCorrection, DifficultyCountRule};
use num_bigint::BigUint;
use num_traits::{One, Zero};

fn two_256() -> BigUint {
    BigUint::one() << 256
}

fn to_hash(value: &BigUint) -> Hash {
    let max = two_256() - BigUint::one();
    let capped = if *value > max { max } else { value.clone() };
    let bytes = capped.to_bytes_be();
    let mut out = [0u8; 32];
    out[32 - bytes.len()..].copy_from_slice(&bytes);
    out
}

/// Variant B (default): count-based target, so each hash wins with probability
/// `1 / (admitted online miners × target interval)`.
pub fn variant_b_target(online_miners: u64, interval_s: u64) -> Hash {
    let denominator = BigUint::from(online_miners.max(1)) * BigUint::from(interval_s.max(1));
    to_hash(&(two_256() / denominator))
}

/// Variant A (comparison): Bitcoin-style retarget after a window of `window_blocks` blocks
/// that took `actual_window_s`, so the window would have lasted `window_blocks × interval_s`,
/// with the change clamped to a factor of `clamp` either way.
pub fn variant_a_retarget(
    old_target: &Hash,
    actual_window_s: u64,
    window_blocks: u64,
    interval_s: u64,
    clamp: u64,
) -> Hash {
    let old = BigUint::from_bytes_be(old_target);
    let expected = BigUint::from(window_blocks.max(1)) * BigUint::from(interval_s.max(1));
    let raw = &old * BigUint::from(actual_window_s) / &expected;
    let low = &old / BigUint::from(clamp.max(1));
    let high = &old * BigUint::from(clamp.max(1));
    let clamped = if raw < low {
        low
    } else if raw > high {
        high
    } else {
        raw
    };
    to_hash(if clamped.is_zero() { &old } else { &clamped })
}

/// One in parts per million: correction factors are integers so every node computes the same
/// target.
pub const PPM: u64 = 1_000_000;

/// A bound such as 0.10 (±10%) in parts per million, rounded to the nearest unit.
pub fn bound_ppm(bound: f64) -> u64 {
    (bound * PPM as f64).round().clamp(0.0, (PPM - 1) as f64) as u64
}

fn clamp_ppm(raw: u128, bound_ppm: u64) -> u64 {
    let bound = u128::from(bound_ppm.min(PPM - 1));
    let one = u128::from(PPM);
    raw.clamp(one - bound, one + bound) as u64
}

/// Variant B correction P1 \[P\]: the factor for the count-based target after a window of
/// `window_blocks` intervals that took `window_s` seconds in total, `mean interval ÷ target
/// interval`, bounded to `1 ± bound`, in ppm (rounded down). Slow blocks raise the target,
/// so difficulty is multiplied by "target interval ÷ recent average interval".
pub fn correction_ratio_ppm(
    window_s: u64,
    window_blocks: u64,
    interval_s: u64,
    bound_ppm: u64,
) -> u64 {
    let expected = u128::from(window_blocks.max(1)) * u128::from(interval_s.max(1));
    clamp_ppm(u128::from(window_s) * u128::from(PPM) / expected, bound_ppm)
}

/// Variant B correction P2 \[P\]: the accumulated factor after one more window,
/// `previous × (mean interval ÷ target interval)`, bounded to `1 ± bound`, in ppm.
pub fn accumulate_correction_ppm(
    previous_ppm: u64,
    window_s: u64,
    window_blocks: u64,
    interval_s: u64,
    bound_ppm: u64,
) -> u64 {
    let expected = u128::from(window_blocks.max(1)) * u128::from(interval_s.max(1));
    clamp_ppm(
        u128::from(previous_ppm) * u128::from(window_s) / expected,
        bound_ppm,
    )
}

/// The Variant B correction factor for the next block, in ppm. `previous_ppm` is the factor
/// used for the last block, `blocks` the number of blocks on this branch so far and
/// `last_window_s` the total length of its last `window_blocks` intervals (`None` while the
/// branch is shorter than a window).
///
/// P1 recomputes the factor from the sliding window at every block. P2 multiplies the factor
/// by the last window's ratio once every `window_blocks` blocks. B-exact uses no factor.
pub fn update_correction_ppm(
    correction: &CountBasedCorrection,
    previous_ppm: u64,
    blocks: u64,
    last_window_s: Option<u64>,
    interval_s: u64,
) -> u64 {
    match *correction {
        CountBasedCorrection::RecentIntervalRatio {
            window_blocks,
            bound,
        } => match last_window_s {
            Some(window_s) => correction_ratio_ppm(
                window_s,
                u64::from(window_blocks),
                interval_s,
                bound_ppm(bound),
            ),
            None => PPM,
        },
        CountBasedCorrection::Accumulated {
            window_blocks,
            bound,
        } => {
            let k = u64::from(window_blocks.max(1));
            match last_window_s {
                Some(window_s) if blocks > 0 && blocks % k == 0 => accumulate_correction_ppm(
                    previous_ppm,
                    window_s,
                    k,
                    interval_s,
                    bound_ppm(bound),
                ),
                _ => previous_ppm,
            }
        }
        CountBasedCorrection::DiscreteExact => PPM,
    }
}

/// `target × factor_ppm / 10⁶`, capped at the largest target.
pub fn scale_target_ppm(target: &Hash, factor_ppm: u64) -> Hash {
    to_hash(&(BigUint::from_bytes_be(target) * BigUint::from(factor_ppm) / BigUint::from(PPM)))
}

/// Fixed-point precision of [`variant_b_exact_target`]: values are integers scaled by 2⁵¹².
const EXACT_BITS: usize = 512;

/// `x^e` for a fixed-point `x` (scaled by `one = 2^EXACT_BITS`), by squaring, truncating after
/// every product.
fn pow_fixed(x: &BigUint, mut e: u64, one: &BigUint) -> BigUint {
    let mut result = one.clone();
    let mut base = x.clone();
    while e > 0 {
        if e & 1 == 1 {
            result = (&result * &base) >> EXACT_BITS;
        }
        e >>= 1;
        if e > 0 {
            base = (&base * &base) >> EXACT_BITS;
        }
    }
    result
}

/// B-exact \[P\] (a Variant B correction candidate, M3): the target for which
/// `1 − (1 − p)^N = 1 / interval` exactly, with `p = T / 2²⁵⁶` and N = `online_miners`, so the
/// expected interval is exactly the target interval given whole-second steps (`t0` is one
/// second after the previous block, so an interval lasts N + 1 steps). There is no lookback.
///
/// Computed in integers: `x = ((I − 1) / I)^(1/N)` by Newton's method from above in
/// fixed point with 512 fractional bits, stopping when an iteration no longer decreases `x`;
/// then `T = ⌊(1 − x) · 2²⁵⁶⌋`. Every node therefore computes the same target.
pub fn variant_b_exact_target(online_miners: u64, interval_s: u64) -> Hash {
    let n = online_miners.max(1);
    if interval_s <= 1 {
        return to_hash(&two_256());
    }
    let one = BigUint::one() << EXACT_BITS;
    let a = (&one * BigUint::from(interval_s - 1)) / BigUint::from(interval_s);
    let scaled_a = &a * &one;
    let mut x = one.clone();
    loop {
        let power = pow_fixed(&x, n - 1, &one);
        if power.is_zero() {
            break;
        }
        let next = (BigUint::from(n - 1) * &x + &scaled_a / &power) / BigUint::from(n);
        if next >= x {
            break;
        }
        x = next;
    }
    to_hash(&((&one - &x) >> (EXACT_BITS - 256)))
}

/// The Variant B target for the next block: B-exact, or the count-based target scaled by the
/// correction factor (`PPM` when there is no correction).
pub fn count_based_target(
    online_miners: u64,
    interval_s: u64,
    correction: Option<&CountBasedCorrection>,
    factor_ppm: u64,
) -> Hash {
    match correction {
        Some(CountBasedCorrection::DiscreteExact) => {
            variant_b_exact_target(online_miners, interval_s)
        }
        Some(_) => scale_target_ppm(&variant_b_target(online_miners, interval_s), factor_ppm),
        None => variant_b_target(online_miners, interval_s),
    }
}

/// Whether a Globally Active miner counts toward Variant B's admitted online count (SPEC
/// §3.8; architect, 2026-10-12): it must be recorded online, and under the default rule its
/// post-admission wait must have ended. A miner that disappeared without announcing it stays
/// recorded online, and so counted, until it is recorded offline. Under the issued-entropy
/// rule (the M3a.1 experiment) this is only the admission condition: the miner must also have
/// received entropy for the round, which its KWC knows and commits per round.
pub fn counts_for_difficulty(
    rule: DifficultyCountRule,
    recorded_online: bool,
    wait_ended: bool,
) -> bool {
    recorded_online
        && match rule {
            DifficultyCountRule::AfterPostAdmissionWait | DifficultyCountRule::IssuedEntropy => {
                wait_ended
            }
            DifficultyCountRule::AllGloballyActive => true,
        }
}

/// Adaptive variant (§3.9): the shortest interval allowed by the cap
/// `R ≤ registered / (k × T_min)`, from registered IDs at the last checkpoint and `k = num/den`.
pub fn adaptive_min_interval_s(registered_at_checkpoint: u64, k: (u64, u64), t_min_s: f64) -> f64 {
    let (num, den) = k;
    num as f64 * t_min_s / (den.max(1) as f64 * registered_at_checkpoint.max(1) as f64)
}

/// The adaptive interval: demand may shorten it, but never below the cap's minimum. The cap
/// may also set the rate below the fixed 30 s target (SPEC §3.9).
pub fn adaptive_interval_s(
    demand_interval_s: f64,
    registered_at_checkpoint: u64,
    k: (u64, u64),
    t_min_s: f64,
) -> f64 {
    demand_interval_s.max(adaptive_min_interval_s(
        registered_at_checkpoint,
        k,
        t_min_s,
    ))
}

/// The checkpoint that applies at time `t`: the start of its checkpoint interval.
pub fn checkpoint_at(t_s: f64, checkpoint_interval_s: f64) -> f64 {
    (t_s / checkpoint_interval_s).floor() * checkpoint_interval_s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_b_wins_one_hash_in_miners_times_interval() {
        // 1,000 miners × 30 s: probability 1/30,000 per hash.
        let t = BigUint::from_bytes_be(&variant_b_target(1_000, 30));
        assert_eq!(t, two_256() / BigUint::from(30_000u32));
    }

    #[test]
    fn variant_a_scales_and_clamps() {
        let old = variant_b_target(1_000, 30);
        let o = BigUint::from_bytes_be(&old);
        // Window twice as long as expected: target doubles (difficulty halves).
        let slow = BigUint::from_bytes_be(&variant_a_retarget(&old, 2 * 144 * 30, 144, 30, 4));
        assert_eq!(slow, &o * BigUint::from(2u8));
        // Ten times faster: clamped to a factor of 4.
        let fast = BigUint::from_bytes_be(&variant_a_retarget(&old, 144 * 3, 144, 30, 4));
        assert_eq!(fast, &o / BigUint::from(4u8));
    }

    fn probability(target: &Hash) -> f64 {
        crate::chain::step_win_probability(target)
    }

    #[test]
    fn b_exact_makes_the_whole_second_interval_exact() {
        for n in [1u64, 2, 10, 1_000, 100_000, 1_000_000] {
            let p = probability(&variant_b_exact_target(n, 30));
            let expected = -((29.0f64 / 30.0).ln() / n as f64).exp_m1();
            assert!(
                (p / expected - 1.0).abs() < 1e-9,
                "n = {n}: {p} vs {expected}"
            );
            // Probability that some miner wins a step: 1/30, so the mean interval is 30 s.
            let any = -(n as f64 * (-p).ln_1p()).exp_m1();
            assert!((any * 30.0 - 1.0).abs() < 1e-9, "n = {n}: {any}");
        }
        // One miner: exactly the count-based target (p = 1/30), up to the last unit.
        let exact = BigUint::from_bytes_be(&variant_b_exact_target(1, 30));
        let count = BigUint::from_bytes_be(&variant_b_target(1, 30));
        let gap = if exact > count {
            &exact - &count
        } else {
            &count - &exact
        };
        assert!(gap <= BigUint::one());
        // Every step wins at a one-second interval.
        assert_eq!(variant_b_exact_target(5, 1), [0xff; 32]);
    }

    #[test]
    fn b_exact_is_easier_than_the_plain_count_by_the_discrete_step_bias() {
        // The plain count gives a mean interval of 1/(1 − (1 − 1/(30N))^N) ≈ 30.503 s; B-exact
        // gives 30 s, so its target is larger.
        let plain = probability(&variant_b_target(10_000, 30));
        let exact = probability(&variant_b_exact_target(10_000, 30));
        let plain_interval = -1.0 / (10_000.0 * (-plain).ln_1p()).exp_m1();
        assert!((plain_interval - 30.5027).abs() < 1e-3, "{plain_interval}");
        assert!(exact > plain);
    }

    #[test]
    fn p1_is_the_bounded_ratio_of_mean_to_target_interval() {
        let b = bound_ppm(0.10);
        assert_eq!(b, 100_000);
        assert_eq!(correction_ratio_ppm(144 * 30, 144, 30, b), PPM);
        assert_eq!(correction_ratio_ppm(144 * 33, 144, 30, b), 1_100_000);
        assert_eq!(correction_ratio_ppm(144 * 60, 144, 30, b), 1_100_000);
        assert_eq!(correction_ratio_ppm(144 * 15, 144, 30, b), 900_000);
        assert_eq!(correction_ratio_ppm(4_392, 144, 30, b), 1_016_666);
        let p1 = CountBasedCorrection::RecentIntervalRatio {
            window_blocks: 144,
            bound: 0.10,
        };
        assert_eq!(update_correction_ppm(&p1, 123, 10, None, 30), PPM);
        assert_eq!(
            update_correction_ppm(&p1, 123, 500, Some(4_392), 30),
            1_016_666
        );
    }

    #[test]
    fn p2_accumulates_only_at_window_boundaries() {
        let p2 = CountBasedCorrection::Accumulated {
            window_blocks: 100,
            bound: 0.10,
        };
        // A window of 100 blocks at 30.6 s each: ratio 1.02.
        let w = 3_060;
        assert_eq!(update_correction_ppm(&p2, PPM, 99, Some(w), 30), PPM);
        let first = update_correction_ppm(&p2, PPM, 100, Some(w), 30);
        assert_eq!(first, 1_020_000);
        assert_eq!(update_correction_ppm(&p2, first, 150, Some(w), 30), first);
        let second = update_correction_ppm(&p2, first, 200, Some(w), 30);
        assert_eq!(second, 1_040_400);
        // Bounded at 1 ± bound.
        assert_eq!(
            accumulate_correction_ppm(1_090_000, w, 100, 30, 100_000),
            1_100_000
        );
        let exact = CountBasedCorrection::DiscreteExact;
        assert_eq!(update_correction_ppm(&exact, 5, 100, Some(w), 30), PPM);
    }

    #[test]
    fn count_based_target_applies_the_chosen_correction() {
        let base = variant_b_target(1_000, 30);
        assert_eq!(count_based_target(1_000, 30, None, 2 * PPM), base);
        let p1 = CountBasedCorrection::RecentIntervalRatio {
            window_blocks: 144,
            bound: 0.10,
        };
        assert_eq!(
            BigUint::from_bytes_be(&count_based_target(1_000, 30, Some(&p1), 1_100_000)),
            BigUint::from_bytes_be(&base) * BigUint::from(11u8) / BigUint::from(10u8)
        );
        assert_eq!(
            count_based_target(1_000, 30, Some(&CountBasedCorrection::DiscreteExact), 7),
            variant_b_exact_target(1_000, 30)
        );
    }

    #[test]
    fn the_count_includes_waiting_miners_only_under_the_comparison_rule() {
        use DifficultyCountRule::{AfterPostAdmissionWait, AllGloballyActive};
        assert!(counts_for_difficulty(AfterPostAdmissionWait, true, true));
        assert!(!counts_for_difficulty(AfterPostAdmissionWait, true, false));
        assert!(counts_for_difficulty(AllGloballyActive, true, false));
        assert!(!counts_for_difficulty(AllGloballyActive, false, true));
    }

    #[test]
    fn adaptive_cap_keeps_the_time_floor() {
        // 2M registered IDs, k = 7/6, T_min = 2 years: at most 2M/(7/6 × 2 y) IDs per year.
        let year = 31_536_000.0;
        let min = adaptive_min_interval_s(2_000_000, (7, 6), 2.0 * year);
        assert!((min - 7.0 / 6.0 * 2.0 * year / 2_000_000.0).abs() < 1e-9);
        assert_eq!(adaptive_interval_s(1.0, 2_000_000, (7, 6), 2.0 * year), min);
        assert_eq!(
            adaptive_interval_s(60.0, 2_000_000, (7, 6), 2.0 * year),
            60.0
        );
        assert_eq!(checkpoint_at(125.0, 50.0), 100.0);
    }
}
