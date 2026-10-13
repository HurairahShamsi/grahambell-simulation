//! The M3a experiments: job lists, a deterministic parallel runner, tables, charts and the
//! validation checks (ARCHITECTURE §4).
//!
//! Each section builds its jobs from the configuration, [`run_jobs`] runs them on scoped
//! threads (each job single-threaded and seeded, results stored by job index, so the thread
//! count never changes an output), and the section's analysis turns the results into tables,
//! chart data, `validation.csv` checks and a part of `SUMMARY.md`. The V0 gate runs first;
//! if it fails, the sections that rely on the stand-in or fast modes are not run.

mod c;
mod v;

use crate::job::{JobResult, JobSpec};
use crate::{SimError, run_job};
use gb_analytic::validation::Check;
use gb_config::Config;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How large the experiments are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// The M3a sizes.
    Full,
    /// Every section at a tiny size (tests and CI).
    Quick,
}

/// Options of one suite run.
#[derive(Debug, Clone)]
pub struct SuiteOptions {
    /// Experiment sizes.
    pub profile: Profile,
    /// Worker threads.
    pub threads: usize,
    /// Sections to run (empty: all). V0 always runs, because the others rely on its gate.
    pub only: Vec<String>,
}

/// A table written as CSV.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// File name.
    pub name: String,
    /// Column names.
    pub header: Vec<String>,
    /// Rows of formatted values.
    pub rows: Vec<Vec<String>>,
}

impl Table {
    fn new(name: &str, header: &[&str]) -> Table {
        Table {
            name: name.to_string(),
            header: header.iter().map(|h| (*h).to_string()).collect(),
            rows: Vec::new(),
        }
    }

    fn push(&mut self, row: Vec<String>) {
        self.rows.push(row);
    }
}

/// One series of a chart.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSeries {
    /// Legend label.
    pub label: String,
    /// Points (x, y).
    pub points: Vec<(f64, f64)>,
}

/// One panel of a chart.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartPanel {
    /// Panel title.
    pub title: String,
    /// x-axis label.
    pub x_label: String,
    /// y-axis label.
    pub y_label: String,
    /// Logarithmic x axis.
    pub log_x: bool,
    /// Series (at most five).
    pub series: Vec<ChartSeries>,
    /// Horizontal reference lines (label, value).
    pub references: Vec<(String, f64)>,
}

/// A chart the CLI draws from table data.
#[derive(Debug, Clone, PartialEq)]
pub struct Chart {
    /// File name (PNG).
    pub file: String,
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: String,
    /// Panels, side by side.
    pub panels: Vec<ChartPanel>,
}

/// Performance of one job (machine-dependent; written under `perf/`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PerfRow {
    /// Job.
    pub job: String,
    /// Section.
    pub section: String,
    /// Events processed.
    pub events: u64,
    /// Simulated days.
    pub simulated_days: f64,
    /// Wall time, seconds.
    pub wall_s: f64,
    /// Events per second.
    pub events_per_s: f64,
}

/// Everything a suite run produces.
#[derive(Debug, Clone, Default)]
pub struct SuiteOutput {
    /// Tables.
    pub tables: Vec<Table>,
    /// Validation checks.
    pub checks: Vec<Check>,
    /// Markdown sections of SUMMARY.md.
    pub summary: Vec<String>,
    /// Charts.
    pub charts: Vec<Chart>,
    /// Performance rows.
    pub perf: Vec<PerfRow>,
    /// Whether the V0 gate passed.
    pub gate_passed: bool,
    /// Sampled event traces by run (empty unless `sim.trace_sample > 0`).
    pub traces: Vec<(String, Vec<crate::job::TraceRow>)>,
}

/// What a section's analysis returns.
#[derive(Debug, Default)]
pub(crate) struct SectionOut {
    pub tables: Vec<Table>,
    pub checks: Vec<Check>,
    pub summary: String,
    pub charts: Vec<Chart>,
}

