//! The M3a.1 experiments (architect, 2026-10-14): E1 fork choice, E2 the issuance count rule,
//! E3 liveness with threshold entropy and failover, E4 the offline points ladder.
//!
//! E1's criterion 4 needs E2's hopping results, and E4's entropy rule needs E3's verdict, so
//! these run in two stages ([`super::run_suite`]): E1's matrix, E2 and E3 first, then E1's
//! criterion-4 reruns and E4. Every selection rule is fixed in advance and applied as written.

use super::c::{realistic, region_names};
use super::v::hop_gain;
use super::{Chart, ChartPanel, ChartSeries, Ctx, SectionOut, Table, amplification, div, f, n, r};
use crate::job::{ForkRule, GroupSpec, JobResult, JobSpec, OfflinePointsSpec, Strategy};
use gb_config::protocol::{
    CountBasedCorrection, DeadlineScope, DifficultyCountRule, DifficultyVariant, EntropySigners,
    EquivocationScope, OfflinePointsOutcome, StartRule,
};

const E1: &str = "E1";
const E2: &str = "E2";
const E3: &str = "E3";
const E4: &str = "E4";

/// The 97.5 % quantile of Student's t with `df` degrees of freedom (95 % two-sided).
fn t95(df: usize) -> f64 {
    match df {
        0 => f64::NAN,
        1 => 12.706,
        2 => 4.303,
        3 => 3.182,
        4 => 2.776,
        5 => 2.571,
        6 => 2.447,
        7 => 2.365,
        8 => 2.306,
        9 => 2.262,
        _ => 1.96,
    }
}

/// Mean of `values` and the half-width of its 95 % confidence interval (t over seeds).
fn mean_ci(values: &[f64]) -> (f64, f64) {
    let k = values.len();
    if k == 0 {
        return (f64::NAN, f64::NAN);
    }
    let mean = values.iter().sum::<f64>() / k as f64;
    if k < 2 {
        return (mean, f64::NAN);
    }
    let var = values.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (k - 1) as f64;
    (mean, t95(k - 1) * (var / k as f64).sqrt())
}

/// Whether the 95 % confidence interval of the mean of paired differences `a − b` (same seeds)
/// includes zero: the E1 tie rule.
fn tied(a: &[f64], b: &[f64]) -> bool {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let (mean, half) = mean_ci(&d);
    half.is_finite() && mean.abs() <= half
}

fn mean_interval(res: &JobResult) -> f64 {
    div(res.interval_sum_s, res.interval_count as f64)
}

fn orphan_rate(res: &JobResult) -> f64 {
    div(res.orphans as f64, res.canonical as f64)
}

// ---------------------------------------------------------------------- E2

/// A Variant B difficulty variant: its count rule and correction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DiffVariant {
    pub count_rule: DifficultyCountRule,
    pub correction: Option<CountBasedCorrection>,
}

fn rule_name(rule: DifficultyCountRule) -> &'static str {
    match rule {
        DifficultyCountRule::AfterPostAdmissionWait => "after_wait",
        DifficultyCountRule::AllGloballyActive => "all_globally_active",
        DifficultyCountRule::IssuedEntropy => "issued_entropy",
    }
}

fn correction_name(c: Option<CountBasedCorrection>) -> String {
    match c {
        None => "none".to_string(),
        Some(CountBasedCorrection::RecentIntervalRatio { bound, .. }) => {
            format!("P1 ×{}", 1.0 + bound)
        }
        Some(CountBasedCorrection::Accumulated { bound, .. }) => format!("P2 ×{}", 1.0 + bound),
        Some(CountBasedCorrection::DiscreteExact) => "B-exact".to_string(),
    }
}

/// E2's Variant B variants: count rule {after the wait, all Globally Active, issued entropy}
/// × correction {none, P1 and P2 at clamps 1.1, 1.25 and 1.5, B-exact}.
pub(crate) fn e2_variants(ctx: &Ctx<'_>) -> Vec<(String, DiffVariant)> {
    let mut corrections: Vec<Option<CountBasedCorrection>> = vec![None];
    for bound in [0.10, 0.25, 0.50] {
        corrections.push(Some(CountBasedCorrection::RecentIntervalRatio {
            window_blocks: 144,
            bound,
        }));
    }
    for bound in [0.10, 0.25, 0.50] {
        corrections.push(Some(CountBasedCorrection::Accumulated {
            window_blocks: 144,
            bound,
        }));
    }
    corrections.push(Some(CountBasedCorrection::DiscreteExact));
    let rules = [
        DifficultyCountRule::AfterPostAdmissionWait,
        DifficultyCountRule::AllGloballyActive,
        DifficultyCountRule::IssuedEntropy,
    ];
    let mut out = Vec::new();
    for rule in rules {
        for c in &corrections {
            out.push((
                format!("{} / {}", rule_name(rule), correction_name(*c)),
                DiffVariant {
                    count_rule: rule,
                    correction: *c,
                },
            ));
        }
    }
    if ctx.quick {
        // Tests: one variant per count rule.
        out.into_iter().step_by(8).collect()
    } else {
        out
    }
}

fn start_name(s: StartRule) -> &'static str {
    match s {
        StartRule::A => "(a)",
        StartRule::B => "(b)",
        StartRule::R => "(r)",
    }
}

const CHURNS: [(&str, Option<[f64; 3]>); 2] =
    [("realistic", None), ("laptop-heavy", Some([0.1, 0.1, 0.8]))];
const HOP_RATIOS: [f64; 4] = [0.1, 0.5, 1.0, 4.0];

/// A hopping job as in V3: idealised, `honest` honest slots and a hopping group of `ratio`
/// times as many, one window of 144 blocks active then two away (`idle` windows idle but
/// admitted first).
fn hop_job(ctx: &Ctx<'_>, id: &str, rep: u64, ratio: f64, idle: u32, v: DiffVariant) -> JobSpec {
    let honest = ctx.pick(1_000u32, 100);
    let cycles = ctx.pick(200.0, 12.0);
    let mut j = ctx.job(id, E2, rep).idealised();
    let total = (f64::from(honest) * (1.0 + ratio)).round() as u32;
    j.miners = total;
    j.registered_ids = ctx.pick(2_900, 1_300).max(u64::from(total) / 30 * 10 + 130);
    j.window_blocks = 144;
    j.groups = vec![GroupSpec {
        label: "hopper".to_string(),
        share: ratio / (1.0 + ratio),
        strategy: Strategy::Hop {
            window_blocks: 144,
            idle_windows: idle,
            rest_windows: 2,
        },
        always_online: true,
    }];
    j.days = cycles * f64::from(3 + idle) * 144.0 * 30.6 / 86_400.0;
    j.difficulty = DifficultyVariant::CountBased;
    j.count_rule = v.count_rule;
    j.correction = v.correction;
    j
}

