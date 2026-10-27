//! `gb`: command-line front end for GrahamBell Stage 1.
//!
//! Run `gb --help` for the list of commands.

use clap::{Parser, Subcommand};
use gb_cli::analytic::{AnalyticOptions, run_analytic};
use gb_cli::crypto::{CryptoOptions, run_crypto};
use gb_cli::load_config;
use gb_cli::params::{ListingFormat, render_listing};
use gb_cli::sim::{SimOptions, run_sim};
use gb_config::registry::parameter_listing;
use std::path::PathBuf;
use std::process::ExitCode;

/// GrahamBell Stage 1 simulation and analysis tool.
#[derive(Debug, Parser)]
#[command(name = "gb", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the M1 analytical baseline and write tables, charts, SUMMARY.md and run.json.
    Analytic {
        /// Configuration file to merge over the defaults.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Seed overriding the configuration's.
        #[arg(long)]
        seed: Option<u64>,
        /// Directory that receives one sub-directory per run.
        #[arg(long, default_value = "results/analytic")]
        out: PathBuf,
        /// Allow a working tree with uncommitted changes (recorded in run.json).
        #[arg(long)]
        allow_dirty: bool,
    },
    /// Run the M2 cryptography measurements and write tables, SUMMARY.md, timings and run.json.
    Crypto {
        /// Configuration file to merge over the defaults.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Seed overriding the configuration's.
        #[arg(long)]
        seed: Option<u64>,
        /// Directory that receives one sub-directory per run.
        #[arg(long, default_value = "results/crypto")]
        out: PathBuf,
        /// Allow a working tree with uncommitted changes (recorded in run.json).
        #[arg(long)]
        allow_dirty: bool,
    },
    /// Run the M3a Tier 1 simulator suite and write tables, charts, SUMMARY.md, validation.csv,
    /// performance and run.json.
    Sim {
        /// Configuration file to merge over the defaults.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Seed overriding the configuration's.
        #[arg(long)]
        seed: Option<u64>,
        /// Directory that receives one sub-directory per run.
        #[arg(long, default_value = "results/sim")]
        out: PathBuf,
        /// Allow a working tree with uncommitted changes (recorded in run.json).
        #[arg(long)]
        allow_dirty: bool,
        /// Experiment sizes: the M3a sizes, or tiny sizes for tests.
        #[arg(long, value_enum, default_value_t = ProfileArg::Full)]
        profile: ProfileArg,
        /// Worker threads (default: `sim.threads`); results do not depend on it.
        #[arg(long)]
        threads: Option<usize>,
        /// Run only these sections (repeat or separate with commas); V0 always runs, and W1,
        /// W2 and W3 (supplementary) run only when named.
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// Write a sampled trace of every run's events, this fraction of them, to trace/.
        #[arg(long)]
        trace: Option<f64>,
        /// Continue an interrupted run in this run directory: finished jobs are taken from its
        /// checkpoints; the configuration, seed, profile and sections are the run's own.
        #[arg(long, conflicts_with_all = ["config", "seed", "only", "trace"])]
        resume: Option<PathBuf>,
        /// Do not print a line per finished job.
        #[arg(long)]
        quiet: bool,
    },
    /// Write the gb-protocol test vectors (SPEC Appendix A) to a directory.
    Vectors {
        /// Directory that receives protocol_v1.json.
        #[arg(long, default_value = "crates/gb-protocol/vectors")]
        out: PathBuf,
    },
    /// Print every SPEC §2 parameter with its value, status tag and sweep.
    Params {
        /// Configuration file to merge over the defaults.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = ListingFormat::Markdown)]
        format: ListingFormat,
    },
    /// Inspect the resolved configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

/// Experiment sizes of `gb sim`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum ProfileArg {
    /// The M3a sizes.
    Full,
    /// Tiny sizes for tests and CI; not results.
    Quick,
}

#[derive(Debug, Subcommand)]
enum ConfigAction {
    /// Print the resolved configuration as TOML.
    Dump {
        /// Configuration file to merge over the defaults.
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<ExitCode> {
    match Cli::parse().command {
        Command::Analytic {
            config,
            seed,
            out,
            allow_dirty,
        } => {
            let options = AnalyticOptions {
                config_path: config,
                seed,
                out_root: out,
                allow_dirty,
                repo_dir: std::env::current_dir()?,
                command_line: std::env::args().collect(),
            };
            let outcome = run_analytic(&options)?;
            println!("Wrote {}", outcome.run_dir.display());
            println!(
                "Cross-checks passed: {} of {}",
                outcome.checks - outcome.failed.len(),
                outcome.checks
            );
            if !outcome.failed.is_empty() {
                eprintln!("Failed checks: {}", outcome.failed.join(", "));
                return Ok(ExitCode::FAILURE);
            }
        }
        Command::Crypto {
            config,
            seed,
            out,
            allow_dirty,
        } => {
            let outcome = run_crypto(&CryptoOptions {
                config_path: config,
                seed,
                out_root: out,
                allow_dirty,
                repo_dir: std::env::current_dir()?,
                command_line: std::env::args().collect(),
            })?;
            println!("Wrote {}", outcome.run_dir.display());
            println!(
                "Cross-checks passed: {} of {}",
                outcome.checks - outcome.failed.len(),
                outcome.checks
            );
            if !outcome.failed.is_empty() {
                eprintln!("Failed checks: {}", outcome.failed.join(", "));
                return Ok(ExitCode::FAILURE);
            }
        }
        Command::Sim {
            config,
            seed,
            out,
            allow_dirty,
            profile,
            threads,
            only,
            trace,
            resume,
            quiet,
        } => {
            let outcome = run_sim(&SimOptions {
                config_path: config,
                seed,
                out_root: out,
                allow_dirty,
                repo_dir: std::env::current_dir()?,
                command_line: std::env::args().collect(),
                profile: match profile {
                    ProfileArg::Full => gb_sim::suite::Profile::Full,
                    ProfileArg::Quick => gb_sim::suite::Profile::Quick,
                },
                threads,
                only,
                trace,
                resume,
                progress: !quiet,
            })?;
            println!("Wrote {}", outcome.run_dir.display());
            println!(
                "Validation checks: {} of {} pass, {} fail, {} reference-limited (V0 gate {})",
                outcome.checks - outcome.failed.len() - outcome.reference_limited,
                outcome.checks,
                outcome.failed.len(),
                outcome.reference_limited,
                if outcome.gate_passed {
                    "passed"
                } else {
                    "failed"
                }
            );
            if !outcome.failed.is_empty() {
                eprintln!("Failed checks: {}", outcome.failed.join(", "));
                return Ok(ExitCode::FAILURE);
            }
        }
        Command::Vectors { out } => {
            std::fs::create_dir_all(&out)?;
            let path = out.join(gb_protocol::vectors::VECTOR_FILE);
            std::fs::write(&path, gb_protocol::vectors::render()?)?;
            println!("Wrote {}", path.display());
        }
        Command::Params { config, format } => {
            let config = load_config(config.as_deref())?;
            print!("{}", render_listing(&parameter_listing(&config)?, format)?);
        }
        Command::Config {
            action: ConfigAction::Dump { config },
        } => {
            print!("{}", load_config(config.as_deref())?.to_toml_string()?);
        }
    }
    Ok(ExitCode::SUCCESS)
}
