# M3a Tier 1 issuance simulator — summary

- Git commit: `1d9ce918617c80482453569698f6115473f1a2e3` (clean working tree)
- Seed: 20261005
- Configuration SHA-256: `5109f41b81744e64f9ae6678444041f02c50c8c4c2f30460e87b00f0c60d7b59`
- Protocol version: 1
- Profile: full
- Sections: all
- Runs: 2978

Every run is seeded and single-threaded, so these tables are reproducible bit for bit whatever the thread count; wall times are machine-dependent and are in `perf/`. Every protocol decision goes through `gb-protocol` (SPEC v0.6 and the architect's decisions of 2026-10-12 and 2026-10-14). Values marked as checks are compared with M1's closed forms or with exact expectations at 5 standard errors (`validation.csv`); every other number is a measurement, reported as it came out.

## Validation

**178 of 210 checks pass, 2 fail and 30 are reference-limited.** The V0 gate passed, so the stand-in and fast modes are used for the other sections. A reference-limited check is kept and reported, but its reference does not model what the simulator does, so it is not a failure either way (ASSUMPTIONS SIM-31).
- *reference-limited* `V2-old-w1299`: old design: restart advantage at keep window 1299 s vs M1 advantage_old_design(q, C_realised, W), allowance 1 % for tie losses (reference 2.6997528581127637, value 2.199565724395834)
- *reference-limited* `V2-old-w3600`: old design: restart advantage at keep window 3600 s vs M1 advantage_old_design(q, C_realised, W), allowance 1 % for tie losses (reference 5.294825031931236, value 4.142325731845382)
- *reference-limited* `V2-old-w5196`: old design: restart advantage at keep window 5196 s vs M1 advantage_old_design(q, C_realised, W), allowance 1 % for tie losses (reference 5.682883345822444, value 4.450098714694388)
- *reference-limited* `V2-old-w20785`: old design: restart advantage at keep window 20785 s vs M1 advantage_old_design(q, C_realised, W), allowance 1 % for tie losses (reference 3.0136553410800477, value 2.652110048036841)
- *reference-limited* `V3-A-K144-clamp4-m4`: Variant A hopping gain (K = 144) vs the discrete equivalent of M1 E1 (1 − (29/30)^(1+m)) / q_B; M1's continuous value 5.000 (reference 4.7559712362459114, value 6.264876534860303)
- *reference-limited* `V3-A-K144-clamp4-m4-recovery`: Variant A recovery window (hours) vs the discrete first-order value, allowance K/(K − 1) − 1 when unclamped (reference 4.739636239288823, value 4.7269209039548015)
- *reference-limited* `V3-A-K144-clampnone-m4`: Variant A hopping gain (K = 144) vs the discrete equivalent of M1 E1 (1 − (29/30)^(1+m)) / q_B; M1's continuous value 5.000 (reference 4.7559712362459114, value 5.574338776849414)
- *reference-limited* `V3-A-K144-clampnone-m4-recovery`: Variant A recovery window (hours) vs the discrete first-order value, allowance K/(K − 1) − 1 when unclamped (reference 5.539129844736366, value 6.725939450686641)
- *reference-limited* `V3-A-K2016-clamp4-m4`: Variant A hopping gain (K = 2016) vs the discrete equivalent of M1 E1 (1 − (29/30)^(1+m)) / q_B; M1's continuous value 5.000 (reference 4.7559712362459114, value 10.997565531626387)
- *reference-limited* `V3-A-K2016-clamp4-m4-recovery`: Variant A recovery window (hours) vs the discrete first-order value, allowance K/(K − 1) − 1 when unclamped (reference 66.35490735004353, value 47.726527777777775)
- *reference-limited* `V3-A-K2016-clampnone-m4`: Variant A hopping gain (K = 2016) vs the discrete equivalent of M1 E1 (1 − (29/30)^(1+m)) / q_B; M1's continuous value 5.000 (reference 4.7559712362459114, value 7.7464737986509915)
- *reference-limited* `V3-A-K2016-clampnone-m4-recovery`: Variant A recovery window (hours) vs the discrete first-order value, allowance K/(K − 1) − 1 when unclamped (reference 77.54781782630913, value 125.60052083333333)
- *reference-limited* `C7-households-d0-unregistered`: household profiles, departures 0/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.009704135068594688, value 0.06610185185185186)
- *reference-limited* `C7-households-d0-registered`: household profiles, departures 0/yr: share of KWCs below 7 + 21 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.12906858277501823, value 0.2020324074074074)
- *reference-limited* `C7-households-d0.1-unregistered`: household profiles, departures 0.1/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.009900565500309384, value 0.06554398148148148)
- *reference-limited* `C7-households-d0.1-registered`: household profiles, departures 0.1/yr: share of KWCs below 7 + 21 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.13017909251195653, value 0.2039212962962963)
- *reference-limited* `C7-households-d0.25-unregistered`: household profiles, departures 0.25/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.00750818358627957, value 0.06136111111111111)
- *reference-limited* `C7-households-d0.25-registered`: household profiles, departures 0.25/yr: share of KWCs below 7 + 21 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.1157633035244159, value 0.19045138888888888)
- **failed** `E2-hop-issued_entropy_lagged / none-m4`: Variant B count without a correction: hopping gain vs 1 (M1 E1: no first-order gain) (reference 1, value 1.0427974991890763)
- *reference-limited* `E3-C7-d0-all-online-no-failover-unregistered`: all-online (One Chance), no failover, C7 departures 0/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.009698120121284164, value 0.06514351851851852)
- *reference-limited* `E3-C7-d0.1-all-online-no-failover-unregistered`: all-online (One Chance), no failover, C7 departures 0.1/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.009889281790726918, value 0.06576388888888889)
- *reference-limited* `E3-C7-d0.25-all-online-no-failover-unregistered`: all-online (One Chance), no failover, C7 departures 0.25/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.007521340053279588, value 0.06192361111111111)
- *reference-limited* `E3-C7-d0-all-online-failover-unregistered`: all-online (One Chance), failover, C7 departures 0/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.009659788117251713, value 0.06484027777777777)
- *reference-limited* `E3-C7-d0.1-all-online-failover-unregistered`: all-online (One Chance), failover, C7 departures 0.1/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.009831615962248342, value 0.06509259259259259)
- *reference-limited* `E3-C7-d0.25-all-online-failover-unregistered`: all-online (One Chance), failover, C7 departures 0.25/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.007474439003944881, value 0.06122685185185185)
- *reference-limited* `E3-C7-d0-t2n3-no-failover-unregistered`: threshold 2n/3, no failover, C7 departures 0/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.009704135068594688, value 0.06610185185185186)
- *reference-limited* `E3-C7-d0.1-t2n3-no-failover-unregistered`: threshold 2n/3, no failover, C7 departures 0.1/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.009900565500309384, value 0.06554398148148148)
- *reference-limited* `E3-C7-d0.25-t2n3-no-failover-unregistered`: threshold 2n/3, no failover, C7 departures 0.25/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.00750818358627957, value 0.06136111111111111)
- *reference-limited* `E3-C7-d0-t2n3-failover-unregistered`: threshold 2n/3, failover, C7 departures 0/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.009654256306073635, value 0.06489814814814815)
- *reference-limited* `E3-C7-d0.1-t2n3-failover-unregistered`: threshold 2n/3, failover, C7 departures 0.1/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.009827874784124743, value 0.06524768518518519)
- *reference-limited* `E3-C7-d0.25-t2n3-failover-unregistered`: threshold 2n/3, failover, C7 departures 0.25/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences) (reference 0.007465087538553581, value 0.06099537037037037)
- **failed** `ALL-deep-reorgs`: reorgs of the network chain below a confirmed ID (2978 runs) (reference 0, value 43)