/// E2: issuance (C4 setup, both churns, start rules (a) and (r), Variant A as the reference)
/// and hopping (V3 setup, every variant at m ∈ {0.1, 0.5, 1, 4}, and one idle-but-counted
/// cycle at m = 1).
pub(crate) fn e2_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    let m = ctx.pick(10_000, 300);
    let registered = ctx.pick(29_000, 2_900);
    let days = ctx.days(20.0, 0.5);
    let starts: Vec<StartRule> = ctx.pick(vec![StartRule::A, StartRule::R], vec![StartRule::R]);
    for start in &starts {
        for (churn, mix) in CHURNS {
            let base = |id: String| {
                let mut j = realistic(ctx, &id, E2, 0, m, registered, days);
                j.start_rule = *start;
                j.device_mix = mix.map(|x| x.to_vec());
                j
            };
            let mut a = base(format!("E2-iss-{}-{churn}-A", start_name(*start)));
            a.difficulty = DifficultyVariant::BitcoinRetarget;
            a.window_blocks = 144;
            a.clamp = Some(4.0);
            jobs.push(a);
            for (label, v) in e2_variants(ctx) {
                let mut j = base(format!("E2-iss-{}-{churn}-{label}", start_name(*start)));
                j.difficulty = DifficultyVariant::CountBased;
                j.count_rule = v.count_rule;
                j.correction = v.correction;
                jobs.push(j);
            }
        }
    }
    let ratios: Vec<f64> = ctx.pick(HOP_RATIOS.to_vec(), vec![1.0]);
    for (label, v) in e2_variants(ctx) {
        for (i, ratio) in ratios.iter().enumerate() {
            // The same seed for every variant at one m: common random numbers.
            jobs.push(hop_job(
                ctx,
                &format!("E2-hop-{label}-m{ratio}"),
                2_000 + i as u64,
                *ratio,
                0,
                v,
            ));
        }
        jobs.push(hop_job(
            ctx,
            &format!("E2-hop-{label}-idle-then-active-m1"),
            2_100,
            1.0,
            1,
            v,
        ));
    }
    jobs
}

/// The E2 variants E1's criterion 4 may use: those whose hopping gain is at most 1.02 at every
/// m (H5), or, if none passes, the one with the smallest worst-case gain.
#[derive(Debug, Clone)]
pub(crate) struct H5Outcome {
    /// (label, variant, worst-case gain).
    pub variants: Vec<(String, DiffVariant, f64)>,
    /// Whether they passed H5 (false: none did, and the best-ranked one is used).
    pub passed: bool,
}

pub(crate) fn e2_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> (SectionOut, H5Outcome) {
    let mut out = SectionOut::default();
    let variants = e2_variants(ctx);
    let starts: Vec<StartRule> = ctx.pick(vec![StartRule::A, StartRule::R], vec![StartRule::R]);
    let ratios: Vec<f64> = ctx.pick(HOP_RATIOS.to_vec(), vec![1.0]);
    let mut issuance = Table::new(
        "E2_issuance.csv",
        &[
            "start_rule",
            "churn",
            "variant",
            "count_rule",
            "correction",
            "final_chain_blocks",
            "mean_interval_s",
            "deviation_from_30s_percent",
            "daily_blocks_sd",
            "daily_blocks_sd_percent",
            "pauses_over_120s",
            "max_gap_s",
        ],
    );
    let mut i = 0;
    let mut interval_err: std::collections::BTreeMap<(String, String), f64> = Default::default();
    for start in &starts {
        for (churn, _) in CHURNS {
            let rows = std::iter::once(("A (K=144, 4×)".to_string(), None)).chain(
                variants
                    .iter()
                    .map(|(l, v)| (l.clone(), Some(*v)))
                    .collect::<Vec<_>>(),
            );
            for (label, v) in rows {
                let Some(res) = results.get(i) else { break };
                i += 1;
                let mean = mean_interval(res);
                // Whole days after the warm-up day, without the last (the final chain stops 12
                // blocks short of the end).
                let days: Vec<f64> = if res.daily_ids.len() > 3 {
                    res.daily_ids[1..res.daily_ids.len() - 1]
                        .iter()
                        .map(|x| *x as f64)
                        .collect()
                } else {
                    Vec::new()
                };
                let (dmean, _) = super::mean_se(&days);
                let dsd = sd(&days);
                issuance.push(vec![
                    start_name(*start).to_string(),
                    churn.to_string(),
                    label.clone(),
                    v.map_or("—".to_string(), |v| rule_name(v.count_rule).to_string()),
                    v.map_or("—".to_string(), |v| correction_name(v.correction)),
                    n(res.canonical),
                    f(mean),
                    f(100.0 * (mean / 30.0 - 1.0)),
                    f(dsd),
                    f(100.0 * div(dsd, dmean)),
                    n(res.pauses),
                    f(res.max_gap_s),
                ]);
                if churn == "realistic" {
                    interval_err.insert(
                        (start_name(*start).to_string(), label.clone()),
                        (mean - 30.0).abs(),
                    );
                }
            }
        }
    }
    let mut hopping = Table::new(
        "E2_hopping.csv",
        &["variant", "m", "hopping_gain", "se", "at_most_1.02"],
    );
    let mut h5 = Table::new(
        "E2_h5.csv",
        &[
            "variant",
            "worst_gain_over_m",
            "passes_h5",
            "idle_then_active_gain_m1",
            "idle_se",
        ],
    );
    let honest = ctx.pick(1_000.0, 100.0);
    let mut worst: Vec<(String, DiffVariant, f64, bool)> = Vec::new();
    let mut points_a = Vec::new();
    let mut points_r = Vec::new();
    for (label, v) in &variants {
        let mut gains = Vec::new();
        for ratio in &ratios {
            let Some(res) = results.get(i) else { break };
            i += 1;
            let (gain, se) = hop_gain(res, honest * (1.0 + ratio));
            gains.push(gain);
            hopping.push(vec![
                label.clone(),
                f(*ratio),
                f(gain),
                f(se),
                (gain <= 1.02).to_string(),
            ]);
        }
        let Some(idle) = results.get(i) else { break };
        i += 1;
        let (idle_gain, idle_se) = hop_gain(idle, honest * 2.0);
        let w = gains.iter().copied().fold(f64::NAN, f64::max);
        let passes = gains.len() == ratios.len() && gains.iter().all(|g| *g <= 1.02);
        h5.push(vec![
            label.clone(),
            f(w),
            passes.to_string(),
            f(idle_gain),
            f(idle_se),
        ]);
        worst.push((label.clone(), *v, w, passes));
        for (start, pts) in [("(a)", &mut points_a), ("(r)", &mut points_r)] {
            if let Some(err) = interval_err.get(&(start.to_string(), label.clone())) {
                pts.push((*err, w));
            }
        }
    }
    let passing: Vec<(String, DiffVariant, f64)> = worst
        .iter()
        .filter(|x| x.3)
        .map(|x| (x.0.clone(), x.1, x.2))
        .collect();
    let outcome = if passing.is_empty() {
        let best = worst
            .iter()
            .filter(|x| x.2.is_finite())
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|x| vec![(x.0.clone(), x.1, x.2)])
            .unwrap_or_default();
        H5Outcome {
            variants: best,
            passed: false,
        }
    } else {
        H5Outcome {
            variants: passing,
            passed: true,
        }
    };
    out.tables.push(issuance);
    out.tables.push(hopping);
    out.tables.push(h5);
    out.charts.push(Chart {
        file: "E2_tradeoff.png".to_string(),
        title: "E2 — interval error against hopping gain".to_string(),
        subtitle: "one point per Variant B variant (realistic churn); H5 allows a gain of 1.02"
            .to_string(),
        panels: vec![ChartPanel {
            title: "the trade-off".to_string(),
            x_label: "|mean interval − 30 s| (s)".to_string(),
            y_label: "worst hopping gain over m".to_string(),
            log_x: false,
            series: vec![
                ChartSeries {
                    label: "start rule (a)".to_string(),
                    points: points_a,
                },
                ChartSeries {
                    label: "start rule (r)".to_string(),
                    points: points_r,
                },
            ],
            references: vec![("H5".to_string(), 1.02)],
        }],
    });
    let list = |o: &H5Outcome| {
        o.variants
            .iter()
            .map(|(l, _, w)| format!("{l} (worst gain {})", r(*w, 3)))
            .collect::<Vec<_>>()
            .join("; ")
    };
    out.summary = format!(
        "## E2 — the issuance count rule\n\nVariant B under three count rules, without a correction and with P1 and P2 at clamps 1.1, 1.25 and 1.5 and B-exact; Variant A is the reference. Intervals in the C4 setup under both churns and start rules (`E2_issuance.csv`); hopping gains in the V3 setup at m ∈ {{0.1, 0.5, 1, 4}} and one idle-but-counted cycle at m = 1 (`E2_hopping.csv`, `E2_h5.csv`). The chart shows the trade-off between interval error and the worst hopping gain.\n\n{}\n",
        if outcome.passed {
            format!(
                "Variants passing H5 (gain ≤ 1.02 at every m), used by E1's criterion 4: {}.",
                list(&outcome)
            )
        } else {
            format!(
                "**No variant passes H5** (gain ≤ 1.02 at every m). Ranked on the smallest worst-case gain, E1's criterion 4 uses {}.",
                list(&outcome)
            )
        }
    );
    (out, outcome)
}

