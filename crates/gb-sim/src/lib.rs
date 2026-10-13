//! Tier 1 discrete-event simulator of GrahamBell PoW-ID issuance (M3, ARCHITECTURE §3).
//!
//! One run ([`run_job`]) simulates miner slots competing for IDs on a network of regions with
//! measured latency, households that go offline, witness chains that lock entropy and sign
//! PoWits, and validators that apply SPEC §3.7 on their own clocks. Every protocol decision
//! goes through `gb-protocol`: validation, fork choice, difficulty (Variants A and B and the
//! M3 correction candidates), allocation, the committee lottery and the One Chance outcomes.
//!
//! - **Determinism.** A run is single-threaded and draws every random number by key from its
//!   seed (`rng`), so a run is reproducible bit for bit, and runs can execute in parallel
//!   across threads without changing any result (`suite`).
//! - **Crypto modes.** Real BLS and SHA-256 (`real`), the M2 C6 stand-in entropy with real
//!   SHA-256 chains, or the fast mode that draws winning steps from the target's per-step
//!   probability (`round`); the fast mode is used only after the agreement gate (V0).
//!
//! | Module | Contents |
//! |---|---|
//! | [`engine`] | event queue ordered by time, then sequence number |
//! | [`rng`], [`samplers`], [`stats`] | keyed random access, samplers, test statistics |
//! | [`network`] | RIPE Atlas latency, loss, partitions, calibrated gossip |
//! | [`household`] | household downtime |
//! | [`job`] | run settings and results |
//! | [`real`] | real cryptography and the test-vector check |
//! | [`suite`] | the M3a experiments, validation checks and tables |

pub mod engine;
pub mod household;
pub mod job;
pub mod network;
pub mod real;
pub mod rng;
pub mod samplers;
pub mod stats;
pub mod suite;

mod chain;
mod round;
mod sim;
mod world;

pub use job::{ForkRule, GroupSpec, JobResult, JobSpec, Sampler, Strategy, TraceRow};

/// Errors of the simulator.
#[derive(Debug, thiserror::Error)]
pub enum SimError {
    /// The run could not be set up.
    #[error("simulation setup: {0}")]
    Setup(String),
}

/// Runs one job.
pub fn run_job(spec: &JobSpec, config: &gb_config::Config) -> Result<JobResult, SimError> {
    sim::Sim::run(spec, config)
}