## V0 — mode agreement (the gate)

The three crypto modes agree, and the fast sampler is exact, when every test below has p ≥ 0.001. **37 of 37 gate checks pass.** Details in `V0_mode_agreement.csv` and `validation.csv`.

## V4 — M2 test vectors

The simulator's real-mode functions rebuild every derived field of `protocol_v1.json` (keys, encodings, digests, signatures, chain steps, verdicts, positions, targets): **60 of 60 fields match** (`V4_test_vectors.csv`).

## V1 — M1 A1/A7/F1 in the idealised simulator

Blocks to 51 % of active IDs, the long-run share and the tie rate are compared with M1's closed forms at G = 2000 (`V1_time_to_threshold.csv`). The mean interval with an exact count is **30.496 s**, not 30 s: blocks start one second after their parent (start rule (a)) and each step is a whole second, so the mean interval is 1/(1 − (1 − p)^r) = 30.502 s for p = 1/(30r). This is the discrete-step bias that B-exact removes (C4).

A reported comparison, not a check: the same runs at the configured defaults (`V1_defaults_comparison.csv`, `V1_defaults_long_run_share.csv`) have a mean interval of 30.272 s (pinned 30.496 s); blocks to 51 %: s = 0.6, f = 1: 11362.0 blocks pinned, 11356.1 at the defaults (closed form 11333.3); s = 0.75, f = 1: 4236.7 blocks pinned, 4236.7 at the defaults (closed form 4250.0); s = 1, f = 1: 2082.0 blocks pinned, 2082.0 at the defaults (closed form 2081.6); s = 0.6, f = 0.7: 4728.5 blocks pinned, 4731.4 at the defaults (closed form 4722.2); long-run share at s = 0.6, f = 0.7: 0.6815 at the defaults, 0.6816 pinned, M1 A7 0.6818.

## V2 — S5 restart attack

Under the old fixed-entropy design the restart advantage matches M1 section D at the simulator's size (q = 1/30000, C ≈ 451.0 s realised through the admission path). Under per-round entropy the restart strategy gains nothing: its largest advantage is **0.095** (H4 asks ≤ 1.02; reported, not a check). `V2_restart_advantage.csv`.

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

A tagged group of honest slots, spread over the regions in proportion to their miners (SIM-51), wins IDs in proportion to its mining slot-time (`C1_amplification.csv`):

- 1000 miners, literal, x = 0.1: amplification 0.9860 (95 % CI 0.9680–1.0040)
- 1000 miners, literal, x = 0.5: amplification 0.9996 (95 % CI 0.9943–1.0048)
- 10000 miners, literal, x = 0.1: amplification 1.0038 (95 % CI 0.9896–1.0181)
- 10000 miners, literal, x = 0.5: amplification 1.0012 (95 % CI 0.9963–1.0062)
- 100000 miners, literal, x = 0.1: amplification 1.0097 (95 % CI 0.9954–1.0240)
- 100000 miners, literal, x = 0.5: amplification 0.9999 (95 % CI 0.9952–1.0047)
- 10000 miners, same-parent [P], x = 0.1: amplification 1.0038 (95 % CI 0.9896–1.0181)
- 10000 miners, same-parent [P], x = 0.5: amplification 1.0012 (95 % CI 0.9963–1.0062)