fn sd(values: &[f64]) -> f64 {
    let k = values.len() as f64;
    if k < 2.0 {
        return f64::NAN;
    }
    let mean = values.iter().sum::<f64>() / k;
    (values.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (k - 1.0)).sqrt()
}

// ---------------------------------------------------------------------- E1

/// One fork-choice configuration of E1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ForkConfig {
    pub start: StartRule,
    pub tie: ForkRule,
    pub header: EquivocationScope,
    pub deadline: Option<f64>,
}

impl ForkConfig {
    fn name(&self) -> String {
        format!(
            "{} {} {} {}",
            start_name(self.start),
            self.tie.name(),
            match self.header {
                EquivocationScope::SameHeight => "literal",
                EquivocationScope::SameParent => "[P]same-parent",
            },
            self.deadline
                .map_or("no-deadline".to_string(), |d| format!("D{d}-soft"))
        )
    }

    /// Simplicity rank, fixed in advance (architect, 2026-10-14): (a) before (r), v0.6 before
    /// first seen, literal before [P], D = 10 s soft before no deadline; compared in that order.
    fn simplicity(&self) -> (u8, u8, u8, u8) {
        (
            u8::from(self.start == StartRule::R),
            u8::from(self.tie == ForkRule::FirstSeen),
            u8::from(self.header == EquivocationScope::SameParent),
            u8::from(self.deadline.is_none()),
        )
    }
}

/// The 16 configurations, simplest first.
pub(crate) fn e1_configs() -> Vec<ForkConfig> {
    let mut v = Vec::new();
    for start in [StartRule::A, StartRule::R] {
        for tie in [ForkRule::V06, ForkRule::FirstSeen] {
            for header in [EquivocationScope::SameHeight, EquivocationScope::SameParent] {
                for deadline in [Some(10.0), None] {
                    v.push(ForkConfig {
                        start,
                        tie,
                        header,
                        deadline,
                    });
                }
            }
        }
    }
    v.sort_by_key(ForkConfig::simplicity);
    v
}

fn e1_seeds(ctx: &Ctx<'_>) -> u64 {
    ctx.reps(5, 1)
}

/// An E1 run: the C3 setup (RIPE latency, 0.5 % loss, one 5-minute partition per region per
/// day, realistic churn) under `c`, with a 10 % withholder or honest.
fn e1_job(ctx: &Ctx<'_>, c: &ForkConfig, withhold: bool, seed: u64, tag: &str) -> JobSpec {
    let mut j = realistic(
        ctx,
        &format!(
            "E1-{}-{}{tag}-s{seed}",
            c.name(),
            if withhold { "withholder" } else { "honest" }
        ),
        E1,
        3_000 + seed,
        ctx.pick(10_000, 300),
        ctx.pick(29_000, 2_900),
        ctx.days(10.0, 0.5),
    );
    j.loss = 0.005;
    j.partition_rate_per_region_per_day = 1.0;
    j.partition_length_s = 300.0;
    j.start_rule = c.start;
    j.fork_rule = c.tie;
    j.equivocation = c.header;
    j.deadline = c.deadline;
    j.deadline_scope = DeadlineScope::NotExtendedOnly;
    if withhold {
        j.groups = vec![GroupSpec {
            label: "withholder".to_string(),
            share: 0.1,
            strategy: Strategy::Withhold,
            always_online: false,
        }];
    }
    j
}

/// Stage 1 of E1: every configuration, honest and with a withholder, over the seeds.
pub(crate) fn e1_stage1_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    for c in e1_configs() {
        for withhold in [false, true] {
            for s in 0..e1_seeds(ctx) {
                jobs.push(e1_job(ctx, &c, withhold, s, ""));
            }
        }
    }
    jobs
}

