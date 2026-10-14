//! A run is a pure function of its settings and seed: the same job twice gives identical
//! results (everything except wall time), and another seed gives different ones.

use gb_sim::{JobResult, JobSpec, run_job};

/// A small realistic run: outages, latency, loss and clock skew all draw random numbers.
fn job(seed: u64) -> JobSpec {
    let config = gb_config::Config::default();
    let mut spec = JobSpec::from_config(&config, "determinism", "T", seed);
    spec.miners = 300;
    spec.registered_ids = 2_900;
    spec.days = 0.5;
    spec.warmup_days = 0.0;
    spec
}

fn run(spec: &JobSpec) -> Result<JobResult, gb_sim::SimError> {
    let mut result = run_job(spec, &gb_config::Config::default())?;
    result.wall_s = 0.0;
    Ok(result)
}

#[test]
fn a_tiny_acceptance_cache_gives_the_same_run() {
    let spec = job(11);
    let mut small = gb_config::Config::default();
    small.sim.acceptance_cache_entries = 50;
    let mut result = run_job(&spec, &small).unwrap();
    result.wall_s = 0.0;
    assert_eq!(result, run(&spec).unwrap());
}

#[test]
fn the_same_seed_gives_the_same_run_and_another_seed_does_not() {
    let first = run(&job(11)).unwrap();
    assert!(first.blocks > 100, "{} blocks", first.blocks);
    assert_eq!(first, run(&job(11)).unwrap());
    assert_ne!(first, run(&job(12)).unwrap());
}