/// Shared settings of a suite run.
#[derive(Debug, Clone)]
pub(crate) struct Ctx<'a> {
    pub cfg: &'a Config,
    pub quick: bool,
    pub seed: u64,
    pub days_scale: f64,
    pub replicate_scale: f64,
    pub k: f64,
}

impl Ctx<'_> {
    /// Simulated days scaled (with a floor so quick runs still produce blocks).
    pub fn days(&self, full: f64, quick: f64) -> f64 {
        if self.quick {
            quick
        } else {
            (full * self.days_scale).max(0.05)
        }
    }

    /// Replicates scaled, at least one.
    pub fn reps(&self, full: u64, quick: u64) -> u64 {
        if self.quick {
            quick
        } else {
            ((full as f64 * self.replicate_scale).round() as u64).max(1)
        }
    }

    /// Picks the full or quick value.
    pub fn pick<T>(&self, full: T, quick: T) -> T {
        if self.quick { quick } else { full }
    }

    /// A job from the configuration with the suite's seed for replicate `rep`.
    pub fn job(&self, id: &str, section: &str, rep: u64) -> JobSpec {
        JobSpec::from_config(self.cfg, id, section, self.seed.wrapping_add(rep))
    }
}

/// Runs jobs on `threads` workers; results come back in job order.
pub fn run_jobs(
    config: &Config,
    jobs: &[JobSpec],
    threads: usize,
) -> Result<Vec<JobResult>, SimError> {
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<Result<JobResult, SimError>>>> =
        jobs.iter().map(|_| Mutex::new(None)).collect();
    // Longest jobs first, so threads finish together; the order does not affect results.
    let mut order: Vec<usize> = (0..jobs.len()).collect();
    order.sort_by(|a, b| {
        let cost = |j: &JobSpec| j.days * (f64::from(j.miners) + j.registered_ids as f64 / 3.0);
        cost(&jobs[*b]).total_cmp(&cost(&jobs[*a])).then(a.cmp(b))
    });
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1).min(jobs.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= order.len() {
                        break;
                    }
                    let j = order[i];
                    let result = run_job(&jobs[j], config);
                    if let Ok(mut slot) = slots[j].lock() {
                        *slot = Some(result);
                    }
                }
            });
        }
    });
    slots
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .ok()
                .flatten()
                .unwrap_or_else(|| Err(SimError::Setup("a job did not finish".to_string())))
        })
        .collect()
}

