# M3a Tier 1 issuance simulator — summary

- Git commit: `44b0f8c158fe6a11a416db2a5fc9766ed5db69e3` (clean working tree)
- Seed: 20261005
- Configuration SHA-256: `fa7baeaa43d34d7a5d0cc20362d68f648bc03ecaa25f2c81ef4a3f7a911489a2`
- Protocol version: 1
- Profile: full
- Sections: V0, W3
- Runs: 530

Every run is seeded and single-threaded, so these tables are reproducible bit for bit whatever the thread count; wall times are machine-dependent and are in `perf/`. Every protocol decision goes through `gb-protocol` (SPEC v0.7). Values marked as checks are compared with M1's closed forms or with exact expectations at 5 standard errors (`validation.csv`); every other number is a measurement, reported as it came out.

## Validation

**40 of 40 checks pass, 0 fail and 0 are reference-limited.** The V0 gate passed, so the stand-in and fast modes are used for the other sections. A reference-limited check is kept and reported, but its reference does not model what the simulator does, so it is not a failure either way (ASSUMPTIONS SIM-31).

## V0 — mode agreement (the gate)

The three crypto modes agree, and the fast sampler is exact, when every test below has p ≥ 0.001. **37 of 37 gate checks pass.** Details in `V0_mode_agreement.csv` and `validation.csv`.

## W3 — the pause at the start of a blackout of the largest region, over many seeds (supplementary)

Not part of the reference run (architect, 2026-10-27); run with `gb sim --only W3`. E3's blackout of as_east (10000 miner slots, 29000 IDs, the configured entropy rule and failover; a 10.88-hour blackout from day 1.50) repeated over many seeds, of which 0–4 are E3's own, in each of six settings: the early-block rule (re-evaluation or permanent rejection) with no PoWit deadline or D = 10 s (soft scope), and, as controls, both rules with no badly synchronised clocks (the 1 % of nodes whose clocks have σ = 2 s set right: no node then receives a block early, so the two rules run alike). The pause is the gap between the last final-chain block broadcast before the blackout began and the first after it; its expected wait is that of the miners able to get entropy during it (the mean over the round samples) at the round's difficulty, and the ratio of the pause to it is compared where at least 100 miners could (`W3_blackout_seeds.csv`, `W3_blackout_cells.csv`, `W3_comparisons.csv`). A pause that is only the difficulty lag plus chance has an exponential ratio, half of it below ln 2.

