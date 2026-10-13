//! `gb sim`: runs the M3a Tier 1 simulator suite and writes a run directory with provenance.
//!
//! Reproducible outputs (the CSV tables, `validation.csv`, the charts and `SUMMARY.md`) depend
//! only on the commit, seed and configuration; the thread count never changes them, because
//! every simulation run is single-threaded and seeded and results are stored by run index.
//! Machine-dependent performance goes to `perf/`, which reproduction checks leave out, as
//! `bench/` is left out of `gb crypto` runs. Sampled event traces (off by default) go to
//! `trace/`.

use crate::charts::{Figure, Panel, Reference, Scale, Series, draw_figure};
use crate::tables::write_csv;
use crate::{load_config, machine_description};
use anyhow::{Context, bail, ensure};
use gb_config::Config;
use gb_runlog::{
    GitInfo, RunDetails, RunIdentity, UtcTimestamp, finish_run, write_resolved_config,
};
use gb_sim::suite::{Chart, Profile, SuiteOptions, SuiteOutput, Table, run_suite};
use plotters::prelude::{RGBColor, SeriesLabelPosition};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Directory, inside a run directory, of machine-dependent performance data.
pub const PERF_DIR: &str = "perf";

/// Directory, inside a run directory, of sampled event traces.
pub const TRACE_DIR: &str = "trace";

/// Directory, inside a run directory, of the charts.
pub const CHART_DIR: &str = "charts";

/// Tables a run of every section writes, besides `validation.csv`.
pub const SIM_TABLES: &[&str] = &[
    "V0_mode_agreement.csv",
    "V1_time_to_threshold.csv",
    "V1_ties_and_interval.csv",
    "V2_restart_advantage.csv",
    "V3_difficulty_hopping.csv",
    "V4_test_vectors.csv",
    "C1_amplification.csv",
    "C1_amplification_by_region.csv",
    "C2_fork_choice.csv",
    "C3_powit_deadline.csv",
    "C3_late_by_region.csv",
    "C3_withholder.csv",
    "C4_issuance_rate.csv",
    "C5_one_chance.csv",
    "C5_honest_losses_by_region_device.csv",
    "C6_clock_skew.csv",
    "C7_quorum_liveness.csv",
    "A6_growth_and_patience.csv",
    "ALL_runs.csv",
];

/// Charts a run of every section draws, in `charts/`.
pub const SIM_CHARTS: &[&str] = &[
    "V1_time_to_threshold.png",
    "V3_hopping_gain.png",
    "C1_amplification.png",
    "C2_orphan_rate.png",
    "C3_powit_deadline.png",
    "C6_clock_skew.png",
];

/// Sections of the suite, in the order they run (V0 always runs: it gates the others).
pub const SECTIONS: &[&str] = &[
    "V0", "V1", "V2", "V3", "V4", "C1", "C2", "C3", "C4", "C5", "C6", "C7", "A6",
];

/// Colours of series that share a panel, in fixed order: the first three are the M1 charts'
/// categorical slots, the fourth was chosen for M3a. All pairs differ by at least 9 ΔE (OKLab
/// × 100) under simulated protanopia, deuteranopia and tritanopia (Machado 2009, full
/// severity) and by at least 21 ΔE in normal vision. The green has 2.7:1 contrast with the
/// surface, so every chart with it has a legend and its data is in a CSV table.
const SERIES_COLOURS: [RGBColor; 4] = [
    RGBColor(0x2a, 0x78, 0xd6),
    RGBColor(0xeb, 0x68, 0x34),
    RGBColor(0x1b, 0xaf, 0x7a),
    RGBColor(0x7e, 0x33, 0x7b),
];

/// Colour of a panel's only series (its panel title names it, so it carries no identity).
const SINGLE_SERIES: RGBColor = RGBColor(0x52, 0x51, 0x4e);

/// Options of `gb sim`.
#[derive(Debug, Clone, PartialEq)]
pub struct SimOptions {
    /// Configuration file merged over the defaults.
    pub config_path: Option<PathBuf>,
    /// Seed overriding the configuration's.
    pub seed: Option<u64>,
    /// Directory that receives one sub-directory per run.
    pub out_root: PathBuf,
    /// Allow a working tree with uncommitted changes.
    pub allow_dirty: bool,
    /// Directory whose git state is recorded.
    pub repo_dir: PathBuf,
    /// Full command line.
    pub command_line: Vec<String>,
    /// Experiment sizes.
    pub profile: Profile,
    /// Worker threads overriding `sim.threads` (not recorded in the configuration: it never
    /// changes a result).
    pub threads: Option<usize>,
    /// Sections to run (empty: all). V0 always runs.
    pub only: Vec<String>,
    /// Fraction of events to trace, overriding `sim.trace_sample`.
    pub trace: Option<f64>,
}

