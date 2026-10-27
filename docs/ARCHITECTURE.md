# Architecture — GrahamBell Stage 1

This document describes how the Stage 1 code is organised, how later milestones plug in, how the protocol core will be shared with the Stage 3 testnet, and how releases move from the private repository to the public one. It is a design document: crates marked *planned* below do not exist yet.

`docs/SPEC.md` is the source of truth for protocol rules. Nothing in this document changes a rule. The allocation algorithm in the last section began as a proposal; the architect adopted it on 2026-10-06 as SPEC §4.2 [D], and the section now records its rationale.

## 1. Goals and constraints

- **Reproducible.** One master seed, a recorded configuration and a recorded git commit fully determine every output (SPEC §11.4). Reruns are checked byte for byte through a SHA-256 manifest.
- **Ground truth first.** Every quantity with a closed form is computed exactly in M1. The simulators must match those values before their other results are trusted (SPEC §11.3).
- **Two tiers** (SPEC §11.1):
  - Tier 1 simulates protocol messages at reduced scale.
  - Tier 2 models millions of participants statistically, and is validated against Tier 1 where their scales overlap.
- **One protocol core.** The rules a simulator enforces are the same code the Stage 3 testnet runs (SPEC §11.7).
- **Configurable everything.** Every SPEC number is a typed parameter with its status tag and sweep (`gb-config`).

## 2. Crate map

| Crate | Status | Milestone | Responsibility |
|---|---|---|---|
| `gb-config` | exists | M0 | Typed parameters for every SPEC §2 row (value, status D/P/O, sweep), M1 grids, M2 measurement settings, run settings; TOML loading by deep merge over defaults. |
| `gb-runlog` | exists | M0 | Run id, git commit and dirty flag, UTC timestamp, resolved-config hash, SHA-256 manifest of outputs, seeded ChaCha20 streams. |
| `gb-analytic` | exists | M1 | Exact closed forms and exact probabilities (sections A–I), with independent cross-checks. Pure functions; no I/O. |
| `gb-cli` | exists | M0+ | Binary `gb`: `analytic`, `crypto`, `sim`, `vectors`, `params`, `config`. Writes CSV/PNG/SUMMARY, the public brief and the run log. Front end only; never recomputes results. |
| `gb-protocol` | exists | M2 | Shared protocol core (section 5): pure functions over `blst` and SHA-256, with test vectors in `vectors/`. |
| `gb-crypto-tests` | exists | M2 | Measurements with real SHA-256 and BLS12-381: S11 grinding, S14, equivocation, beacon withholding, placement grinding, timings, the threshold-BLS prototype (benchmark only), and the equivalence of the simulator's entropy stand-in. |
| `gb-sim` | exists | M3a (M3b, M4 extend it) | Tier 1 discrete-event simulator of PoW-ID issuance (section 3), its validation ladder and the M3a experiments. |
| `gb-scale` | planned | M3b (to be built), M6 (full-scale runs) | Tier 2 aggregate model at full scale. |
| `gb-bench` | planned | M5 | S27 hardware benchmark and S28 money-cost model. |

Dependencies point one way: `gb-cli` → analysis crates (`gb-analytic`, `gb-crypto-tests`, `gb-sim`, `gb-scale`, `gb-bench`) → `gb-protocol`, `gb-config`, `gb-runlog`. No analysis crate depends on `gb-cli`. `gb-protocol` uses `gb-analytic` only in its tests, to reproduce M1's ring and composition rates. `gb-sim` also depends on two analysis crates: `gb-analytic`, for the closed forms its checks compare with and the `Check` type, and `gb-crypto-tests`, to rerun M2's entropy stand-in test (C6) and reuse its χ² and Kolmogorov–Smirnov statistics.

## 3. The two tiers

### Tier 1 — `gb-sim` (protocol-level discrete-event simulation)

- **Event engine:** a priority queue of timestamped events with a deterministic tie order (time, then event sequence number).
- **Actors:** unregistered miners (/64s), registered IDs acting as witness nodes and miners, KWC proposers, and the CAC.
- **Network:** messages with regional latency distributions, loss, partitions and per-node rate limits.
- **Clocks:** per-node clocks with configurable skew, so δ-tolerance rules can be tested (S12).
- **Churn and adversaries:** household downtime profiles (H7), botnet churn (S4), and attacker strategies as pluggable policies (restart, hopping, stalling, griefing, eclipsing).
- **Cryptography:** real `gb-protocol` code where affordable. Where a statistically equivalent abstraction is used for speed, M2 must first document and test the equivalence.
- **Scale:** up to about 100,000 miners.
- **Milestones:**
  - M3 covers issuance scenarios S1–S10, including S5 restart, S6 difficulty hopping and S7 adaptive issuance. It is split in two (decided 2026-10-12):
    - **M3a (built):** the engine, the network and household models, honest growth and patience, the honest baselines (S1, S5, S6, fork choice, the PoWit deadline), the validation ladder (V0–V4) and the experiments C1–C7 and A6. Command: `gb sim`.
    - **M3b (next):** the attackers S2–S4, S7 and S8, the full S9, S10, Tier 2 `gb-scale` at millions of miners, and the hypothesis verdict table.
  - M4 covers the witness layer, S12–S26.