Each region's miners as one group, which measures the region effect apart from group placement (amplification and 95 % CI, `C1_amplification_by_region.csv`):

- 1000 miners: na_east 1.005 (0.985–1.026), na_west 1.023 (1.000–1.046), sa_east 1.010 (0.992–1.027), eu_west 1.017 (1.005–1.030), af 0.981 (0.968–0.994), as_south 0.990 (0.980–1.000), as_east 1.002 (0.995–1.008), oc 0.995 (0.935–1.055)
- 10000 miners: na_east 1.021 (1.003–1.039), na_west 0.999 (0.976–1.023), sa_east 1.021 (1.001–1.041), eu_west 0.998 (0.985–1.011), af 0.992 (0.976–1.008), as_south 0.987 (0.977–0.998), as_east 1.004 (0.997–1.010), oc 0.966 (0.902–1.031)
- 100000 miners: na_east 1.032 (1.016–1.048), na_west 1.028 (1.002–1.055), sa_east 1.007 (0.991–1.022), eu_west 1.010 (0.997–1.023), af 0.992 (0.979–1.006), as_south 0.981 (0.971–0.992), as_east 1.002 (0.995–1.008), oc 0.966 (0.896–1.036)

## C2 — fork choice under latency

Orphan and fork rates for the v0.6 rule, first-seen and lower-hash-only (`C2_fork_choice.csv`; no loss below):

- v0.6, literal: orphan rate 1.90 %, fork heights 1.85 %, lower-hash loser 10.4 % of orphans, max reorg 1, deeper than confirmation 0
- v0.6, same-parent: orphan rate 93.53 %, fork heights 2.01 %, lower-hash loser 6.2 % of orphans, max reorg 3, deeper than confirmation 0
- first-seen, literal: orphan rate 1.70 %, fork heights 1.68 %, lower-hash loser 51.1 % of orphans, max reorg 2, deeper than confirmation 0
- first-seen, same-parent: orphan rate 1.70 %, fork heights 1.68 %, lower-hash loser 51.1 % of orphans, max reorg 2, deeper than confirmation 0
- lower-hash-only, literal: orphan rate 52.67 %, fork heights 20.26 %, lower-hash loser 14.7 % of orphans, max reorg 2, deeper than confirmation 0
- lower-hash-only, same-parent: orphan rate – %, fork heights – %, lower-hash loser – % of orphans, max reorg 3, deeper than confirmation 0

## C3 — PoWit arrival deadline and the withholder

With partitions (one per region per day, 5 min) and 0.5 % loss (`C3_powit_deadline.csv`, `C3_late_by_region.csv`, `C3_withholder.csv`; withholder values are measurements, not checks):

- D = none (permanent): 0.000 % of blocks late at the panel majority, fork rate 1.81 % (no deadline: 1.81 %), split panel nodes 0, deepest reorg 1 (0 below a confirmed ID); withholder success 12.9 %, amplification 0.152
- D = 5 (permanent): 0.073 % of blocks late at the panel majority, fork rate 2.05 % (no deadline: 1.81 %), split panel nodes 63, deepest reorg 1 (0 below a confirmed ID); withholder success 0.1 %, amplification 0.003
- D = 5 (not_extended_only): 0.131 % of blocks late at the panel majority, fork rate 1.89 % (no deadline: 1.81 %), split panel nodes 0, deepest reorg 2 (0 below a confirmed ID); withholder success 0.1 %, amplification 0.001
- D = 10 (permanent): 0.044 % of blocks late at the panel majority, fork rate 1.79 % (no deadline: 1.81 %), split panel nodes 63, deepest reorg 1 (0 below a confirmed ID); withholder success 0.1 %, amplification 0.002
- D = 10 (not_extended_only): 0.144 % of blocks late at the panel majority, fork rate 1.85 % (no deadline: 1.81 %), split panel nodes 0, deepest reorg 2 (0 below a confirmed ID); withholder success 0.1 %, amplification 0.001
- D = 30 (permanent): 0.098 % of blocks late at the panel majority, fork rate 1.84 % (no deadline: 1.81 %), split panel nodes 63, deepest reorg 1 (0 below a confirmed ID); withholder success 0.4 %, amplification 0.006
- D = 30 (not_extended_only): 0.122 % of blocks late at the panel majority, fork rate 1.85 % (no deadline: 1.81 %), split panel nodes 0, deepest reorg 1 (0 below a confirmed ID); withholder success 0.3 %, amplification 0.003
- D = 60 (permanent): 0.084 % of blocks late at the panel majority, fork rate 2.19 % (no deadline: 1.81 %), split panel nodes 63, deepest reorg 1 (0 below a confirmed ID); withholder success 0.2 %, amplification 0.003
- D = 60 (not_extended_only): 0.116 % of blocks late at the panel majority, fork rate 1.86 % (no deadline: 1.81 %), split panel nodes 0, deepest reorg 6 (0 below a confirmed ID); withholder success 0.4 %, amplification 0.005

## C4 — issuance rate under churn

Mean interval and stability per difficulty rule (`C4_issuance_rate.csv`; realistic churn, count after the post-admission wait):

- A (K=144, 4×): mean interval 30.22 s (0.73 % from 30 s), daily IDs 2858 ± 18 (SD)
- B: mean interval 31.29 s (4.29 % from 30 s), daily IDs 2761 ± 48 (SD)
- B+P1: mean interval 30.81 s (2.70 % from 30 s), daily IDs 2804 ± 35 (SD)
- B+P2: mean interval 30.65 s (2.17 % from 30 s), daily IDs 2818 ± 36 (SD)
- B-exact: mean interval 30.76 s (2.53 % from 30 s), daily IDs 2808 ± 47 (SD)