/// What a finished run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct SimOutcome {
    /// Run directory.
    pub run_dir: PathBuf,
    /// Validation checks run.
    pub checks: usize,
    /// Identifiers of failed checks.
    pub failed: Vec<String>,
    /// Whether the V0 gate passed.
    pub gate_passed: bool,
}

/// Runs the suite and writes every output except `run.json` into `dir`.
pub fn produce(
    dir: &Path,
    config: &Config,
    git: &GitInfo,
    suite: &SuiteOptions,
) -> anyhow::Result<SuiteOutput> {
    let out = run_suite(config, suite).map_err(|e| anyhow::anyhow!("{e}"))?;
    let config_sha256 = write_resolved_config(dir, &config.to_toml_string()?)?;
    for table in &out.tables {
        write_table(&dir.join(&table.name), table)?;
    }
    write_csv(&dir.join("validation.csv"), &out.checks)?;
    if !out.charts.is_empty() {
        let charts = dir.join(CHART_DIR);
        std::fs::create_dir_all(&charts)?;
        for chart in &out.charts {
            draw_figure(&charts.join(&chart.file), &figure(chart)?)
                .with_context(|| format!("drawing {}", chart.file))?;
        }
    }
    let summary = render_summary(config, &out, git, &config_sha256, suite);
    std::fs::write(dir.join("SUMMARY.md"), summary).context("writing SUMMARY.md")?;
    let perf = dir.join(PERF_DIR);
    std::fs::create_dir_all(&perf)?;
    write_csv(&perf.join("perf.csv"), &out.perf)?;
    std::fs::write(perf.join("PERF.md"), render_perf(&out, suite))
        .context("writing perf/PERF.md")?;
    if !out.traces.is_empty() {
        let traces = dir.join(TRACE_DIR);
        std::fs::create_dir_all(&traces)?;
        for (run, rows) in &out.traces {
            write_csv(&traces.join(format!("{run}.csv")), rows)?;
        }
    }
    Ok(out)
}

/// Runs `gb sim`.
pub fn run_sim(options: &SimOptions) -> anyhow::Result<SimOutcome> {
    let mut config = load_config(options.config_path.as_deref())?;
    if let Some(seed) = options.seed {
        config.run.seed = seed;
    }
    if let Some(fraction) = options.trace {
        ensure!(
            fraction > 0.0 && fraction <= 1.0,
            "--trace takes a fraction of events in (0, 1], got {fraction}"
        );
        config.sim.trace_sample = fraction;
    }
    for section in &options.only {
        ensure!(
            SECTIONS.contains(&section.as_str()),
            "unknown section {section}; the sections are {}",
            SECTIONS.join(", ")
        );
    }
    let git = GitInfo::read(&options.repo_dir);
    if !git.is_clean_commit() && !options.allow_dirty {
        bail!(
            "the working tree is not a clean git commit ({}); commit your changes so results can be attributed, or pass --allow-dirty (recorded in run.json)",
            git.short_label()
        );
    }
    let identity = RunIdentity {
        analysis: "sim".to_string(),
        started: UtcTimestamp::now(),
        git,
        seed: config.run.seed,
    };
    let run_dir = options.out_root.join(identity.run_id());
    ensure!(
        !run_dir.exists(),
        "run directory {} already exists",
        run_dir.display()
    );
    std::fs::create_dir_all(&run_dir).with_context(|| format!("creating {}", run_dir.display()))?;
    let suite = SuiteOptions {
        profile: options.profile,
        threads: options
            .threads
            .unwrap_or(config.sim.threads as usize)
            .max(1),
        only: options.only.clone(),
    };
    let out = produce(&run_dir, &config, &identity.git, &suite)?;
    let config_sha256 = gb_runlog::sha256_hex(config.to_toml_string()?.as_bytes());
    let details = RunDetails {
        command_line: options.command_line.clone(),
        config_path: options
            .config_path
            .as_ref()
            .map(|p| p.display().to_string()),
        config_sha256,
        crate_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    finish_run(&run_dir, &identity, &details)?;
    Ok(SimOutcome {
        run_dir,
        checks: out.checks.len(),
        failed: out
            .checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| c.check.clone())
            .collect(),
        gate_passed: out.gate_passed,
    })
}

