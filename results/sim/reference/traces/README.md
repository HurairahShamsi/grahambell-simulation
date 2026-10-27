# E3 blackout traces

Worst-case traces of E3's scripted blackouts at the final M3a.1 defaults (threshold entropy with t = ⌈2n/3⌉, no failover, no PoWit deadline, early blocks re-evaluated; SPEC v0.7): the three longest pauses between final-chain blocks during a blackout, out of E3's 41 blackout runs (the day-9.19 event and one blackout of every region over 5 seeds; 10,000 miner slots, 29,000 IDs; each blackout lasts 10.88 h). Each file covers an hour before its blackout began to an hour after its pause ended.

They come from the supplementary section W2, which reruns E3's blackout runs with a trace that only observes: the runs are the reference run's own (`E3_blackout_test.csv` and `E3_blackout_gaps.csv` in the folder above), under other names, and match them on every column of `ALL_runs.csv`.

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
| `tip_pool`, `tip_pool_able` | The round on the network tip: slots not yet drawn, and those of them whose miner could mine |
| `tip_skipped`, `tip_skipped_able` | The same round's slots skipped since its last refresh because their miner could not mine, and those of them whose miner could mine again (held only by the simulator's skip rule, ASSUMPTIONS SIM-48) |

`E3_pause<k>_skipped.csv`, one row per skipped slot of the pause's round whose miner could mine again during the pause: when it was skipped and why, when its miner could mine again (the first 30-second sample that found it so), when it left the skipped state and how (drawn again at a refresh, dropped because a block at its height was held on time, or the pause's own block opening the next round), and the time it was held during the pause.

## The three pauses

In a deliberate stress test, an entire region holding about 38% of witnesses loses power for about 11 hours. Issuance pauses at the blackout's start while difficulty catches up with the miners who remain: across 100 simulated blackouts, the median pause was about 7.6 minutes, over 10 minutes in 41%, over 30 minutes in 6%, and the longest 49 minutes. Block production then resumes automatically at ~30 s on average; in about a third of the blackouts a second gap of over 10 minutes followed later in the blackout (longest 39 minutes). This is protocol behaviour, not a simulator artefact. (The 100 blackouts are the supplementary section W3, in [`../../supplement-w3/`](../../supplement-w3/); the three pauses below are the longest of E3's 41.)

| Pause | Run | Start, from the blackout's start | Length | Round's count | Miners able | Expected wait | Skip rule |
|---|---|---|---|---|---|---|---|
| 1 | blackout of as_east, seed 0 | −5 s | 2,130 s (35.5 min) | 9,311 | 464–538 | 531 s | 0 % (3 slots, 0.004 expected blocks) |
| 2 | blackout of as_east, seed 3 | −1 s | 1,708 s (28.5 min) | 9,223 | 599–690 | 431 s | 0 % (1 slot, 0.004 expected blocks) |
| 3 | blackout of as_east, seed 2 | −17 s | 1,227 s (20.5 min) | 9,149 | 536–583 | 527 s | 0 % (none) |

In detail, all three are the difficulty lag: the round in progress when the blackout began keeps the difficulty set from the miners counted before it, while only about 5–7 % of them can still get entropy, and every sample is "difficulty too high". They lasted 2.3 to 4 times their expected wait, mostly by chance (in the longest, about two draws reached able miners where about four were expected), and the skip rule accounts for none of it. An isolation run (W3; ASSUMPTIONS SIM-52) found that the PoWit deadline does not affect these pauses and that the default early-block rule, re-evaluation, lengthens the typical one modestly; over 100 seeds of this blackout, a pause over 30 minutes came in 4–7 % of runs under every rule.

## Regenerating them

Check out commit 82cfb13 (or a later one that leaves E3, W2 and the defaults unchanged) and run

```sh
cargo run --release -p gb-cli -- sim --only W2
```

It runs the V0 gate and the 41 traced runs (about 10 minutes with 4 threads on a laptop) and writes the same nine files to `results/sim/<run-id>/traces/`, byte for byte: their SHA-256 hashes are below. These copies come from the supplementary run `gb sim --only W1,W2` at 82cfb13, committed in [`../../supplement/`](../../supplement/), whose `run.json` lists the same hashes; `W2_pauses.csv` and its `SUMMARY.md` (section W2) summarise each pause.

## Files and hashes

| File | Rows | Bytes | SHA-256 |
|---|---|---|---|
| `E3_pause1_blocks.csv` | 261 | 28,414 | `5de50c9c5d7f1656fcb1cca1a04109fba17a8c602be7b037b28832a94172409e` |
| `E3_pause1_samples.csv` | 190 | 17,134 | `bd1ddd25f5bb415324826bf4fde7f7896db5c806257c0ddf8d896450dfbbcbc6` |
| `E3_pause1_skipped.csv` | 3 | 450 | `010a6964195886f85c161ad378013c96dcce06388aa471291f283f581f9c8858` |
| `E3_pause2_blocks.csv` | 259 | 28,511 | `e6a227635b81e4c7881ad89244a2224397a8fcf54c48405e4fad37499ea89786` |
| `E3_pause2_samples.csv` | 176 | 15,964 | `de013c77b4fb5526052b57dc541f47fa363d3108ce972d5b4f9e5466ab180fe5` |
| `E3_pause2_skipped.csv` | 1 | 236 | `53007403e22207d33a4d28daa43944675ea742a0b5a8587790a1e3ec4df757df` |
| `E3_pause3_blocks.csv` | 252 | 27,593 | `419e5a4eaf04f48411eb98afed80a84f7e0d252d59916bef42b1a00f4a4d5ace` |
| `E3_pause3_samples.csv` | 160 | 14,306 | `4bf67f292de4b854bffe62ef291d80f3b2236954d40f7ddb7331ecb1f46a34c2` |
| `E3_pause3_skipped.csv` | 0 | 128 | `9d078859fc2f9a9bd5f6b932ba4d71aac835f3362b7d005a9294398bf4182f97` |