- re-evaluation, no deadline (the configured defaults): 100 blackouts; the pause's median 457 s (7.6 minutes), mean 641 s, longest 2947 s (49.1 minutes); over 10, 20 and 30 minutes in 41 %, 13 % and 6 %; its ratio to the expected wait, where at least 100 miners could get entropy (85 pauses): median 1.05 (an exponential's is 0.69), 28 below ln 2 (z = -3.15); afterwards the next 20 blocks came 30.6 s apart and the rest of the blackout's blocks 30.5 s apart (medians over the seeds), and the longest later gap during the blackout was 2364 s.
- re-evaluation, D = 10 s: 100 blackouts; the pause's median 464 s (7.7 minutes), mean 648 s, longest 2947 s (49.1 minutes); over 10, 20 and 30 minutes in 42 %, 13 % and 6 %; its ratio to the expected wait, where at least 100 miners could get entropy (86 pauses): median 1.05 (an exponential's is 0.69), 28 below ln 2 (z = -3.23); afterwards the next 20 blocks came 30.6 s apart and the rest of the blackout's blocks 30.6 s apart (medians over the seeds), and the longest later gap during the blackout was 2989 s.
- permanent rejection, no deadline: 100 blackouts; the pause's median 309 s (5.1 minutes), mean 478 s, longest 3672 s (61.2 minutes); over 10, 20 and 30 minutes in 27 %, 9 % and 4 %; its ratio to the expected wait, where at least 100 miners could get entropy (78 pauses): median 0.69 (an exponential's is 0.69), 39 below ln 2 (z = 0.00); afterwards the next 20 blocks came 31.4 s apart and the rest of the blackout's blocks 30.9 s apart (medians over the seeds), and the longest later gap during the blackout was 1855 s.
- permanent rejection, D = 10 s: 100 blackouts; the pause's median 309 s (5.1 minutes), mean 478 s, longest 3672 s (61.2 minutes); over 10, 20 and 30 minutes in 27 %, 9 % and 4 %; its ratio to the expected wait, where at least 100 miners could get entropy (78 pauses): median 0.69 (an exponential's is 0.69), 39 below ln 2 (z = 0.00); afterwards the next 20 blocks came 31.4 s apart and the rest of the blackout's blocks 31.0 s apart (medians over the seeds), and the longest later gap during the blackout was 2420 s.
- no badly synchronised clocks, re-evaluation: 100 blackouts; the pause's median 279 s (4.6 minutes), mean 560 s, longest 4021 s (67.0 minutes); over 10, 20 and 30 minutes in 29 %, 17 % and 7 %; its ratio to the expected wait, where at least 100 miners could get entropy (85 pauses): median 0.62 (an exponential's is 0.69), 47 below ln 2 (z = 0.98); afterwards the next 20 blocks came 30.6 s apart and the rest of the blackout's blocks 30.7 s apart (medians over the seeds), and the longest later gap during the blackout was 2165 s.
- no badly synchronised clocks, permanent rejection: 20 blackouts; the pause's median 199 s (3.3 minutes), mean 355 s, longest 1639 s (27.3 minutes); over 10, 20 and 30 minutes in 20 %, 5 % and 0 %; its ratio to the expected wait, where at least 100 miners could get entropy (16 pauses): median 0.58 (an exponential's is 0.69), 11 below ln 2 (z = 1.50); afterwards the next 20 blocks came 29.7 s apart and the rest of the blackout's blocks 30.9 s apart (medians over the seeds), and the longest later gap during the blackout was 1787 s.

The same pause with and without D = 10 s in 99 of 100 seeds under re-evaluation and 100 of 100 under permanent rejection; with no badly synchronised clocks the two rules gave the same pause in 20 of 20 seeds. Rank tests (Mann–Whitney, two-sided): re-evaluation against permanent rejection p = 0.014 on the pauses and 0.069 on the ratios with no deadline (0.009 and 0.063 with D = 10 s); re-evaluation against no badly synchronised clocks 0.060 and 0.017; permanent rejection against no badly synchronised clocks 0.811 and 0.429.

## Invariants of every run

**3 of 3 hold** across all 530 runs: recounted caches and counters, self-validation of every broadcast block, and no reorg below a confirmed ID (`validation.csv`, section ALL; every run in `ALL_runs.csv`). No run reorganised the network chain below a confirmed ID.

**Fork storms stopped 1 runs early**, when more blocks than `sim.world.fork_storm_unsettled` were unsettled at once (SIM-29): `W3-as_east-s45-perm-d10` (day 1.97). Their tables cover the time before (`ALL_runs.csv`, column `fork_storm_day`).

**The skip rule (SIM-48).** 805 rounds ran out of draws without a block anyone extends and drew their skipped slots again, in 227 runs (`ALL_runs.csv`, column `round_refreshes`). Before their round's first block, 166347567 drawn slots were skipped because their miner could not mine (103515410 not mining, 0 offline, 62832157 with a KWC unable to issue entropy); 574825 of them could mine again by that block. Had they been drawn again once able, they could have added at most 1242253.0 wins over 3883642 rounds, 319.868 per 1,000 rounds: an upper bound on what the rule took from issuance (at most 8087.570 in one run, `W3-as_east-s25-noslow-reeval`). By section, per 1,000 rounds: V0 0.001, W3 325.293.

