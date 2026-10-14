//! The M3a experiments: job lists, a deterministic parallel runner, tables, charts and the
//! validation checks (ARCHITECTURE §4).
//!
//! Each section builds its jobs from the configuration, [`run_jobs`] runs them on scoped
//! threads (each job single-threaded and seeded, results stored by job index, so the thread
//! count never changes an output), and the section's analysis turns the results into tables,
//! chart data, `validation.csv` checks and a part of `SUMMARY.md`. The V0 gate runs first;
//! if it fails, the sections that rely on the stand-in or fast modes are not run.

mod c;
mod e;
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
    /// How runs report progress and keep finished results.
    pub control: RunControl,
}

/// How [`run_jobs`] reports progress and keeps finished results. Neither changes a result.
#[derive(Debug, Clone, Default)]
pub struct RunControl {
    /// Print a line to standard error as each job finishes, with an estimate of the time left.
    pub progress: bool,
    /// Directory where each finished job's result is saved with its specification. A later
    /// run of the same specification takes the saved result instead of running the job, so an
    /// interrupted suite can resume.
    pub checkpoint: Option<std::path::PathBuf>,
}

/// A saved job result (checkpoint file).
#[derive(serde::Serialize, serde::Deserialize)]
struct Checkpoint {
    spec: serde_json::Value,
    wall_s: f64,
    result: JobResult,
}

/// Rough relative cost of a job, to start the longest first and to estimate the time left.
fn job_cost(job: &JobSpec) -> f64 {
    let crypto = match job.crypto {
        gb_config::sim::CryptoMode::Real => 2_000.0,
        gb_config::sim::CryptoMode::StandIn => 20.0,
        gb_config::sim::CryptoMode::Fast => 1.0,
    };
    job.days * (f64::from(job.miners) + job.registered_ids as f64 / 3.0 + 100.0) * crypto
}

/// The saved result of `job` in `dir`, if one exists for exactly this specification.
fn load_checkpoint(dir: &std::path::Path, file: &str, job: &JobSpec) -> Option<JobResult> {
    let text = std::fs::read_to_string(dir.join(file)).ok()?;
    let saved: Checkpoint = serde_json::from_str(&text).ok()?;
    if saved.spec != serde_json::to_value(job).ok()? {
        return None;
    }
    let mut result = saved.result;
    result.wall_s = saved.wall_s;
    Some(result)
}

/// Saves `result` for `job` in `dir` (written to a temporary file, then renamed).
fn save_checkpoint(dir: &std::path::Path, file: &str, job: &JobSpec, result: &JobResult) {
    let Ok(spec) = serde_json::to_value(job) else {
        return;
    };
    let saved = Checkpoint {
        spec,
        wall_s: result.wall_s,
        result: result.clone(),
    };
    let Ok(text) = serde_json::to_string(&saved) else {
        return;
    };
    let tmp = dir.join(format!("{file}.tmp"));
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, dir.join(file));
    }
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