**How M3a is built.**

- **Crypto modes.** Real BLS and SHA-256; the M2 C6 stand-in (uniform entropy, real SHA-256 chains); and a fast mode that draws each round's winning steps exactly from the target's per-step probability (block hashes stay real SHA-256). The fast mode is used for the large runs only after the V0 gate shows all three agree.
- **Network.** Region-to-region latency from RIPE Atlas anchor-mesh pings (`data/latency/`), a gossip overlay calibrated into arrival quantiles, loss with retransmission, partitions that hold messages until they heal, and per-node clock skew.
- **Households.** Per-node on/off timelines: device schedules, crashes, glitches, grid faults and load-shedding (with UPS), provider and regional outages, long absences and departures, each tagged announced or unannounced by its cause.
- **Network chain.** 101 ideal validators spread over the regions stand for the network: the network chain is the longest chain more than half of them hold. An ID passes confirmation depth when 6 blocks are built on its block there.
- **Invariants.** Every run recounts its caches (active list, seats, WC, miner and KWC counters) daily and at the end, and every broadcast block must pass `validate_facts` at its own miner.
- **Fork storms.** After a stall of hours with no PoWit deadline, start rule (a) lets hundreds of blocks compete for one height; while one header per height was not fully enforced, some of these bursts kept growing until the run crawled. A run therefore stops once more than `sim.world.fork_storm_unsettled` (4,000) blocks are unsettled at once, and the outputs flag it (ASSUMPTIONS SIM-29). No run of the reference run comes near it, and the limit never touches a run that stays below it.

### Tier 2 — `gb-scale` (aggregate model)

- Represents populations by counts and distributions instead of individual messages: issuance shares, KWC compositions, committee compositions, attrition.
- Uses M1 closed forms wherever they are exact. Stochastic components are calibrated against Tier 1.
- **Validated against Tier 1** on overlapping scales (10³ to 10⁵ miners) before results are reported at millions (M6).

## 4. Validation ladder

Each M1 table becomes a check that a later simulator must pass.

**Standing rule for every milestone (architect, 2026-10-20):**

- Every new result that overlaps an earlier published one (M1 tables, M2 crypto results, the M1 public brief) gets a validation check against it, with the earlier value, the new value and the tolerance in `validation.csv`.
- A mismatch is never tuned away. It is reported as fail or reference-limited, with the explanation, and the public summary states it.
- Before any push to public, every published number that the new results confirm, refine or contradict is listed, so the public docs can be updated consistently (§9).