/// Per configuration: its honest and withholder runs from stage 1.
fn e1_runs<'r>(
    ctx: &Ctx<'_>,
    results: &'r [JobResult],
) -> Vec<(Vec<&'r JobResult>, Vec<&'r JobResult>)> {
    let seeds = e1_seeds(ctx) as usize;
    e1_configs()
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let base = i * 2 * seeds;
            let honest = (0..seeds).filter_map(|s| results.get(base + s)).collect();
            let with = (0..seeds)
                .filter_map(|s| results.get(base + seeds + s))
                .collect();
            (honest, with)
        })
        .collect()
}

/// Criteria 1–3 of E1, from stage 1: (zero reorgs below confirmation, withholder
/// amplification ≤ 1, no validator split for good) for each configuration. A validator merely
/// on another branch at the top when a run ends is not split for good.
fn e1_criteria(ctx: &Ctx<'_>, results: &[JobResult]) -> Vec<(bool, bool, bool)> {
    e1_runs(ctx, results)
        .iter()
        .map(|(honest, with)| {
            let all = honest.iter().chain(with.iter());
            let c1 = all.clone().all(|r| r.deep_reorgs == 0);
            let (amp, _, _) = amplification(with, 1);
            let c2 = amp <= 1.0;
            let c3 = all.clone().all(|r| r.permanent_split_panel_nodes == 0);
            (c1, c2, c3)
        })
        .collect()
}

/// Stage 2 of E1 (criterion 4): every configuration that meets criteria 1–3 is rerun honest
/// over the seeds with every E2 variant that passes H5.
pub(crate) fn e1_stage2_jobs(ctx: &Ctx<'_>, stage1: &[JobResult], h5: &H5Outcome) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    let criteria = e1_criteria(ctx, stage1);
    for (c, ok) in e1_configs().iter().zip(&criteria) {
        if !(ok.0 && ok.1 && ok.2) {
            continue;
        }
        for (label, v, _) in &h5.variants {
            for s in 0..e1_seeds(ctx) {
                let mut j = e1_job(ctx, c, false, s, &format!("-{label}"));
                j.difficulty = DifficultyVariant::CountBased;
                j.count_rule = v.count_rule;
                j.correction = v.correction;
                jobs.push(j);
            }
        }
    }
    jobs
}

/// A configuration's criterion-4 result: its best H5-passing variant, the per-seed
/// |mean interval − 30 s| with it, their mean, and whether no rerun reorganised below
/// confirmation.
#[derive(Debug, Clone)]
struct Crit4 {
    variant: String,
    per_seed: Vec<f64>,
    dev: f64,
    clean: bool,
}

