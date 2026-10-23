# E3 blackout traces

Worst-case traces of E3's scripted blackouts at the final M3a.1 defaults (threshold entropy with t = ⌈2n/3⌉, no failover; SPEC v0.7): the three longest pauses between final-chain blocks during a blackout, out of E3's 41 blackout runs (the day-9.19 event and one blackout of every region over 5 seeds; 10,000 miner slots, 29,000 IDs; each blackout lasts 10.88 h). Each file covers an hour before its blackout began to an hour after its pause ended.

They come from the supplementary section W2, which reruns E3's blackout runs with a trace that only observes: the runs are the reference run's own (`E3_blackout_test.csv` and `E3_blackout_gaps.csv` in the folder above), under other names.

## Files

`E3_pause<k>_blocks.csv`, one row per final-chain block:

| Column | Meaning |
|---|---|
| `height`, `timestamp_s` | Height and timestamp (`t0 + N`, simulated seconds) |
| `broadcast_s` | When the block was broadcast, simulated seconds |
| `since_blackout_start_s` | The same, from the blackout's start |
| `gap_s` | Seconds since the previous final-chain block was broadcast |
| `count` | The difficulty count the block's target was set from: the issued-entropy count of its parent's round, taken when the parent arrived (SPEC §3.8) |
| `p_step` | That target's per-step success probability |
| `factor` | That target's P2 correction factor |
| `kwcs`, `kwcs_able` | Active KWCs, and those able to issue entropy, when the block was broadcast |
| `miners_active`, `miners_able` | Miners mining with their household online, and those whose KWC could issue entropy, when the block was broadcast |

`E3_pause<k>_samples.csv`, one row per round (30 s) from the blackout's start, as E3's diagnosis samples it:

| Column | Meaning |
|---|---|
| `time_s`, `since_blackout_start_s` | Simulated seconds, and from the blackout's start |
| `in_pause` | 1 inside the pause |
| `miners_active`, `miners_able` | As above, at that moment |
| `p_step` | The per-step success probability of the network tip's difficulty |
| `expected_wait_s` | The expected wait for a block with `miners_able` miners at `p_step` (empty when no miner holds entropy) |
| `state` | E3's diagnosis: no miners, no entropy, difficulty too high (the expected wait is over four target intervals), or other |

`miners_able` counts the miners whose KWC can issue entropy at that moment. A miner whose KWC could not issue when its round opened was skipped, and mines again when the round runs out of draws or at the next round (ASSUMPTIONS SIM-48).

## Regenerating them

Check out commit 8cb4a44 (or a later one that leaves E3, W2 and the defaults unchanged) and run

```sh
cargo run --release -p gb-cli -- sim --only W2
```

It runs the V0 gate and the 41 traced runs (about 9 minutes with 4 threads on a laptop) and writes the same six files to `results/sim/<run-id>/traces/`, byte for byte: their SHA-256 hashes are below. These copies come from the supplementary run `gb sim --only W1,W2` at 8cb4a44, committed in [`../../supplement/`](../../supplement/), whose `run.json` lists the same hashes; `gb sim --only W2` alone gave the same files. Its `W2_pauses.csv` and `SUMMARY.md` (section W2) summarise each pause.

## The three pauses

| Pause | Run | Start, from the blackout's start | Length | What held it up |
|---|---|---|---|---|
| 1 | blackout of as_east, seed 2 | −19 s (the round began just before it) | 782 s (13.0 min) | The difficulty lag: the round's difficulty counted 9,143 miners while 521–564 could get entropy, an expected wait of 514 s; every sample "difficulty too high" |
| 2 | the day-9.19 event (as_east) | +19,877 s (5.5 h; about 23:15 UTC) | 725 s (12.1 min) | Entropy: the count had fallen to 1 miner, but 0–3 miners could get entropy, none in 50 % of samples |
| 3 | blackout of as_east, seed 0 | +38,031 s (10.6 h; about 22:34 UTC) | 678 s (11.3 min) | Entropy: count 1–2, 0–4 miners able to get entropy, none in 61 % of samples |

In pauses 2 and 3 the samples with 1–3 miners able to get entropy imply an 18-second wait at the tip's difficulty, so the miners able then were not mining in that round: a slot skipped when its round opened mines again only when the round runs out of draws or at the next round (ASSUMPTIONS SIM-48, SIM-52). That step is an inference from the simulator's rules, not traced directly.

## Files and hashes

| File | Rows | Bytes | SHA-256 |
|---|---|---|---|
| `E3_pause1_blocks.csv` | 256 | 28,448 | `cb719eed69a2a30d7b55d85d87126e79e2fc56b20ce7d1238d1f2774fe4176d8` |
| `E3_pause1_samples.csv` | 145 | 10,639 | `1c08f430c1b270bd7f37b27777b88842f1ef7c93009a985f2055ff341a059cba` |
| `E3_pause2_blocks.csv` | 793 | 81,270 | `4da3496a515a7ef456eccc1db8c29e94fb786c5c5b7f7017f31d2604ae91f71e` |
| `E3_pause2_samples.csv` | 806 | 57,954 | `f7c4614936d1cf58c02f2aeda794fb37357666c79a38ae25c6f846cc82524bde` |
| `E3_pause3_blocks.csv` | 1517 | 161,626 | `040607d50f8d3a64ab9b36813f28b6f717895e61dc61e358d8e731fbe1f5dbb8` |
| `E3_pause3_samples.csv` | 1410 | 99,212 | `ba91f1163cc5025a0d87b6e0ba768b1f109bce943bacc56fb8027a006977c6a2` |
