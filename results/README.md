# results/

Generated outputs. Everything in this folder is ignored by git except this README and the reference runs. The M1 **reference run** is in [`analytic/reference/`](analytic/reference/). The reference run is committed so that the repository contains the tables, charts, summary and cross-check verdicts behind the public brief; start with its [`SUMMARY.md`](analytic/reference/SUMMARY.md). Its `run.json` records the commit it was produced from and a SHA-256 of every file. New runs stay ignored. Regenerate outputs with the `gb` tool rather than editing them.

Each analysis writes one directory per run, `results/<analysis>/<run-id>/`. The run id is `YYYYMMDDTHHMMSSZ_<commit>_s<seed>`. Each run directory contains:

| File | Contents |
|---|---|
| `*.csv` | Tables, one per analysis output |
| `charts/*.png` | Charts drawn from those tables |
| `SUMMARY.md` | Plain-English summary of the tables |
| `validation.csv` | Every cross-check, with its reference value, tolerance and verdict |
| `parameters.csv` | Every SPEC §2 parameter with its value, status and sweep |
| `config.resolved.toml` | The exact configuration used |
| `run.json` | Run id, time, git commit, dirty flag, seed, config hash, toolchain, and a SHA-256 for every file above |

The M2 **crypto reference run** in [`crypto/reference/`](crypto/reference/) is committed the same way. Its `bench/` timings were measured on the machine named in `crypto/reference/bench/BENCH.md` (CPU, logical cores and rustc version) and will differ elsewhere; everything outside `bench/` reproduces from the commit in its `run.json`. New crypto runs stay ignored.

The M3a.1 **sim reference run** in [`sim/reference/`](sim/reference/), at the final SPEC v0.7 defaults (run at 7cf4ff2 and re-analysed at fdf6e2e from its checkpoints), is committed the same way; start with its [`SUMMARY.md`](sim/reference/SUMMARY.md). Besides its tables, `validation.csv`, `charts/` and `run.json`, it has `ALL_runs.csv`, which lists every simulation run with its seed, size, blocks, invariant counts, the most blocks it had unsettled at once and the day a fork storm stopped it, if one did. Its `perf/` timings were measured on the machine named in `sim/reference/perf/PERF.md` and will differ elsewhere; everything outside `perf/` reproduces from the commit in its `run.json`, whatever the thread count. New sim runs stay ignored. Its `traces/` folder holds traces of E3's three longest blackout pauses, from the supplementary run below (their README says how to regenerate them). The supplementary run W1 + W2 (`gb sim --only W1,W2`, from 82cfb13: the withholder placed three ways, and E3's blackout runs traced) is committed in [`sim/supplement/`](sim/supplement/) the same way; it is not part of the reference. The reference replaced the 1d9ce91 run (in the history at 8cb4a44), the 0b711f5 run (before the final defaults; at 99943fd) and, before it, the M3a reference, which predated two bugs found in M3a.1 (a region-wide feeder group and provider, and entropy issued below the quorum; [ASSUMPTIONS SIM-46](../docs/ASSUMPTIONS.md) lists the numbers they changed); the M3a run stays in the history at 90683ab.

To check that a rerun reproduces a run, compare the `sha256` fields in the two `run.json` files. To check the reference run, check out the commit in `analytic/reference/run.json`, run `cargo run --release -p gb-cli -- analytic`, and compare the new run's `run.json` with the reference one.