/// Runs jobs on `threads` workers; results come back in job order. `pool` names this set of
/// jobs in progress lines and checkpoint files.
pub fn run_jobs(
    config: &Config,
    jobs: &[JobSpec],
    threads: usize,
    control: &RunControl,
    pool: &str,
) -> Result<Vec<JobResult>, SimError> {
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<Result<JobResult, SimError>>>> =
        jobs.iter().map(|_| Mutex::new(None)).collect();
    // Longest jobs first, so threads finish together; the order does not affect results.
    let mut order: Vec<usize> = (0..jobs.len()).collect();
    order.sort_by(|a, b| {
        job_cost(&jobs[*b])
            .total_cmp(&job_cost(&jobs[*a]))
            .then(a.cmp(b))
    });
    if let Some(dir) = &control.checkpoint {
        std::fs::create_dir_all(dir).map_err(|e| SimError::Setup(e.to_string()))?;
    }
    let total_cost: f64 = jobs.iter().map(job_cost).sum();
    let done = Mutex::new((0usize, 0.0f64));
    let started = std::time::Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1).min(jobs.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= order.len() {
                        break;
                    }
                    let j = order[i];
                    let file = format!("{pool}-{j:05}.json");
                    let saved = control
                        .checkpoint
                        .as_deref()
                        .and_then(|dir| load_checkpoint(dir, &file, &jobs[j]));
                    let resumed = saved.is_some();
                    let result = match saved {
                        Some(result) => Ok(result),
                        None => {
                            let result = run_job(&jobs[j], config);
                            if let (Some(dir), Ok(r)) = (&control.checkpoint, &result) {
                                save_checkpoint(dir, &file, &jobs[j], r);
                            }
                            result
                        }
                    };
                    if control.progress {
                        if let Ok(mut d) = done.lock() {
                            d.0 += 1;
                            d.1 += job_cost(&jobs[j]);
                            let elapsed = started.elapsed().as_secs_f64();
                            let left = if d.1 > 0.0 {
                                elapsed * (total_cost - d.1) / d.1
                            } else {
                                0.0
                            };
                            eprintln!(
                                "[{pool} {}/{}] {} {}; elapsed {:.0} min, about {:.0} min left",
                                d.0,
                                jobs.len(),
                                jobs[j].id,
                                if resumed {
                                    "taken from its checkpoint".to_string()
                                } else {
                                    match &result {
                                        Ok(r) => format!("{:.0} s", r.wall_s),
                                        Err(e) => format!("failed: {e}"),
                                    }
                                },
                                elapsed / 60.0,
                                left / 60.0
                            );
                        }
                    }
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

/// E4 runs at once at most (memory; results do not depend on the thread count).
const E4_THREADS: usize = 2;

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
    let mut v0_jobs = run_section(&ctx, options, v0_specs.clone(), &mut out)?;
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
    // The M3a.1 experiments, stage 1 (E1 needs E2's hopping results; E4 needs E3's verdict).
    let run_e1 = wanted("E1");
    let run_e2 = run_e1 || wanted("E2");
    let run_e4 = wanted("E4");
    let run_e3 = run_e4 || wanted("E3");
    let mut stage1 = |run: bool, list: fn(&Ctx<'_>) -> Vec<JobSpec>| {
        run.then(|| {
            let jobs = list(&ctx);
            let range = (all.len(), all.len() + jobs.len());
            all.extend(jobs);
            range
        })
    };
    let e2_range = stage1(run_e2, e::e2_jobs);
    let e1_range = stage1(run_e1, e::e1_stage1_jobs);
    let e3_range = stage1(run_e3, e::e3_jobs);
    let results = run_jobs(config, &all, options.threads, &options.control, "main")?;
    for (job, result) in all.iter().zip(&results) {
        out.perf.push(perf_row(job, result));
    }
    for ((_, _, analyse), range) in sections.iter().zip(ranges) {
        if let Some((a, b)) = range {
            absorb(&mut out, analyse(&ctx, &results[a..b]));
        }
    }
    let (e2_out, h5) = match e2_range {
        Some((a, b)) => {
            let (o, h) = e::e2_analyse(&ctx, &results[a..b]);
            (Some(o), Some(h))
        }
        None => (None, None),
    };
    let (e3_out, setting) = match e3_range {
        Some((a, b)) => {
            let (o, s) = e::e3_analyse(&ctx, &results[a..b]);
            (Some(o), Some(s))
        }
        None => (None, None),
    };
    // Stage 2: E1's criterion-4 reruns and E4.
    let e1_reruns = match (e1_range, &h5) {
        (Some((a, b)), Some(h)) => e::e1_stage2_jobs(&ctx, &results[a..b], h),
        _ => Vec::new(),
    };
    let e4_list = match (&setting, run_e4) {
        (Some(s), true) => e::e4_jobs(&ctx, s),
        _ => Vec::new(),
    };
    let reruns = e1_reruns.len();
    let mut stage2_results = if e1_reruns.is_empty() {
        Vec::new()
    } else {
        run_jobs(
            config,
            &e1_reruns,
            options.threads,
            &options.control,
            "stage2",
        )?
    };
    // A year-long E4 run holds about a million blocks, rounds and IDs, so fewer run at once.
    if !e4_list.is_empty() {
        stage2_results.extend(run_jobs(
            config,
            &e4_list,
            options.threads.min(E4_THREADS),
            &options.control,
            "e4",
        )?);
    }
    let stage2: Vec<JobSpec> = e1_reruns.into_iter().chain(e4_list).collect();
    for (job, result) in stage2.iter().zip(&stage2_results) {
        out.perf.push(perf_row(job, result));
    }
    if let (Some((a, b)), Some(h)) = (e1_range, &h5) {
        absorb(
            &mut out,
            e::e1_analyse(&ctx, &results[a..b], h, &stage2_results[..reruns]),
        );
    }
    if let Some(o) = e2_out {
        absorb(&mut out, o);
    }
    if let Some(o) = e3_out {
        absorb(&mut out, o);
    }
    if let (true, Some(s)) = (run_e4, &setting) {
        absorb(&mut out, e::e4_analyse(&ctx, &stage2_results[reruns..], s));
    }
    let specs: Vec<JobSpec> = v0_specs.into_iter().chain(all).chain(stage2).collect();
    let mut runs: Vec<JobResult> = v0_jobs
        .into_iter()
        .chain(results)
        .chain(stage2_results)
        .collect();
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
/// "; stopped by a fork storm on day …" for the runs that a fork storm stopped (SIM-29), for
/// summary lines; empty when every run went to its end.
pub(crate) fn storm_note(runs: &[&JobResult]) -> String {
    let days: Vec<String> = runs
        .iter()
        .filter_map(|run| run.fork_storm_s.map(|s| r(s / 86_400.0, 2)))
        .collect();
    match days.len() {
        0 => String::new(),
        1 => format!("; stopped by a fork storm on day {} (SIM-29)", days[0]),
        _ => format!(
            "; stopped by fork storms on days {} (SIM-29)",
            days.join(", ")
        ),
    }
}

/// How a check counts in the simulator's `validation.csv` (architect, 2026-10-14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// Within tolerance of its reference.
    Pass,
    /// Outside tolerance: a failure.
    Fail,
    /// Kept and reported, but not a failure either way, because its reference does not model
    /// what the simulator does (ASSUMPTIONS SIM-31).
    ReferenceLimited,
}

