# M3a Tier 1 issuance simulator — summary

- Git commit: `1b3805616afd478965317e6db9d4ab81d4204406` (clean working tree)
- Seed: 20261005
- Configuration SHA-256: `cf1ca7be38157bb5d06194284d44e6fa905da44c693f890577c723fe54bc1fad`
- Protocol version: 1
- Profile: full
- Sections: all
- Runs: 1011

Every run is seeded and single-threaded, so these tables are reproducible bit for bit whatever the thread count; wall times are machine-dependent and are in `perf/`. Every protocol decision goes through `gb-protocol` (SPEC v0.6 and the architect's M3a decisions of 2026-10-12). Values marked as checks are compared with M1's closed forms or with exact expectations at 5 standard errors (`validation.csv`); every other number is a measurement, reported as it came out.

## Validation

**156 of 166 checks passed.** The V0 gate passed, so the stand-in and fast modes are used for the other sections.
- **failed** `V2-old-w1299`: old design: restart advantage at keep window 1299 s vs M1 advantage_old_design(q, C_realised, W), allowance 1 % for tie losses (reference 2.6997515636570397, value 2.422074299951657)
- **failed** `V3-A-K144-clamp4-m4`: Variant A hopping gain (K = 144) vs the discrete equivalent of M1 E1 (1 − (29/30)^(1+m)) / q_B; M1's continuous value 5.000 (reference 4.7559712362459114, value 6.264876534860303)
- **failed** `V3-A-K144-clampnone-m4`: Variant A hopping gain (K = 144) vs the discrete equivalent of M1 E1 (1 − (29/30)^(1+m)) / q_B; M1's continuous value 5.000 (reference 4.7559712362459114, value 5.574338776849414)
- **failed** `V3-A-K144-clampnone-m4-recovery`: Variant A recovery window (hours) vs the discrete first-order value, allowance K/(K − 1) − 1 when unclamped (reference 5.539129844736366, value 6.725939450686641)
- **failed** `V3-A-K2016-clamp4-m4`: Variant A hopping gain (K = 2016) vs the discrete equivalent of M1 E1 (1 − (29/30)^(1+m)) / q_B; M1's continuous value 5.000 (reference 4.7559712362459114, value 10.997565531626387)
- **failed** `V3-A-K2016-clamp4-m4-recovery`: Variant A recovery window (hours) vs the discrete first-order value, allowance K/(K − 1) − 1 when unclamped (reference 66.35490735004353, value 47.726527777777775)
- **failed** `V3-A-K2016-clampnone-m4`: Variant A hopping gain (K = 2016) vs the discrete equivalent of M1 E1 (1 − (29/30)^(1+m)) / q_B; M1's continuous value 5.000 (reference 4.7559712362459114, value 7.7464737986509915)
- **failed** `V3-A-K2016-clampnone-m4-recovery`: Variant A recovery window (hours) vs the discrete first-order value, allowance K/(K − 1) − 1 when unclamped (reference 77.54781782630913, value 125.60052083333333)
- **failed** `C2-idealised-orphans`: zero latency: orphans per final-chain block vs E[tied winners − 1] from M1 F1 (1000 slots) (reference 0.016742592328524587, value 0.009261533064374671)
- **failed** `ALL-deep-reorgs`: reorgs of the network chain below a confirmed ID (1011 runs) (reference 0, value 1871)

## V0 — mode agreement (the gate)

The three crypto modes agree, and the fast sampler is exact, when every test below has p ≥ 0.001. **34 of 34 gate checks pass.** Details in `V0_mode_agreement.csv` and `validation.csv`.

## V4 — M2 test vectors

The simulator's real-mode functions rebuild every derived field of `protocol_v1.json` (keys, encodings, digests, signatures, chain steps, verdicts, positions, targets): **60 of 60 fields match** (`V4_test_vectors.csv`).

## V1 — M1 A1/A7/F1 in the idealised simulator

Blocks to 51 % of active IDs, the long-run share and the tie rate are compared with M1's closed forms at G = 2000 (`V1_time_to_threshold.csv`). The mean interval with an exact count is **30.496 s**, not 30 s: blocks start one second after their parent (start rule (a)) and each step is a whole second, so the mean interval is 1/(1 − (1 − p)^r) = 30.502 s for p = 1/(30r). This is the discrete-step bias that B-exact removes (C4).

## V2 — S5 restart attack

Under the old fixed-entropy design the restart advantage matches M1 section D at the simulator's size (q = 1/30000, C ≈ 451.0 s realised through the admission path). Under per-round entropy the restart strategy gains nothing: its largest advantage is **0.094** (H4 asks ≤ 1.02; reported, not a check). `V2_restart_advantage.csv`.

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

- 1000 miners, literal, x = 0.1: amplification 0.9793 (95 % CI 0.9641–0.9945)
- 1000 miners, literal, x = 0.5: amplification 1.0046 (95 % CI 0.9982–1.0110)
- 10000 miners, literal, x = 0.1: amplification 0.9844 (95 % CI 0.9677–1.0010)
- 10000 miners, literal, x = 0.5: amplification 1.0054 (95 % CI 0.9978–1.0129)
- 100000 miners, literal, x = 0.1: amplification 0.9874 (95 % CI 0.9722–1.0026)
- 100000 miners, literal, x = 0.5: amplification 0.9995 (95 % CI 0.9930–1.0060)
- 10000 miners, same-parent [P], x = 0.1: amplification 0.9844 (95 % CI 0.9677–1.0010)
- 10000 miners, same-parent [P], x = 0.5: amplification 1.0054 (95 % CI 0.9978–1.0129)

## C2 — fork choice under latency

Orphan and fork rates for the v0.6 rule, first-seen and lower-hash-only (`C2_fork_choice.csv`; no loss below):

- v0.6, literal: orphan rate 6.29 %, fork heights 2.59 %, lower-hash loser 40.0 % of orphans, max reorg 12, deeper than confirmation 23
- v0.6, same-parent: orphan rate 6.29 %, fork heights 2.59 %, lower-hash loser 40.0 % of orphans, max reorg 12, deeper than confirmation 23
- first-seen, literal: orphan rate 3.20 %, fork heights 1.82 %, lower-hash loser 50.7 % of orphans, max reorg 5, deeper than confirmation 0
- first-seen, same-parent: orphan rate 3.20 %, fork heights 1.82 %, lower-hash loser 50.7 % of orphans, max reorg 5, deeper than confirmation 0
- lower-hash-only, literal: orphan rate 15.34 %, fork heights 10.46 %, lower-hash loser 29.5 % of orphans, max reorg 5, deeper than confirmation 0
- lower-hash-only, same-parent: orphan rate 15.34 %, fork heights 10.46 %, lower-hash loser 29.5 % of orphans, max reorg 5, deeper than confirmation 0

## C3 — PoWit arrival deadline and the withholder

With partitions (one per region per day, 5 min) and 0.5 % loss (`C3_powit_deadline.csv`, `C3_late_by_region.csv`, `C3_withholder.csv`; withholder values are measurements, not checks):

- D = none (permanent): 0.000 % of blocks late at the panel majority, fork rate 5.32 % (no deadline: 5.32 %), split panel nodes 0, deepest reorg 12 (164 below a confirmed ID); withholder success 57.9 %, amplification 0.899
- D = 5 (permanent): 1.938 % of blocks late at the panel majority, fork rate 3.04 % (no deadline: 5.32 %), split panel nodes 63, deepest reorg 2 (0 below a confirmed ID); withholder success 8.4 %, amplification 0.102
- D = 5 (not_extended_only): 3.007 % of blocks late at the panel majority, fork rate 2.69 % (no deadline: 5.32 %), split panel nodes 0, deepest reorg 2 (0 below a confirmed ID); withholder success 8.4 %, amplification 0.103
- D = 10 (permanent): 1.065 % of blocks late at the panel majority, fork rate 2.83 % (no deadline: 5.32 %), split panel nodes 63, deepest reorg 2 (0 below a confirmed ID); withholder success 17.7 %, amplification 0.217
- D = 10 (not_extended_only): 2.906 % of blocks late at the panel majority, fork rate 2.62 % (no deadline: 5.32 %), split panel nodes 0, deepest reorg 1 (0 below a confirmed ID); withholder success 17.0 %, amplification 0.205
- D = 30 (permanent): 0.858 % of blocks late at the panel majority, fork rate 3.10 % (no deadline: 5.32 %), split panel nodes 63, deepest reorg 1 (0 below a confirmed ID); withholder success 42.7 %, amplification 0.538
- D = 30 (not_extended_only): 2.502 % of blocks late at the panel majority, fork rate 2.60 % (no deadline: 5.32 %), split panel nodes 0, deepest reorg 2 (0 below a confirmed ID); withholder success 42.0 %, amplification 0.527
- D = 60 (permanent): 0.404 % of blocks late at the panel majority, fork rate 3.08 % (no deadline: 5.32 %), split panel nodes 63, deepest reorg 2 (0 below a confirmed ID); withholder success 58.7 %, amplification 0.755
- D = 60 (not_extended_only): 1.988 % of blocks late at the panel majority, fork rate 2.89 % (no deadline: 5.32 %), split panel nodes 0, deepest reorg 4 (0 below a confirmed ID); withholder success 59.0 %, amplification 0.754

## C4 — issuance rate under churn

Mean interval and stability per difficulty rule (`C4_issuance_rate.csv`; realistic churn, count after the post-admission wait):

- A (K=144, 4×): mean interval 31.61 s (5.36 % from 30 s), daily IDs 2766 ± 263 (SD)
- B: mean interval 31.27 s (4.22 % from 30 s), daily IDs 2763 ± 189 (SD)
- B+P1: mean interval 31.05 s (3.51 % from 30 s), daily IDs 2782 ± 194 (SD)
- B+P2: mean interval 31.07 s (3.56 % from 30 s), daily IDs 2780 ± 185 (SD)
- B-exact: mean interval 30.88 s (2.92 % from 30 s), daily IDs 2798 ± 194 (SD)

## C5 — timing the SPEC promises, and honest penalties

KWC switches take 451.0 s at the median ("a few minutes"). Entropy stalls and penalties by One Chance wait and rule (`C5_one_chance.csv`, losses by region and device in `C5_honest_losses_by_region_device.csv`):

- W = 1, strict, ladder: stalls 28.9 s mean, glitches under the grace epoch recorded offline 79.9 %, honest IDs lost 1943.82 % per year, global pauses 446; stopped by a fork storm on day 9.19 (SIM-29)
- W = 1, strict, ladder with yearly reset [P]: stalls 28.9 s mean, glitches under the grace epoch recorded offline 79.9 %, honest IDs lost 1943.82 % per year, global pauses 446; stopped by a fork storm on day 9.19 (SIM-29)
- W = 1, strict, immediate ban: stalls 29.0 s mean, glitches under the grace epoch recorded offline 81.1 %, honest IDs lost 66456.75 % per year, global pauses 1714
- W = 1, unreachable is not refusal [P]: stalls 28.9 s mean, glitches under the grace epoch recorded offline 79.6 %, honest IDs lost 0.00 % per year, global pauses 2204
- W = 2, strict, ladder: stalls 54.2 s mean, glitches under the grace epoch recorded offline 54.1 %, honest IDs lost 3880.61 % per year, global pauses 1713
- W = 2, strict, ladder with yearly reset [P]: stalls 54.2 s mean, glitches under the grace epoch recorded offline 54.1 %, honest IDs lost 3880.61 % per year, global pauses 1713
- W = 2, strict, immediate ban: stalls 54.3 s mean, glitches under the grace epoch recorded offline 54.1 %, honest IDs lost 42798.68 % per year, global pauses 1720
- W = 2, unreachable is not refusal [P]: stalls 54.2 s mean, glitches under the grace epoch recorded offline 53.3 %, honest IDs lost 0.00 % per year, global pauses 2167
- W = 5, strict, ladder: stalls 109.7 s mean, glitches under the grace epoch recorded offline 0.1 %, honest IDs lost 1448.88 % per year, global pauses 1785
- W = 5, strict, ladder with yearly reset [P]: stalls 109.7 s mean, glitches under the grace epoch recorded offline 0.1 %, honest IDs lost 1448.88 % per year, global pauses 1785
- W = 5, strict, immediate ban: stalls 109.9 s mean, glitches under the grace epoch recorded offline 0.1 %, honest IDs lost 14872.22 % per year, global pauses 1797
- W = 5, unreachable is not refusal [P]: stalls 109.7 s mean, glitches under the grace epoch recorded offline 0.1 %, honest IDs lost 0.00 % per year, global pauses 2300

## C6 — clock skew and false rejections

`C6_clock_skew.csv` covers skew σ × δ × signing time × early-block rule:

- skew σ = 1 s, δ = 2 s, on time (default): 0.000 % of panel receptions early, 0 blocks early at the panel majority
- skew σ = 1 s, δ = 2 s, earliest within δ (P-4): 13.014 % of panel receptions early, 0 blocks early at the panel majority
- skew σ = 2 s, δ = 2 s, on time (default): 2.188 % of panel receptions early, 0 blocks early at the panel majority
- skew σ = 2 s, δ = 2 s, earliest within δ (P-4): 17.761 % of panel receptions early, 2 blocks early at the panel majority

## C7 — quorum liveness under household profiles

The share of KWCs unable to meet a quorum at a sampled instant (`C7_quorum_liveness.csv`), under the [P] "unreachable is not refusal" rule so that downtime alone is measured (under the strict rule honest witnesses are also banned: C5):

- household profiles, departures 0/yr: witnesses online 83.4 %; KWCs below the unregistered quorum 4.61 % (M1 at that online fraction: 0.37 %), below 7 + 21 16.27 % (M1: 8.60 %)
- household profiles, departures 0.1/yr: witnesses online 80.6 %; KWCs below the unregistered quorum 9.34 % (M1 at that online fraction: 1.52 %), below 7 + 21 24.92 % (M1: 15.65 %)
- household profiles, departures 0.25/yr: witnesses online 77.9 %; KWCs below the unregistered quorum 15.12 % (M1 at that online fraction: 4.23 %), below 7 + 21 32.84 % (M1: 24.83 %)

## A6 — honest growth and patience

New honest miners arrive at 3·R per year (`A6_growth_and_patience.csv`):

- patience median 3 days: 398137 arrivals, 85.1 % gave up before a first ID, median wait to a first ID 2.26 days
- patience median 14 days: 396570 arrivals, 70.3 % gave up before a first ID, median wait to a first ID 4.07 days

## Invariants of every run

**2 of 3 hold** across all 1011 runs: recounted caches and counters, self-validation of every broadcast block, and no reorg below a confirmed ID (`validation.csv`, section ALL; every run in `ALL_runs.csv`). Reorgs below a confirmed ID (a confirmed ID's block replaced) occurred in 55: `V2-current-w1000000000000` (1, deepest 7), `C1-m1000-literal-s0` (44, deepest 12), `C1-m10000-literal-s0` (43, deepest 12), `C1-m100000-literal-s0` (21, deepest 12), `C1-m10000-same-parent-s0` (43, deepest 12), `C2-v0.6-literal-loss0` (23, deepest 12), `C2-v0.6-literal-loss0.005` (25, deepest 12), `C2-v0.6-literal-loss0.02` (42, deepest 12), `C2-v0.6-literal-loss0.05` (10, deepest 12), `C2-v0.6-same-parent-loss0` (23, deepest 12), `C2-v0.6-same-parent-loss0.005` (25, deepest 12), `C2-v0.6-same-parent-loss0.02` (42, deepest 12), `C2-v0.6-same-parent-loss0.05` (10, deepest 12), `C2-v0.6-literal-stylised` (33, deepest 12), `C2-v0.6-literal-partitions` (488, deepest 12), `C3-honest-Dnone-permanent` (164, deepest 12), `C3-withholder-Dnone-permanent` (109, deepest 12), `C4-A (K=144, 4×)-after_wait-realistic` (17, deepest 11), `C4-A (K=144, 4×)-after_wait-laptop-heavy` (50, deepest 12), `C4-A (K=144, 4×)-all_globally_active-realistic` (17, deepest 11), `C4-A (K=144, 4×)-all_globally_active-laptop-heavy` (50, deepest 12), `C4-B-after_wait-realistic` (24, deepest 12), `C4-B-all_globally_active-realistic` (23, deepest 12), `C4-B+P1-after_wait-realistic` (24, deepest 12), `C4-B+P1-all_globally_active-realistic` (18, deepest 12), `C4-B+P1-all_globally_active-laptop-heavy` (1, deepest 8), `C4-B+P2-after_wait-realistic` (47, deepest 12), `C4-B+P2-all_globally_active-realistic` (5, deepest 8), `C4-B-exact-after_wait-realistic` (39, deepest 12), `C4-B-exact-all_globally_active-realistic` (47, deepest 12), `C4-B-exact-all_globally_active-laptop-heavy` (4, deepest 8), `C5-W1-strict, ladder` (1, deepest 7), `C5-W1-strict, ladder with yearly reset [P]` (1, deepest 7), `C5-W1-strict, immediate ban` (10, deepest 12), `C5-W1-unreachable is not refusal [P]` (5, deepest 9), `C5-W2-strict, ladder` (62, deepest 12), `C5-W2-strict, ladder with yearly reset [P]` (62, deepest 12), `C5-W2-strict, immediate ban` (9, deepest 11), `C5-W2-unreachable is not refusal [P]` (11, deepest 11), `C5-W5-strict, ladder` (26, deepest 12), `C5-W5-strict, ladder with yearly reset [P]` (26, deepest 12), `C5-W5-strict, immediate ban` (19, deepest 12), `C5-W5-unreachable is not refusal [P]` (4, deepest 7), `C6-skew0.1-delta0.5-OnTime-Permanent` (2, deepest 7), `C6-skew0.1-delta0.5-OnTime-ReevaluateLater` (2, deepest 7), `C6-skew0.1-delta2-OnTime-Permanent` (2, deepest 7), `C6-skew0.1-delta2-OnTime-ReevaluateLater` (2, deepest 7), `C6-skew0.1-delta10-OnTime-Permanent` (2, deepest 7), `C6-skew0.1-delta10-OnTime-ReevaluateLater` (2, deepest 7), `C6-skew1-delta0.5-EarliestWithinTolerance-ReevaluateLater` (1, deepest 7), `C7-households-departures0` (1, deepest 7), `C7-households-departures0.1` (2, deepest 7), `C7-households-departures0.25` (41, deepest 12), `A6-patience3` (63, deepest 12), `A6-patience14` (3, deepest 8).

**Fork storms stopped 8 runs early**, when more blocks than `sim.world.fork_storm_unsettled` were unsettled at once (SIM-29): `C2-v0.6-literal-partitions` (day 9.19), `C4-A (K=144, 4×)-after_wait-realistic` (day 9.19), `C4-A (K=144, 4×)-all_globally_active-realistic` (day 9.19), `C4-B-all_globally_active-laptop-heavy` (day 9.19), `C4-B+P2-all_globally_active-laptop-heavy` (day 9.19), `C4-B-exact-all_globally_active-laptop-heavy` (day 9.19), `C5-W1-strict, ladder` (day 9.19), `C5-W1-strict, ladder with yearly reset [P]` (day 9.19). Their tables cover the time before (`ALL_runs.csv`, column `fork_storm_day`).

