# M3a Tier 1 issuance simulator — summary

- Git commit: `b6688fb13a50e7212532793e8d72de37fd6b5a14` (clean working tree)
- Seed: 20261005
- Configuration SHA-256: `cf1ca7be38157bb5d06194284d44e6fa905da44c693f890577c723fe54bc1fad`
- Protocol version: 1
- Profile: full
- Sections: all
- Runs: 1011

Every run is seeded and single-threaded, so these tables are reproducible bit for bit whatever the thread count; wall times are machine-dependent and are in `perf/`. Every protocol decision goes through `gb-protocol` (SPEC v0.6 and the architect's M3a decisions of 2026-10-12). Values marked as checks are compared with M1's closed forms or with exact expectations at 5 standard errors (`validation.csv`); every other number is a measurement, reported as it came out.

## Validation

**153 of 166 checks passed.** The V0 gate passed, so the stand-in and fast modes are used for the other sections.
- **failed** `V2-old-w1299`: old design: restart advantage at keep window 1299 s vs M1 advantage_old_design(q, C_realised, W), allowance 1 % for tie losses (reference 2.699751580323843, value 2.078719146097398)
- **failed** `V2-old-w3600`: old design: restart advantage at keep window 3600 s vs M1 advantage_old_design(q, C_realised, W), allowance 1 % for tie losses (reference 5.294821431355288, value 3.4042074684908403)
- **failed** `V2-old-w5196`: old design: restart advantage at keep window 5196 s vs M1 advantage_old_design(q, C_realised, W), allowance 1 % for tie losses (reference 5.682876871076375, value 3.6796881804720845)
- **failed** `V2-old-w20785`: old design: restart advantage at keep window 20785 s vs M1 advantage_old_design(q, C_realised, W), allowance 1 % for tie losses (reference 3.0136551434864027, value 2.4198717948717947)
- **failed** `V3-A-K144-clamp4-m4`: Variant A hopping gain (K = 144) vs the discrete equivalent of M1 E1 (1 − (29/30)^(1+m)) / q_B; M1's continuous value 5.000 (reference 4.7559712362459114, value 6.264876534860303)
- **failed** `V3-A-K144-clampnone-m4`: Variant A hopping gain (K = 144) vs the discrete equivalent of M1 E1 (1 − (29/30)^(1+m)) / q_B; M1's continuous value 5.000 (reference 4.7559712362459114, value 5.574338776849414)
- **failed** `V3-A-K144-clampnone-m4-recovery`: Variant A recovery window (hours) vs the discrete first-order value, allowance K/(K − 1) − 1 when unclamped (reference 5.539129844736366, value 6.725939450686641)
- **failed** `V3-A-K2016-clamp4-m4`: Variant A hopping gain (K = 2016) vs the discrete equivalent of M1 E1 (1 − (29/30)^(1+m)) / q_B; M1's continuous value 5.000 (reference 4.7559712362459114, value 10.997565531626387)
- **failed** `V3-A-K2016-clamp4-m4-recovery`: Variant A recovery window (hours) vs the discrete first-order value, allowance K/(K − 1) − 1 when unclamped (reference 66.35490735004353, value 47.726527777777775)
- **failed** `V3-A-K2016-clampnone-m4`: Variant A hopping gain (K = 2016) vs the discrete equivalent of M1 E1 (1 − (29/30)^(1+m)) / q_B; M1's continuous value 5.000 (reference 4.7559712362459114, value 7.7464737986509915)
- **failed** `V3-A-K2016-clampnone-m4-recovery`: Variant A recovery window (hours) vs the discrete first-order value, allowance K/(K − 1) − 1 when unclamped (reference 77.54781782630913, value 125.60052083333333)
- **failed** `C2-idealised-orphans`: zero latency: orphans per final-chain block vs E[tied winners − 1] from M1 F1 (1000 slots) (reference 0.016742592328524587, value 0.009261533064374671)
- **failed** `ALL-deep-reorgs`: reorgs of the network chain below a confirmed ID (1011 runs) (reference 0, value 154)

## V0 — mode agreement (the gate)

The three crypto modes agree, and the fast sampler is exact, when every test below has p ≥ 0.001. **34 of 34 gate checks pass.** Details in `V0_mode_agreement.csv` and `validation.csv`.

## V4 — M2 test vectors

The simulator's real-mode functions rebuild every derived field of `protocol_v1.json` (keys, encodings, digests, signatures, chain steps, verdicts, positions, targets): **60 of 60 fields match** (`V4_test_vectors.csv`).

## V1 — M1 A1/A7/F1 in the idealised simulator

Blocks to 51 % of active IDs, the long-run share and the tie rate are compared with M1's closed forms at G = 2000 (`V1_time_to_threshold.csv`). The mean interval with an exact count is **30.496 s**, not 30 s: blocks start one second after their parent (start rule (a)) and each step is a whole second, so the mean interval is 1/(1 − (1 − p)^r) = 30.502 s for p = 1/(30r). This is the discrete-step bias that B-exact removes (C4).

## V2 — S5 restart attack

Under the old fixed-entropy design the restart advantage matches M1 section D at the simulator's size (q = 1/30000, C ≈ 451.0 s realised through the admission path). Under per-round entropy the restart strategy gains nothing: its largest advantage is **0.092** (H4 asks ≤ 1.02; reported, not a check). `V2_restart_advantage.csv`.

## V3 — S6 difficulty hopping

Variant A reproduces M1 E1 once both use whole-second steps (`V3_difficulty_hopping.csv`). Variant B and its correction candidates (H5 input; reported, not a check):

- B m=0.1: gain 0.9918 ± 0.0382
- B m=0.5: gain 0.9989 ± 0.0200
- B m=1: gain 0.9995 ± 0.0163
- B m=4: gain 1.0101 ± 0.0156
- B+P1 m=0.1: gain 0.9813 ± 0.0379
- B+P1 m=0.5: gain 1.0105 ± 0.0202
- B+P1 m=1: gain 1.0075 ± 0.0164
- B+P1 m=4: gain 1.0427 ± 0.0159
- B+P2 m=0.1: gain 1.0048 ± 0.0384
- B+P2 m=0.5: gain 1.0258 ± 0.0203
- B+P2 m=1: gain 1.0243 ± 0.0167
- B+P2 m=4: gain 1.0880 ± 0.0163
- B-exact m=0.1: gain 1.0225 ± 0.0386
- B-exact m=0.5: gain 1.0102 ± 0.0201
- B-exact m=1: gain 1.0296 ± 0.0166
- B-exact m=4: gain 1.0159 ± 0.0158
- B+P1, one idle-but-counted window then one active window: gain 1.0786 ± 0.0192
- B+P2, one idle-but-counted window then one active window: gain 1.0882 ± 0.0196

## C1 — S1 honest baseline (H1 control)

A tagged group of honest slots wins IDs in proportion to its mining slot-time (`C1_amplification.csv`, by region in `C1_amplification_by_region.csv`):

- 1000 miners, literal, x = 0.1: amplification 0.9752 (95 % CI 0.9586–0.9918)
- 1000 miners, literal, x = 0.5: amplification 1.0051 (95 % CI 0.9983–1.0119)
- 10000 miners, literal, x = 0.1: amplification 0.9763 (95 % CI 0.9597–0.9929)
- 10000 miners, literal, x = 0.5: amplification 1.0102 (95 % CI 1.0021–1.0183)
- 100000 miners, literal, x = 0.1: amplification 0.9715 (95 % CI 0.9545–0.9884)
- 100000 miners, literal, x = 0.5: amplification 1.0036 (95 % CI 0.9958–1.0114)
- 10000 miners, same-parent [P], x = 0.1: amplification 0.9844 (95 % CI 0.9677–1.0010)
- 10000 miners, same-parent [P], x = 0.5: amplification 1.0054 (95 % CI 0.9978–1.0129)

## C2 — fork choice under latency

Orphan and fork rates for the v0.6 rule, first-seen and lower-hash-only (`C2_fork_choice.csv`; no loss below):

- v0.6, literal: orphan rate 2.74 %, fork heights 2.25 %, lower-hash loser 24.0 % of orphans, max reorg 4, deeper than confirmation 0
- v0.6, same-parent: orphan rate 6.29 %, fork heights 2.59 %, lower-hash loser 40.0 % of orphans, max reorg 12, deeper than confirmation 23
- first-seen, literal: orphan rate 3.20 %, fork heights 1.82 %, lower-hash loser 50.7 % of orphans, max reorg 5, deeper than confirmation 0
- first-seen, same-parent: orphan rate 3.20 %, fork heights 1.82 %, lower-hash loser 50.7 % of orphans, max reorg 5, deeper than confirmation 0
- lower-hash-only, literal: orphan rate 13.66 %, fork heights 10.13 %, lower-hash loser 21.1 % of orphans, max reorg 4, deeper than confirmation 0
- lower-hash-only, same-parent: orphan rate 15.34 %, fork heights 10.46 %, lower-hash loser 29.5 % of orphans, max reorg 5, deeper than confirmation 0

## C3 — PoWit arrival deadline and the withholder

With partitions (one per region per day, 5 min) and 0.5 % loss (`C3_powit_deadline.csv`, `C3_late_by_region.csv`, `C3_withholder.csv`; withholder values are measurements, not checks):

- D = none (permanent): 0.000 % of blocks late at the panel majority, fork rate 3.40 % (no deadline: 3.40 %), split panel nodes 0, deepest reorg 4 (0 below a confirmed ID); withholder success 66.9 %, amplification 0.854
- D = 5 (permanent): 1.148 % of blocks late at the panel majority, fork rate 3.06 % (no deadline: 3.40 %), split panel nodes 63, deepest reorg 2 (0 below a confirmed ID); withholder success 7.2 %, amplification 0.087
- D = 5 (not_extended_only): 2.800 % of blocks late at the panel majority, fork rate 2.55 % (no deadline: 3.40 %), split panel nodes 0, deepest reorg 1 (0 below a confirmed ID); withholder success 6.8 %, amplification 0.084
- D = 10 (permanent): 0.975 % of blocks late at the panel majority, fork rate 3.14 % (no deadline: 3.40 %), split panel nodes 63, deepest reorg 1 (0 below a confirmed ID); withholder success 15.4 %, amplification 0.192
- D = 10 (not_extended_only): 3.925 % of blocks late at the panel majority, fork rate 2.63 % (no deadline: 3.40 %), split panel nodes 0, deepest reorg 2 (0 below a confirmed ID); withholder success 17.4 %, amplification 0.214
- D = 30 (permanent): 1.465 % of blocks late at the panel majority, fork rate 2.97 % (no deadline: 3.40 %), split panel nodes 63, deepest reorg 1 (0 below a confirmed ID); withholder success 37.9 %, amplification 0.506
- D = 30 (not_extended_only): 2.642 % of blocks late at the panel majority, fork rate 2.63 % (no deadline: 3.40 %), split panel nodes 0, deepest reorg 1 (0 below a confirmed ID); withholder success 37.8 %, amplification 0.473
- D = 60 (permanent): 0.734 % of blocks late at the panel majority, fork rate 2.78 % (no deadline: 3.40 %), split panel nodes 63, deepest reorg 1 (0 below a confirmed ID); withholder success 50.6 %, amplification 0.728
- D = 60 (not_extended_only): 2.093 % of blocks late at the panel majority, fork rate 2.68 % (no deadline: 3.40 %), split panel nodes 0, deepest reorg 2 (0 below a confirmed ID); withholder success 53.7 %, amplification 0.663

## C4 — issuance rate under churn

Mean interval and stability per difficulty rule (`C4_issuance_rate.csv`; realistic churn, count after the post-admission wait):

- A (K=144, 4×): mean interval 32.50 s (8.33 % from 30 s), daily IDs 2658 ± 191 (SD)
- B: mean interval 39.54 s (31.79 % from 30 s), daily IDs 2185 ± 236 (SD)
- B+P1: mean interval 36.80 s (22.68 % from 30 s), daily IDs 2347 ± 196 (SD)
- B+P2: mean interval 36.88 s (22.92 % from 30 s), daily IDs 2340 ± 184 (SD)
- B-exact: mean interval 36.99 s (23.30 % from 30 s), daily IDs 2335 ± 201 (SD)

## C5 — timing the SPEC promises, and honest penalties

KWC switches take 451.0 s at the median ("a few minutes"). Entropy stalls and penalties by One Chance wait and rule (`C5_one_chance.csv`, losses by region and device in `C5_honest_losses_by_region_device.csv`):

- W = 1, strict, ladder: stalls 28.9 s mean, glitches under the grace epoch recorded offline 80.9 %, honest IDs lost 6002.60 % per year, global pauses 1865
- W = 1, strict, ladder with yearly reset [P]: stalls 28.9 s mean, glitches under the grace epoch recorded offline 80.9 %, honest IDs lost 6002.60 % per year, global pauses 1865
- W = 1, strict, immediate ban: stalls 29.0 s mean, glitches under the grace epoch recorded offline 81.1 %, honest IDs lost 69134.68 % per year, global pauses 1960
- W = 1, unreachable is not refusal [P]: stalls 28.9 s mean, glitches under the grace epoch recorded offline 79.5 %, honest IDs lost 0.00 % per year, global pauses 2303
- W = 2, strict, ladder: stalls 54.2 s mean, glitches under the grace epoch recorded offline 54.1 %, honest IDs lost 3943.88 % per year, global pauses 1841
- W = 2, strict, ladder with yearly reset [P]: stalls 54.2 s mean, glitches under the grace epoch recorded offline 54.1 %, honest IDs lost 3943.88 % per year, global pauses 1841
- W = 2, strict, immediate ban: stalls 54.3 s mean, glitches under the grace epoch recorded offline 54.1 %, honest IDs lost 44529.07 % per year, global pauses 1976
- W = 2, unreachable is not refusal [P]: stalls 54.2 s mean, glitches under the grace epoch recorded offline 53.3 %, honest IDs lost 0.00 % per year, global pauses 2305
- W = 5, strict, ladder: stalls 109.7 s mean, glitches under the grace epoch recorded offline 0.1 %, honest IDs lost 1458.25 % per year, global pauses 1903
- W = 5, strict, ladder with yearly reset [P]: stalls 109.7 s mean, glitches under the grace epoch recorded offline 0.1 %, honest IDs lost 1458.25 % per year, global pauses 1903
- W = 5, strict, immediate ban: stalls 109.9 s mean, glitches under the grace epoch recorded offline 0.1 %, honest IDs lost 15225.01 % per year, global pauses 1964
- W = 5, unreachable is not refusal [P]: stalls 109.7 s mean, glitches under the grace epoch recorded offline 0.1 %, honest IDs lost 0.00 % per year, global pauses 2312

## C6 — clock skew and false rejections

`C6_clock_skew.csv` covers skew σ × δ × signing time × early-block rule:

- skew σ = 1 s, δ = 2 s, on time (default): 0.000 % of panel receptions early, 0 blocks early at the panel majority
- skew σ = 1 s, δ = 2 s, earliest within δ (P-4): 12.956 % of panel receptions early, 0 blocks early at the panel majority
- skew σ = 2 s, δ = 2 s, on time (default): 2.264 % of panel receptions early, 0 blocks early at the panel majority
- skew σ = 2 s, δ = 2 s, earliest within δ (P-4): 17.647 % of panel receptions early, 0 blocks early at the panel majority

## C7 — quorum liveness under household profiles

The share of KWCs unable to meet a quorum at a sampled instant (`C7_quorum_liveness.csv`), under the [P] "unreachable is not refusal" rule so that downtime alone is measured (under the strict rule honest witnesses are also banned: C5):

- household profiles, departures 0/yr: witnesses online 83.4 %; KWCs below the unregistered quorum 4.66 % (M1 at that online fraction: 0.38 %), below 7 + 21 16.46 % (M1: 8.70 %)
- household profiles, departures 0.1/yr: witnesses online 80.5 %; KWCs below the unregistered quorum 9.59 % (M1 at that online fraction: 1.57 %), below 7 + 21 25.19 % (M1: 15.92 %)
- household profiles, departures 0.25/yr: witnesses online 77.8 %; KWCs below the unregistered quorum 15.39 % (M1 at that online fraction: 4.40 %), below 7 + 21 33.13 % (M1: 25.30 %)

## A6 — honest growth and patience

New honest miners arrive at 3·R per year (`A6_growth_and_patience.csv`):

- patience median 3 days: 398137 arrivals, 86.7 % gave up before a first ID, median wait to a first ID 2.37 days
- patience median 14 days: 396570 arrivals, 73.8 % gave up before a first ID, median wait to a first ID 4.54 days

## Invariants of every run

**2 of 3 hold** across all 1011 runs: recounted caches and counters, self-validation of every broadcast block, and no reorg below a confirmed ID (`validation.csv`, section ALL; every run in `ALL_runs.csv`). Reorgs below a confirmed ID (a confirmed ID's block replaced) occurred in 11: `C1-m10000-literal-s0` (3, deepest 8), `C1-m10000-same-parent-s0` (43, deepest 12), `C2-v0.6-same-parent-loss0` (23, deepest 12), `C2-v0.6-same-parent-loss0.005` (25, deepest 12), `C2-v0.6-same-parent-loss0.02` (42, deepest 12), `C2-v0.6-same-parent-loss0.05` (10, deepest 12), `C2-v0.6-literal-stylised` (2, deepest 7), `C3-withholder-D30-permanent` (1, deepest 10), `C4-B-all_globally_active-laptop-heavy` (2, deepest 7), `C4-B+P1-after_wait-laptop-heavy` (2, deepest 7), `C4-B+P1-all_globally_active-laptop-heavy` (1, deepest 7).

No run had a fork storm: at most 328 unsettled blocks ever shared a height.

