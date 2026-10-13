//! `gb crypto`: runs the M2 measurements and writes a run directory with provenance.
//!
//! Reproducible outputs (tables `K*.csv`, `validation.csv`, `SUMMARY.md`) depend only on the
//! commit, seed and configuration. Machine-dependent timings go to `bench/`, which reproduction
//! checks leave out.

use crate::tables::write_csv;
use crate::{load_config, machine_description};
use anyhow::{Context, bail, ensure};
use gb_config::Config;
use gb_crypto_tests::{CryptoResults, Timings};
use gb_runlog::{
    GitInfo, RunDetails, RunIdentity, UtcTimestamp, finish_run, write_resolved_config,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Directory, inside a run directory, of machine-dependent timings.
pub const BENCH_DIR: &str = "bench";

/// Reproducible table files, in the order written.
pub const CRYPTO_TABLES: &[&str] = &[
    "K1_grinding.csv",
    "K1_equivocation.csv",
    "K2_sorted_hex.csv",
    "K3_beacon_withholding.csv",
    "K3_placement_grinding.csv",
    "K5_threshold_bls.csv",
    "K6_equivalence.csv",
    "validation.csv",
];

/// Options of `gb crypto`.
#[derive(Debug, Clone, PartialEq)]
pub struct CryptoOptions {
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
}

/// What a finished run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct CryptoOutcome {
    /// Run directory.
    pub run_dir: PathBuf,
    /// Cross-checks run.
    pub checks: usize,
    /// Identifiers of failed checks.
    pub failed: Vec<String>,
}

/// Computes everything and writes all outputs except `run.json` into `dir`.
pub fn produce(dir: &Path, config: &Config, git: &GitInfo) -> anyhow::Result<CryptoResults> {
    let results = gb_crypto_tests::run(config);
    let timings = gb_crypto_tests::timings(config);
    let config_sha256 = write_resolved_config(dir, &config.to_toml_string()?)?;
    write_csv(&dir.join(CRYPTO_TABLES[0]), &results.grinding)?;
    write_csv(&dir.join(CRYPTO_TABLES[1]), &results.equivocation)?;
    write_csv(&dir.join(CRYPTO_TABLES[2]), &results.sorted_hex)?;
    write_csv(&dir.join(CRYPTO_TABLES[3]), &results.beacon)?;
    write_csv(&dir.join(CRYPTO_TABLES[4]), &results.placement)?;
    write_csv(&dir.join(CRYPTO_TABLES[5]), &results.threshold)?;
    write_csv(&dir.join(CRYPTO_TABLES[6]), &results.equivalence)?;
    write_csv(&dir.join(CRYPTO_TABLES[7]), &results.checks)?;
    let summary = render_summary(config, &results, git, &config_sha256);
    std::fs::write(dir.join("SUMMARY.md"), summary).context("writing SUMMARY.md")?;
    let bench = dir.join(BENCH_DIR);
    std::fs::create_dir_all(&bench)?;
    write_csv(&bench.join("K4_timings.csv"), &timings.operations)?;
    write_csv(&bench.join("K4_server_cpu.csv"), &timings.server_cpu)?;
    write_csv(&bench.join("K5_threshold_timings.csv"), &timings.threshold)?;
    std::fs::write(bench.join("BENCH.md"), render_bench(config, &timings))
        .context("writing bench/BENCH.md")?;
    Ok(results)
}

/// Runs `gb crypto`.
pub fn run_crypto(options: &CryptoOptions) -> anyhow::Result<CryptoOutcome> {
    let mut config = load_config(options.config_path.as_deref())?;
    if let Some(seed) = options.seed {
        config.run.seed = seed;
    }
    let git = GitInfo::read(&options.repo_dir);
    if !git.is_clean_commit() && !options.allow_dirty {
        bail!(
            "the working tree is not a clean git commit ({}); commit your changes so results can be attributed, or pass --allow-dirty (recorded in run.json)",
            git.short_label()
        );
    }
    let identity = RunIdentity {
        analysis: "crypto".to_string(),
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
    std::fs::create_dir_all(&run_dir)?;
    let results = produce(&run_dir, &config, &identity.git)?;
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
    Ok(CryptoOutcome {
        run_dir,
        checks: results.checks.len(),
        failed: results
            .checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| c.check.clone())
            .collect(),
    })
}