## C5 — timing the SPEC promises, and honest penalties

KWC switches take 451.0 s at the median ("a few minutes"). Entropy stalls and penalties by One Chance wait and rule (`C5_one_chance.csv`, losses by region and device in `C5_honest_losses_by_region_device.csv`):

- W = 1, strict, ladder: stalls 29.6 s mean, glitches under the grace epoch recorded offline 81.7 %, honest IDs lost 5636.53 % per year, global pauses 1583
- W = 1, strict, ladder with yearly reset [P]: stalls 29.6 s mean, glitches under the grace epoch recorded offline 81.7 %, honest IDs lost 5636.53 % per year, global pauses 1583
- W = 1, strict, immediate ban: stalls 29.6 s mean, glitches under the grace epoch recorded offline 81.7 %, honest IDs lost 62991.62 % per year, global pauses 1597
- W = 1, unreachable is not refusal [P]: stalls 29.6 s mean, glitches under the grace epoch recorded offline 81.3 %, honest IDs lost 0.00 % per year, global pauses 1568
- W = 2, strict, ladder: stalls 58.0 s mean, glitches under the grace epoch recorded offline 54.6 %, honest IDs lost 3677.12 % per year, global pauses 1567
- W = 2, strict, ladder with yearly reset [P]: stalls 58.0 s mean, glitches under the grace epoch recorded offline 54.6 %, honest IDs lost 3677.12 % per year, global pauses 1567
- W = 2, strict, immediate ban: stalls 58.0 s mean, glitches under the grace epoch recorded offline 54.6 %, honest IDs lost 40379.49 % per year, global pauses 1605
- W = 2, unreachable is not refusal [P]: stalls 58.0 s mean, glitches under the grace epoch recorded offline 54.5 %, honest IDs lost 0.00 % per year, global pauses 1568
- W = 5, strict, ladder: stalls 135.8 s mean, glitches under the grace epoch recorded offline 0.1 %, honest IDs lost 1336.69 % per year, global pauses 1623
- W = 5, strict, ladder with yearly reset [P]: stalls 135.8 s mean, glitches under the grace epoch recorded offline 0.1 %, honest IDs lost 1336.69 % per year, global pauses 1623
- W = 5, strict, immediate ban: stalls 135.9 s mean, glitches under the grace epoch recorded offline 0.1 %, honest IDs lost 13523.22 % per year, global pauses 1590
- W = 5, unreachable is not refusal [P]: stalls 135.8 s mean, glitches under the grace epoch recorded offline 0.1 %, honest IDs lost 0.00 % per year, global pauses 1568

## C6 — clock skew and false rejections

`C6_clock_skew.csv` covers skew σ × δ × signing time × early-block rule:

- skew σ = 1 s, δ = 2 s, on time (default): 0.000 % of panel receptions early, 0 blocks early at the panel majority; D = 10 s (not_extended_only): 0.000 % of blocks late at the panel majority, 0 panel nodes split for good (0 off the final chain at the end)
- skew σ = 1 s, δ = 2 s, earliest within δ (P-4): 18.151 % of panel receptions early, 0 blocks early at the panel majority; D = 10 s (not_extended_only): 0.000 % of blocks late at the panel majority, 37 panel nodes split for good (37 off the final chain at the end)
- skew σ = 2 s, δ = 2 s, on time (default): 2.129 % of panel receptions early, 0 blocks early at the panel majority; D = 10 s (not_extended_only): 0.000 % of blocks late at the panel majority, 16 panel nodes split for good (16 off the final chain at the end)
- skew σ = 2 s, δ = 2 s, earliest within δ (P-4): 30.121 % of panel receptions early, 0 blocks early at the panel majority; D = 10 s (not_extended_only): 0.000 % of blocks late at the panel majority, 39 panel nodes split for good (43 off the final chain at the end)

## C7 — quorum liveness under household profiles

The share of KWCs unable to meet a quorum at a sampled instant (`C7_quorum_liveness.csv`), under the [P] "unreachable is not refusal" rule so that downtime alone is measured (under the strict rule honest witnesses are also banned: C5):

- household profiles, departures 0/yr: witnesses online 81.6 %; KWCs below the unregistered quorum 6.61 % (M1 at that online fraction: 0.97 %), below 7 + 21 20.20 % (M1: 12.91 %)
- household profiles, departures 0.1/yr: witnesses online 81.5 %; KWCs below the unregistered quorum 6.55 % (M1 at that online fraction: 0.99 %), below 7 + 21 20.39 % (M1: 13.02 %)
- household profiles, departures 0.25/yr: witnesses online 82.1 %; KWCs below the unregistered quorum 6.14 % (M1 at that online fraction: 0.75 %), below 7 + 21 19.05 % (M1: 11.58 %)

## A6 — honest growth and patience

New honest miners arrive at 3·R per year (`A6_growth_and_patience.csv`):

- patience median 3 days: 398137 arrivals, 84.8 % gave up before a first ID, median wait to a first ID 2.22 days
- patience median 14 days: 396570 arrivals, 69.7 % gave up before a first ID, median wait to a first ID 3.96 days

The wait to a first ID with no patience limit (`A6_first_id_wait.csv`):

