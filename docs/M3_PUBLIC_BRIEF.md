# GrahamBell Stage 1 — M3a and M3a.1 issuance simulator: public brief

> Results describe the SPEC version and settings they were run with; see the disclaimer in [README.md](../README.md#status-and-disclaimer).

GrahamBell is a proposed Layer 1 blockchain. It issues new identities (IDs) one at a time at a fixed global pace, one every 30 s on target, through proof of work capped at one hash per second per participant. Witness Chains of other participants supply each round's entropy and enforce the cap. Its claim is that taking control requires sustained participation over years, not a burst of hardware or capital. M1 computed the exact consequences of the rules ([M1 brief](M1_PUBLIC_BRIEF.md)); M3a and M3a.1 simulate them on a realistic network.

## What M3a and M3a.1 tested, and why

M1 treated the network as ideal: no latency, no outages, a perfectly random assignment of witnesses. M3a builds a discrete-event simulator of ID issuance at 1,000 to 100,000 miners. It models:

- measured latency between 8 world regions (RIPE Atlas);
- packet loss, regional partitions and clock skew;
- household power cuts, load-shedding and internet outages, correlated within a region.

The simulator first checks itself against M1 and M2 (the validation steps V0–V4), then runs honest baselines.

M3a.1 tests the rules that M3a found fragile, with selection criteria fixed before the runs:

- fork choice and the start rule (E1);
- the difficulty count and its correction (E2);
- liveness under regional blackouts, with threshold entropy and failover (E3);
- the offline penalty ladder (E4).

The rules are those of SPEC v0.7. The hypotheses H1–H9 and their thresholds were locked before M3, and nothing was tuned to pass them.

## What held

- **M1 reproduced.** Where M1's assumptions apply, the simulator matches it, and the M1 and M2 reference tables still reproduce byte for byte. In the reference run, 178 of 210 checks pass. The 2 failures are in comparison settings, not the defaults. The other 30 checks are reported as "reference-limited": their reference does not model what the simulator does.

  | Measure | Simulated | M1 |
  |---|---|---|
  | Blocks to 51 % of active IDs, at a 60 % attacker share | 11,362 | 11,336 |
  | Long-run attacker share | 0.6816 | 0.6818 |
  | Same-step tie rate | 0.01660 | 0.01656 |

  Witness seat-capture odds match exactly, and so do group failure rates under independent outages.
- **Issuance stays near 30 s.** Under realistic household churn, the default difficulty keeps the mean interval at 30.23 s, with a daily spread of 0.87 % (30.27 s under a laptop-heavy device mix). The default counts the miners whose Witness Chain issued entropy for the round, with a correction bounded at ×1.1.
- **Withholding does not pay.** A 10 % miner that holds back its blocks to release them strategically wins about 15 % of its fair share at the defaults, wherever it sits on the network. With a 10 s PoWit deadline it wins almost nothing.
- **Restarting does not pay.** Reconnecting to fish for early wins earns less than a tenth of what staying connected earns, because entropy is fresh every round.
- **Difficulty hopping stays within its limit.** Miners that join and leave to exploit the difficulty's lag gain at most 1.7 %, inside the locked limit of 2 % (H5).
- **Honest participants are protected.**
  - Under the earlier strict One Chance rule, ordinary household outages would have banned honest IDs at 1,337 % to 62,746 % a year. Under the v0.7 default, "unreachable is not refusal", the rate is 0 %.
  - The offline points ladder (24-hour threshold) costs honest IDs at most 0.50 % a year in every region, inside the 1 % limit (H7).
  - With every node's clock skewed (standard deviation 2 s), re-evaluating early blocks splits no validator off the chain. Rejecting them for good split 16 of 101.

## What failed and was fixed

M3a.1 found five faults in the simulator, fixed each one and re-measured its effect (details in [ASSUMPTIONS.md](ASSUMPTIONS.md)):

- **Region-wide feeders and providers (SIM-35).** Every household of a region shared one power feeder and one internet provider, so every local fault took the whole region offline. M3a's 11-hour stall was such an artefact.
- **Entropy below the quorum (SIM-43).** During outages, Witness Chains with fewer than the 27 members required still issued entropy, so stalls were shorter than the rules allow.
- **Rounds that ran out of draws (SIM-48).** A round drew each miner once, so under heavy churn it could run out and stop the chain for good. A round that runs out now draws again. What the remaining rule can cost is bounded: at most 1.9 IDs per 1,000 rounds in the reference run.
- **Test groups placed in one region (SIM-51).** The honest groups used to check linearity all sat in one region, so the check measured that region instead of the network.
- **Late blocks (commit 24eecb0).** A block that arrived after its deadline still closed the round below it, which could stall the chain.

One known limitation is documented but not fixed (SIM-25). The simulator stops a round once every validator holds a higher block, which assumes every miner accepts that block. Runs in which many miners reject blocks for good are therefore approximate. Those runs are comparison settings only.

## Against the M1 brief

- **Confirmed:**
  - the long-run share s / (s + f(1 − s));
  - the time to 51 % counted in blocks;
  - same-step tie rates;
  - the seat-capture odds;
  - the end of the restart advantage once entropy is fresh every round.
- **Refined:**
  - **Issuance rate.** The measured interval is 30.23 s, so about 1.04M IDs a year rather than 1,051,200, and times in years lengthen by about 0.75 %. M1's 16.4 years (an attacker winning 40 % of new IDs, half of honest IDs online, 2.9M genesis IDs) becomes about 16.5 years.
  - **Old-design restart advantage.** M1's formula leaves out two effects the simulator models. At the simulator's size the advantage came out 12–22 % below that formula, and the conclusion stands.
- **Contradicted:**
  - **Group liveness under correlated outages.** M1's uptime figures assume members are absent independently. With correlated household outages, 6.1–6.6 % of ID-issuance groups are below 27 of 40 at any moment, where M1 predicts under 1 % at the same online fraction (19–20 % against 12–13 % for transaction groups). Correlated outages, not average uptime, set liveness.

## Known limits

- **Blackout pauses.** In a deliberate stress test, an entire region holding about 38 % of witnesses loses power for about 11 hours.
  - **The pause.** Issuance pauses at the blackout's start while difficulty catches up with the miners who remain. Across 100 simulated blackouts, the median pause was about 7.6 minutes; 41 % lasted over 10 minutes, 6 % over 30 minutes, and the longest 49 minutes.
  - **After the pause.** Blocks then come every ~30 s on average, but in about a third of the blackouts a second gap of over 10 minutes followed later in the blackout (longest 39 minutes).
  - **The early-block rule.** Re-evaluating early blocks, the default, lengthens the typical pause somewhat (median 7.6 against 5.1 minutes when early blocks are rejected for good), for a reason not yet identified.
  - **A lower threshold.** At 21 of 40, every gap in E3's 41 blackouts stayed under 9.4 minutes. But a 33 % attacker would then hold enough seats to sign alone in 0.84 % of groups, against 8.5 × 10⁻⁶ at 27 of 40.
- **Regional win share.** Honest miners in some regions win up to about 3 % less than their share of mining time, and in others up to about 3 % more (0.981 in south Asia to 1.027 in eastern North America at 100,000 miners). This is accepted for now as a known caveat.
- **The PoWit deadline.** Across four runs, the fixed criteria chose no deadline, then 10 s, then none, then 10 s, because the two finalists cannot be told apart: the last flip turned on 0.002 s in one criterion. The default stays at no deadline. A 10 s deadline, which removes a withholder's remaining 15 % yield, stays proposed for the adversarial tests.
- **Scope.** All behaviour is honest apart from one withholder. Tier 1 stops at 100,000 miners, there are no money costs, and the transaction layer is modelled only as far as issuance needs.

## What comes next

- **M3b, attackers:**
  - a datacenter attacker holding 5–60 % of slots, a patient accumulator and botnets;
  - adaptive-issuance manipulation and slot exhaustion;
  - a sleeper attacker that switches behaviour at once, and ID rotation;
  - withholding under a 10 s deadline, and early release under re-evaluation;
  - whether the difficulty correction's bound can be exploited, and an always-on attacker's share by hour of day;
  - difficulty that eases within a round, to cap blackout pauses;
  - Tier 2 at millions of miners.
- **M4, the witness layer:**
  - both sides of threshold entropy (capture and blocking), and backdated start times;
  - the 30-day refresh of key shares, and separating signature validity from payment;
  - failover under correlated outages, and who may hold witness seats.
- **M5, money:**
  - what sustaining each attacker share costs, honest competition over time, and admission capacity;
  - measured witness hardware on a mini PC, an old desktop, a laptop and a phone, replacing the [calculated requirements](PROTOCOL_OVERVIEW.md#running-a-witness-node).

## How to reproduce

Every run is seeded and records its commit, configuration and file hashes in `run.json`.

| Command | What it runs | Time |
|---|---|---|
| `cargo run --release -p gb-cli -- sim` | every section | about a day with 4 threads on a laptop |
| `cargo run --release -p gb-cli -- sim --only W3` | the 100 blackouts above | about an hour |
| `cargo run --release -p gb-cli -- sim --profile quick` | tiny sizes, for tests | minutes |

A full run reproduces the committed reference in `results/sim/reference/` byte for byte, apart from the timings in `perf/`. The rules are in [SPEC.md](SPEC.md) and every modelling assumption is in [ASSUMPTIONS.md](ASSUMPTIONS.md).

All tables, summaries, check verdicts and the code that produces them are in the repository.