pub(crate) fn e1_analyse(
    ctx: &Ctx<'_>,
    stage1: &[JobResult],
    h5: &H5Outcome,
    stage2: &[JobResult],
) -> SectionOut {
    let mut out = SectionOut::default();
    let configs = e1_configs();
    let runs = e1_runs(ctx, stage1);
    let criteria = e1_criteria(ctx, stage1);
    let seeds = e1_seeds(ctx) as usize;
    let regions = region_names(ctx);
    // Criterion 4 from the reruns: per surviving configuration, the variant with the smallest
    // |mean interval − 30 s|; a reorg below confirmation in any rerun eliminates it too.
    let mut crit4: Vec<Option<Crit4>> = vec![None; configs.len()];
    let mut c4_table = Table::new(
        "E1_criterion4.csv",
        &[
            "config",
            "variant",
            "mean_interval_s",
            "ci95",
            "abs_deviation_s",
            "reorgs_below_confirmation",
        ],
    );
    let mut k = 0;
    for (ci, ok) in criteria.iter().enumerate() {
        if !(ok.0 && ok.1 && ok.2) {
            continue;
        }
        let mut best: Option<Crit4> = None;
        let mut deep = false;
        for (label, _, _) in &h5.variants {
            let reruns: Vec<&JobResult> = (0..seeds).filter_map(|s| stage2.get(k + s)).collect();
            k += seeds;
            let means: Vec<f64> = reruns.iter().map(|r| mean_interval(r)).collect();
            let (mean, half) = mean_ci(&means);
            let reorgs: u64 = reruns.iter().map(|r| r.deep_reorgs).sum();
            deep |= reorgs > 0;
            let dev = (mean - 30.0).abs();
            c4_table.push(vec![
                configs[ci].name(),
                label.clone(),
                f(mean),
                f(half),
                f(dev),
                n(reorgs),
            ]);
            if best.as_ref().is_none_or(|b| dev < b.dev) {
                best = Some(Crit4 {
                    variant: label.clone(),
                    per_seed: means.iter().map(|m| (m - 30.0).abs()).collect(),
                    dev,
                    clean: true,
                });
            }
        }
        if let Some(mut c) = best {
            c.clean = !deep;
            crit4[ci] = Some(c);
        }
    }
    // The mechanical selection: criterion 4, then 5, then the simplest of the tied.
    let survivors: Vec<(usize, &Crit4)> = crit4
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.as_ref().filter(|c| c.clean).map(|c| (i, c)))
        .collect();
    let orphans: Vec<Vec<f64>> = runs
        .iter()
        .map(|(h, _)| h.iter().map(|r| orphan_rate(r)).collect())
        .collect();
    let mut tied4: Vec<usize> = Vec::new();
    let mut tied5: Vec<usize> = Vec::new();
    if let Some(&(best4, b4)) = survivors.iter().min_by(|a, b| a.1.dev.total_cmp(&b.1.dev)) {
        tied4 = survivors
            .iter()
            .filter(|(i, c)| *i == best4 || tied(&c.per_seed, &b4.per_seed))
            .map(|(i, _)| *i)
            .collect();
        let mean_orphans = |i: usize| mean_ci(&orphans[i]).0;
        if let Some(&best5) = tied4
            .iter()
            .min_by(|a, b| mean_orphans(**a).total_cmp(&mean_orphans(**b)))
        {
            tied5 = tied4
                .iter()
                .copied()
                .filter(|i| *i == best5 || tied(&orphans[*i], &orphans[best5]))
                .collect();
        }
    }
    let winner = tied5
        .iter()
        .copied()
        .min_by_key(|i| configs[*i].simplicity());
    let mut table = Table::new(
        "E1_fork_choice.csv",
        &[
            "config",
            "start_rule",
            "tie_break",
            "header_rule",
            "deadline",
            "orphan_rate",
            "orphan_rate_ci95",
            "mean_interval_s",
            "mean_interval_ci95",
            "reorgs_below_confirmation",
            "max_reorg",
            "withholder_amplification",
            "withholder_amplification_se",
            "permanently_split_validators_max",
            "validators_off_chain_at_end_max",
            "c1_no_deep_reorg",
            "c2_amplification_at_most_1",
            "c3_no_split",
            "c4_best_variant",
            "c4_abs_deviation_s",
            "c4_reruns_no_deep_reorg",
            "tied_on_c4",
            "tied_on_c5",
            "winner",
        ],
    );
    let mut region_table = Table::new(
        "E1_region_win_share.csv",
        &[
            "config",
            "region",
            "final_chain_ids",
            "win_share",
            "slot_share",
        ],
    );
    let mut lines = Vec::new();
    for (i, c) in configs.iter().enumerate() {
        let (honest, with) = &runs[i];
        let (om, oh) = mean_ci(&orphans[i]);
        let intervals: Vec<f64> = honest.iter().map(|r| mean_interval(r)).collect();
        let (im, ih) = mean_ci(&intervals);
        let all: Vec<&&JobResult> = honest.iter().chain(with.iter()).collect();
        let deep: u64 = all.iter().map(|r| r.deep_reorgs).sum();
        let max_reorg = all.iter().map(|r| r.max_reorg).max().unwrap_or(0);
        let (amp, amp_se, _) = amplification(with, 1);
        let split = all
            .iter()
            .map(|r| r.permanent_split_panel_nodes)
            .max()
            .unwrap_or(0);
        let off_chain = all.iter().map(|r| r.split_panel_nodes).max().unwrap_or(0);
        let c4 = crit4[i].as_ref();
        table.push(vec![
            c.name(),
            start_name(c.start).to_string(),
            c.tie.name().to_string(),
            match c.header {
                EquivocationScope::SameHeight => "literal".to_string(),
                EquivocationScope::SameParent => "same_parent".to_string(),
            },
            c.deadline
                .map_or("none".to_string(), |d| format!("{d} s soft")),
            f(om),
            f(oh),
            f(im),
            f(ih),
            n(deep),
            n(max_reorg),
            f(amp),
            f(amp_se),
            n(split),
            n(off_chain),
            criteria[i].0.to_string(),
            criteria[i].1.to_string(),
            criteria[i].2.to_string(),
            c4.map_or(String::new(), |x| x.variant.clone()),
            c4.map_or(String::new(), |x| f(x.dev)),
            c4.map_or(String::new(), |x| x.clean.to_string()),
            tied4.contains(&i).to_string(),
            tied5.contains(&i).to_string(),
            (winner == Some(i)).to_string(),
        ]);
        let canonical: u64 = honest.iter().map(|r| r.canonical).sum();
        let slot_all: f64 = honest
            .iter()
            .map(|r| r.slot_s_by_region.iter().sum::<f64>())
            .sum();
        for (rg, name) in regions.iter().enumerate() {
            let ids: u64 = honest
                .iter()
                .map(|r| r.ids_by_region.get(rg).copied().unwrap_or(0))
                .sum();
            let slots: f64 = honest
                .iter()
                .map(|r| r.slot_s_by_region.get(rg).copied().unwrap_or(0.0))
                .sum();
            region_table.push(vec![
                c.name(),
                name.clone(),
                n(ids),
                f(div(ids as f64, canonical as f64)),
                f(div(slots, slot_all)),
            ]);
        }
        lines.push(format!(
            "{}: orphans {} % (±{}), interval {} s (±{}), {} reorgs below confirmation (deepest {}), withholder amplification {} (SE {}), at most {} validators split for good ({} off the final chain at the end){}",
            c.name(),
            r(100.0 * om, 2),
            r(100.0 * oh, 2),
            r(im, 2),
            r(ih, 2),
            deep,
            max_reorg,
            r(amp, 3),
            r(amp_se, 3),
            split,
            off_chain,
            super::storm_note(&all.iter().map(|x| **x).collect::<Vec<_>>())
        ));
    }
    out.tables.push(table);
    out.tables.push(region_table);
    out.tables.push(c4_table);
    let meets: Vec<String> = (0..configs.len())
        .filter(|i| criteria[*i].0 && criteria[*i].1 && criteria[*i].2)
        .map(|i| configs[i].name())
        .collect();
    let verdict = match winner {
        Some(w) => format!(
            "**Winner: {}.** Criteria 1–3 are met by {} of 16 configurations ({}); criterion 4 used {} {}; after the criterion-4 reruns {} remain; ties on criterion 4: {}; on criterion 5: {}.",
            configs[w].name(),
            meets.len(),
            meets.join(", "),
            if h5.passed {
                "the E2 variants that pass H5:"
            } else {
                "(no E2 variant passes H5) the variant with the smallest worst-case gain:"
            },
            h5.variants
                .iter()
                .map(|v| v.0.clone())
                .collect::<Vec<_>>()
                .join(", "),
            survivors.len(),
            tied4
                .iter()
                .map(|i| configs[*i].name())
                .collect::<Vec<_>>()
                .join(", "),
            tied5
                .iter()
                .map(|i| configs[*i].name())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        None if meets.is_empty() => {
            "**No configuration meets criteria 1–3.** Nothing is selected; the criteria were not loosened.".to_string()
        }
        None => format!(
            "**No configuration survives:** {} met criteria 1–3 in the matrix ({}), but each had a reorg below confirmation in a criterion-4 rerun. Nothing is selected; the criteria were not loosened.",
            meets.len(),
            meets.join(", ")
        ),
    };
    out.summary = format!(
        "## E1 — fork choice\n\nC3 setup (RIPE latency, 0.5 % loss, one 5-minute partition per region per day, realistic churn), {} seeds per configuration, honest and with a 10 % withholder (`E1_fork_choice.csv`, `E1_region_win_share.csv`, `E1_criterion4.csv`). Criteria, fixed in advance and applied mechanically: 1. zero reorgs below confirmation in every run (and in every criterion-4 rerun); 2. withholder amplification ≤ 1; 3. no validator split for good (one that can never accept a settled block) in any run; 4. the smallest |mean interval − 30 s| with the configuration's best H5-passing E2 variant; 5. the lowest orphan rate. A tie is a 95 % CI of the paired difference over seeds that includes zero; ties go to the simplest rule ((a) before (r), v0.6 before first seen, literal before [P], D = 10 s soft before no deadline).\n\n{verdict}\n\n{}\n",
        seeds,
        lines
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    out
}

// ---------------------------------------------------------------------- E3

/// E3's configurations: entropy rule × failover.
pub(crate) fn e3_configs() -> Vec<(EntropySigners, Option<u32>)> {
    let mut v = Vec::new();
    for signers in [
        EntropySigners::AllOnlineMembers,
        EntropySigners::ThresholdTwoThirds,
        EntropySigners::ThresholdMajority,
    ] {
        for failover in [None, Some(2)] {
            v.push((signers, failover));
        }
    }
    v
}

fn signers_name(s: EntropySigners) -> &'static str {
    match s {
        EntropySigners::AllOnlineMembers => "all-online (One Chance)",
        EntropySigners::ThresholdTwoThirds => "threshold 2n/3",
        EntropySigners::ThresholdMajority => "threshold 21/40",
    }
}