- 1·R arrivals: of 89347 newcomers followed 30 days, 5.5 % had a first ID within a day, 29.7 % within 7 days, 61.7 % within 30 days; median wait 17.94 days
- 2·R arrivals: of 178321 newcomers followed 30 days, 3.6 % had a first ID within a day, 18.9 % within 7 days, 42.1 % within 30 days; median wait inf days
- 3·R arrivals: of 200000 newcomers followed 30 days, 3.3 % had a first ID within a day, 16.3 % within 7 days, 35.5 % within 30 days; median wait inf days

H4 — is the post-admission wait still needed now that restarts gain nothing? V2's current-design restarters (half the slots, keep window 1 h) with and without it (`A6_post_admission_wait.csv`):

- post-admission wait 0 rounds: 3556023 restarts, restarter advantage 0.115 (SE 0.002)
- post-admission wait 5 rounds: 2521438 restarts, restarter advantage 0.085 (SE 0.001)

## E1 — fork choice

C3 setup (RIPE latency, 0.5 % loss, one 5-minute partition per region per day, realistic churn), 5 seeds per configuration, honest and with a 10 % withholder (`E1_fork_choice.csv`, `E1_region_win_share.csv`, `E1_criterion4.csv`). Criteria, fixed in advance and applied mechanically: 1. zero reorgs below confirmation in every run (and in every criterion-4 rerun; a run stopped by a fork storm, SIM-29, never reached its end, so it does not show zero and fails); 2. withholder amplification ≤ 1; 3. no validator split for good (one that can never accept a settled block) in any run; 4. the smallest |mean interval − 30 s| with the configuration's best H5-passing E2 variant; 5. the lowest orphan rate. A tie is a 95 % CI of the paired difference over seeds that includes zero; ties go to the simplest rule ((a) before (r), v0.6 before first seen, literal before [P], D = 10 s soft before no deadline).

**Winner: (r) first_seen literal no-deadline.** Criteria 1–3 are met by 10 of 16 configurations ((a) v0.6 literal no-deadline, (a) first_seen literal D10-soft, (a) first_seen literal no-deadline, (a) first_seen [P]same-parent D10-soft, (a) first_seen [P]same-parent no-deadline, (r) v0.6 literal no-deadline, (r) first_seen literal D10-soft, (r) first_seen literal no-deadline, (r) first_seen [P]same-parent D10-soft, (r) first_seen [P]same-parent no-deadline); criterion 4 used the E2 variants that pass H5: after_wait / none, after_wait / B-exact, all_globally_active / none, all_globally_active / B-exact, issued_entropy / none, issued_entropy / P1 ×1.1, issued_entropy / P1 ×1.25, issued_entropy / P1 ×1.5, issued_entropy / P2 ×1.1; after the criterion-4 reruns 10 remain; ties on criterion 4: (a) first_seen literal D10-soft, (a) first_seen literal no-deadline, (a) first_seen [P]same-parent D10-soft, (a) first_seen [P]same-parent no-deadline, (r) first_seen literal D10-soft, (r) first_seen literal no-deadline, (r) first_seen [P]same-parent D10-soft, (r) first_seen [P]same-parent no-deadline; on criterion 5: (r) first_seen literal no-deadline, (r) first_seen [P]same-parent no-deadline.