/// Runs the M3a suite.
pub fn run_suite(config: &Config, options: &SuiteOptions) -> Result<SuiteOutput, SimError> {
    let ctx = Ctx {
        cfg: config,
        quick: options.profile == Profile::Quick,
        seed: config.run.seed,
        days_scale: config.sim.suite.days_scale,
        replicate_scale: config.sim.suite.replicate_scale,
        k: config.run.monte_carlo.tolerance_standard_errors,
    };
    let wanted = |name: &str| options.only.is_empty() || options.only.iter().any(|s| s == name);
    let mut out = SuiteOutput::default();
    // V0 first: the gate decides whether the other sections may run.
    let v0_specs = v::v0_jobs(&ctx);
    let mut v0_jobs = run_section(&ctx, options.threads, v0_specs.clone(), &mut out)?;
    let v0 = v::v0_analyse(&ctx, &v0_jobs);
    let gate_passed = v0.checks.iter().all(|c| c.passed);
    absorb(&mut out, v0);
    out.gate_passed = gate_passed;
    if wanted("V4") {
        absorb(&mut out, v::v4_analyse(&ctx));
    }
    if !gate_passed {
        out.summary.push(
            "**The V0 gate failed, so no result that relies on the stand-in or fast modes is reported.**\n"
                .to_string(),
        );
        take_traces(&mut out, &mut v0_jobs);
        return Ok(out);
    }
    type Section<'a> = (
        &'static str,
        fn(&Ctx<'_>) -> Vec<JobSpec>,
        fn(&Ctx<'_>, &[JobResult]) -> SectionOut,
    );
    let sections: Vec<Section<'_>> = vec![
        ("V1", v::v1_jobs, v::v1_analyse),
        ("V2", v::v2_jobs, v::v2_analyse),
        ("V3", v::v3_jobs, v::v3_analyse),
        ("C1", c::c1_jobs, c::c1_analyse),
        ("C2", c::c2_jobs, c::c2_analyse),
        ("C3", c::c3_jobs, c::c3_analyse),
        ("C4", c::c4_jobs, c::c4_analyse),
        ("C5", c::c5_jobs, c::c5_analyse),
        ("C6", c::c6_jobs, c::c6_analyse),
        ("C7", c::c7_jobs, c::c7_analyse),
        ("A6", c::a6_jobs, c::a6_analyse),
    ];
    // All remaining jobs run in one pool, so threads stay busy across sections.
    let mut all: Vec<JobSpec> = Vec::new();
    let mut ranges = Vec::new();
    for (name, jobs, _) in &sections {
        if !wanted(name) {
            ranges.push(None);
            continue;
        }
        let list = jobs(&ctx);
        ranges.push(Some((all.len(), all.len() + list.len())));
        all.extend(list);
    }
    let results = run_jobs(config, &all, options.threads)?;
    for (job, result) in all.iter().zip(&results) {
        out.perf.push(perf_row(job, result));
    }
    for ((_, _, analyse), range) in sections.iter().zip(ranges) {
        if let Some((a, b)) = range {
            absorb(&mut out, analyse(&ctx, &results[a..b]));
        }
    }
    let specs: Vec<JobSpec> = v0_specs.into_iter().chain(all).collect();
    let mut runs: Vec<JobResult> = v0_jobs.into_iter().chain(results).collect();
    absorb(&mut out, invariants(&specs, &runs));
    take_traces(&mut out, &mut runs);
    Ok(out)
}

/// Moves the runs' sampled traces into the output.
fn take_traces(out: &mut SuiteOutput, runs: &mut [JobResult]) {
    for run in runs {
        if !run.trace.is_empty() {
            out.traces
                .push((run.id.clone(), std::mem::take(&mut run.trace)));
        }
    }
}

/// Checks that must hold in every run of the suite: the recounts of `check_invariants`,
/// self-validation of every broadcast block, and no reorg of the network chain below a
/// confirmed ID. Also the index of every run (`ALL_runs.csv`).
fn invariants(specs: &[JobSpec], runs: &[JobResult]) -> SectionOut {
    let all: Vec<&JobResult> = runs.iter().collect();
    let mut table = Table::new(
        "ALL_runs.csv",
        &[
            "run",
            "section",
            "seed",
            "simulated_days",
            "miner_slots",
            "registered_ids",
            "crypto",
            "blocks",
            "final_chain_blocks",
            "orphans",
            "max_reorg",
            "reorgs_deeper_than_confirmation",
            "pauses_over_120s",
            "pause_seconds",
            "invariant_failures",
            "self_invalid",
            "events",
        ],
    );
    for (spec, res) in specs.iter().zip(runs) {
        table.push(vec![
            spec.id.clone(),
            spec.section.clone(),
            n(spec.seed),
            f(res.sim_s / 86_400.0),
            n(u64::from(spec.miners)),
            n(spec.registered_ids),
            format!("{:?}", spec.crypto).to_lowercase(),
            n(res.blocks),
            n(res.canonical),
            n(res.orphans),
            n(res.max_reorg),
            n(res.deep_reorgs),
            n(res.pauses),
            f(res.pause_s),
            n(res.invariant_failures),
            n(res.self_invalid),
            n(res.events),
        ]);
    }
    let deep: Vec<String> = all
        .iter()
        .filter(|r| r.deep_reorgs > 0)
        .map(|r| format!("`{}` ({}, deepest {})", r.id, r.deep_reorgs, r.max_reorg))
        .collect();
    let sum = |field: fn(&JobResult) -> u64| all.iter().map(|r| field(r)).sum::<u64>();
    let checks = vec![
        count_check(
            "ALL",
            "ALL-invariants",
            &format!(
                "cached active list, seats, WC, miner and KWC counters equal a recount, daily and at the end ({} runs)",
                all.len()
            ),
            0,
            sum(|r| r.invariant_failures),
        ),
        count_check(
            "ALL",
            "ALL-self-validation",
            &format!(
                "every broadcast block passes validate_facts at its own miner ({} runs)",
                all.len()
            ),
            0,
            sum(|r| r.self_invalid),
        ),
        count_check(
            "ALL",
            "ALL-deep-reorgs",
            &format!(
                "reorgs of the network chain below a confirmed ID ({} runs)",
                all.len()
            ),
            0,
            sum(|r| r.deep_reorgs),
        ),
    ];
    let passed = checks.iter().filter(|c| c.passed).count();
    let deep_text = if deep.is_empty() {
        "No run reorganised the network chain below a confirmed ID.".to_string()
    } else {
        format!(
            "Reorgs below a confirmed ID (a confirmed ID's block replaced) occurred in {}: {}.",
            deep.len(),
            deep.join(", ")
        )
    };
    SectionOut {
        summary: format!(
            "## Invariants of every run\n\n**{passed} of {} hold** across all {} runs: recounted caches and counters, self-validation of every broadcast block, and no reorg below a confirmed ID (`validation.csv`, section ALL; every run in `ALL_runs.csv`). {deep_text}\n",
            checks.len(),
            all.len()
        ),
        checks,
        tables: vec![table],
        ..SectionOut::default()
    }
}