fn e3_name(c: &(EntropySigners, Option<u32>)) -> String {
    format!(
        "{}, {}",
        signers_name(c.0),
        if c.1.is_some() {
            "failover"
        } else {
            "no failover"
        }
    )
}

/// The day-9.19 event of the M3a reference runs: a feeder fault in as_east (region 6) from
/// day 8.738735 to 9.192047, which a bug made region-wide (ASSUMPTIONS SIM-35); E3 replays it
/// as a regional blackout, and gives the scripted blackouts of every region its length.
pub(crate) const EVENT_919: (usize, f64, f64) = (6, 8.738_735, 39_166.0);
const E3_DEPARTURES: [f64; 3] = [0.0, 0.1, 0.25];

fn e3_seeds(ctx: &Ctx<'_>) -> u64 {
    ctx.reps(5, 1)
}

fn e3_regions(ctx: &Ctx<'_>) -> Vec<usize> {
    ctx.pick((0..8).collect(), vec![6])
}

fn configure(j: &mut JobSpec, c: &(EntropySigners, Option<u32>)) {
    j.entropy_signers = c.0;
    j.failover_after_rounds = c.1;
    j.track_outages = true;
}

/// E3: C7's three departure profiles, the day-9.19 event, and a scripted blackout per region
/// over the seeds, each under every configuration.
pub(crate) fn e3_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    for c in e3_configs() {
        for (i, d) in E3_DEPARTURES.iter().enumerate() {
            let mut j = realistic(
                ctx,
                &format!("E3-C7-departures{d}-{}", e3_name(&c)),
                E3,
                10 + i as u64,
                ctx.pick(1_000, 200),
                ctx.pick(290_000, 2_900),
                ctx.days(30.0, 0.5),
            );
            j.departures_per_year = Some(*d);
            j.kwc_sample_interval_s = 1_800.0;
            j.kwc_sample_size = 300;
            configure(&mut j, &c);
            jobs.push(j);
        }
        // The day-9.19 event, in the seed-0 setup where M3a saw it.
        let (region, day, secs) = EVENT_919;
        let mut j = realistic(
            ctx,
            &format!("E3-919-{}", e3_name(&c)),
            E3,
            0,
            ctx.pick(10_000, 300),
            ctx.pick(29_000, 2_900),
            1.0,
        );
        // To day 9.5: the event ends on day 9.192 (quick: a 1-hour blackout on day 0.3).
        j.days = ctx.pick(9.5, 0.7);
        j.scripted_blackouts = vec![(region, ctx.pick(day, 0.3), ctx.pick(secs, 3_600.0))];
        configure(&mut j, &c);
        jobs.push(j);
        for region in e3_regions(ctx) {
            for s in 0..e3_seeds(ctx) {
                let mut j = realistic(
                    ctx,
                    &format!("E3-blackout-r{region}-s{s}-{}", e3_name(&c)),
                    E3,
                    4_000 + s,
                    ctx.pick(10_000, 300),
                    ctx.pick(29_000, 2_900),
                    1.0,
                );
                // A day of warm-up, the blackout from day 1.5 (to 1.95), then half a day more.
                j.days = ctx.pick(2.5, 0.7);
                j.scripted_blackouts = vec![(region, ctx.pick(1.5, 0.3), ctx.pick(secs, 3_600.0))];
                configure(&mut j, &c);
                jobs.push(j);
            }
        }
    }
    jobs
}

/// E4's entropy rule and failover, as E3 decides them.
#[derive(Debug, Clone)]
pub(crate) struct E4Setting {
    pub signers: EntropySigners,
    pub failover: Option<u32>,
    pub reason: String,
}

/// UPS share and runtime of a region's households (SIM-34; amendment of 2026-10-14).
fn backup_power(ctx: &Ctx<'_>, region: usize) -> (f64, f64) {
    let net = &ctx.cfg.sim.network;
    let unreliable = net
        .unreliable_grid_regions
        .iter()
        .any(|x| net.regions.get(region) == Some(x));
    let grid = if unreliable {
        &ctx.cfg.sim.households.unreliable_grid
    } else {
        &ctx.cfg.sim.households.reliable_grid
    };
    (grid.ups_share, grid.ups_runtime_h)
}

/// P(X ≥ k) for X binomial with `seats` trials and success probability `s`.
fn binomial_tail(seats: u64, k: u64, s: f64) -> f64 {
    use num_traits::ToPrimitive;
    (k..=seats)
        .map(|i| {
            gb_analytic::exact::binomial_coefficient(seats, i)
                .to_f64()
                .unwrap_or(f64::INFINITY)
                * s.powi(i as i32)
                * (1.0 - s).powi((seats - i) as i32)
        })
        .sum()
}