/// Writes a suite table as CSV.
fn write_table(path: &Path, table: &Table) -> anyhow::Result<()> {
    let mut writer =
        csv::Writer::from_path(path).with_context(|| format!("creating {}", path.display()))?;
    writer.write_record(&table.header)?;
    for row in &table.rows {
        writer.write_record(row)?;
    }
    writer.flush()?;
    Ok(())
}

/// The figure for a suite chart. Series that share a panel take colours by label, in order of
/// first appearance across the figure, so one entity keeps one colour; a panel's only series
/// is drawn in neutral ink. Points that a log axis cannot show (x ≤ 0) and undefined values
/// are left out; they remain in the tables.
pub fn figure(chart: &Chart) -> anyhow::Result<Figure> {
    let mut identities: Vec<String> = Vec::new();
    let mut panels = Vec::with_capacity(chart.panels.len());
    for panel in &chart.panels {
        let shared = panel.series.len() >= 2;
        let mut series = Vec::with_capacity(panel.series.len());
        for s in &panel.series {
            let color = if shared {
                let slot = match identities.iter().position(|l| *l == s.label) {
                    Some(i) => i,
                    None => {
                        identities.push(s.label.clone());
                        identities.len() - 1
                    }
                };
                *SERIES_COLOURS.get(slot).with_context(|| {
                    format!(
                        "{} has more than {} series identities; fold or split them",
                        chart.file,
                        SERIES_COLOURS.len()
                    )
                })?
            } else {
                SINGLE_SERIES
            };
            // Every sim chart plots a sweep against x, so points are drawn in x order.
            let mut points: Vec<(f64, f64)> = s
                .points
                .iter()
                .filter(|(x, y)| x.is_finite() && y.is_finite() && (!panel.log_x || *x > 0.0))
                .map(|(x, y)| (if panel.log_x { x.log10() } else { *x }, *y))
                .collect();
            points.sort_by(|a, b| a.0.total_cmp(&b.0));
            series.push(Series {
                label: s.label.clone(),
                points,
                color,
                markers: true,
            });
        }
        let references: Vec<Reference> = panel
            .references
            .iter()
            .map(|(label, value)| Reference {
                label: label.clone(),
                value: *value,
            })
            .collect();
        let legend = legend_corner(&series, &references);
        panels.push(Panel {
            title: panel.title.clone(),
            x_label: panel.x_label.clone(),
            y_label: panel.y_label.clone(),
            x_scale: if panel.log_x {
                Scale::Log10
            } else {
                Scale::Linear
            },
            y_scale: Scale::Linear,
            series,
            references,
            legend,
            y_range: None,
        });
    }
    let size = if panels.len() >= 2 {
        (1800, 680)
    } else {
        (1200, 760)
    };
    Ok(Figure {
        title: chart.title.clone(),
        subtitle: chart.subtitle.clone(),
        panels,
        size,
    })
}

/// The plot corner whose legend box would cover the least data: the lines are sampled along
/// each segment, in axis units normalised to the data range (the drawn range adds a small
/// margin, which this ignores). Ties go to the upper left, then upper right, lower right and
/// lower left.
fn legend_corner(series: &[Series], references: &[Reference]) -> SeriesLabelPosition {
    let mut samples: Vec<(f64, f64)> = Vec::new();
    for s in series {
        samples.extend(&s.points);
        for pair in s.points.windows(2) {
            for k in 1..20 {
                let t = f64::from(k) / 20.0;
                samples.push((
                    pair[0].0 + t * (pair[1].0 - pair[0].0),
                    pair[0].1 + t * (pair[1].1 - pair[0].1),
                ));
            }
        }
    }
    let (x_lo, x_hi) = samples
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| {
            (a.0.min(p.0), a.1.max(p.0))
        });
    for r in references {
        for k in 0..=20 {
            let t = f64::from(k) / 20.0;
            samples.push((x_lo + t * (x_hi - x_lo), r.value));
        }
    }
    let (y_lo, y_hi) = samples
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| {
            (a.0.min(p.1), a.1.max(p.1))
        });
    let unit = |v: f64, lo: f64, hi: f64| if hi > lo { (v - lo) / (hi - lo) } else { 0.5 };
    // The legend box: about a quarter of the width, and a line of height per series.
    let width = 0.3;
    let height = 0.08 + 0.07 * series.len() as f64;
    let corners = [
        (SeriesLabelPosition::UpperLeft, false, true),
        (SeriesLabelPosition::UpperRight, true, true),
        (SeriesLabelPosition::LowerRight, true, false),
        (SeriesLabelPosition::LowerLeft, false, false),
    ];
    let covered = |right: bool, upper: bool| {
        samples
            .iter()
            .filter(|(x, y)| {
                let (u, v) = (unit(*x, x_lo, x_hi), unit(*y, y_lo, y_hi));
                let in_x = if right { u >= 1.0 - width } else { u <= width };
                let in_y = if upper {
                    v >= 1.0 - height
                } else {
                    v <= height
                };
                in_x && in_y
            })
            .count()
    };
    let mut best = (usize::MAX, SeriesLabelPosition::UpperLeft);
    for (position, right, upper) in corners {
        let n = covered(right, upper);
        if n < best.0 {
            best = (n, position);
        }
    }
    best.1
}

