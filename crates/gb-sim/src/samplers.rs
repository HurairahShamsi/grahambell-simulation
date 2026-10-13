//! Samplers written in-repo (ARCHITECTURE §6), as functions of uniforms so they combine with
//! keyed draws. Transcendental functions come from `libm`, so results do not depend on the
//! platform's math library.

/// Exponential variate with rate `rate` from a uniform in `(0, 1]`.
pub fn exponential(u_positive: f64, rate: f64) -> f64 {
    -libm::log(u_positive) / rate
}

/// Standard normal variate from two uniforms (Box–Muller; `u1` in `(0, 1]`).
pub fn normal(u1_positive: f64, u2: f64) -> f64 {
    libm::sqrt(-2.0 * libm::log(u1_positive)) * libm::cos(2.0 * core::f64::consts::PI * u2)
}

/// Log-normal variate with the given median and sigma, from a standard normal `z`.
pub fn lognormal(median: f64, sigma: f64, z: f64) -> f64 {
    median * libm::exp(sigma * z)
}

/// Lomax variate: `P(X > x) = (1 + x/scale)^(−shape)`, from a uniform in `(0, 1]`.
pub fn lomax(u_positive: f64, scale: f64, shape: f64) -> f64 {
    scale * (libm::pow(u_positive, -1.0 / shape) - 1.0)
}

/// Failures before the first success at success probability `q`, from a uniform in `(0, 1]`.
pub fn geometric_failures(u_positive: f64, q: f64) -> u64 {
    if q >= 1.0 {
        return 0;
    }
    if q <= 0.0 {
        return u64::MAX;
    }
    let draw = libm::log(u_positive) / libm::log1p(-q);
    if draw >= u64::MAX as f64 {
        u64::MAX
    } else {
        draw.floor() as u64
    }
}

/// `1 − (1 − p)^r`: the probability that at least one of `r` independent trials with success
/// probability `p` succeeds.
pub fn any_success(p: f64, r: u64) -> f64 {
    -libm::expm1(r as f64 * libm::log1p(-p))
}

/// Binomial(r, p) conditioned on at least one success, by inversion from a uniform in
/// `[0, 1)`. Intended for small `r·p` (a step with a success has one winner almost always).
pub fn zero_truncated_binomial(u: f64, r: u64, p: f64) -> u64 {
    if r <= 1 || p >= 1.0 {
        return r.max(1);
    }
    let any = any_success(p, r);
    // P(K = k | K ≥ 1) = pmf(k) / any, with pmf(1) = r p (1 − p)^(r − 1).
    let mut k = 1u64;
    let mut pmf = r as f64 * p * libm::exp((r - 1) as f64 * libm::log1p(-p));
    let mut cumulative = pmf / any;
    let target = u;
    while cumulative <= target && k < r {
        pmf *= (r - k) as f64 / (k + 1) as f64 * p / (1.0 - p);
        k += 1;
        cumulative += pmf / any;
        if pmf <= 0.0 {
            break;
        }
    }
    k
}

/// Piecewise-linear quantile function through `(probability, value)` knots sorted by
/// probability, the first at 0 and the last at 1.
pub fn quantile(knots: &[(f64, f64)], u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    for pair in knots.windows(2) {
        let ((p0, v0), (p1, v1)) = (pair[0], pair[1]);
        if u <= p1 {
            if p1 <= p0 {
                return v1;
            }
            return v0 + (v1 - v0) * (u - p0) / (p1 - p0);
        }
    }
    knots.last().map_or(0.0, |k| k.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{Keyed, Purpose, unit, unit_positive};

    fn uniforms(n: usize) -> Vec<f64> {
        let mut k = Keyed::new(3, "samplers");
        (0..n.div_ceil(8) as u64)
            .flat_map(|i| k.words(Purpose::Setup, 0, i))
            .take(n)
            .map(unit)
            .collect()
    }

    #[test]
    fn exponential_and_lomax_have_their_means() {
        let u = uniforms(200_000);
        let exp_mean = u.iter().map(|x| exponential(1.0 - x, 2.0)).sum::<f64>() / u.len() as f64;
        assert!((exp_mean - 0.5).abs() < 0.01, "{exp_mean}");
        // Lomax mean scale/(shape − 1) = 2/(1.5 − 1) = 4 converges slowly (infinite variance);
        // check the median instead: scale (2^(1/shape) − 1).
        let mut l: Vec<f64> = u.iter().map(|x| lomax(1.0 - x, 2.0, 1.5)).collect();
        l.sort_by(f64::total_cmp);
        let median = l[l.len() / 2];
        let expected = 2.0 * (libm::pow(2.0, 1.0 / 1.5) - 1.0);
        assert!(
            (median / expected - 1.0).abs() < 0.02,
            "{median} vs {expected}"
        );
    }

    #[test]
    fn normal_has_unit_variance() {
        let u = uniforms(200_000);
        let z: Vec<f64> = u.chunks(2).map(|c| normal(1.0 - c[0], c[1])).collect();
        let mean = z.iter().sum::<f64>() / z.len() as f64;
        let var = z.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / z.len() as f64;
        assert!(
            mean.abs() < 0.01 && (var - 1.0).abs() < 0.02,
            "{mean} {var}"
        );
    }

    #[test]
    fn geometric_failures_follow_their_law() {
        let u = uniforms(200_000);
        let q = 0.1;
        let zeros = u
            .iter()
            .filter(|x| geometric_failures(unit_positive_of(**x), q) == 0)
            .count() as f64
            / u.len() as f64;
        assert!((zeros - q).abs() < 0.003, "{zeros}");
        assert_eq!(geometric_failures(0.5, 1.0), 0);
    }

    fn unit_positive_of(u: f64) -> f64 {
        1.0 - u
    }

    #[test]
    fn zero_truncated_binomial_matches_its_law() {
        // r = 3, p = 0.5: P(K = 1 | K ≥ 1) = 3/7, P(2) = 3/7, P(3) = 1/7.
        let u = uniforms(140_000);
        let mut counts = [0u64; 4];
        for x in &u {
            counts[zero_truncated_binomial(*x, 3, 0.5) as usize] += 1;
        }
        let n = u.len() as f64;
        assert!((counts[1] as f64 / n - 3.0 / 7.0).abs() < 0.006);
        assert!((counts[2] as f64 / n - 3.0 / 7.0).abs() < 0.006);
        assert!((counts[3] as f64 / n - 1.0 / 7.0).abs() < 0.006);
        // Small p: one winner with probability 1 − (r − 1) p / 2 approximately.
        let ones = u
            .iter()
            .filter(|x| zero_truncated_binomial(**x, 100_000, 1.0 / 3_000_000.0) == 1)
            .count() as f64
            / n;
        assert!((ones - 0.98347).abs() < 0.003, "{ones}");
    }

    #[test]
    fn any_success_is_the_complement_of_all_failing() {
        assert!((any_success(0.5, 2) - 0.75).abs() < 1e-15);
        assert!((any_success(1.0 / 30_000.0, 1_000) - (1.0 - libm::exp(-1.0 / 30.0))).abs() < 1e-6);
    }

    #[test]
    fn quantile_interpolates_between_knots() {
        let knots = [(0.0, 1.0), (0.5, 2.0), (1.0, 4.0)];
        assert_eq!(quantile(&knots, 0.0), 1.0);
        assert_eq!(quantile(&knots, 0.25), 1.5);
        assert_eq!(quantile(&knots, 0.75), 3.0);
        assert_eq!(quantile(&knots, 1.0), 4.0);
        let _ = unit_positive(0);
    }
}
