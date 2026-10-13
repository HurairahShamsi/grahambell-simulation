//! Typed configuration for GrahamBell Stage 1.
//!
//! Every parameter in SPEC section 2 is a [`Param`] carrying its value, its SPEC status tag
//! (`D` decided, `P` proposed, `O` open) and its sweep range. Configuration files are TOML. A
//! file only needs the values it overrides, because it is deep-merged over
//! [`Config::default`] before being deserialized. Unknown keys are rejected, and a file may
//! not change a status tag, because status tags belong to the SPEC.
//!
//! `configs/default.toml` is a readable mirror of [`Config::default`]; a test checks that the
//! two are identical, and another test checks that every row of the SPEC §2 table maps to
//! configuration keys with matching status tags.

pub mod analytic;
mod config;
pub mod crypto_tests;
mod error;
mod param;
pub mod protocol;
pub mod registry;
pub mod sim;

pub use analytic::{AnalyticConfig, ModelConstants, MonteCarloConfig, RunConfig};
pub use config::Config;
pub use crypto_tests::CryptoTestsConfig;
pub use error::ConfigError;
pub use param::{Param, Status, Sweep};
pub use sim::SimConfig;