fn fmt(x: f64) -> String {
    if x == 0.0 {
        "0".to_string()
    } else if x.abs() >= 100.0 {
        format!("{x:.0}")
    } else if x.abs() >= 1.0 {
        format!("{x:.3}")
    } else {
        format!("{x:.4}")
    }
}

/// The reproducible summary (no timings).
pub fn render_summary(
    config: &Config,
    r: &CryptoResults,
    git: &GitInfo,
    config_sha256: &str,
) -> String {
    let c = &config.crypto_tests;
    let mut s = String::new();
    let commit = match (&git.commit, git.dirty) {
        (Some(h), false) => format!("`{h}` (clean working tree)"),
        (Some(h), true) => format!("`{h}` with uncommitted changes"),
        (None, _) => "not a git repository".to_string(),
    };
    let _ = writeln!(s, "# M2 cryptography measurements — summary\n");
    let _ = writeln!(s, "- Git commit: {commit}");
    let _ = writeln!(s, "- Seed: {}", config.run.seed);
    let _ = writeln!(s, "- Configuration SHA-256: `{config_sha256}`");
    let _ = writeln!(s, "- Protocol version: {}\n", gb_protocol::PROTOCOL_VERSION);
    let _ = writeln!(
        s,
        "Real SHA-256 and BLS12-381 (blst, min-pk, Ethereum PoP ciphersuite) throughout. Monte Carlo trials are seeded, so these tables are reproducible; timings are machine-dependent and are in `bench/BENCH.md`.\n"
    );

    let _ = writeln!(s, "## C1 — Entropy grinding (S11, H3)\n");
    let _ = writeln!(
        s,
        "Each trial builds a fresh header and its entropy, and counts attempts to the first winning step at a test difficulty of 1 in {}. An attacker with budget G keeps the best of G candidate entropies. Advantage = honest ÷ attacker mean attempts, 95% CI by batch means; the last column is the advantage if the candidates were independent.\n",
        c.expected_winning_attempts
    );
    let _ = writeln!(
        s,
        "| experiment | G | advantage | 95% CI | independent tries | distinct entropies |\n|---|---|---|---|---|---|"
    );
    for g in &r.grinding {
        let _ = writeln!(
            s,
            "| {} ({}) | {} | {} | {} – {} | {} | {} |",
            g.experiment,
            g.description,
            g.budget,
            fmt(g.advantage),
            fmt(g.ci_low),
            fmt(g.ci_high),
            fmt(g.independent_tries_advantage),
            fmt(g.distinct_entropies)
        );
    }
    let _ = writeln!(
        s,
        "\n**Equivocation (d):**\n\n| case | pairs | flagged | expected |\n|---|---|---|---|"
    );
    for e in &r.equivocation {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} |",
            e.case, e.pairs, e.flagged, e.expected
        );
    }

    let _ = writeln!(s, "\n## C2 — Sorted-hex rule (S14)\n");
    let _ = writeln!(
        s,
        "| experiment | pairs | sorted-hex collisions | fixed-order collisions |\n|---|---|---|---|"
    );
    for x in &r.sorted_hex {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} |",
            x.experiment, x.pairs, x.sorted_hex_collisions, x.fixed_order_collisions
        );
    }
    let _ = writeln!(
        s,
        "\nUnder the sorted-hex rule, {} different headers share each header's hash (values exchanged among fields of equal width); under fixed order, 1.",
        r.equivalent_headers
    );

    let _ = writeln!(
        s,
        "\n## C3 — Beacon withholding and placement grinding (ARCHITECTURE §8.6)\n"
    );
    let _ = writeln!(
        s,
        "The attacker mines each candidate beacon block with probability s and withholds it, giving up that block, when the outcome is unfavourable. Formula: q / (1 − s(1 − q)).\n"
    );
    let _ = writeln!(
        s,
        "| beacon | k | s | q | without | with withholding (± SE) | formula | gain | blocks given up per beacon |\n|---|---|---|---|---|---|---|---|---|"
    );
    for b in &r.beacon {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} | {} ± {} | {} | {}× | {} |",
            b.beacon,
            b.k.map_or("—".to_string(), |k| k.to_string()),
            b.attacker_block_share,
            fmt(b.favourable_share),
            fmt(b.p_without_withholding),
            fmt(b.p_with_withholding),
            fmt(b.standard_error),
            fmt(b.p_with_withholding_formula),
            fmt(b.gain_formula),
            fmt(b.blocks_forgone)
        );
    }
    let _ = writeln!(
        s,
        "\n**Placement grinding:** an ID is the hash of its winning block, fixed before its allocation beacon exists, so a rule applied when minting cannot steer the seat:\n\n| IDs | trials | share in target seats (± SE) | target share |\n|---|---|---|---|"
    );
    for p in &r.placement {
        let _ = writeln!(
            s,
            "| {} | {} | {} ± {} | {} |",
            p.ids,
            p.trials,
            fmt(p.p_target),
            fmt(p.standard_error),
            fmt(p.target_share)
        );
    }

    let _ = writeln!(
        s,
        "\n## C5 — Threshold BLS key setup (SPEC §12 [P]; benchmark only, not audited, not used by gb-protocol)\n"
    );
    let _ = writeln!(
        s,
        "Joint-Feldman DKG with t = ⌈2n/3⌉. Per member:\n\n| n | t | messages sent | messages received | bytes sent | bytes received | G1 multiplications | threshold signature verifies |\n|---|---|---|---|---|---|---|---|"
    );
    for t in &r.threshold {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            t.members,
            t.threshold,
            t.messages_sent,
            t.messages_received,
            t.bytes_sent,
            t.bytes_received,
            t.g1_multiplications,
            t.combined_signature_verifies
        );
    }
    let _ = writeln!(s, "\nTimes are in `bench/BENCH.md`.");

    let _ = writeln!(
        s,
        "\n## C6 — Simulator entropy stand-in\n\n| test | trials | statistic | p-value | passes (p ≥ {}) |\n|---|---|---|---|---|",
        c.equivalence_min_p_value
    );
    for e in &r.equivalence {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} |",
            e.test,
            e.trials,
            fmt(e.statistic),
            fmt(e.p_value),
            e.passes
        );
    }

    let passed = r.checks.iter().filter(|c| c.passed).count();
    let _ = writeln!(
        s,
        "\n## Cross-checks\n\n{passed} of {} checks passed (`validation.csv`).",
        r.checks.len()
    );
    for f in r.checks.iter().filter(|c| !c.passed) {
        let _ = writeln!(
            s,
            "- **failed** `{}`: {} (reference {}, value {})",
            f.check, f.description, f.reference, f.value
        );
    }
    s
}