impl CheckStatus {
    /// The status as written in `validation.csv`.
    pub fn name(self) -> &'static str {
        match self {
            CheckStatus::Pass => "pass",
            CheckStatus::Fail => "fail",
            CheckStatus::ReferenceLimited => "reference-limited",
        }
    }
}

/// Checks whose references are limited (architect, 2026-10-14): M1 section D's old-design
/// restart advantage, which models neither one-header sit-outs nor start-rule catch-up (V2),
/// and M1 E1's first-order hopping gain, which does not hold for hops of four times the
/// network (V3 Variant A at m = 4). No new references are used.
pub fn reference_limited(check: &str) -> bool {
    check.starts_with("V2-old-")
        || (check.starts_with("V3-A-")
            && (check.ends_with("-m4") || check.ends_with("-m4-recovery")))
}

/// The status of a check.
pub fn check_status(check: &Check) -> CheckStatus {
    if reference_limited(&check.check) {
        CheckStatus::ReferenceLimited
    } else if check.passed {
        CheckStatus::Pass
    } else {
        CheckStatus::Fail
    }
}

/// The simulated day a fork storm stopped the run, or an empty cell (SIM-29).
pub(crate) fn storm_day(res: &JobResult) -> String {
    res.fork_storm_s
        .map(|s| f(s / 86_400.0))
        .unwrap_or_default()
}

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
            "max_unsettled",
            "max_fork_width",
            "fork_storm_day",
            "partition_cuts",
            "voided_ids",
            "quick_readmits",
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
            n(res.max_unsettled),
            n(res.max_fork_width),
            storm_day(res),
            n(res.partition_cuts),
            n(res.voided_ids),
            n(res.quick_readmits),
            n(res.events),
        ]);
    }
    let deep: Vec<String> = all
        .iter()
        .filter(|r| r.deep_reorgs > 0)
        .map(|r| format!("`{}` ({}, deepest {})", r.id, r.deep_reorgs, r.max_reorg))
        .collect();
    let storms: Vec<String> = all
        .iter()
        .filter_map(|run| {
            run.fork_storm_s
                .map(|s| format!("`{}` (day {})", run.id, r(s / 86_400.0, 2)))
        })
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
    let widest = all.iter().map(|r| r.max_fork_width).max().unwrap_or(0);
    let storm_text = if storms.is_empty() {
        format!("No run had a fork storm: at most {widest} unsettled blocks ever shared a height.")
    } else {
        format!(
            "**Fork storms stopped {} runs early**, when more blocks than `sim.world.fork_storm_unsettled` were unsettled at once (SIM-29): {}. Their tables cover the time before (`ALL_runs.csv`, column `fork_storm_day`).",
            storms.len(),
            storms.join(", ")
        )
    };
    SectionOut {
        summary: format!(
            "## Invariants of every run\n\n**{passed} of {} hold** across all {} runs: recounted caches and counters, self-validation of every broadcast block, and no reorg below a confirmed ID (`validation.csv`, section ALL; every run in `ALL_runs.csv`). {deep_text}\n\n{storm_text}\n",
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
    options: &SuiteOptions,
    jobs: Vec<JobSpec>,
    out: &mut SuiteOutput,
) -> Result<Vec<JobResult>, SimError> {
    let results = run_jobs(ctx.cfg, &jobs, options.threads, &options.control, "V0")?;
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
    fn checkpoints_round_trip_and_are_reused_only_for_the_same_specification() {
        let cfg = Config::default();
        let dir = std::env::temp_dir().join(format!("gb-sim-checkpoint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut job = JobSpec::from_config(&cfg, "checkpoint", "T", 3).idealised();
        job.miners = 50;
        job.registered_ids = 400;
        job.days = 0.05;
        let control = RunControl {
            progress: false,
            checkpoint: Some(dir.clone()),
        };
        let run = |j: &JobSpec| run_jobs(&cfg, std::slice::from_ref(j), 1, &control, "t").unwrap();
        let mut first = run(&job).remove(0);
        // The saved result equals the run's, every float included.
        let file = dir.join("t-00000.json");
        let text = std::fs::read_to_string(&file).unwrap();
        let mut saved: Checkpoint = serde_json::from_str(&text).unwrap();
        first.wall_s = 0.0;
        assert_eq!(saved.result, first);
        // A saved result is taken instead of running the job again …
        saved.result.blocks = 999_999;
        std::fs::write(&file, serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(run(&job)[0].blocks, 999_999);
        // … but only for the same specification.
        let mut other = job.clone();
        other.seed += 1;
        assert_ne!(run(&other)[0].blocks, 999_999);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Runs one job of the full profile and prints its digest: the SHA-256 of its result as
    /// JSON, without the event count (an implementation detail). Used to profile a job and to
    /// check that a performance change leaves results byte-identical.
    #[test]
    #[ignore = "harness: GB_JOB_SECTION (V0, V1, … A6), GB_JOB_INDEX, GB_JOB_DAYS, GB_STORM_UNSETTLED, GB_JOB_CONFIG (a file merged over the defaults), GB_JOB_JSON (write the result here), GB_TRACK_OUTAGES=1"]
    fn run_one_full_job() {
        let mut cfg = match std::env::var("GB_JOB_CONFIG") {
            Ok(path) => Config::from_file(std::path::Path::new(&path)).unwrap(),
            Err(_) => Config::default(),
        };
        if let Ok(limit) = std::env::var("GB_STORM_UNSETTLED") {
            cfg.sim.world.fork_storm_unsettled = limit.parse().unwrap();
        }
        let ctx = Ctx {
            cfg: &cfg,
            quick: false,
            seed: cfg.run.seed,
            days_scale: 1.0,
            replicate_scale: 1.0,
            k: cfg.run.monte_carlo.tolerance_standard_errors,
        };
        let var = |name: &str| std::env::var(name).unwrap_or_default();
        let jobs = match var("GB_JOB_SECTION").as_str() {
            "V1" => v::v1_jobs(&ctx),
            "V2" => v::v2_jobs(&ctx),
            "V3" => v::v3_jobs(&ctx),
            "C1" => c::c1_jobs(&ctx),
            "C2" => c::c2_jobs(&ctx),
            "C3" => c::c3_jobs(&ctx),
            "C4" => c::c4_jobs(&ctx),
            "C5" => c::c5_jobs(&ctx),
            "C6" => c::c6_jobs(&ctx),
            "C7" => c::c7_jobs(&ctx),
            "A6" => c::a6_jobs(&ctx),
            "E1" => e::e1_stage1_jobs(&ctx),
            "E2" => e::e2_jobs(&ctx),
            "E3" => e::e3_jobs(&ctx),
            "E4" => e::e4_jobs(
                &ctx,
                &e::E4Setting {
                    signers: gb_config::protocol::EntropySigners::ThresholdTwoThirds,
                    failover: None,
                    reason: String::new(),
                },
            ),
            _ => v::v0_jobs(&ctx),
        };
        if var("GB_JOB_INDEX") == "list" {
            for (i, j) in jobs.iter().enumerate() {
                println!(
                    "LIST {i} {} days {} miners {} ids {} crypto {:?} idealised {}",
                    j.id, j.days, j.miners, j.registered_ids, j.crypto, j.idealised
                );
            }
            return;
        }
        let mut job = jobs[var("GB_JOB_INDEX").parse::<usize>().unwrap_or(0)].clone();
        if let Ok(days) = var("GB_JOB_DAYS").parse::<f64>() {
            job.days = days;
        }
        if var("GB_TRACK_OUTAGES") == "1" {
            job.track_outages = true;
        }
        let started = std::time::Instant::now();
        let mut res = run_job(&job, &cfg).unwrap();
        let wall = started.elapsed().as_secs_f64();
        let events = res.events;
        res.events = 0;
        let json = serde_json::to_string(&res).unwrap();
        if let Ok(path) = std::env::var("GB_JOB_JSON") {
            std::fs::write(path, &json).unwrap();
        }
        let digest = gb_runlog::sha256_hex(json.as_bytes());
        println!(
            "JOB {} days {} miners {} ids {} events {events} blocks {} wall {wall:.1}s digest {digest}",
            job.id, job.days, job.miners, job.registered_ids, res.blocks
        );
        println!(
            "STORM {} max_unsettled {} max_fork_width {} fork_storm_s {:?}",
            job.id, res.max_unsettled, res.max_fork_width, res.fork_storm_s
        );
        for o in &res.outages {
            println!(
                "OUTAGE kind {} region {} day {:.3} hours {:.3} households {} miners_lost_power {} witnessing_lost_max {} kwcs_unable_max {} max_gap_s {:.0}",
                o.kind,
                o.region,
                o.start_s / 86_400.0,
                o.duration_s / 3_600.0,
                o.households,
                o.miners_lost_power,
                o.witnessing_lost_max,
                o.kwcs_unable_max,
                o.max_gap_s
            );
        }
    }

    #[test]
    fn v2_old_and_v3_large_hop_checks_are_reference_limited() {
        for id in [
            "V2-old-w1299",
            "V2-old-w20785",
            "V3-A-K144-clamp4-m4",
            "V3-A-K2016-clampnone-m4-recovery",
        ] {
            assert!(reference_limited(id), "{id}");
        }
        for id in [
            "V2-restart-cost",
            "V2-current-control",
            "V3-A-K144-clamp4-m1",
            "V3-A-K144-clamp4-m0.5-recovery",
            "V3-B-m4",
            "C2-idealised-orphans",
        ] {
            assert!(!reference_limited(id), "{id}");
        }
        let mut c = Check::exact("V2", "V2-old-w1299", "test", 1.0, 2.0, false);
        assert!(!c.passed);
        assert_eq!(check_status(&c), CheckStatus::ReferenceLimited);
        c.check = "C2-idealised-orphans".to_string();
        assert_eq!(check_status(&c), CheckStatus::Fail);
        c.passed = true;
        assert_eq!(check_status(&c), CheckStatus::Pass);
    }

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