| M1 output | Later check |
|---|---|
| A1–A3, A7 time to threshold, long-run share | M3 issuance simulator (S2, S3) within its confidence interval; H2 within ±5% |
| A4 adaptive cap (checkpoints, worst phase, safety factor k = 7/6) | M3 S7 |
| A6 genesis to issue | M3/M6 with household downtime profiles |
| B1–B3 per-KWC capture/stall probabilities | M4 allocation simulation (S15) at the same p and network sizes |
| B4 distinct episodes and compositions over 10 years (adopted allocation, bans) | M4 count of distinct episodes, with compositions and the one-seat entry ratio |
| B5 the seat rule (v0.3 against v0.4 at each L) under illustrative absences | M4 with M3 household downtime profiles |
| C1–C2 committee stall/capture, entries per year, episode lengths (seat lottery) | M4 S21 with the real next-position rule and registration-order layouts |
| C3 committee departures (Little's law share) | M4 S21 simulating departures and extra draws |
| D1 restart advantage, including the realistic cost of 450 s | M3 S5 (H4) |
| E1 difficulty-hopping gain against the Variant A comparison | M3 S6 (H5), with Variant B's correction once proposed |
| F1 same-step tie rate | M3 fork/orphan rate |
| G1–G3 quorum trade-off (stall, sign alone, conflicting approvals) | M4 S15–S21 under whichever quorums the architect chooses; conflicting approvals also need the proposer rules |
| G4 ten-year episodes under each seat rule | M4 with M3 household downtime profiles |
| H1–H3 quorum feasibility under honest downtime, minimum uptime, absent seats per L | M3 household profiles (H7), M4 with the forced-signing and ban rules (§4.5–§4.6) |
| I1–I4 KWC size trade-off: security, liveness, load, on-demand connections | M2 threshold BLS key setup at each size, M4, M5 household limits (router tables, upload) |
| M2 `gb-protocol` test vectors (`vectors/protocol_v1.json`) | the M3 simulator and the Stage 3 node reproduce every hash, signature and verdict |
| M2 allocation reproduces M1's 4.7 and 4.6 compositions per insertion and removal | M4 allocation simulation (S15) |
| M2 C1 grinding advantages (old flows grow with G; current flow 1) | M3 S11/H3 under network timing |
| M2 C3 beacon withholding `q / (1 − s(1 − q))` | M4 with real block timing and beacon offsets |
| M2 C4 timings and the §13 server estimate | M5 S27 hardware benchmark; Stage 3 staging run |
| M2 C5 threshold-BLS setup cost | the M4 decision on SPEC §12 [P] threshold entropy |
| M2 C6 entropy stand-in equivalence | M3 may use the stand-in only while this check passes |
| M2 C1(c′) online-set lever (1.88× for one colluding member) | M4 S11(f) under the strict rule, the [P] rule and threshold BLS entropy |
| M2.1 witness double-signing detection (`witness_equivocators`) | M4 ban scenarios |
| M2.1 optimistic entropy verification (SPEC §3.4 [P]) | M4/M5 load model |
| M3a V0 gate: the three crypto modes agree, the gap sampler is exact, the horizon audit finds nothing | every Tier 1 result: the stand-in and fast modes are used only while it passes |
| M3a V1–V4 (M1 A1/A7/F1, D1, E1 and the M2 vectors in the simulator) | M3b attacker scenarios on the same engine |
| M3a C5 One Chance costs, honest penalties and IDs lost per year by region and device | H7/H8 (M3b, M4) |
| M3a C7 KWC quorum liveness under household profiles | M4 witness layer |
| M3a Tier 1 at 10³–10⁵ miners | Tier 2 `gb-scale` validation (M3b), then full scale (M6) |
| M1 G1 binomial seat tails (at least 21 or 27 of 40) | M3a.1 E3 security table, exact (`E3-security-*`) |
| M1 E1 Variant B (no first-order hopping gain) | M3a.1 E2, each count rule without a correction (`E2-hop-*`) |
| M1 H1 KWC failure at an online fraction (independent absences) | M3a.1 C7 household profiles and E3's C7 setups, reference-limited (`C7-households-*`, `E3-C7-*`) |

### Planned for M4–M6 (decided 2026-10-09)

- **M4** compares two quorum regimes, each with and without threshold BLS entropy, at KWC sizes of 40, 60, 100 and 200 seats:
  - two-thirds everywhere;
  - the split rule: 51% for the PoWit and for threshold-BLS entropy, two-thirds for bans, MOBu/MOBr and offline records. Under the split rule the entropy threshold is also 51%.

  M4 also measures the online-set grinding lever (S11(f)) and the witness double-signing rule.
- **M5** benchmarks household load at 30, 40, 60, 100 and 200 seats.
- **M6** reruns the analysis and simulations with the chosen configuration, and publishes the selection trade-offs alongside the final results.

**[P] proposals recorded in SPEC v0.4 §12 and where they are tested:**

- separate PoWit validity from payment (any 27 of 40 is valid; an unsigned leader member's share is never generated): M4 against 7 + 21, on liveness (H), leaders' incentives and issuance per block;
- the split rule, a lower PoWit-only quorum: M4 (G2 maps it);
- larger KWCs: M2 and M5 (I maps them);
- threshold BLS entropy: M2 measures the key-setup cost;
- witness peer topology option 2 (no standing witness-to-witness connections): M3 and M5 (I4 estimates the on-demand connections).

**Decided 2026-10-12 for M4:**

- a [P] threshold-BLS randomness beacon to replace the block-hash beacons for seat placement and the committee lottery; M4 compares it with the current beacons;
- in the meantime strict One Chance stays the default, and "unreachable is not refusal" is a [P] comparison only (superseded on 2026-10-14: "unreachable is not refusal" is the default).

**Added to M4 on 2026-10-21 (architect, after the M3a.1 report):**

- threshold entropy, now the [P] default at t = ⌈2n/3⌉: the blocking side of the threshold (n − t + 1 refusing or unreachable members stop a KWC's entropy: 14 at t = 27 against 20 at t = 21) next to the capture side (t colluders), as SPEC scenario S31; and S30 (backdated `t0` under rule (r)) at both thresholds;
- failover ([P], off by default), revisited with correlated outages;
- **witness eligibility:** witness seats require an always-on device class or a minimum uptime record (for example at least 95 % over 30 days); laptops can mine but hold no seats. Compare with the current rule on the daily KWC availability trough (ASSUMPTIONS SIM-50), honest seats lost, the share of households eligible per region, and the attacker's seat share (its servers are always on: measure whether eligibility raises it). Also measure witness concentration by provider/ASN, and compare a per-ASN seat cap (data-centre ASNs especially) with no cap, on the attacker's seat share and on honest households' eligibility. Sweep network growth with (a) the same device mix, (b) a regional spread shifting towards the Americas and (c) a rising share of always-on witnesses, and report the daily KWC trough for each with and without failover at 2× capacity, to separate what growth fixes from what it does not.

### Backlog (planned, not M3a.1; architect, 2026-10-20)

- **M3b, adversarial: a sleeper attacker** that accumulates IDs quietly for years and keeps them active, then switches behaviour all at once.
- **M3b, adversarial: a rotation attack** (architect, 2026-10-21; the idea came from a suggestion on Reddit). The attacker keeps a pool of IDs and deactivates and re-activates them, timed just before committee lotteries, seat beacons and KWC decisions, to inflate its share of the canonical active list at the moments that count. Measure its gain in committee and KWC seats against an always-on attacker of the same size, under the current re-activation delay (placement at the beacon 6 blocks after the re-activation is confirmed) and longer delays.
- **M3b: adversarial withholding with the PoWit deadline,** D = 10 s with the soft scope ([P] since 2026-10-24, no deadline being the default; the decision is revisited only with M3b's evidence): in the reference run at the final defaults it cut a 10 % withholder's amplification to 0.002, against 0.154 without a deadline (0.148 in 0b711f5, 0.204 in the corrected E1), and the supplementary set W1 found the same wherever the withholder sits (spread over the regions 0.001 against 0.154; all in oc 0.0004 against 0.148; ASSUMPTIONS SIM-51). At the 1d9ce91 defaults (permanent early-block rejection), with the withholder in oc, one of five runs with D = 10 s reorganised 7 blocks below confirmation: explain it with the P1 + deadline reorganisations below.
- **M3b, rotation logging:** at every committee lottery, seat beacon and KWC decision, log the share of influence held by IDs active for fewer than N blocks (N swept), honest-only runs against the rotating attacker (architect, 2026-10-21; from a Reddit review).
- **M3b: the regional win-share deficit** (architect, 2026-10-22; published as a known caveat, SPEC §3.7): under the defaults each region's honest miners win between 0.981 (as_south) and 1.027 (na_east) of their share of mining time at 100,000 miners (C1 in the reference run at the final defaults; ASSUMPTIONS SIM-51). Explain it, separating tie losses from literal sit-outs, both correlated by region, and test mitigations.
- **M3b: an adversarial early release under re-evaluation** (architect, 2026-10-24). With re-evaluation the default early-block rule, a block sent before its timestamp is declined and reconsidered rather than rejected for good; test whether releasing blocks early, to position them or to probe nodes' clocks, gains a withholder or an attacker anything. Also confirm with more seeds, and explain, E3's isolation finding (2026-10-26; the supplementary section W3; ASSUMPTIONS SIM-52): re-evaluation lengthens the typical pause at a blackout's start (median 457 s against 309 s under permanent rejection, over 100 seeds), through the 1 % of nodes with badly synchronised clocks.
- **M3b: P2 ×1.1 at its bound** (architect, 2026-10-24). The correction sits at its bound in 21–27 % of honest 144-block windows and 57–66 % with a 10 % withholder (reference run 1d9ce91; 22–27 % and 57–64 % in the reference run at the final defaults); measure whether that saturation can be exploited, for example by hopping or withholding timed to the windows when the factor is pinned.
- **M3b: the P1 + deadline reorganisations.** In the corrected E1, with the P1 correction and D = 10 s, 6 of 15 reruns reorganised 7–8 blocks below confirmation (with P2 none did, nor did any run without a deadline); explain them.
- **M3b: an always-on attacker's share of IDs by UTC hour** (architect, 2026-10-26), under laptop-heavy and realistic churn. Under laptop-heavy churn at 21 UTC only 0.6 % of KWCs can issue entropy and 0.2 % of miners can get it (24-hour mean 50.2 %; realistic churn 82.2 % and 56.7 % at its worst hour, mean 78.5 %; `E2_daily_curves.csv`, ASSUMPTIONS SIM-50). The M5 fixed-budget item prices the same trough.
- **M3b: within-round difficulty relaxation** (real-time targeting; architect, 2026-10-26): the target eases as the time since the parent grows, capping pauses when most miners vanish mid-round. E3's longest pauses are one round of difficulty lag at a blackout's start (ASSUMPTIONS SIM-52). Measure the pause length in E3's blackouts and also the hopping gain (H5) and withholder amplification, since relaxation can create new timing incentives (SPEC §12).
- **Admission variants:** caps per /48 and per provider or ASN, against /64-only admission; measure what they cost honest households behind shared ISPs. Include an attacker spread across many small hosting providers (sweep the number of providers and the cost per extra provider), to test whether the caps hold when provider identity is cheap, for admission and for witness seats (architect, 2026-10-21; from a Reddit review).
- **M5, cost model:**
  - *Per-slot load:* connections per slot (one per KWC member, 40), bytes per round (about 12 KB per slot per 30 s, estimated) and CPU per round, split into protocol-forced work (header signing 0.379 ms, sequential SHA-256 steps 0.969 µs each, and transport security on 40 witness connections: TLS/QUIC handshakes, keepalives and encryption of every message, to be benchmarked) and optional work (verifying the entropy aggregate, 4.397 ms at 40 members); timings from `results/crypto/reference/bench/K4_timings.csv`. The attacker is priced at the forced minimum, the optional work reported separately. Derive the cores, the memory for 4M+ connections, the servers and the monthly cost alongside bandwidth for 10k, 100k, 1M and 10M attacker slots (10M slots means 400M witness connections, close to the usable unregistered capacity, so report what happens when the attacker alone would nearly fill admission: honest miners crowded out, or the attacker blocked by the cap).
  - *Competition:* the attacker needs s/(1 − s) × H slots to win share s against H honest slots (registered IDs that keep mining included in H). Report the monthly and total cost for s ∈ {10, 25, 33, 40, 51} % against H ∈ {100k, 1M, 10M}, sustained over the time-floor horizon. Honest competition over time is an assumption with a sweep (growth, flat, decline), reporting how the attacker's cost and time to threshold change in each.
  - *Capacity:* from the SPEC defaults, 2.9M genesis IDs give 290,000 WCs × 50 unregistered per leader WC = 14.5M unregistered places, about 10.9M usable at the +33 % spare-capacity target; capacity grows with registered IDs (every 10 new IDs form a WC, adding room for 50 unregistered miners). Genesis is swept over {1M, 2.9M, 10M} (1M gives 5M places). When admission is full, report who is admitted and whether an attacker can crowd out honest miners.
  - *Full grid:* attacker slots A ∈ {10k, 100k, 1M, 10M} × honest fill h ∈ {0, 10, …, 100} % of usable capacity × genesis {1M, 2.9M, 10M} × honest online fraction f ∈ {1.0, 0.8, 0.5} (attacker online 100 %). Per cell, never dropped: whether A fits and what admission does if it does not; the attacker's share of new IDs s; the time to 33, 51 and 67 % of active IDs, t = T·f·G / (R·(s·(1 − T) − T·f·(1 − s))) or "never" when the denominator is not positive (M1's realistic variant; t = T·G / (R·(s − T)) at f = 1, the C2 formula at s = 1; check cell s = 40 %, f = 0.5, G = 2.9M, T = 51 %: 16.4 years); the monthly cost and the total cost to each threshold; the load on honest witnesses (verify and sign per watched miner) and how capacity limits bound it. One table per A and a chart of time to 51 % and total cost against h; weak (low-h) cells are shown as prominently as strong ones.
  - *Fixed attacker budget* (architect, 2026-10-21; from a Reddit review): hold the attacker's slots constant and sweep the honest online fraction f, including the measured daily trough by UTC hour (SIM-50); report the attacker's share of new IDs and its time to threshold, so the gain from honest outages alone is visible.
  - *Cost per point of influence* (same source): in every cell, the monthly cost ÷ the attacker's share of new IDs, and the total cost per percentage point at each threshold, so cells compare across attacker sizes.
  - *Witness hardware* (architect, 2026-10-21): benchmark a witness node at full watch load (1,000 watched miners) on a mini PC, an old desktop, a modern laptop and a current phone; report CPU, RAM, bandwidth per month, and whether each can stay online (sleep, background limits, NAT). Publish the results as a "Hardware requirements" README section.
  - *Honest multi-slotting,* an assumption with a sweep: {1, 10, 50} % of households × {1, 4, 16, 256} slots per household.
  - *Admission granularity,* compared: pacing per /64, /56 and /48, each with and without per-provider or ASN caps (tunnel brokers that hand out free /48s, identified by ASN); report slots per household, slots per free /48 for an attacker, and the honest households hurt (shared ISPs, CGNAT, mobile /64).

## 5. `gb-protocol` and the Stage 3 testnet

**Status (M2):** built in `crates/gb-protocol`, following SPEC v0.5 Appendix A [P]. Cryptography comes only from `blst` and `sha2`.

`gb-protocol` holds protocol rules as pure, deterministic functions with no networking, storage or clock access:

- **§3.3 headers:** fixed-length encodings and domain tags.
- **§3.4 entropy lock:** miner signature, member signatures, BLS aggregate (via `blst`), and `E = SHA256("GB/entropy-out" ‖ aggregate ‖ bitfield)`.
- **§3.5 hash chain:** `h_n`, start rule (a), and rule (b) for S13.
- **§3.6 witness signing condition** and **§3.7 validation:** quorum with bitfield and proof of possession, full chain recomputation, timestamp rules, the same-height tie-break, uniqueness.
- **§3.8 difficulty** Variant B (default) and the Variant A comparison, and **§3.9 issuance** (fixed; adaptive with the checkpointed cap and its safety factor).
- **§3.11 registration index** and the canonical active list.
- **§4.2 allocation** (section 8), with the v0.4 seat rule, and the **§4.3 seat lottery** with its extra draws, all recomputed by every node from chain data.

**Sharing.** `gb-sim` calls these functions directly, and so will the Stage 3 node. M2 produces test vectors (inputs and expected hashes, signatures and validation verdicts) that both the simulator and the testnet node must reproduce. A rule change therefore lands in one place and is caught by the vectors everywhere.

**Versioning.** `gb-protocol` carries a protocol version. Simulator outputs record it next to the git commit.

## 6. Determinism and outputs

- **Randomness:** one master seed. Each component draws from `rng_stream(seed, label)`, a ChaCha20 stream seeded by `SHA-256(seed ‖ label)`. Samplers (uniform, geometric, exponential) are written in-repo, so outputs do not depend on another crate's sampling algorithms.
- **Arithmetic:**
  - Exact rationals for tail probabilities.
  - `libm` (pure Rust) for transcendental functions, so results do not depend on the platform math library.
  - No hash-map iteration in output paths.
- **Parallelism.** Tier 1 runs execute in parallel on scoped threads, but each run is single-threaded and seeded and its result is stored by its index in the job list, so the thread count never changes an output. Within a run, random numbers are drawn by key (purpose, entity, index) through ChaCha20 random access, so a value does not depend on the order in which the simulation asks for it. Tier 2 will use deterministic partitioning and ordered reductions only.
- **Speed without changing results.** Each of these was checked to leave every output byte-identical:
  - timers set a constant delay after the current time (One Chance retries, deactivation and long-absence checks) wait in FIFO lanes that share the event heap's sequence numbers;
  - the canonical active list is a Fenwick tree, so the committee lottery reads it without copying it;
  - a keyed draw computes only the ChaCha20 block it needs, bit-identical to `rand_chacha` (a test compares them);
  - each node's acceptance of an unsettled block is cached: its own reception for good, the acceptance through the parent chain until the next height settles. The cache holds at most `sim.acceptance_cache_entries` entries (2 million, about 160 MB); past that it is emptied and refills as needed, which bounds memory in a fork storm (a test runs a job with a 50-entry cache and gets the same result).
- **Resuming.** `gb sim` saves each finished job's result, with its specification, in the run directory's `checkpoint/`. `gb sim --resume <run-dir>` continues an interrupted run from the same commit and configuration and reuses every saved result whose specification matches exactly (floats round-trip exactly). The checkpoints and `resume.json` are removed when the run finishes, so a resumed run's outputs equal those of a run done in one go.
- **Run directory:** `results/<analysis>/<run-id>/` holds the tables (CSV), charts (PNG), `SUMMARY.md`, `config.resolved.toml` and `run.json`. `run.json` records commit, dirty flag, seed, config hash, toolchain and a SHA-256 for every output file. Machine-dependent timings go to a sub-directory that reproduction checks leave out (`bench/` for `gb crypto`, `perf/` for `gb sim`); `gb sim --trace` adds sampled event traces in `trace/`.
- **Front ends** read these outputs only (SPEC §11.6).

## 7. Configuration toggles

**Typed in M3a** (each with its status and sweep in `gb-config`, and in `configs/default.toml`):

- `issuance.count_based_correction`: Variant B's correction from recent block times (§3.8), P1, P2 or B-exact; P2 ×1.1 by default since SPEC v0.7;
- `issuance.difficulty_count_rule`: Variant B counts the miners whose KWC issued entropy for the round (default since SPEC v0.7), those past their post-admission wait, all Globally Active miners, or the lagged issued count;
- `entropy.equivocation_scope`: one header per miner per height (default) or per parent [P];
- `offline.unreachable_rule`: "unreachable is not refusal" (default since 2026-10-14) or strict One Chance (§4.6, S17);
- `penalties.strict_offence`: the §4.8 ladder without reset (default), with a yearly reset [P], or an immediate ban;
- `timing.witness_signing`: honest witnesses sign on their own clock at t0 + N (default) or at the earliest time within δ (the P-4 comparison);
- `timing.powit_deadline_scope`: a late PoWit only stops the block being extended (default since 2026-10-14) or invalidates it for good;
- `timing.early_block`: a block received early is rejected for good (default) or re-evaluated later.

The simulator's own settings (crypto mode, threads, world, network, households, growth and experiment sizes) are in the `[sim]` section.

**Still to add with M3b/M4** when the simulators use them:

- mandatory handshake vs none (§3.1, S26);
- hash-priority tie-break for pending claims vs timestamps (§3.1);
- sorted-hash field ordering (S14; M2 measured it separately);
- diversity constraints in allocation (§4.2);
- proposer rotation, proposer rights and failure behaviour (§4.4);
- the SPEC v0.4 §12 [P] proposals: validity separate from payment, the split rule, larger KWCs, threshold BLS entropy and witness peer topology option 2.

The lottery beacon offset k, the allocation beacon delay and the long-absence threshold L are already typed parameters (all [O]). M1 does not depend on the two beacon parameters, because it models the draws as uniform; M2 and M4 measure what the miner of a beacon block can gain by withholding it. L enters sections B, G and H through its sweep.

## 8. Allocation — adopted as SPEC §4.2 [D]

The architect adopted this algorithm on 2026-10-06 (SPEC v0.3 §4.2, with the registration index of §3.11) and amended its removal rule on 2026-10-07 (SPEC v0.4 §4.7). The SPEC states the rule; this section keeps the reasoning behind it.

SPEC §4.2 requires allocation to be deterministic and publicly recomputable, with placement unknown to an ID's owner at minting time. SPEC §2 requires:

- every WC to have exactly 10 members;
- each WC to lead exactly one KWC and be a subordinate in exactly three;
- no mutual monitoring pairs.

The earlier SPEC gave no algorithm that met all of these at once. Because the inside-out shuffle keeps every assignment uniformly random, M1's uniform-partition model of seats remains exact under the adopted rule.

### 8.1 Seats: inside-out Fisher–Yates insertion

1. IDs are allocated in canonical order: the registration index of SPEC §3.11, which follows PoW-ID confirmation order. Genesis IDs are allocated first, using a public launch seed in place of a beacon.
2. **Seats.** Seats are numbered 0, 1, 2, …, and WC `w` is seats `10w` to `10w + 9`.
3. **Inserting an ID.** When an ID is inserted while `n` seats are filled, compute `j = SHA256("GB/alloc" ‖ ID ‖ beacon) mod (n + 1)`, reading the hash as a 256-bit integer; the modulo bias is below 2⁻²⁰⁰. Without removals, `n` is the ID's registration index. The beacon is the hash of the PoW-ID block 6 blocks after the ID's confirmation (SPEC §2, [O]).
   - If `j = n`, the ID takes seat `n`.
   - Otherwise the ID in seat `j` moves to seat `n`, and the new ID takes seat `j`.
4. **Why it is uniform.** This is the inside-out Fisher–Yates shuffle. After every step, the assignment of allocated IDs to seats is a uniformly random permutation, given uniform hash outputs.
5. **Active and pending WCs.** A WC is active when all 10 of its seats are filled, so the number of active WCs is `W = ⌊allocated / 10⌋`. The partial tail WC is pending. Its members may mine (§3.10) but do not witness until it fills, about 5 minutes at 30 s per ID.
6. **Removals** (a ban, or a continuous absence longer than the long-absence threshold L; SPEC v0.4 §4.7). The ID in the last filled seat moves into the vacated seat. If that empties a seat of the last active WC, that WC returns to pending, `W` decreases by one, and the KWC ring (8.2) is recomputed. Going offline or being deactivated does not move anyone: the absent ID keeps its seat as an offline member. A returning ID whose seat was vacated is re-inserted by step 3, with the beacon of the block 6 blocks after its re-activation is confirmed (architect, 2026-10-07).
7. **When placement takes effect.** At the ID's beacon block, without waiting for the Allocation Committee Block, which records and attests it (SPEC v0.4 §4.2–§4.3).

### 8.2 KWCs: a Golomb-ruler ring

For `W ≥ 13` active WCs, KWC `w` has leader WC `w` and subordinate WCs `(w + 1)`, `(w + 4)` and `(w + 6)`, all mod `W`.

- **Each WC leads exactly one KWC and is a subordinate in exactly three:** the KWCs led by `w − 1`, `w − 4` and `w − 6`.
- **No mutual monitoring pairs.** WC `j` monitors WC `i` exactly when `(j − i) mod W ∈ {1, 4, 6}`. A mutual pair would need `d₁ + d₂ ≡ 0 (mod W)` for some `d₁, d₂ ∈ {1, 4, 6}`, but every such sum is at most 12, which is less than `W`.
- **Any two KWCs share at most one WC.** `{0, 1, 4, 6}` is a Golomb ruler: its 12 nonzero pairwise differences, ±1 to ±6, are distinct mod `W` for `W ≥ 13`. So each KWC overlaps exactly 12 others, each in one WC. That is the overlap degree M1 uses for its structure-free bound.
- **30-node comparison.** Use marks `{0, 1, 3}`, so subordinates are `(w + 1)` and `(w + 3)`, and `W ≥ 7`.

Everything depends only on chain data: the ID order, the beacons and the removal record. Anyone can recompute it. The CAC attests a hash of the resulting assignment but cannot choose it.

Other sizes use other optimal Golomb rulers (M1 section I): {1, 3} for 3 WCs, {1, 4, 9, 11} for 5, {1, 4, 10, 12, 17} for 6, {1, 4, 9, 15, 22, 32, 34} for 8 and {1, 6, 10, 23, 26, 34, 41, 53, 55} for 10.

### 8.3 What it replaced

- **§4.2 formula.** SPEC v0.2 wrote placement as a pure per-ID function, `chain(ID) = f(SHA256(ID ‖ beacon))`. Under the adopted rule, placement also depends on earlier allocations, and an existing ID moves when a new ID displaces it.
- **Fixed-membership principle.** The earlier principle was that chain membership stays fixed unless someone is banned. The adopted rule gives it up: every new ID changes the membership of one existing WC, and moves one existing ID into the pending tail WC.

### 8.4 Security reason for the change

The alternative that keeps membership fixed is **append-only** allocation: new IDs fill new WCs in arrival order, and existing WCs never change. Under append-only allocation, a new WC's composition mirrors the attacker's share of **recent issuance** (s), not its share of the **population** (p).

- An attacker pays to sustain issuance share at the margin. Early in an attack, s is far above p.
- In any ring layout, a new WC's KWC neighbours are also recent WCs. So a whole new KWC reflects s.
- **Example.** An attacker winning 60% of recent issuance holds at least 7 of 10 seats in about 38% of new WCs (P[Bin(10, 0.6) ≥ 7] = 0.382). Under uniform allocation with a 10% population share, the same event has probability about 9 × 10⁻⁶.

Insertion keeps every WC's composition at the population share, so the section B probabilities apply to new KWCs too.

### 8.5 Cost: composition changes

- **Seat changes:** each new ID changes one existing WC, and so 4 KWC compositions (a WC sits in 4 KWCs). That is 40 per new WC.
- **Ring relinks:** each new active WC adds its own KWC and relinks the 6 KWCs whose subordinate offsets wrap around the ring. That is 7 per new WC.
- **Total:** about 47 composition changes per new WC (4.7 per new ID), against 1 for append-only. A removal changes about 4.6, so an absence that vacates a seat and ends in a re-insertion changes about 9.3.
- **Effect on the H6 count:** counting every composition as an independent draw, the adopted rule raises the count about 43-fold over ten years from 100,000 KWCs (section B4). Most changed compositions differ from their predecessor by one seat, so the changes are correlated: at p = 25% a one-seat change enters the sign-capable state with about 0.43 to 0.46 times the probability of a fresh draw. Since SPEC v0.4 the primary measure is the number of distinct episodes, which is about half the composition count at p = 25% (0.476 against 0.926 for the unregistered quorum).
- **Effect of the v0.4 seat rule:** under v0.3 every absence that deactivated an ID cost about 9.3 compositions. Under v0.4 only absences longer than L do; with the illustrative inputs (4 absences per ID per year, Lomax durations) that leaves 3.6% of the v0.3 count at L = 30 days (section B5). The price is liveness: absent IDs keep their seats, which needs more honest uptime (section H).

### 8.6 Open risks

- **Beacon withholding.** The miner of the beacon block can withhold it to re-roll placements, at the cost of that block.
- **Placement grinding.** An unregistered miner cannot choose its ID hash freely, because the ID is the hash of the winning PoW-ID block and §3.4 allows one header per round. The residual freedom should still be measured in M2.

## 9. Release workflow (private → public)

Development and publication use two repositories.

| Remote | Repository | Pushing |
|---|---|---|
| `origin` | the private development repository | the default: all work, CI runs and reviews happen here |
| `public` | `github.com/HurairahShamsi/grahambell-simulation` | disabled: its push URL is set to `DISABLED` (`git remote set-url --push public DISABLED`), so a plain push can never reach it |

- **Day to day:** commits are pushed to `origin` only; `main` tracks `origin/main`.
- **Release:** only when the architect writes the exact words "push to public":
  1. restore the push URL: `git remote set-url --push public https://github.com/HurairahShamsi/grahambell-simulation.git`;
  2. push `main` only, with `git push public main`: never another branch, never `--force`;
  3. disable it again with `git remote set-url --push public DISABLED`, and check with `git remote -v`.
- **Reference results:** `results/` is ignored by git except the reference runs in `results/analytic/reference/`, `results/crypto/reference/` and `results/sim/reference/`. Each is produced from a clean commit, and its `run.json` records that commit and a SHA-256 of every file, so anyone can check out the commit, rerun the analysis and compare (outside the machine-dependent `bench/` and `perf/`).
- **Before a release push**, check the commit authors (`git log --format='%an <%ae>' | sort -u`), files over 1 MB, and anything that looks like a secret, a key or a local path, and list every published number that the new results confirm, refine or contradict (standing rule, §4).