/// The machine-dependent timing report.
pub fn render_bench(config: &Config, t: &Timings) -> String {
    let c = &config.crypto_tests;
    let mut s = String::new();
    let _ = writeln!(s, "# M2 timings (machine-dependent)\n");
    let _ = writeln!(
        s,
        "Machine: {}. Single-threaded, so ops/s are per core. Release build recommended.\n",
        machine_description()
    );
    let _ = writeln!(
        s,
        "## C4 — Operations\n\n| operation | signers | ops/s per core | µs per op |\n|---|---|---|---|"
    );
    for r in &t.operations {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} |",
            r.operation,
            r.signers,
            fmt(r.ops_per_second),
            fmt(r.microseconds)
        );
    }
    let _ = writeln!(
        s,
        "\n## C4 — §13 testnet witness server\n\nAssumptions: one server runs {} witness nodes as one WC; every miner gets fresh entropy every {} s round; per miner and witness each round: one miner-signature verify, one entropy signature, one entropy-aggregate verify and one SHA-256 chain step per second of the round. PoWit signing (one block per round network-wide), networking, serialisation and disk are not counted.\n",
        c.testnet_witnesses_per_server,
        fmt(c.testnet_round_s)
    );
    let _ = writeln!(
        s,
        "| miners | CPU-seconds per round | cores busy | logical cores here |\n|---|---|---|---|"
    );
    for r in &t.server_cpu {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} |",
            r.miners,
            fmt(r.cpu_seconds_per_round),
            fmt(r.cores_needed),
            r.cores_on_this_machine
        );
    }
    let _ = writeln!(
        s,
        "\n## C5 — Threshold BLS (benchmark only, not audited, not used by gb-protocol)\n\n| n | t | deal (s) | check shares (s) | setup per member (s) | partial sign (s) | combine (s) | verify (s) |\n|---|---|---|---|---|---|---|---|"
    );
    for r in &t.threshold {
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            r.members,
            r.threshold,
            fmt(r.deal_s),
            fmt(r.verify_shares_s),
            fmt(r.setup_per_member_s),
            fmt(r.partial_sign_s),
            fmt(r.combine_s),
            fmt(r.verify_s)
        );
    }
    s
}