pub(crate) fn e3_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> (SectionOut, E4Setting) {
    let mut out = SectionOut::default();
    let configs = e3_configs();
    let regions = region_names(ctx);
    let per_config = E3_DEPARTURES.len() + 1 + e3_regions(ctx).len() * e3_seeds(ctx) as usize;
    let mut liveness = Table::new(
        "E3_liveness.csv",
        &[
            "config",
            "setup",
            "kwc_samples",
            "kwcs_below_quorum_share",
            "max_gap_s",
            "max_gap_start_day",
            "pauses_over_120s",
            "pause_hours",
            "final_chain_blocks",
            "miners_reassigned",
            "failover_triggers",
            "failover_switch_median_s",
            "quorum_failures",
        ],
    );
    let mut outages = Table::new(
        "E3_outages.csv",
        &[
            "config",
            "run",
            "kind",
            "region",
            "ups_share",
            "ups_runtime_h",
            "start_day",
            "hours",
            "households",
            "miners_lost_power",
            "miners_lost_witnessing_max",
            "witnessing_lost_miner_rounds",
            "kwcs_unable_max",
            "max_gap_s",
        ],
    );
    let mut test = Table::new(
        "E3_blackout_test.csv",
        &[
            "config",
            "blackouts",
            "with_a_gap_over_10_min",
            "worst_gap_s",
            "event_919_gap_s",
            "no_gap_over_10_min",
        ],
    );
    let mut lines = Vec::new();
    let mut passes: Vec<bool> = Vec::new();
    for (ci, c) in configs.iter().enumerate() {
        let runs = &results
            [(ci * per_config).min(results.len())..((ci + 1) * per_config).min(results.len())];
        let mut gaps: Vec<f64> = Vec::new();
        let mut event = f64::NAN;
        for (k, res) in runs.iter().enumerate() {
            let setup = if k < E3_DEPARTURES.len() {
                format!("C7 departures {}", E3_DEPARTURES[k])
            } else if k == E3_DEPARTURES.len() {
                "day-9.19 event".to_string()
            } else {
                let b = k - E3_DEPARTURES.len() - 1;
                let seeds = e3_seeds(ctx) as usize;
                format!(
                    "blackout {} seed {}",
                    regions
                        .get(e3_regions(ctx)[b / seeds])
                        .cloned()
                        .unwrap_or_default(),
                    b % seeds
                )
            };
            let mut sw = res.failover_s.clone();
            sw.sort_by(f64::total_cmp);
            liveness.push(vec![
                e3_name(c),
                setup.clone(),
                n(res.kwc_samples),
                f(div(
                    res.kwc_fail_unregistered as f64,
                    res.kwc_samples as f64,
                )),
                f(res.max_gap_s),
                f(res.max_gap_start_s / 86_400.0),
                n(res.pauses),
                f(res.pause_s / 3_600.0),
                n(res.canonical),
                n(res.failover_moves),
                n(res.failover_triggers),
                f(super::quantile(&sw, 0.5)),
                n(res.quorum_failures),
            ]);
            for o in &res.outages {
                let (ups, runtime) = backup_power(ctx, o.region as usize);
                outages.push(vec![
                    e3_name(c),
                    setup.clone(),
                    [
                        "regional blackout",
                        "provider outage",
                        "scripted regional blackout",
                    ]
                    .get(o.kind as usize)
                    .copied()
                    .unwrap_or("other")
                    .to_string(),
                    regions.get(o.region as usize).cloned().unwrap_or_default(),
                    f(ups),
                    f(runtime),
                    f(o.start_s / 86_400.0),
                    f(o.duration_s / 3_600.0),
                    n(o.households),
                    n(o.miners_lost_power),
                    n(o.witnessing_lost_max),
                    n(o.witnessing_lost_miner_rounds),
                    n(o.kwcs_unable_max),
                    f(o.max_gap_s),
                ]);
                if o.kind == 2 {
                    if k == E3_DEPARTURES.len() {
                        event = o.max_gap_s;
                    }
                    gaps.push(o.max_gap_s);
                }
            }
        }
        let over = gaps.iter().filter(|g| **g > 600.0).count();
        let worst = gaps.iter().copied().fold(0.0, f64::max);
        let pass = !gaps.is_empty() && over == 0;
        passes.push(pass);
        test.push(vec![
            e3_name(c),
            n(gaps.len() as u64),
            n(over as u64),
            f(worst),
            f(event),
            pass.to_string(),
        ]);
        lines.push(format!(
            "{}: {} of {} scripted blackouts (the day-9.19 event and one per region over the seeds) had a gap over 10 minutes; the longest {} min (day-9.19 event: {} min){}",
            e3_name(c),
            over,
            gaps.len(),
            r(worst / 60.0, 1),
            r(event / 60.0, 1),
            if pass { " — **no gap over 10 minutes**" } else { "" }
        ));
    }
    let mut security = Table::new(
        "E3_security.csv",
        &[
            "attacker_share",
            "p_at_least_21_of_40",
            "p_at_least_27_of_40",
            "ratio",
        ],
    );
    for s in [0.10, 0.25, 0.33] {
        let (p21, p27) = (binomial_tail(40, 21, s), binomial_tail(40, 27, s));
        security.push(vec![f(s), f(p21), f(p27), f(div(p21, p27))]);
    }
    out.tables.push(liveness);
    out.tables.push(outages);
    out.tables.push(test);
    out.tables.push(security);
    // E4's setting: the two-thirds threshold if it passes the blackout test (without failover
    // first), otherwise the protocol rule.
    let find = |s: EntropySigners, fo: Option<u32>| {
        configs
            .iter()
            .position(|c| *c == (s, fo))
            .is_some_and(|i| passes[i])
    };
    let setting = if find(EntropySigners::ThresholdTwoThirds, None) {
        E4Setting {
            signers: EntropySigners::ThresholdTwoThirds,
            failover: None,
            reason: "threshold entropy (t = ⌈2n/3⌉) passed E3's blackout test without failover"
                .to_string(),
        }
    } else if find(EntropySigners::ThresholdTwoThirds, Some(2)) {
        E4Setting {
            signers: EntropySigners::ThresholdTwoThirds,
            failover: Some(2),
            reason: "threshold entropy (t = ⌈2n/3⌉) passed E3's blackout test with failover"
                .to_string(),
        }
    } else {
        E4Setting {
            signers: EntropySigners::AllOnlineMembers,
            failover: None,
            reason: "threshold entropy did not pass E3's blackout test, so E4 keeps the all-online-members rule".to_string(),
        }
    };
    let p_candidates: Vec<String> = configs
        .iter()
        .enumerate()
        .filter(|(i, c)| passes[*i] && c.0 != EntropySigners::AllOnlineMembers)
        .map(|(_, c)| e3_name(c))
        .collect();
    out.summary = format!(
        "## E3 — liveness: threshold entropy and failover\n\nEvery configuration (entropy rule × failover after 2 rounds below quorum) on C7's three departure profiles, on the day-9.19 event replayed as a regional blackout of as_east (10.88 h, seed 0), and on a scripted regional blackout of the same length in every region over {} seeds (`E3_liveness.csv`, `E3_outages.csv` with each outage's miners that lost power and miners that lost witnessing while powered, `E3_blackout_test.csv`). The test fixed in advance: no gap between final-chain blocks longer than 10 minutes during any blackout. The security cost of the 21-of-40 threshold is in `E3_security.csv`.\n\n{}\n\n{}\n\nE4 then runs with: {}.\n",
        e3_seeds(ctx),
        lines
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n"),
        if p_candidates.is_empty() {
            "No threshold configuration removes the global stall: threshold entropy is not marked [P].".to_string()
        } else {
            format!(
                "**[P] candidate for SPEC v0.7:** {} (no gap over 10 minutes in any blackout).",
                p_candidates.join("; ")
            )
        },
        setting.reason
    );
    (out, setting)
}

// ---------------------------------------------------------------------- E4

const POINT_NS: [f64; 3] = [3_600.0, 21_600.0, 86_400.0];

fn outcome_name(o: OfflinePointsOutcome) -> &'static str {
    match o {
        OfflinePointsOutcome::SeatLoss => "seat_loss",
        OfflinePointsOutcome::Blacklist => "blacklist",
    }
}

