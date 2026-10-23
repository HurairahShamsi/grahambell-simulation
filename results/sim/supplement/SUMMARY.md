# M3a Tier 1 issuance simulator — summary

- Git commit: `8cb4a44fd325ac5fffbb5fd28ab764af161387ce` (clean working tree)
- Seed: 20261005
- Configuration SHA-256: `5109f41b81744e64f9ae6678444041f02c50c8c4c2f30460e87b00f0c60d7b59`
- Protocol version: 1
- Profile: full
- Sections: V0, W1, W2
- Runs: 87

Every run is seeded and single-threaded, so these tables are reproducible bit for bit whatever the thread count; wall times are machine-dependent and are in `perf/`. Every protocol decision goes through `gb-protocol` (SPEC v0.7). Values marked as checks are compared with M1's closed forms or with exact expectations at 5 standard errors (`validation.csv`); every other number is a measurement, reported as it came out.

## Validation

**39 of 40 checks pass, 1 fail and 0 are reference-limited.** The V0 gate passed, so the stand-in and fast modes are used for the other sections. A reference-limited check is kept and reported, but its reference does not model what the simulator does, so it is not a failure either way (ASSUMPTIONS SIM-31).
- **failed** `ALL-deep-reorgs`: reorgs of the network chain below a confirmed ID (87 runs) (reference 0, value 1)

## V0 — mode agreement (the gate)

The three crypto modes agree, and the fast sampler is exact, when every test below has p ≥ 0.001. **37 of 37 gate checks pass.** Details in `V0_mode_agreement.csv` and `validation.csv`.

## W1 — where the withholder sits (supplementary)

Not part of the reference run (architect, 2026-10-23); run with `gb sim --only W1`. E1's two finalists and C3's withholder runs at the final defaults, with the 10 % withholder placed as in the reference run, spread over the regions in proportion to their miners, or all in oc, the smallest region (0.6 % of miners) and among the farthest from the network's miners (median round trip 281 ms; the farthest, sa_east, 286 ms; as_east 176 ms). The runs placed as in the reference are the reference run's E1 and C3 withholder runs under other names. Withholder amplification, with and without the PoWit deadline (`W1_withholder_placement.csv`; the withholder's slot time by region in `W1_withholder_regions.csv`):

- E1's finalists, (r) first seen literal, 5 seeds each: as in the reference (as_east 94 %, oc 6 %): D = 10 s soft 0.001 (SE 0.000), no deadline 0.154 (SE 0.003); spread over the regions (as_east 38 %, as_south 22 %, eu_west 12 %, af 11 %, na_east 7 %, sa_east 6 %, na_west 3 %, oc 1 %): D = 10 s soft 0.000 (SE 0.000), no deadline 0.154 (SE 0.003); all in one small, distant region (oc 100 %): D = 10 s soft 0.000 (SE 0.000), no deadline 0.144 (SE 0.003)
- C3, one seed: as in the reference (as_east 94 %, oc 6 %): D = 10 s soft 0.001 (SE 0.001), no deadline 0.152 (SE 0.007); spread over the regions (as_east 38 %, as_south 22 %, eu_west 12 %, af 11 %, na_east 7 %, sa_east 6 %, na_west 3 %, oc 1 %): D = 10 s soft 0.001 (SE 0.001), no deadline 0.136 (SE 0.007); all in one small, distant region (oc 100 %): D = 10 s soft 0.000 (SE 0.000), no deadline 0.143 (SE 0.007)

## W2 — E3's blackout traces (supplementary)

Not part of the reference run (architect, 2026-10-23); run with `gb sim --only W2`. E3's 41 scripted blackout runs under the configured entropy rule and failover, traced: the reference run's runs under other names (the trace only observes). For each of the 3 longest pauses between final-chain blocks during a blackout, every final-chain block (`traces/E3_pause<k>_blocks.csv`: its difficulty count, per-step success probability and correction factor, and the KWCs and miners able to issue and get entropy when it was broadcast) and every round's sample (`traces/E3_pause<k>_samples.csv`, from the blackout's start: miners mining and those able to get entropy, the tip's difficulty, the expected wait, and E3's diagnosis of what held it up) from an hour before the blackout to an hour after the pause; the pauses in `W2_pauses.csv`. A round's difficulty is set from the count taken when the previous round's block arrived (SPEC §3.8), so a round open when a blackout begins keeps the difficulty of the miners counted before it. Miners able to get entropy are those whose KWC can issue it now; a miner skipped when its round opened, because its KWC could not issue then, mines again when the round runs out of draws or at the next round (SIM-48), so "other" includes waits for those miners.

1. `E3-blackout-r6-s2-threshold 2n/3, no failover-W2` (as_east): a pause of 782 s that began 19 s before the blackout. The round's difficulty was set from a count of 9143 miners (the block before the pause had 9143 miners able to get entropy); during the pause 521–564 miners could get entropy (mean 543), so that difficulty implied an expected wait of 514 s (17.1 target intervals) and the pause lasted 1.52 times that. The block that ended it counted 559 miners, and the next 10 blocks came 35.5 s apart on average. Samples during the pause (25): no miners 0 %, no entropy 0 %, difficulty too high 100 %, other 0 % (most: difficulty too high).
2. `E3-919-threshold 2n/3, no failover-W2` (as_east): a pause of 725 s from 19877 s after the blackout began. The round's difficulty was set from a count of 1 miners (the block before the pause had 1 miners able to get entropy); during the pause 0–3 miners could get entropy (mean 1), so that difficulty implied an expected wait of 18 s (0.6 target intervals) and the pause lasted 40.30 times that. The block that ended it counted 3 miners, and the next 10 blocks came 51.3 s apart on average. Samples during the pause (24): no miners 0 %, no entropy 50 %, difficulty too high 0 %, other 50 % (most: other).
3. `E3-blackout-r6-s0-threshold 2n/3, no failover-W2` (as_east): a pause of 678 s from 38031 s after the blackout began. The round's difficulty was set from a count of 1 miners (the block before the pause had 1 miners able to get entropy); during the pause 0–4 miners could get entropy (mean 1), so that difficulty implied an expected wait of 18 s (0.6 target intervals) and the pause lasted 37.23 times that. The block that ended it counted 4 miners, and the next 10 blocks came 30.4 s apart on average. Samples during the pause (23): no miners 0 %, no entropy 61 %, difficulty too high 0 %, other 39 % (most: no entropy).

## Invariants of every run

**2 of 3 hold** across all 87 runs: recounted caches and counters, self-validation of every broadcast block, and no reorg below a confirmed ID (`validation.csv`, section ALL; every run in `ALL_runs.csv`). Reorgs below a confirmed ID (a confirmed ID's block replaced) occurred in 1: `E1-(r) first_seen literal D10-soft-withholder-s0-W1-oc` (1, deepest 7).

No run had a fork storm: at most 110 unsettled blocks ever shared a height.

**The skip rule (SIM-48).** 11 rounds ran out of draws without a block anyone extends and drew their skipped slots again, in 2 runs (`ALL_runs.csv`, column `round_refreshes`). Before their round's first block, 3535073 drawn slots were skipped because their miner could not mine (2300101 not mining, 0 offline, 1234972 with a KWC unable to issue entropy); 46079 of them could mine again by that block. Had they been drawn again once able, they could have added at most 20254.0 wins over 1600015 rounds, 12.659 per 1,000 rounds: an upper bound on what the rule took from issuance (at most 433.686 in one run, `E3-blackout-r6-s4-threshold 2n/3, no failover-W2`). By section, per 1,000 rounds: V0 0.001, W1 13.557, W2 11.789.

