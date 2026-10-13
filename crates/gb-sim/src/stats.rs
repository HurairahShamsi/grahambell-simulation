//! Test statistics for the validation checks. χ² and the one-sample Kolmogorov–Smirnov test
//! are M2's (`gb_crypto_tests::stats`); this module adds the two-sample KS test, the
//! randomised probability integral transform for discrete laws, Wilson intervals and ratio
//! estimates with batch-means standard errors.

use gb_analytic::mc::{Estimate, ratio_of_sums};
pub use gb_crypto_tests::stats::{chi_square_p, chi_square_two_sample, kolmogorov_q, ks_uniform};

/// Two-sample Kolmogorov–Smirnov test; returns (D, p-value).
pub fn ks_two_sample(a: &[f64], b: &[f64]) -> (f64, f64) {
    if a.is_empty() || b.is_empty() {
        return (0.0, 1.0);
    }
    let mut x = a.to_vec();
    let mut y = b.to_vec();
    x.sort_by(f64::total_cmp);
    y.sort_by(f64::total_cmp);
    let (n, m) = (x.len(), y.len());
    let (mut i, mut j, mut d) = (0usize, 0usize, 0.0f64);
    while i < n && j < m {
        let v = x[i].min(y[j]);
        while i < n && x[i] <= v {
            i += 1;
        }
        while j < m && y[j] <= v {
            j += 1;
        }
        d = d.max((i as f64 / n as f64 - j as f64 / m as f64).abs());
    }
    let ne = (n * m) as f64 / (n + m) as f64;
    let sqrt = ne.sqrt();
    (d, kolmogorov_q((sqrt + 0.12 + 0.11 / sqrt) * d))
}

/// Randomised probability integral transform of a geometric count `n` (failures before the
/// first success at probability `q`) with an independent uniform `u`: uniform on (0, 1) when
/// `n` follows that law.
pub fn geometric_pit(n: u64, q: f64, u: f64) -> f64 {
    // F(k) = 1 − (1 − q)^(k + 1); P(X = n) = (1 − q)^n q.
    let survive = |k: f64| libm::exp(k * libm::log1p(-q));
    let below = 1.0 - survive(n as f64);
    let mass = survive(n as f64) * q;
    (below + u * mass).clamp(0.0, 1.0)
}

/// Wilson score interval for `k` successes in `n` trials at `z` standard errors.
pub fn wilson(k: u64, n: u64, z: f64) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let n = n as f64;
    let p = k as f64 / n;
    let denom = 1.0 + z * z / n;
    let centre = (p + z * z / (2.0 * n)) / denom;
    let half = z * ((p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt()) / denom;
    ((centre - half).max(0.0), (centre + half).min(1.0))
}

/// Ratio of sums with a batch-means standard error, one batch per entry (days).
pub fn ratio(numerators: &[f64], denominators: &[f64]) -> Estimate {
    ratio_of_sums(numerators, denominators, numerators.len())
}

/// Counts of `values` in bins with the given upper edges (the last bin is open).
pub fn binned(values: &[f64], edges: &[f64]) -> Vec<u64> {
    let mut counts = vec![0u64; edges.len() + 1];
    for v in values {
        let i = edges.iter().position(|e| v < e).unwrap_or(edges.len());
        counts[i] += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{Keyed, Purpose, unit};

    #[test]
    fn two_samples_from_one_law_pass_and_shifted_ones_fail() {
        let mut k = Keyed::new(11, "stats");
        let a: Vec<f64> = (0..2_000)
            .map(|i| unit(k.words(Purpose::Setup, 1, i)[0]))
            .collect();
        let b: Vec<f64> = (0..2_000)
            .map(|i| unit(k.words(Purpose::Setup, 2, i)[0]))
            .collect();
        assert!(ks_two_sample(&a, &b).1 > 0.001);
        let shifted: Vec<f64> = b.iter().map(|x| x + 0.1).collect();
        assert!(ks_two_sample(&a, &shifted).1 < 1e-6);
    }

    #[test]
    fn the_geometric_pit_is_uniform_for_geometric_draws() {
        let mut k = Keyed::new(12, "pit");
        let q = 0.05;
        let pits: Vec<f64> = (0..5_000)
            .map(|i| {
                let w = k.uniforms(Purpose::Setup, 3, i);
                let n = crate::samplers::geometric_failures(1.0 - w[0], q);
                geometric_pit(n, q, w[1])
            })
            .collect();
        assert!(ks_uniform(&pits).1 > 0.001);
        // A wrong q is rejected.
        let wrong: Vec<f64> = (0..5_000)
            .map(|i| {
                let w = k.uniforms(Purpose::Setup, 4, i);
                let n = crate::samplers::geometric_failures(1.0 - w[0], q);
                geometric_pit(n, 0.06, w[1])
            })
            .collect();
        assert!(ks_uniform(&wrong).1 < 0.001);
    }

    #[test]
    fn wilson_intervals_cover_the_proportion() {
        let (lo, hi) = wilson(50, 100, 1.96);
        assert!(lo < 0.5 && hi > 0.5 && hi - lo < 0.2);
        assert_eq!(wilson(0, 0, 1.96), (0.0, 1.0));
    }

    #[test]
    fn bins_count_values() {
        assert_eq!(
            binned(&[0.5, 1.5, 2.5, 9.0], &[1.0, 2.0, 3.0]),
            vec![1, 1, 1, 1]
        );
    }
}