/// E4: the C5 setup (every region, load-shedding included) for 365 days, n × outcome, under
/// "unreachable is not refusal" and E3's entropy setting.
pub(crate) fn e4_jobs(ctx: &Ctx<'_>, setting: &E4Setting) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    let pen = &ctx.cfg.penalties;
    for after_s in POINT_NS {
        for outcome in [
            OfflinePointsOutcome::SeatLoss,
            OfflinePointsOutcome::Blacklist,
        ] {
            let mut j = realistic(
                ctx,
                &format!("E4-n{}h-{}", after_s / 3_600.0, outcome_name(outcome)),
                E4,
                0,
                ctx.pick(10_000, 300),
                ctx.pick(290_000, 2_900),
                ctx.days(365.0, 1.0),
            );
            j.offline_points = Some(OfflinePointsSpec {
                after_s,
                steps: pen.offline_points_steps.value.clone(),
                cap: pen.offline_points_cap.value,
                reset_s: pen.offline_points_reset_s.value,
                outcome,
                seat_cooldown_s: pen.offline_points_seat_cooldown_s.value,
            });
            j.unreachable = gb_config::protocol::UnreachableRule::UnreachableIsNotRefusal;
            j.entropy_signers = setting.signers;
            j.failover_after_rounds = setting.failover;
            jobs.push(j);
        }
    }
    jobs
}

const CAUSES: [&str; 13] = [
    "night shutdown",
    "other announced shutdown",
    "crash",
    "network glitch",
    "long absence",
    "departure",
    "feeder fault",
    "provider outage",
    "regional blackout",
    "scripted",
    "load-shedding",
    "scripted regional blackout",
    "partition cut",
];

pub(crate) fn e4_analyse(ctx: &Ctx<'_>, results: &[JobResult], setting: &E4Setting) -> SectionOut {
    let mut out = SectionOut::default();
    let regions = region_names(ctx);
    let devices = ["always_on", "desktop", "laptop"];
    let year = ctx.cfg.model.seconds_per_year as f64;
    let mut table = Table::new(
        "E4_points_by_region_device.csv",
        &[
            "n_hours",
            "outcome",
            "region",
            "device",
            "ups_share",
            "ups_runtime_h",
            "witness_id_years",
            "point_awards",
            "reached_cap",
            "reached_cap_by_load_shedding_only",
            "seats_lost",
            "seats_lost_per_id_year",
            "ids_blacklisted",
            "ids_lost_per_id_year",
        ],
    );
    let mut regional = Table::new(
        "E4_h7_by_region.csv",
        &[
            "n_hours",
            "outcome",
            "region",
            "ups_share",
            "witness_id_years",
            "ids_lost_per_id_year",
            "seats_lost_per_id_year",
            "h7_at_most_1_percent",
        ],
    );
    let mut causes = Table::new(
        "E4_points_by_cause.csv",
        &["n_hours", "outcome", "cause", "awards"],
    );
    let mut lines = Vec::new();
    let mut i = 0;
    for after_s in POINT_NS {
        for outcome in [
            OfflinePointsOutcome::SeatLoss,
            OfflinePointsOutcome::Blacklist,
        ] {
            let Some(res) = results.get(i) else { break };
            i += 1;
            let nh = after_s / 3_600.0;
            let mut worst_region = (String::new(), 0.0f64);
            let mut fails = Vec::new();
            for (rg, name) in regions.iter().enumerate() {
                let (ups, runtime) = backup_power(ctx, rg);
                let mut region_years = 0.0;
                let (mut region_lost, mut region_seats) = (0u64, 0u64);
                for (d, dev) in devices.iter().enumerate() {
                    let at =
                        |v: &Vec<Vec<u64>>| v.get(rg).and_then(|x| x.get(d)).copied().unwrap_or(0);
                    let years = res
                        .witness_id_s
                        .get(rg)
                        .and_then(|x| x.get(d))
                        .copied()
                        .unwrap_or(0.0)
                        / year;
                    region_years += years;
                    region_lost += at(&res.ids_blacklisted);
                    region_seats += at(&res.seats_lost);
                    table.push(vec![
                        f(nh),
                        outcome_name(outcome).to_string(),
                        name.clone(),
                        dev.to_string(),
                        f(ups),
                        f(runtime),
                        f(years),
                        n(at(&res.point_awards)),
                        n(at(&res.points_capped)),
                        n(at(&res.points_capped_loadshed_only)),
                        n(at(&res.seats_lost)),
                        f(div(at(&res.seats_lost) as f64, years)),
                        n(at(&res.ids_blacklisted)),
                        f(div(at(&res.ids_blacklisted) as f64, years)),
                    ]);
                }
                let lost_rate = div(region_lost as f64, region_years);
                let seat_rate = div(region_seats as f64, region_years);
                let h7 = lost_rate <= 0.01;
                if !h7 {
                    fails.push(name.clone());
                }
                if lost_rate.max(seat_rate) > worst_region.1 {
                    worst_region = (name.clone(), lost_rate.max(seat_rate));
                }
                regional.push(vec![
                    f(nh),
                    outcome_name(outcome).to_string(),
                    name.clone(),
                    f(ups),
                    f(region_years),
                    f(lost_rate),
                    f(seat_rate),
                    h7.to_string(),
                ]);
            }
            for (k, awards) in res.point_awards_by_cause.iter().enumerate() {
                causes.push(vec![
                    f(nh),
                    outcome_name(outcome).to_string(),
                    CAUSES.get(k).copied().unwrap_or("other").to_string(),
                    n(*awards),
                ]);
            }
            let capped: u64 = res.points_capped.iter().flatten().sum();
            let shed: u64 = res.points_capped_loadshed_only.iter().flatten().sum();
            lines.push(format!(
                "n = {nh} h, {}: {} awards, {} members reached 20 points ({} by load-shedding alone), {} seats lost, {} IDs blacklisted; highest yearly loss rate {} % ({}){}",
                outcome_name(outcome),
                res.point_awards.iter().flatten().sum::<u64>(),
                capped,
                shed,
                res.seats_lost.iter().flatten().sum::<u64>(),
                res.ids_blacklisted.iter().flatten().sum::<u64>(),
                r(100.0 * worst_region.1, 2),
                worst_region.0,
                if outcome == OfflinePointsOutcome::Blacklist {
                    if fails.is_empty() {
                        "; H7 (≤ 1 % of honest IDs a year, per region) holds in every region".to_string()
                    } else {
                        format!("; **H7 fails** in {}", fails.join(", "))
                    }
                } else {
                    String::new()
                }
            ));
        }
    }
    out.tables.push(table);
    out.tables.push(regional);
    out.tables.push(causes);
    out.summary = format!(
        "## E4 — offline points ladder\n\nThe C5 setup (10,000 miner slots, 290,000 IDs, every region with load-shedding) for 365 days under \"unreachable is not refusal\"; {}. A member earns points for every offline period of at least n, whatever its cause (1, 1, 2, 3, 4, 5, then 5 again; cap 20; reset 12 months after the latest point), recorded by a two-thirds KWC decision (`E4_points_by_region_device.csv`, `E4_h7_by_region.csv`, `E4_points_by_cause.csv`; each region's UPS share is shown):\n\n{}\n",
        setting.reason,
        lines
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    out
}