- (a) v0.6 literal D10-soft: orphans 2.20 % (±0.11), interval 31.44 s (±0.30), 3 reorgs below confirmation (deepest 11), withholder amplification 0.232 (SE 0.006), at most 0 validators split for good (0 off the final chain at the end)
- (a) v0.6 literal no-deadline: orphans 4.64 % (±0.81), interval 32.48 s (±0.58), 0 reorgs below confirmation (deepest 4), withholder amplification 0.783 (SE 0.015), at most 0 validators split for good (0 off the final chain at the end); criterion-4 reruns: 0 reorganised below confirmation, 0 stopped by a fork storm
- (a) v0.6 [P]same-parent D10-soft: orphans – % (±–), interval – s (±–), 0 reorgs below confirmation (deepest 3), withholder amplification – (SE 0.000), at most 0 validators split for good (0 off the final chain at the end); stopped by fork storms on days 2.11, 3.76, 1.38, 1.48, 0.01, 0.56, 0.03, 0.45, 0.55, 0.55 (SIM-29)
- (a) v0.6 [P]same-parent no-deadline: orphans – % (±–), interval – s (±–), 0 reorgs below confirmation (deepest 5), withholder amplification – (SE 0.000), at most 0 validators split for good (0 off the final chain at the end); stopped by fork storms on days 2.14, 0.76, 1.44, 0.59, 0.01, 0.04, 0.02, 0.01, 0.03, 0.04 (SIM-29)
- (a) first_seen literal D10-soft: orphans 1.96 % (±0.08), interval 30.32 s (±0.08), 0 reorgs below confirmation (deepest 4), withholder amplification 0.001 (SE 0.000), at most 0 validators split for good (0 off the final chain at the end); criterion-4 reruns: 0 reorganised below confirmation, 0 stopped by a fork storm
- (a) first_seen literal no-deadline: orphans 2.03 % (±0.16), interval 30.33 s (±0.09), 0 reorgs below confirmation (deepest 5), withholder amplification 0.163 (SE 0.003), at most 0 validators split for good (0 off the final chain at the end); criterion-4 reruns: 0 reorganised below confirmation, 0 stopped by a fork storm
- (a) first_seen [P]same-parent D10-soft: orphans 1.96 % (±0.08), interval 30.32 s (±0.08), 0 reorgs below confirmation (deepest 4), withholder amplification 0.001 (SE 0.000), at most 0 validators split for good (0 off the final chain at the end); criterion-4 reruns: 0 reorganised below confirmation, 0 stopped by a fork storm
- (a) first_seen [P]same-parent no-deadline: orphans 2.03 % (±0.16), interval 30.33 s (±0.09), 0 reorgs below confirmation (deepest 5), withholder amplification 0.163 (SE 0.003), at most 0 validators split for good (0 off the final chain at the end); criterion-4 reruns: 0 reorganised below confirmation, 0 stopped by a fork storm
- (r) v0.6 literal D10-soft: orphans 2.00 % (±0.15), interval 31.83 s (±0.64), 2 reorgs below confirmation (deepest 12), withholder amplification 0.236 (SE 0.007), at most 0 validators split for good (0 off the final chain at the end)
- (r) v0.6 literal no-deadline: orphans 1.94 % (±0.14), interval 31.86 s (±0.25), 0 reorgs below confirmation (deepest 2), withholder amplification 0.814 (SE 0.020), at most 0 validators split for good (0 off the final chain at the end); criterion-4 reruns: 0 reorganised below confirmation, 0 stopped by a fork storm
- (r) v0.6 [P]same-parent D10-soft: orphans – % (±–), interval – s (±–), 0 reorgs below confirmation (deepest 4), withholder amplification – (SE 0.000), at most 0 validators split for good (0 off the final chain at the end); stopped by fork storms on days 2.93, 3.01, 1.88, 1.79, 0.01, 0.71, 0.18, 0.20, 0.41, 0.38 (SIM-29)
- (r) v0.6 [P]same-parent no-deadline: orphans – % (±–), interval – s (±–), 0 reorgs below confirmation (deepest 3), withholder amplification – (SE 0.000), at most 0 validators split for good (0 off the final chain at the end); stopped by fork storms on days 4.74, 2.60, 9.72, 4.29, 0.01, 0.04, 0.11, 0.16, 0.04, 0.04 (SIM-29)
- (r) first_seen literal D10-soft: orphans 1.85 % (±0.07), interval 30.32 s (±0.05), 0 reorgs below confirmation (deepest 2), withholder amplification 0.001 (SE 0.000), at most 0 validators split for good (14 off the final chain at the end); criterion-4 reruns: 0 reorganised below confirmation, 0 stopped by a fork storm
- (r) first_seen literal no-deadline: orphans 1.77 % (±0.09), interval 30.34 s (±0.06), 0 reorgs below confirmation (deepest 2), withholder amplification 0.154 (SE 0.003), at most 0 validators split for good (0 off the final chain at the end); criterion-4 reruns: 0 reorganised below confirmation, 0 stopped by a fork storm
- (r) first_seen [P]same-parent D10-soft: orphans 1.85 % (±0.07), interval 30.32 s (±0.05), 0 reorgs below confirmation (deepest 2), withholder amplification 0.001 (SE 0.000), at most 0 validators split for good (14 off the final chain at the end); criterion-4 reruns: 0 reorganised below confirmation, 0 stopped by a fork storm
- (r) first_seen [P]same-parent no-deadline: orphans 1.77 % (±0.09), interval 30.34 s (±0.06), 0 reorgs below confirmation (deepest 2), withholder amplification 0.154 (SE 0.003), at most 0 validators split for good (0 off the final chain at the end); criterion-4 reruns: 0 reorganised below confirmation, 0 stopped by a fork storm

## E2 — the issuance count rule

Variant B under three count rules, without a correction and with P1 and P2 at clamps 1.1, 1.25 and 1.5 and B-exact; Variant A is the reference. Intervals in the C4 setup under both churns and start rules (`E2_issuance.csv`); hopping gains in the V3 setup at m ∈ {0.1, 0.5, 1, 4} and one idle-but-counted cycle at m = 1 (`E2_hopping.csv`, `E2_h5.csv`). The chart shows the trade-off between interval error and the worst hopping gain.

Variants passing H5 (gain ≤ 1.02 at every m), used by E1's criterion 4: after_wait / none (worst gain 1.002); after_wait / B-exact (worst gain 1.019); all_globally_active / none (worst gain 1.002); all_globally_active / B-exact (worst gain 1.019); issued_entropy / none (worst gain 1.003); issued_entropy / P1 ×1.1 (worst gain 1.007); issued_entropy / P1 ×1.25 (worst gain 1.007); issued_entropy / P1 ×1.5 (worst gain 1.007); issued_entropy / P2 ×1.1 (worst gain 1.017).

## E3 — liveness: threshold entropy and failover