fn profile_name(profile: Profile) -> &'static str {
    match profile {
        Profile::Full => "full",
        Profile::Quick => "quick",
    }
}

/// The reproducible summary (no timings, no thread count).
pub fn render_summary(
    config: &Config,
    out: &SuiteOutput,
    git: &GitInfo,
    config_sha256: &str,
    suite: &SuiteOptions,
) -> String {
    let mut s = String::new();
    let commit = match (&git.commit, git.dirty) {
        (Some(h), false) => format!("`{h}` (clean working tree)"),
        (Some(h), true) => format!("`{h}` with uncommitted changes"),
        (None, _) => "not a git repository".to_string(),
    };
    let _ = writeln!(s, "# M3a Tier 1 issuance simulator — summary\n");
    let _ = writeln!(s, "- Git commit: {commit}");
    let _ = writeln!(s, "- Seed: {}", config.run.seed);
    let _ = writeln!(s, "- Configuration SHA-256: `{config_sha256}`");
    let _ = writeln!(s, "- Protocol version: {}", gb_protocol::PROTOCOL_VERSION);
    let _ = writeln!(
        s,
        "- Profile: {}{}",
        profile_name(suite.profile),
        if suite.profile == Profile::Quick {
            " (tiny sizes for tests; not results)"
        } else {
            ""
        }
    );
    let sections = if suite.only.is_empty() {
        "all".to_string()
    } else {
        let mut v = vec!["V0".to_string()];
        v.extend(suite.only.iter().filter(|x| *x != "V0").cloned());
        v.join(", ")
    };
    let _ = writeln!(s, "- Sections: {sections}");
    let _ = writeln!(s, "- Runs: {}\n", out.perf.len());
    let _ = writeln!(
        s,
        "Every run is seeded and single-threaded, so these tables are reproducible bit for bit whatever the thread count; wall times are machine-dependent and are in `perf/`. Every protocol decision goes through `gb-protocol` (SPEC v0.6 and the architect's M3a decisions of 2026-10-12). Values marked as checks are compared with M1's closed forms or with exact expectations at 5 standard errors (`validation.csv`); every other number is a measurement, reported as it came out.\n"
    );
    let passed = out.checks.iter().filter(|c| c.passed).count();
    let _ = writeln!(
        s,
        "## Validation\n\n**{passed} of {} checks passed.** The V0 gate {}.",
        out.checks.len(),
        if out.gate_passed {
            "passed, so the stand-in and fast modes are used for the other sections"
        } else {
            "failed, so no result that relies on the stand-in or fast modes is reported"
        }
    );
    for f in out.checks.iter().filter(|c| !c.passed) {
        let _ = writeln!(
            s,
            "- **failed** `{}`: {} (reference {}, value {})",
            f.check, f.description, f.reference, f.value
        );
    }
    let _ = writeln!(s);
    for part in &out.summary {
        let _ = writeln!(s, "{}", part.trim_end());
        let _ = writeln!(s);
    }
    s
}

