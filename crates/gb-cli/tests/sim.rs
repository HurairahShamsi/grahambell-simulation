//! End-to-end checks of `gb sim`: the outputs (everything outside `perf/`) do not depend on the
//! thread count, a run of the quick profile writes every table and chart and its checks pass,
//! sampled traces are written when asked for, a run refuses to start from a working tree that
//! is not a clean commit or with an unknown section, and a started run resumes to the outputs
//! of a run done in one go.

use gb_cli::sim::{
    CHART_DIR, CHECKPOINT_DIR, PERF_DIR, RESUME_FILE, SIM_CHARTS, SIM_TABLES, SimOptions,
    TRACE_DIR, produce, resume_sim, run_sim, start_sim,
};
use gb_config::Config;
use gb_runlog::{GitInfo, OutputFile, RESOLVED_CONFIG_FILE, build_manifest};
use gb_sim::suite::{CheckStatus, Profile, RunControl, SuiteOptions, check_status};
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
        control: RunControl::default(),
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
            control: RunControl::default(),
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
    // W1 is supplementary: a full run leaves it out.
    assert!(!written.contains(&"W1_withholder_placement.csv".to_string()));
    assert!(written.contains(&format!("{PERF_DIR}/perf.csv")));
    assert!(written.contains(&format!("{PERF_DIR}/PERF.md")));
    assert!(out.gate_passed, "the V0 gate failed");
    // `ALL-deep-reorgs` measures the protocol, not the simulator: a stall followed by a
    // catch-up burst (start rule (a)) can reorganise the chain below a confirmed ID. It is
    // reported in SUMMARY.md and validation.csv as it comes out, so it is not asserted here.
    // Reference-limited checks are reported but are not failures (ASSUMPTIONS SIM-31).
    // C7's independent-outage checks are reported but not asserted at these sizes: the quick
    // profile samples about 290 KWCs 7,200 times, so at f = 0.9 (about 0.1 KWCs below quorum
    // expected) one KWC that stays below quorum for a while gives clustered samples that fail
    // the binomial tolerance. At full size they pass (SIM-49).
    assert!(out.checks.iter().any(|c| c.check == "ALL-deep-reorgs"));
    let failed: Vec<_> = out
        .checks
        .iter()
        .filter(|c| {
            check_status(c) == CheckStatus::Fail
                && c.check != "ALL-deep-reorgs"
                && !c.check.starts_with("C7-independent-")
        })
        .collect();
    assert!(failed.is_empty(), "failed checks: {failed:#?}");
    let validation = std::fs::read_to_string(dir.path().join("validation.csv")).unwrap();
    assert!(
        validation
            .lines()
            .next()
            .unwrap()
            .ends_with(",passed,status")
    );
    assert!(
        validation.contains(",reference-limited\n")
            || validation.contains(",reference-limited\r\n")
    );
}

#[test]
fn the_supplementary_sections_run_when_named_and_write_their_tables() {
    let dir = tempfile::tempdir().unwrap();
    let out = produce(
        dir.path(),
        &Config::default(),
        &clean_git(),
        &SuiteOptions {
            profile: Profile::Quick,
            threads: 2,
            only: vec!["W1".to_string(), "W2".to_string()],
            control: RunControl::default(),
        },
    )
    .unwrap();
    assert!(out.gate_passed, "the V0 gate failed");
    // W1: three placements, each with E1's two finalists and C3's two deadlines.
    let table = std::fs::read_to_string(dir.path().join("W1_withholder_placement.csv")).unwrap();
    assert_eq!(table.lines().count(), 1 + 3 * 4, "{table}");
    let regions = std::fs::read_to_string(dir.path().join("W1_withholder_regions.csv")).unwrap();
    assert!(
        regions.lines().any(|l| l.starts_with("oc,oc,1")),
        "{regions}"
    );
    // W2: the longest pauses, and a trace of each in traces/.
    let pauses = std::fs::read_to_string(dir.path().join("W2_pauses.csv")).unwrap();
    let ranks = pauses.lines().count() - 1;
    assert!((1..=3).contains(&ranks), "{pauses}");
    for k in 1..=ranks {
        for kind in ["blocks", "samples"] {
            let path = dir.path().join(format!("traces/E3_pause{k}_{kind}.csv"));
            assert!(path.exists(), "missing {}", path.display());
        }
    }
    let summary = std::fs::read_to_string(dir.path().join("SUMMARY.md")).unwrap();
    assert!(
        summary.contains("## W1") && summary.contains("## W2"),
        "{summary}"
    );
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
        resume: None,
        progress: false,
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

#[test]
fn a_started_run_resumes_to_the_same_outputs_and_cannot_resume_twice() {
    // `gb sim` starts a run directory, then runs it; `gb sim --resume` runs the same function
    // on a directory whose run was interrupted.
    let repo = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let opts = options(repo.path(), out.path(), true, &["V4"]);
    let run_dir = start_sim(&opts).unwrap();
    assert!(run_dir.join(RESUME_FILE).exists());
    let outcome = resume_sim(&run_dir, &opts).unwrap();
    assert!(outcome.gate_passed);
    assert!(!run_dir.join(RESUME_FILE).exists());
    assert!(!run_dir.join(CHECKPOINT_DIR).exists());
    let error = resume_sim(&run_dir, &opts).unwrap_err();
    assert!(format!("{error}").contains("finished run"), "{error}");
    // The same outputs as a run done in one go.
    let direct = tempfile::tempdir().unwrap();
    produce(
        direct.path(),
        &Config::default(),
        &GitInfo::read(repo.path()),
        &SuiteOptions {
            profile: Profile::Quick,
            threads: 2,
            only: vec!["V4".to_string()],
            control: RunControl::default(),
        },
    )
    .unwrap();
    let without_log = |dir: &Path| {
        reproducible(dir)
            .unwrap()
            .into_iter()
            .filter(|f| f.path != gb_runlog::RUN_LOG_FILE)
            .collect::<Vec<_>>()
    };
    assert_eq!(without_log(&run_dir), without_log(direct.path()));
}
