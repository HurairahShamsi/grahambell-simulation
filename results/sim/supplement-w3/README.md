# W3: the pause at a blackout's start, over 100 seeds

The supplementary section W3 of `gb sim`, not part of the reference run (architect, 2026-10-27). It repeats E3's blackout of the largest region, as_east (about 38 % of witness nodes; 10,000 miner slots and 29,000 IDs; a 10.88-hour blackout from day 1.5, 12:00 UTC), over 100 seeds. Seeds 0–4 are E3's own blackouts of as_east, so they reproduce the reference run's E3 runs. Every seed runs under each setting:

| Setting | Seeds |
|---|---|
| Re-evaluation, no deadline (the configured defaults) | 100 |
| Re-evaluation, D = 10 s (soft scope) | 100 |
| Permanent rejection, no deadline | 100 |
| Permanent rejection, D = 10 s | 100 |
| No badly synchronised clocks, re-evaluation | 100 |
| No badly synchronised clocks, permanent rejection | 20 |

"No badly synchronised clocks" sets right the 1 % of nodes whose clocks have σ = 2 s. No node then receives a block early, so the two early-block rules run alike; that is the control. `SUMMARY.md` (section W3) gives the results.

The public figures on these pauses (`docs/PROTOCOL_OVERVIEW.md`, `results/sim/reference/traces/README.md`) are the first row, the configured defaults: across 100 blackouts the median pause was 457 s (7.6 minutes), 41 % lasted over 10 minutes, 6 % over 30 minutes, and the longest 2,947 s (49.1 minutes). After the pause, the next 20 blocks came 30.6 s apart and the rest of the blackout's blocks 30.5 s apart (medians over the seeds). Separately, 35 of the 100 blackouts had a further gap over 10 minutes later in the blackout (column `longest_later_gap_s`; longest 2,364 s).

## Files

- `W3_blackout_seeds.csv`, one row per run:
  - the pause at the blackout's start: the gap between the last final-chain block broadcast before the blackout began and the first after it, and when it began;
  - the round's difficulty count and per-step success probability;
  - the mean number of miners able to get entropy during the pause, the expected wait at that difficulty, and the ratio of the pause to it;
  - how blocks came after it: the next 20 blocks' mean interval, the rest of the blackout's, and the longest later gap.
- `W3_blackout_cells.csv`, one row per setting:
  - the median, mean, 90th percentile and longest pause, and the shares over 10, 20 and 30 minutes;
  - the ratio to the expected wait against an exponential, where at least 100 miners could get entropy (difficulty lag plus chance alone gives an exponential, half of it below ln 2);
  - the intervals after the pause.
- `W3_comparisons.csv`:
  - seeds with the same pause with and without D = 10 s;
  - seeds with the same pause under both rules with no badly synchronised clocks;
  - two-sided Mann–Whitney rank tests between settings.
- `SUMMARY.md`, `validation.csv`, `ALL_runs.csv`, `run.json`, `config.resolved.toml` and `perf/`, as for every `gb sim` run.

## Reproducing it

At commit 44b0f8c, or a later one that leaves E3, W3 and the defaults unchanged:

```sh
cargo run --release -p gb-cli -- sim --only W3
```

It runs the V0 gate and the 520 runs, which take about 55 minutes with 6 threads on a laptop. It writes the same tables to `results/sim/<run-id>/`, byte for byte, apart from `perf/`; `run.json` lists their SHA-256 hashes.

The four-setting check on all 41 of E3's blackouts (ASSUMPTIONS SIM-52) is W2 under each setting: `gb sim --only W2 --config FILE`, where FILE overrides the two rules. For D = 10 s with permanent rejection, for example:

```toml
[timing]
powit_deadline_s = { value = 10.0 }
early_block = { value = "permanent" }
```