Every configuration (entropy rule × failover after 2 rounds below quorum) on C7's three departure profiles, on the day-9.19 event replayed as a regional blackout of as_east (10.88 h, seed 0), and on a scripted regional blackout of the same length in every region over 5 seeds (`E3_liveness.csv`, `E3_outages.csv` with each outage's miners that lost power and miners that lost witnessing while powered, `E3_blackout_test.csv`). Next to the test, a diagnosis of every scripted blackout (`E3_blackout_diagnosis.csv`: the region's share of witness nodes and miners, KWCs that could not issue entropy, failover moves and spare capacity) and of every gap over 10 minutes that overlaps one (`E3_blackout_gaps.csv`: its cause, sampled each round — no miners with power, no entropy, a difficulty that implies over four target intervals with the miners holding entropy, or other). The test fixed in advance: no gap between final-chain blocks longer than 10 minutes during any blackout. The security cost of the 21-of-40 threshold is in `E3_security.csv`.

- all-online (One Chance), no failover: 1 of 41 scripted blackouts (the day-9.19 event and one per region over the seeds) had a gap over 10 minutes; the longest 12.6 min (day-9.19 event: 12.6 min); causes of the gaps over 10 minutes: other 1
- all-online (One Chance), failover: 1 of 41 scripted blackouts (the day-9.19 event and one per region over the seeds) had a gap over 10 minutes; the longest 15.1 min (day-9.19 event: 8.0 min); causes of the gaps over 10 minutes: other 1
- threshold 2n/3, no failover: 3 of 41 scripted blackouts (the day-9.19 event and one per region over the seeds) had a gap over 10 minutes; the longest 13.0 min (day-9.19 event: 12.1 min); causes of the gaps over 10 minutes: no entropy 2, difficulty too high 1, other 2
- threshold 2n/3, failover: 2 of 41 scripted blackouts (the day-9.19 event and one per region over the seeds) had a gap over 10 minutes; the longest 16.6 min (day-9.19 event: 16.6 min); causes of the gaps over 10 minutes: difficulty too high 2
- threshold 21/40, no failover: 0 of 41 scripted blackouts (the day-9.19 event and one per region over the seeds) had a gap over 10 minutes; the longest 6.2 min (day-9.19 event: 4.1 min) — **no gap over 10 minutes**; causes of the gaps over 10 minutes: none
- threshold 21/40, failover: 0 of 41 scripted blackouts (the day-9.19 event and one per region over the seeds) had a gap over 10 minutes; the longest 6.2 min (day-9.19 event: 4.0 min) — **no gap over 10 minutes**; causes of the gaps over 10 minutes: none

**[P] candidate for SPEC v0.7:** threshold 21/40, no failover; threshold 21/40, failover (no gap over 10 minutes in any blackout).

## E4 — offline points ladder

The C5 setup (10,000 miner slots, 290,000 IDs, every region with load-shedding) for 365 days under "unreachable is not refusal"; the configured defaults: threshold entropy with t = ⌈2n/3⌉, failover off. A member earns points for every offline period of at least n, whatever its cause (1, 1, 2, 3, 4, 5, then 5 again; cap 20; reset 12 months after the latest point), recorded by a two-thirds KWC decision (`E4_points_by_region_device.csv`, `E4_h7_by_region.csv`, `E4_points_by_cause.csv`; each region's UPS share is shown):

- n = 1 h, seat_loss: 184115669 awards, 25759284 members reached 20 points (6034215 by load-shedding alone), 1884608 seats lost, 0 IDs blacklisted; highest yearly loss rate 946.91 % (as_south)
- n = 1 h, blacklist: 7702196 awards, 941820 members reached 20 points (65203 by load-shedding alone), 0 seats lost, 941820 IDs blacklisted; highest yearly loss rate 1199.93 % (as_south); **H7 fails** in na_east, na_west, sa_east, eu_west, af, as_south, as_east, oc
- n = 6 h, seat_loss: 89868105 awards, 12352794 members reached 20 points (5816 by load-shedding alone), 680149 seats lost, 0 IDs blacklisted; highest yearly loss rate 150.60 % (na_west)
- n = 6 h, blacklist: 6137114 awards, 639416 members reached 20 points (287 by load-shedding alone), 0 seats lost, 639416 IDs blacklisted; highest yearly loss rate 206.28 % (na_west); **H7 fails** in na_east, na_west, sa_east, eu_west, af, as_south, as_east, oc
- n = 24 h, seat_loss: 1866186 awards, 3348 members reached 20 points (0 by load-shedding alone), 3348 seats lost, 0 IDs blacklisted; highest yearly loss rate 0.50 % (af)
- n = 24 h, blacklist: 1864879 awards, 3342 members reached 20 points (0 by load-shedding alone), 0 seats lost, 3342 IDs blacklisted; highest yearly loss rate 0.50 % (af); H7 (≤ 1 % of honest IDs a year, per region) holds in every region

## Invariants of every run

**2 of 3 hold** across all 2978 runs: recounted caches and counters, self-validation of every broadcast block, and no reorg below a confirmed ID (`validation.csv`, section ALL; every run in `ALL_runs.csv`). Reorgs below a confirmed ID (a confirmed ID's block replaced) occurred in 16: `C6-skew0.5-delta0.5-EarliestWithinTolerance-Permanent` (1, deepest 9), `C6-skew0.5-delta2-EarliestWithinTolerance-Permanent` (3, deepest 12), `C6-skew0.5-delta10-EarliestWithinTolerance-Permanent` (1, deepest 8), `C6-skew1-delta0.5-OnTime-Permanent` (1, deepest 8), `C6-skew1-delta0.5-EarliestWithinTolerance-Permanent` (2, deepest 9), `C6-skew1-delta2-EarliestWithinTolerance-Permanent` (7, deepest 12), `C6-skew1-delta10-EarliestWithinTolerance-Permanent` (4, deepest 12), `C6-skew2-delta0.5-OnTime-Permanent` (4, deepest 12), `C6-skew2-delta0.5-EarliestWithinTolerance-Permanent` (5, deepest 12), `C6-skew2-delta2-EarliestWithinTolerance-Permanent` (6, deepest 12), `C6-skew2-delta10-EarliestWithinTolerance-Permanent` (4, deepest 12), `E1-(a) v0.6 literal D10-soft-honest-s2` (1, deepest 11), `E1-(a) v0.6 literal D10-soft-withholder-s1` (1, deepest 7), `E1-(a) v0.6 literal D10-soft-withholder-s4` (1, deepest 10), `E1-(r) v0.6 literal D10-soft-withholder-s3` (1, deepest 12), `E1-(r) v0.6 literal D10-soft-withholder-s4` (1, deepest 7).

**Fork storms stopped 47 runs early**, when more blocks than `sim.world.fork_storm_unsettled` were unsettled at once (SIM-29): `C2-v0.6-same-parent-loss0` (day 1.24), `C2-v0.6-same-parent-loss0.02` (day 0.54), `C2-v0.6-same-parent-loss0.05` (day 2.05), `C2-lower-hash-only-same-parent-loss0` (day 0.02), `C2-lower-hash-only-same-parent-loss0.005` (day 0.10), `C2-lower-hash-only-same-parent-loss0.02` (day 0.18), `C2-lower-hash-only-same-parent-loss0.05` (day 0.90), `E1-(a) v0.6 [P]same-parent D10-soft-honest-s0` (day 2.11), `E1-(a) v0.6 [P]same-parent D10-soft-honest-s1` (day 3.76), `E1-(a) v0.6 [P]same-parent D10-soft-honest-s2` (day 1.38), `E1-(a) v0.6 [P]same-parent D10-soft-honest-s3` (day 1.48), `E1-(a) v0.6 [P]same-parent D10-soft-honest-s4` (day 0.01), `E1-(a) v0.6 [P]same-parent D10-soft-withholder-s0` (day 0.56), `E1-(a) v0.6 [P]same-parent D10-soft-withholder-s1` (day 0.03), `E1-(a) v0.6 [P]same-parent D10-soft-withholder-s2` (day 0.45), `E1-(a) v0.6 [P]same-parent D10-soft-withholder-s3` (day 0.55), `E1-(a) v0.6 [P]same-parent D10-soft-withholder-s4` (day 0.55), `E1-(a) v0.6 [P]same-parent no-deadline-honest-s0` (day 2.14), `E1-(a) v0.6 [P]same-parent no-deadline-honest-s1` (day 0.76), `E1-(a) v0.6 [P]same-parent no-deadline-honest-s2` (day 1.44), `E1-(a) v0.6 [P]same-parent no-deadline-honest-s3` (day 0.59), `E1-(a) v0.6 [P]same-parent no-deadline-honest-s4` (day 0.01), `E1-(a) v0.6 [P]same-parent no-deadline-withholder-s0` (day 0.04), `E1-(a) v0.6 [P]same-parent no-deadline-withholder-s1` (day 0.02), `E1-(a) v0.6 [P]same-parent no-deadline-withholder-s2` (day 0.01), `E1-(a) v0.6 [P]same-parent no-deadline-withholder-s3` (day 0.03), `E1-(a) v0.6 [P]same-parent no-deadline-withholder-s4` (day 0.04), `E1-(r) v0.6 [P]same-parent D10-soft-honest-s0` (day 2.93), `E1-(r) v0.6 [P]same-parent D10-soft-honest-s1` (day 3.01), `E1-(r) v0.6 [P]same-parent D10-soft-honest-s2` (day 1.88), `E1-(r) v0.6 [P]same-parent D10-soft-honest-s3` (day 1.79), `E1-(r) v0.6 [P]same-parent D10-soft-honest-s4` (day 0.01), `E1-(r) v0.6 [P]same-parent D10-soft-withholder-s0` (day 0.71), `E1-(r) v0.6 [P]same-parent D10-soft-withholder-s1` (day 0.18), `E1-(r) v0.6 [P]same-parent D10-soft-withholder-s2` (day 0.20), `E1-(r) v0.6 [P]same-parent D10-soft-withholder-s3` (day 0.41), `E1-(r) v0.6 [P]same-parent D10-soft-withholder-s4` (day 0.38), `E1-(r) v0.6 [P]same-parent no-deadline-honest-s0` (day 4.74), `E1-(r) v0.6 [P]same-parent no-deadline-honest-s1` (day 2.60), `E1-(r) v0.6 [P]same-parent no-deadline-honest-s2` (day 9.72), `E1-(r) v0.6 [P]same-parent no-deadline-honest-s3` (day 4.29), `E1-(r) v0.6 [P]same-parent no-deadline-honest-s4` (day 0.01), `E1-(r) v0.6 [P]same-parent no-deadline-withholder-s0` (day 0.04), `E1-(r) v0.6 [P]same-parent no-deadline-withholder-s1` (day 0.11), `E1-(r) v0.6 [P]same-parent no-deadline-withholder-s2` (day 0.16), `E1-(r) v0.6 [P]same-parent no-deadline-withholder-s3` (day 0.04), `E1-(r) v0.6 [P]same-parent no-deadline-withholder-s4` (day 0.04). Their tables cover the time before (`ALL_runs.csv`, column `fork_storm_day`).

**The skip rule (SIM-48).** 2481 rounds ran out of draws without a block anyone extends and drew their skipped slots again, in 28 runs (`ALL_runs.csv`, column `round_refreshes`). Before their round's first block, 155079298 drawn slots were skipped because their miner could not mine (108291390 not mining, 0 offline, 46787908 with a KWC unable to issue entropy); 694367 of them could mine again by that block. Had they been drawn again once able, they could have added at most 367742.8 wins over 65446121 rounds, 5.619 per 1,000 rounds: an upper bound on what the rule took from issuance (at most 7268.253 in one run, `C6-skew2-delta2-EarliestWithinTolerance-Permanent`). By section, per 1,000 rounds: A6 0.000, C1 0.002, C2 0.094, C3 25.137, C4 0.172, C5 0.000, C6 9.680, C7 0.017, E1 17.777, E2 0.115, E3 1.363, E4 0.000, V0 0.001, V1 0.000, V2 0.001, V3 0.000.

