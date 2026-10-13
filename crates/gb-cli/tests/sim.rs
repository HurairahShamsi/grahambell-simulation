//! End-to-end checks of `gb sim`: the outputs (everything outside `perf/`) do not depend on the
//! thread count, a run of the quick profile writes every table and chart and its checks pass,
//! sampled traces are written when asked for, and a run refuses to start from a working tree
//! that is not a clean commit or with an unknown section.

use gb_cli::sim::{
    CHART_DIR, PERF_DIR, SIM_CHARTS, SIM_TABLES, SimOptions, TRACE_DIR, produce, run_sim,
};
use gb_config::Config;
use gb_runlog::{GitInfo, OutputFile, RESOLVED_CONFIG_FILE, build_manifest};
use gb_sim::suite::{Profile, SuiteOptions};
use std::path::Path;

fn clean_git() -> GitInfo {
    GitInfo {
        commit: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
        dirty: false,
    }
}

/// Every output except the machine-dependent performance files.
fn reproducible(dir: &Path) -> Result<Vec<OutputFile>, gb_runlog::RunLogError> {
    Ok(build_manifest(dir, &[])?
        .into_iter()
        .filter(|f| !f.path.starts_with(&format!("{PERF_DIR}/")))
        .collect())
}

#[test]
fn outputs_and_traces_do_not_depend_on_the_thread_count() {
    // V0 (which always runs: all three crypto modes) and A6, with a sampled trace.
    let mut config = Config::default();
    config.sim.trace_sample = 0.001;
    let options = |threads| SuiteOptions {
        profile: Profile::Quick,
        threads,
        only: vec!["A6".to_string()],
    };
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    produce(first.path(), &config, &clean_git(), &options(1)).unwrap();
    produce(second.path(), &config, &clean_git(), &options(3)).unwrap();
    // Reference: the first run's SHA-256 manifest; tolerance: every byte identical.
    let manifest = reproducible(first.path()).unwrap();
    assert_eq!(manifest, reproducible(second.path()).unwrap());
    let traces = manifest
        .iter()
        .filter(|f| f.path.starts_with(&format!("{TRACE_DIR}/")))
        .count();
    assert!(traces > 0, "no trace was written");
}

#[test]
fn the_quick_profile_writes_every_output_and_its_checks_pass() {
    let config = Config::default();
    let dir = tempfile::tempdir().unwrap();
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().min(4));
    let out = produce(
        dir.path(),
        &config,
        &clean_git(),
        &SuiteOptions {
            profile: Profile::Quick,
            threads,
            only: Vec::new(),
        },
    )
    .unwrap();
    let written: Vec<String> = build_manifest(dir.path(), &[])
        .unwrap()
        .into_iter()
        .map(|f| f.path)
        .collect();
    for table in SIM_TABLES.iter().chain(&["validation.csv", "SUMMARY.md"]) {
        assert!(written.contains(&table.to_string()), "missing {table}");
    }
    for chart in SIM_CHARTS {
        let path = format!("{CHART_DIR}/{chart}");
        assert!(written.contains(&path), "missing {path}");
    }
    assert!(written.contains(&RESOLVED_CONFIG_FILE.to_string()));
    assert!(written.contains(&format!("{PERF_DIR}/perf.csv")));
    assert!(written.contains(&format!("{PERF_DIR}/PERF.md")));
    assert!(out.gate_passed, "the V0 gate failed");
    // `ALL-deep-reorgs` measures the protocol, not the simulator: under SPEC v0.6 the PoWit
    // deadline has no default, and a stall followed by a catch-up burst (start rule (a)) can
    // reorganise the chain below a confirmed ID. It is reported in SUMMARY.md and
    // validation.csv as it comes out, so it is not asserted here.
    assert!(out.checks.iter().any(|c| c.check == "ALL-deep-reorgs"));
    let failed: Vec<_> = out
        .checks
        .iter()
        .filter(|c| !c.passed && c.check != "ALL-deep-reorgs")
        .collect();
    assert!(failed.is_empty(), "failed checks: {failed:#?}");
}

fn options(repo: &Path, out: &Path, allow_dirty: bool, only: &[&str]) -> SimOptions {
    SimOptions {
        config_path: None,
        seed: None,
        out_root: out.to_path_buf(),
        allow_dirty,
        repo_dir: repo.to_path_buf(),
        command_line: vec!["gb".to_string(), "sim".to_string()],
        profile: Profile::Quick,
        threads: Some(1),
        only: only.iter().map(|s| (*s).to_string()).collect(),
        trace: None,
    }
}

#[test]
fn a_run_outside_a_clean_commit_is_refused_without_allow_dirty() {
    let not_a_repository = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let error = run_sim(&options(not_a_repository.path(), out.path(), false, &[])).unwrap_err();
    assert!(format!("{error}").contains("--allow-dirty"), "{error}");
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
}

#[test]
fn an_unknown_section_is_refused() {
    let repo = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let error = run_sim(&options(repo.path(), out.path(), true, &["C9"])).unwrap_err();
    assert!(format!("{error}").contains("unknown section C9"), "{error}");
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
}