/// The machine-dependent performance report.
pub fn render_perf(out: &SuiteOutput, suite: &SuiteOptions) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# M3a performance (machine-dependent)\n");
    let _ = writeln!(
        s,
        "Machine: {}. Threads: {}. Each run is single-threaded; `perf.csv` has every run.\n",
        machine_description(),
        suite.threads
    );
    let mut sections: Vec<(String, f64, u64, f64)> = Vec::new();
    for p in &out.perf {
        match sections.iter_mut().find(|x| x.0 == p.section) {
            Some(x) => {
                x.1 += p.wall_s;
                x.2 += p.events;
                x.3 += p.simulated_days;
            }
            None => sections.push((p.section.clone(), p.wall_s, p.events, p.simulated_days)),
        }
    }
    let _ = writeln!(
        s,
        "| section | runs | CPU seconds | events | simulated days | events per second |\n|---|---|---|---|---|---|"
    );
    for (name, wall, events, days) in &sections {
        let runs = out.perf.iter().filter(|p| &p.section == name).count();
        let _ = writeln!(
            s,
            "| {name} | {runs} | {wall:.1} | {events} | {days:.2} | {:.0} |",
            if *wall > 0.0 {
                *events as f64 / wall
            } else {
                0.0
            }
        );
    }
    let total: f64 = out.perf.iter().map(|p| p.wall_s).sum();
    let _ = writeln!(
        s,
        "\nTotal CPU time {total:.0} s over {} runs.\n",
        out.perf.len()
    );
    let mut slowest: Vec<_> = out.perf.iter().collect();
    slowest.sort_by(|a, b| b.wall_s.total_cmp(&a.wall_s));
    let _ = writeln!(
        s,
        "## Slowest runs\n\n| run | seconds | events | simulated days |\n|---|---|---|---|"
    );
    for p in slowest.iter().take(10) {
        let _ = writeln!(
            s,
            "| {} | {:.1} | {} | {:.2} |",
            p.job, p.wall_s, p.events, p.simulated_days
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use gb_sim::suite::{ChartPanel, ChartSeries};

    fn panel(series: &[&str], log_x: bool) -> ChartPanel {
        ChartPanel {
            title: "t".to_string(),
            x_label: "x".to_string(),
            y_label: "y".to_string(),
            log_x,
            series: series
                .iter()
                .map(|l| ChartSeries {
                    label: (*l).to_string(),
                    points: vec![(0.0, 1.0), (10.0, f64::NAN), (100.0, 2.0)],
                })
                .collect(),
            references: vec![("one".to_string(), 1.0)],
        }
    }

    fn chart(panels: Vec<ChartPanel>) -> Chart {
        Chart {
            file: "c.png".to_string(),
            title: "c".to_string(),
            subtitle: "s".to_string(),
            panels,
        }
    }

    #[test]
    fn an_entity_keeps_its_colour_across_panels() {
        let f = figure(&chart(vec![
            panel(&["a", "b"], false),
            panel(&["b", "c"], false),
        ]))
        .unwrap();
        assert_eq!(f.panels[0].series[1].color, f.panels[1].series[0].color);
        assert_eq!(f.panels[1].series[1].color, SERIES_COLOURS[2]);
    }

    #[test]
    fn a_single_series_is_neutral_and_log_axes_drop_what_they_cannot_show() {
        let f = figure(&chart(vec![panel(&["a"], true)])).unwrap();
        let s = &f.panels[0].series[0];
        assert_eq!(s.color, SINGLE_SERIES);
        // x = 0 cannot be shown on a log axis and y = NaN is undefined: one point remains.
        assert_eq!(s.points, vec![(2.0, 2.0)]);
        assert_eq!(f.panels[0].x_scale, Scale::Log10);
    }

    #[test]
    fn the_legend_avoids_the_data() {
        let line = |points: Vec<(f64, f64)>| Series {
            label: "s".to_string(),
            points,
            color: SINGLE_SERIES,
            markers: true,
        };
        // Data rising from lower left to upper right leaves the upper left and lower right
        // empty; the upper left wins the tie.
        let rising = [
            line(vec![(0.0, 0.0), (1.0, 1.0)]),
            line(vec![(0.0, 0.1), (1.0, 1.1)]),
        ];
        assert!(matches!(
            legend_corner(&rising, &[]),
            SeriesLabelPosition::UpperLeft
        ));
        // Falling data fills the upper left and lower right: the legend goes upper right.
        let falling = [
            line(vec![(0.0, 1.0), (1.0, 0.0)]),
            line(vec![(0.0, 1.1), (1.0, 0.1)]),
        ];
        assert!(matches!(
            legend_corner(&falling, &[]),
            SeriesLabelPosition::UpperRight
        ));
    }

    #[test]
    fn more_identities_than_colours_is_an_error() {
        let labels = ["a", "b", "c", "d", "e"];
        assert!(figure(&chart(vec![panel(&labels, false)])).is_err());
    }
}