fn run_section(
    ctx: &Ctx<'_>,
    threads: usize,
    jobs: Vec<JobSpec>,
    out: &mut SuiteOutput,
) -> Result<Vec<JobResult>, SimError> {
    let results = run_jobs(ctx.cfg, &jobs, threads)?;
    for (job, result) in jobs.iter().zip(&results) {
        out.perf.push(perf_row(job, result));
    }
    Ok(results)
}

fn perf_row(job: &JobSpec, r: &JobResult) -> PerfRow {
    PerfRow {
        job: job.id.clone(),
        section: job.section.clone(),
        events: r.events,
        simulated_days: r.sim_s / 86_400.0,
        wall_s: r.wall_s,
        events_per_s: if r.wall_s > 0.0 {
            r.events as f64 / r.wall_s
        } else {
            0.0
        },
    }
}

fn absorb(out: &mut SuiteOutput, s: SectionOut) {
    out.tables.extend(s.tables);
    out.checks.extend(s.checks);
    if !s.summary.is_empty() {
        out.summary.push(s.summary);
    }
    out.charts.extend(s.charts);
}

// ---------------------------------------------------------------------- helpers

/// A number in a table: shortest round-trip form, or empty for NaN.
pub(crate) fn f(x: f64) -> String {
    if x.is_nan() {
        String::new()
    } else {
        format!("{x}")
    }
}

/// A number rounded for SUMMARY.md.
pub(crate) fn r(x: f64, digits: usize) -> String {
    if x.is_nan() {
        "–".to_string()
    } else {
        format!("{x:.digits$}")
    }
}

/// An integer in a table.
pub(crate) fn n(x: u64) -> String {
    x.to_string()
}

/// Ratio with zero guarded.
pub(crate) fn div(a: f64, b: f64) -> f64 {
    if b == 0.0 { f64::NAN } else { a / b }
}

/// The 95 % interval `value ± 1.96 SE`.
pub(crate) fn ci95(value: f64, se: f64) -> (f64, f64) {
    (value - 1.96 * se, value + 1.96 * se)
}

