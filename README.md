# GrahamBell — Stage 1 simulation

## What is GrahamBell

- A proposed Layer 1 blockchain whose Sybil resistance comes from time rather than hardware or capital: **Proof of Infrastructure Endurance (PoIE)**, also called **Proof of Time (PoT)**.
- Identities (IDs) are issued one at a time at a fixed global pace, about one every 30 seconds, through capped proof of work: every miner hashes at one hash per second, whatever its hardware.
- A Witness Chain of other participants supplies each round's entropy and checks the pace, so neither precomputed nor parallel work can win.
- An attacker can still create IDs, but only as fast as the shared schedule allows, so taking control of a mature network takes years of sustained participation.
- How a round works, rule by rule and with each rule's status: [docs/PROTOCOL_OVERVIEW.md](docs/PROTOCOL_OVERVIEW.md).
- What GrahamBell does not claim, and what this repository does not cover: [What this is not](docs/PROTOCOL_OVERVIEW.md#what-this-is-not).

**Stage 1** tests whether PoIE's claims hold under realistic and adversarial conditions, and publishes the results whether they confirm or refute the model. The rules under test are in [`docs/SPEC.md`](docs/SPEC.md), the single source of truth.

**Running a witness node.** Every registered ID must witness, on an always-on device; an unregistered miner cannot witness until it mints an ID. At full watch load (1,000 watched miners) a witness node needs about 9 % of one core, under 1 MB of per-round state plus the ID registry (about 0.26 GB at genesis), about 22 kB/s (57 GB a month) before transport overhead, and to stay online. These figures are calculated from measured cryptographic timings, not yet benchmarked on real devices; M5 replaces them with measured figures ([details](docs/PROTOCOL_OVERVIEW.md#running-a-witness-node)). Mining alone runs on any device: one hash per second.

## Status and disclaimer

GrahamBell is under active research and testing. Nothing in this repository is a guarantee of security, performance or future behaviour. Every result describes the specific protocol version and simulation settings it was produced with (see each run’s configuration and SPEC version). As testing continues, rules and parameters will change, and new results may differ from earlier ones, better or worse. When that happens, the change and its reason are recorded in the SPEC changelog and ASSUMPTIONS, and earlier results stay in the repository history, so nothing is quietly replaced. Requirements marked “calculated” or “estimated” are not yet measured. This is research, not a product, an offering or financial advice.

## What is in this repository

| Milestone | Status | Contents |
|---|---|---|
| M0 | done | Rust workspace, typed configuration of every SPEC parameter, run provenance, CI |
| M1 | done | **Analytical baseline**: exact formulas and exact probabilities for issuance, witness capture, the allocation committee, the restart attack, difficulty hopping and same-step ties |
| M1.1 | done | The architect's decisions on M1 (SPEC v0.3): 2.9M genesis IDs, adaptive-cap safety factor, count-based difficulty, committee seat lottery, adopted allocation algorithm, deactivation instead of bans. Adds the quorum trade-off (section G) and a [public brief](docs/M1_PUBLIC_BRIEF.md) |
| M1.2 | done | The architect's decisions on M1.1 (SPEC v0.4): seats survive downtime (only bans and absences longer than L vacate them), distinct episodes as the H6 measure, placement without the committee, committee departures, conflicting decisions and proposer rights. Adds quorum feasibility under honest downtime (section H), the KWC size trade-off (section I) and the Apache 2.0 licence |
| M2 | done | SPEC v0.5 (M1.2 answers, PoWit arrival deadline, Stage 3 testnet decisions, wire formats). The shared protocol core `gb-protocol` with test vectors, and `gb-crypto-tests`: grinding, sorted-hash ambiguity, beacon withholding, placement grinding, benchmarks, a threshold-BLS prototype and the simulator's entropy stand-in, all with real SHA-256 and BLS12-381 |
| M2.1 | done | SPEC v0.6: H1–H9 thresholds locked before M3, the online-set grinding lever (S11(f)), optimistic entropy verification, the fork-choice order and witness double-signing evidence; a committed crypto reference run |
| M3a | done | **Tier 1 issuance simulator** `gb-sim` (`gb sim`): measured inter-region latency (RIPE Atlas), household downtime with correlated outages, the validation ladder against M1 and M2 (V0–V4), and the honest baselines C1–C7 and A6: tagged-group amplification, fork choice, the PoWit deadline and a withholder, the issuance rate under Variant B and its correction candidates, One Chance costs and honest penalties, clock skew, quorum liveness and newcomers' patience; a committed sim reference run |
| M3a.1 | done | SPEC v0.7: the architect's decisions on M3a and M3a.1 (One Chance as "unreachable is not refusal", void IDs in replaced blocks) and four experiments with criteria fixed in advance: E1 fork choice and start rule (rule (r), first-seen), E2 issuance (the issued-entropy count with the P2 correction), E3 liveness under regional blackouts (threshold entropy, failover) and E4 the offline points ladder. The final v0.7 defaults: difficulty from the issued-entropy count with the P2 ×1.1 correction, threshold entropy (27 of 40), early blocks re-evaluated, and no PoWit deadline (10 s soft as [P]). Simulator bugs found and fixed on the way: region-wide feeder groups and providers (SIM-35), entropy issued below the quorum (SIM-43), rounds that ran out of draws and stopped the chain (SIM-48), C1's tagged groups placed in one region (SIM-51) and a late block that still closed the round below it (24eecb0); SIM-25, the horizon's limitation when miners reject a block for good, is documented, not fixed (all in [`docs/ASSUMPTIONS.md`](docs/ASSUMPTIONS.md)). A sim reference run at the final defaults ([`results/sim/reference/`](results/sim/reference/), with traces of E3's longest blackout pauses), the supplementary withholder placements ([`results/sim/supplement/`](results/sim/supplement/)), W3, the pause at a blackout's start over 100 seeds ([`results/sim/supplement-w3/`](results/sim/supplement-w3/)), and a [public brief](docs/M3_PUBLIC_BRIEF.md) |
| M3b–M6 | planned | Attacker scenarios and Tier 2 at full scale, the witness layer, cost model, final runs; see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) |

M1 results are the ground truth that later simulators must reproduce before their own results are trusted. Every modelling assumption behind them is listed in [`docs/ASSUMPTIONS.md`](docs/ASSUMPTIONS.md).

All M1 results assume the attacker's IDs are online 100% of the time while honest IDs are online a fraction f of the time. This is a deliberate worst case; M3 adds realistic outages for both sides.

For a short overview of what M1 found, written for a general technical audience, read [`docs/M1_PUBLIC_BRIEF.md`](docs/M1_PUBLIC_BRIEF.md); for M3a and M3a.1, read [`docs/M3_PUBLIC_BRIEF.md`](docs/M3_PUBLIC_BRIEF.md).

## Build and test

You need a stable Rust toolchain, 1.85 or newer; `rustup` picks it up from `rust-toolchain.toml`.

```sh
cargo build --release
cargo test --workspace
```

The same checks run in CI on every push and pull request:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

## Run the analysis

```sh
cargo run --release -p gb-cli -- analytic
```

This takes about fifteen seconds after compilation. It writes a new directory `results/analytic/<run-id>/` and copies its `SUMMARY.md` to `results/analytic/SUMMARY.md`. The run id is `<UTC time>_<git commit>_s<seed>`.

The command refuses to run from a working tree with uncommitted changes, so that every result can be traced to a commit. Pass `--allow-dirty` to override; the run log records that you did. It exits with a non-zero status if any cross-check fails, after writing all outputs.

Options:

| Option | Meaning |
|---|---|
| `--config FILE` | TOML file with the values you want to change, merged over the defaults; `value = "none"` unsets an optional parameter, such as `timing.powit_deadline_s` |
| `--seed N` | Master seed for the Monte Carlo cross-checks (default 20261005) |
| `--out DIR` | Where run directories go (default `results/analytic`) |
| `--allow-dirty` | Allow uncommitted changes (recorded in `run.json`) |

## Run the M2 measurements

```sh
cargo run --release -p gb-cli -- crypto        # tables, SUMMARY.md and run.json in results/crypto/<run-id>/
cargo bench -p gb-crypto-tests                 # criterion benchmarks of the same operations
cargo run --release -p gb-cli -- vectors       # rewrite crates/gb-protocol/vectors/protocol_v1.json
```

`gb crypto` takes the same options as `gb analytic` and also refuses a dirty working tree. It takes a few minutes. Its tables (`K1_…` to `K6_….csv`), `validation.csv` and `SUMMARY.md` are reproducible from the commit, seed and configuration. Timings depend on the machine, so they go to `bench/` (with the machine described in `bench/BENCH.md`) and are left out of reproducibility checks. The test vectors are checked by `cargo test`; regenerate them only when SPEC Appendix A changes.

## Run the M3a simulator

```sh
cargo run --release -p gb-cli -- sim                    # every section, in results/sim/<run-id>/
cargo run --release -p gb-cli -- sim --only C2,C3       # V0 (always) and the named sections
cargo run --release -p gb-cli -- sim --only W1          # the supplementary withholder placements
cargo run --release -p gb-cli -- sim --only W2          # traces of E3's longest blackout pauses
cargo run --release -p gb-cli -- sim --only W3          # the pause at a blackout's start, 100 seeds
cargo run --release -p gb-cli -- sim --profile quick    # tiny sizes, for tests; not results
```

`gb sim` takes the options of `gb analytic` and also refuses a dirty working tree. It runs the V0 gate first (the three crypto modes agree, the fast sampler is exact, the horizon audit finds nothing); if the gate fails, it reports no result that relies on the stand-in or fast modes. A full run takes a day or more of wall time with 4 threads on a laptop (the M3a.1 reference run at 1d9ce91: 26.2 hours and 101 CPU-hours; the rerun at 7cf4ff2, which reran 2,122 of its 2,978 jobs on an 8 GB laptop that swapped: 30.6 hours, and 120 CPU-hours in all its jobs; `perf/PERF.md` records the machine and the time of every run). It exits with a non-zero status if any check fails, after writing all outputs.

| Option | Meaning |
|---|---|
| `--profile full\|quick` | Experiment sizes (default `full`) |
| `--threads N` | Worker threads (default `sim.threads`, 4); results do not depend on it |
| `--only V1,C2,…` | Run only these sections; V0 always runs. Two supplementary sets run only when named: W1 (E1's finalists and C3 with the withholder placed three ways) and W2 (E3's blackout runs at the defaults, traced around their three longest pauses, in `traces/`) |
| `--trace F` | Also write a sampled trace of a fraction F of every run's events to `trace/` |
| `--resume DIR` | Continue an interrupted run in its run directory (same commit; its own configuration and sections) |
| `--quiet` | Do not print a line per finished job |

While it runs, `gb sim` prints a line per finished job with an estimate of the time left. It saves each finished job's result in the run directory's `checkpoint/`; if the run is interrupted (the machine sleeps, the process is killed), `gb sim --resume results/sim/<run-id>` continues it, and the finished run is the same as one done in one go.

A run whose fork storm grows past `sim.world.fork_storm_unsettled` (4,000) unsettled blocks stops there; such storms follow a stall of hours when no PoWit deadline is set. `SUMMARY.md` lists those runs, and `ALL_runs.csv` gives the day each one stopped (ASSUMPTIONS SIM-29).

Every run is single-threaded and seeded and is stored by its index, so the tables are byte-identical whatever the thread count. Wall times depend on the machine, so they go to `perf/` and are left out of reproducibility checks, as `bench/` is for `gb crypto`.

## Where each output lives

The reference run for this release is committed in [`results/analytic/reference/`](results/analytic/reference/); start with its [`SUMMARY.md`](results/analytic/reference/SUMMARY.md). It was produced from a clean commit, which its `run.json` records together with a SHA-256 of every file. New runs are written to `results/analytic/<run-id>/` and stay ignored by git.

The M2 crypto reference run is committed in [`results/crypto/reference/`](results/crypto/reference/), with its [`SUMMARY.md`](results/crypto/reference/SUMMARY.md). Its `bench/` timings come from the machine named in [`bench/BENCH.md`](results/crypto/reference/bench/BENCH.md) and will differ on other machines; everything else reproduces from the commit in its `run.json`. New crypto runs go to `results/crypto/<run-id>/` and stay ignored.

The M3a sim reference run is committed in [`results/sim/reference/`](results/sim/reference/), with its [`SUMMARY.md`](results/sim/reference/SUMMARY.md). It holds one CSV per table (`V0_…` to `V4_…`, `C1_…` to `C7_…`, `A6_…`, and `ALL_runs.csv`, which lists every run), `validation.csv`, `charts/`, `config.resolved.toml` and `run.json`; its `perf/` timings come from the machine named in `perf/PERF.md`. New sim runs go to `results/sim/<run-id>/` and stay ignored.

Inside each run directory (`results/analytic/<run-id>/`, or `results/analytic/reference/`):

| File | What it holds |
|---|---|
| `SUMMARY.md` | Plain-English summary of every table, written for non-experts |
| `M1_PUBLIC_BRIEF.md` | Short brief for a general technical audience, from exact tables only; a test keeps `docs/M1_PUBLIC_BRIEF.md` identical to it |
| `A1_…` to `A7_….csv` | Section A: time for an attacker to reach 33%, 51% or 67% of active IDs; online fraction; honest growth; adaptive cap with its safety factor; genesis size needed; long-run shares |
| `B1_…` to `B5_….csv` | Section B: probability that an attacker can block, sign alone, or hold every seat of a Witness Chain group; network-level counts; binomial vs hypergeometric; 10-year episode and composition counts under the adopted allocation (B4) and under each seat rule with illustrative absences (B5) |
| `C1_…` to `C3_….csv` | Section C: Chain Allocation Committee stalling and capture under the seat lottery, with episode lengths and entries per year; the v0.2 rule as a comparison; the effect of honest members leaving the committee when deactivated (C3) |
| `D1_…`, `D2_….csv` | Section D: restart attack under the old and current entropy designs, including the realistic restart cost (450 s) |
| `E1_….csv` | Section E: difficulty hopping against the Variant A comparison |
| `F1_….csv` | Section F: same-step ties between identity blocks |
| `G1_…` to `G4_….csv` | Section G: quorum trade-off — stalling, signing alone and conflicting approvals for each quorum option; the split rule; committee quorums; 10-year episodes under each seat rule (G4) |
| `H1_…` to `H3_….csv` | Section H: how often a group cannot meet its quorum when honest members are offline and attacker members withhold; the minimum honest uptime; absent IDs holding seats under each long-absence threshold L |
| `I1_…` to `I4_….csv` | Section I: KWC size trade-off — security, liveness, load per node under three capacity policies, and on-demand witness connections under the proposed peer topology |
| `validation.csv` | Every cross-check: reference value, tolerance, verdict |
| `parameters.csv` | Every SPEC §2 parameter with its value, status tag and sweep |
| `charts/*.png` | Charts drawn from the tables |
| `config.resolved.toml` | The exact configuration used |
| `run.json` | Run id, time, git commit, dirty flag, seed, config hash, toolchain, and a SHA-256 of every file above |

Probabilities can be far smaller than a floating-point number can hold (some committee tails are near 10⁻⁴⁰⁰). The CSV files therefore give them as exact scientific-notation strings, with a `log10` column alongside.

## Reproduce a result

Every run is determined by three things: the git commit, the seed and the configuration. To reproduce one:

1. Check out the commit recorded in its `run.json`.
2. Run `cargo run --release -p gb-cli -- analytic --seed <seed> --config <file>` with the same seed and configuration (`config.resolved.toml` from the run directory works as the configuration file).
3. Compare the `outputs` list in the two `run.json` files (for `gb crypto` and `gb sim`, outside `bench/` and `perf/`).

CSV and Markdown outputs are designed to be byte-identical across machines: exact arithmetic, a pure-Rust maths library and in-repo random samplers remove the usual platform differences. A test checks byte-identity of reruns on the same machine, and an independent rebuild on Linux x86_64 with rustc 1.97.0 reproduced `SUMMARY.md` (apart from its git-commit line) and `M1_PUBLIC_BRIEF.md` byte for byte. PNG charts are byte-identical on the same platform; their identity across platforms is still untested.

## Configuration

Every parameter in SPEC section 2 is a typed parameter with its value, its SPEC status tag (`D` decided, `P` proposed, `O` open) and its sweep range. The defaults live in [`configs/default.toml`](configs/default.toml); a test fails if that file and the built-in defaults ever disagree, and another test fails if a SPEC §2 row has no matching parameter.

```sh
cargo run --release -p gb-cli -- params             # every parameter, as a Markdown table
cargo run --release -p gb-cli -- params --format csv
cargo run --release -p gb-cli -- config dump        # the resolved configuration as TOML
```

To change values, write a small TOML file with only the keys you change and pass it with `--config`. For example:

```toml
[genesis]
ids.value = 5000000

[analytic.cac]
committee_sizes = [600]
```

Unknown keys are rejected. A file cannot change a status tag, because status tags belong to the SPEC.

## Repository layout

| Path | Contents |
|---|---|
| `docs/SPEC.md` | Stage 1 specification (source of truth), with its changelog |
| `docs/ARCHITECTURE.md` | Two-tier design, planned crates, validation ladder, rationale of the adopted allocation |
| `docs/ASSUMPTIONS.md` | Every modelling assumption (M1, M2, M3a) with its source, and open questions |
| `LICENSE` | Apache License 2.0 |
| `docs/M1_PUBLIC_BRIEF.md` | Public brief on the M1 results (generated; see above) |
| `docs/M3_PUBLIC_BRIEF.md` | Public brief on the M3a and M3a.1 simulator results |
| `docs/PROTOCOL_OVERVIEW.md` | One-page overview of the protocol, with each rule's status and the calculated witness-node requirements |
| `configs/default.toml` | Default configuration |
| `crates/gb-config` | Typed configuration and the SPEC §2 parameter registry |
| `crates/gb-runlog` | Run provenance: run ids, git commit, seed, output checksums, seeded random streams |
| `crates/gb-analytic` | M1 analytical baseline: exact formulas, exact probabilities, cross-checks |
| `crates/gb-protocol` | M2 protocol core: header encoding, entropy and BLS, hash chain, validation, difficulty and issuance, allocation, committee lottery; test vectors in `vectors/` |
| `crates/gb-crypto-tests` | M2 measurements with real cryptography, and criterion benchmarks in `benches/` |
| `crates/gb-sim` | M3a Tier 1 discrete-event simulator: engine, network and household models, rounds and fork choice, the experiments and their checks |
| `crates/gb-cli` | The `gb` command-line tool: tables, charts, summary |
| `data/latency/` | Inter-region round-trip times from RIPE Atlas, the simulator's latency input, with their provenance |
| `tools/` | `ripe_atlas_latency.py`, which rebuilds `data/latency/` from public RIPE Atlas data (Python standard library only) |
| `results/` | Generated outputs; only the reference runs in `results/analytic/reference/`, `results/crypto/reference/` and `results/sim/reference/` are committed |

## Licence

Licensed under the Apache License 2.0; see [`LICENSE`](LICENSE). The bundled DejaVu Sans font (`crates/gb-cli/assets/fonts/`) keeps its own licence, included next to it.