/// Amplification of group `g` (its share of final-chain IDs ÷ its share of mining
/// slot-seconds) with a batch-means standard error over days, pooled over `results`.
pub(crate) fn amplification(results: &[&JobResult], g: usize) -> (f64, f64, u64) {
    let mut values = Vec::new();
    let (mut ids, mut all_ids, mut slot, mut all_slot) = (0.0, 0.0, 0.0, 0.0);
    for res in results {
        let days = res
            .daily_ids_by_group
            .len()
            .min(res.daily_slot_s_by_group.len());
        for d in 0..days {
            let gi = res.daily_ids_by_group[d].get(g).copied().unwrap_or(0) as f64;
            let ti: f64 = res.daily_ids_by_group[d].iter().map(|x| *x as f64).sum();
            let gs = res.daily_slot_s_by_group[d].get(g).copied().unwrap_or(0.0);
            let ts: f64 = res.daily_slot_s_by_group[d].iter().sum();
            if ti > 0.0 && ts > 0.0 && gs > 0.0 {
                values.push((gi / ti) / (gs / ts));
            }
            ids += gi;
            all_ids += ti;
            slot += gs;
            all_slot += ts;
        }
    }
    let value = div(div(ids, all_ids), div(slot, all_slot));
    let n = values.len() as f64;
    let batch = if n >= 2.0 {
        let mean = values.iter().sum::<f64>() / n;
        let var = values.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (n - 1.0);
        (var / n).sqrt()
    } else {
        0.0
    };
    // Binomial floor: the group's share of IDs as a binomial proportion.
    let share = div(ids, all_ids);
    let binomial = div(
        (share * (1.0 - share) / all_ids).sqrt(),
        div(slot, all_slot),
    );
    let se = if binomial.is_nan() {
        batch
    } else {
        batch.max(binomial)
    };
    (value, se, values.len() as u64)
}

/// A Monte Carlo check: `value ± k·SE (+ allowance)` must cover the reference.
#[allow(clippy::too_many_arguments)]
pub(crate) fn mc_check(
    section: &'static str,
    id: &str,
    description: &str,
    reference: f64,
    value: f64,
    se: f64,
    samples: u64,
    k: f64,
    allowance: f64,
) -> Check {
    let estimate = gb_analytic::mc::Estimate {
        value,
        standard_error: if se.is_nan() { 0.0 } else { se },
        samples,
    };
    Check::monte_carlo(section, id, description, reference, estimate, k, allowance)
}

/// A p-value check (p ≥ minimum).
pub(crate) fn p_check(
    section: &'static str,
    id: &str,
    description: &str,
    p: f64,
    samples: u64,
    minimum: f64,
) -> Check {
    Check::at_least(section, id, description, minimum, p, 0.0, samples)
}

/// An exact count check (`value == expected`).
pub(crate) fn count_check(
    section: &'static str,
    id: &str,
    description: &str,
    expected: u64,
    value: u64,
) -> Check {
    Check::exact(
        section,
        id,
        description,
        expected as f64,
        value as f64,
        expected == value,
    )
}

/// Mean and standard error of values.
pub(crate) fn mean_se(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    if n == 0.0 {
        return (f64::NAN, f64::NAN);
    }
    let mean = values.iter().sum::<f64>() / n;
    if n < 2.0 {
        return (mean, f64::NAN);
    }
    let var = values.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (n - 1.0);
    (mean, (var / n).sqrt())
}

/// The `q`-quantile of values (nearest rank).
pub(crate) fn quantile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    v[((q * (v.len() - 1) as f64).round() as usize).min(v.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_format_and_summarise() {
        assert_eq!(f(0.5), "0.5");
        assert_eq!(f(f64::NAN), "");
        assert_eq!(r(1.23456, 2), "1.23");
        let (m, se) = mean_se(&[1.0, 2.0, 3.0]);
        assert_eq!(m, 2.0);
        assert!((se - (1.0f64 / 3.0).sqrt()).abs() < 1e-12);
        assert_eq!(quantile(&[3.0, 1.0, 2.0], 0.5), 2.0);
    }
}
