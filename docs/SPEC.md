# GrahamBell — Stage 1 Simulation Specification

**Version:** 0.7 (compiled from the protocol architect's papers and design decisions; amended 2026-10-05, 2026-10-06, 2026-10-07, 2026-10-08 and 2026-10-09, and in v0.7 with the decisions of 2026-10-12, 2026-10-14, 2026-10-19, 2026-10-21 and 2026-10-22; see the Changelog at the end)
**Owner:** Hurairah Shamsi, Protocol Architect
**Purpose of this file:** single source of truth for building and running the Stage 1 simulation. Where this file and older papers disagree, **this file wins**. Where this file is silent or ambiguous, the implementer must **stop and ask**, not guess.

### Status legend

- **[D] Decided** by the protocol architect.
- **[P] Proposed**: recommended during design review; must be implemented so it can be compared against the alternative named.
- **[O] Open**: undecided value or rule; implement as a configurable parameter and sweep it.

All numbers are provisional defaults. Every number in this file must be a configurable parameter in code, never a hard-coded constant.

---

## 0. What Stage 1 must establish

Stage 1 tests whether **Proof of Infrastructure Endurance (PoIE)**, GrahamBell's time-based Sybil resistance built on serialized, paced identity issuance, holds up under realistic and adversarial conditions. The claims under test:

- **C1 Linearity.** An attacker's share of newly issued IDs is proportional to its share of admitted /64s. No strategy produces more than proportional share.
- **C2 Time floor.** Reaching a threshold T of 33%, 51% or 67% of **active** IDs requires time bounded below by (T/(1−T)) × H_active / R, where H_active is the existing active base and R the issuance rate, even for an attacker who wins every ID.
- **C3 No acceleration.** Hardware, parallelism, precomputation and entropy grinding give no advantage per /64 or per ID.
- **C4 Witness integrity.** Random allocation plus quorum rules make witness capture or stalling expensive; misbehaviour is attributable; honest participants are not cheaply harmed.
- **C5 Cost.** The real-world cost of sustaining a given attacker share is quantified in money and scales linearly.

The deliverable is not "the model works." It is a **reproducible map** of the conditions under which each claim holds, and where its margins become thin, with code anyone can rerun.

### In scope

Unregistered-to-registered identity issuance (PoW-ID); the witness layer as it affects issuance; the Chain Allocation Committee; a minimal transaction-layer model only where needed (witness seats come from IDs; committee seats come from a lottery over active IDs, §4.3; active-ID share).

### Out of scope for Stage 1

Proof of Call, Murphy, Data Saving Groups, Proof of Funds; throughput (transactions per second, TPS); the Single Chain Architecture (SCA); the Shamsi Operating System (SOS) VM and smart contracts; the software Block Processing Unit and Block Validation Unit (BPU/BVU); rewards and token economics, including the full reward schedule (used only as economic inputs, §6); storage, including pruning; and the Stage 3 centralized-witness testnet itself. These are separate designs, not tested in Stage 1.

PoIE bounds how fast IDs can accumulate. It does not prove that one person holds one ID: genesis IDs are issued one per verified person (§2), but nothing ties an ID issued after launch to a person.

---

## 1. Terminology

| Term | Meaning |
|---|---|
| /64 | Externally visible IPv6 /64 prefix, derived from the socket source address **after a transport handshake** that proves the sender controls the address. |
| Unregistered miner | A /64 competing for an ID. |
| ID (registered ID) | An identity. Its value is the hash of the winning PoW-ID block. Bound to a key pair registered in that block. |
| PoW-ID block | Identity-issuance block (older papers: PoW1). |
| PoW-Tx block | Transaction block (older papers: PoW2). |
| PoWit | Proof of Witness block accompanying a PoW block. |
| Round | Interval between consecutive blocks of one type. PoW-ID target: 30 s. |
| Epoch (grace) | 5 rounds of the miner's own block type (PoW-ID for unregistered, PoW-Tx for registered). |
| WC | Witness Chain: 10 registered IDs acting as witness nodes. |
| KWC | King Witness Chain: 1 leader WC + 3 subordinate WCs = 40 nodes. |
| Leader WC / Sultan | The WC that leads a KWC and is paid when a block is mined through it. |
| Subordinate WC / Wazir | A WC that monitors another WC's KWC without being paid for it. |
| CAC | Chain Allocation Committee. |
| MOBu / MOBr | Miner Online Block for unregistered / registered miners. |
| One Chance | Relay-and-retry procedure run before any member or miner is penalized for a missing signature. |

---

## 2. Parameters

| Parameter | Default | Sweep / variants | Status |
|---|---|---|---|
| Hash pacing | 1 hash/s per admitted /64 (unregistered) or per ID (registered) | — | D |
| PoW-ID target interval | 30 s (≈1.05M IDs/yr) | adaptive variant (§3.9) | D / P |
| Difficulty rule (§3.8) | Variant B: count-based, from the miners whose KWC issued entropy for the round | Variant A (Bitcoin-style retarget), for comparison; counts: the miners past their post-admission wait (the default until v0.6), all Globally Active miners, the lagged issued-entropy count | D |
| Variant A comparison settings (§3.8) | window K = 144 blocks, 4× clamp per retarget | — | D |
| Variant B correction from recent block times (§3.8) | P2: every 144 blocks the factor is multiplied by the last 144 intervals' mean ÷ the target, kept within ×0.9 to ×1.1 | none; P1 within ×0.9–×1.1, ×0.75–×1.25 and ×0.5–×1.5; P2 within ×0.75–×1.25 and ×0.5–×1.5 (both miss H5 at the final defaults); B-exact | D |
| PoW-Tx interval | 10 s | 1–10 s | O |
| WC size | 10 | 10 | D |
| KWC composition | 1 leader + 3 subordinate WCs (40 nodes) | 1 + 2 (30 nodes) for comparison | D |
| Number of KWCs | equals number of WCs (each WC leads exactly one KWC, subordinate in exactly three) | — | D |
| KWC ring offsets (§4.2) | 1, 4, 6 | 1, 3 for the 30-node comparison | D |
| Allocation beacon delay (§4.2) | 6 blocks after the ID's confirmation, or after its re-activation is confirmed when a returning ID is re-inserted | 3, 6, 12 | O |
| Registered-miner PoWit quorum | ≥7 of 10 leader WC **and** ≥21 of 30 subordinates (28 total) | 30-node comparison: ≥7 of 10 leader **and** ≥14 of 20 subordinates (21 total) | D |
| Unregistered-miner PoWit quorum | any ≥27 of 40 | 30-node comparison: any ≥20 of 30 | D |
| Entropy signatures | **threshold BLS entropy**: any t = ⌈2n/3⌉ of the members seated at key setup (27 of 40) give the KWC's unique group signature (§3.4) | t = 21 of 40 [P]; every online member, with at least the quorum actually signing (the rule until v0.6) | P |
| Miner capacity per leader WC | 200 registered + 50 unregistered ² | registered/unregistered split: 200/50, 100/150, 15/235 | D default, O split |
| Miners watched per witness node | 1,000 (800 registered + 200 unregistered) | — | D |
| Spare witnessing capacity target | +33% over demand | — | D |
| Per-node request rate limit | 1,000 req/s | — | D |
| Grace epoch | 5 rounds of own block type (~150 s unregistered) | — | D |
| Post-admission wait before mining | 5 rounds | 1–10 | O |
| Convergence interval (admission) | 1 epoch | 0.5–3 epochs | O |
| Peer bootstrap count (newcomers) | 8 | 4–8 | D |
| CAC size | 600, first-in first-out | 100–1,000 for comparison | D |
| CAC seat selection (§4.3) | lottery over the canonical active list, one new member per 10th PoW-Tx block; a seat left by a banned or deactivated member is filled by an extra draw at the next 10th PoW-Tx block, at position SHA256("GB/cac" ‖ hash of block B+k ‖ i) with i = 1, 2, … [P] | old rule (miner of every 10th PoW-Tx block), for comparison | D |
| CAC lottery beacon offset k (§4.3) | 3 blocks | 1, 3, 6, 12 | O |
| CAC approval threshold | at least two-thirds of members, rounded up: ⌈2n/3⌉ (400 of 600) | — | D |
| Genesis IDs (G) | 2,900,000 ¹ | 1M, 2.1M, 2.9M, 5M, 10M | D |
| Genesis distribution | KYC, one per verified person | fraction secretly controlled by one party: 0–20% | D / O |
| Confirmation depth for new IDs | 6 PoW-ID blocks | 3–12 | O |
| Clock tolerance δ | 2 s | 0.5–10 s | O |
| Offline deactivation threshold | — | 1 h, 6 h, 24 h, 3 d, 7 d | O |
| Re-activation wait after return | — | 1 d, 7 d, 14 d, 30 d | O |
| Long-absence threshold L (§4.7) | 30 days (86,400 PoW-ID blocks at 30 s), the primary value | 3 d (8,640 blocks), 7 d (20,160), 30 d (86,400), 90 d (259,200), 365 d (1,051,200) | O |
| One Chance wait (§4.5) | 1 grace epoch | 1, 2, 5 rounds | O |
| PoWit arrival deadline D (§3.7) | 10 s with the soft scope (a node that got the PoWit late does not extend the block, but accepts a longer chain through it), judged on each node's own clock | none; 5, 30, 60 s; the permanent scope, for comparison | D |
| Penalty ladder (lesser offences) | 24 h, then 30 days, then permanent | — | D |
| Minimum attack time floor (adaptive issuance) | 2 years | 1–10 years | P |
| Adaptive-cap checkpoint interval (§3.9) | T_min | T_min/4, T_min/2, T_min | D |
| Adaptive-cap safety factor k (§3.9) | 7/6, for checkpoints every T_min | k for the other intervals is reported by M1 | D |
| Signature scheme | BLS12-381 aggregate signatures with signer bitfield and proof of possession | — | D |
| Hash function | SHA-256 | — | P |

¹ With a 30 s rate (R ≈ 1,051,200 IDs per year), a 100%-capture attacker needs about 2.02M active IDs in the existing base before reaching 51% takes 2 years ((0.51/0.49) × H_active / R = 2 years). The floor depends on genesis IDs staying **active**. With 2.9M genesis IDs it holds while at least 69.7% of them remain active, that is with up to 30.3% inactive. At full activity the time is about 2.87 years.

² Each registered ID sits in exactly one WC and mines at most one channel, so a KWC averages at most 10 registered miners. The registered capacity of 200 per leader WC (and 800 registered miners watched per node) cannot be reached network-wide; it is reached only where miners concentrate. The registered/unregistered split stays a sweep.

---

## 3. Identity issuance (PoW-ID)

### 3.1 Admission lifecycle [D]

States, in order: **Locally Validated → Globally Pending → Canonically Selected → Convergence Complete → Globally Active.** Only Globally Active grants mining rights.

1. **Bootstrap.** A newcomer connects to 4–8 registered peers chosen at random, which supply recent PoW-ID headers, KWC membership, free capacity, and the current CAC. Registered peers accept only a narrow message set from unregistered nodes.
2. **Local validation** by the chosen KWC: structure, signature, and the /64 derived from the socket source after a handshake **[P: handshake mandatory; compare with no-handshake in the spoofing scenario]**.
3. **Global claim broadcast** to all WCs and validators; checked against all active and pending claims.
4. **Canonical tie-break** between conflicting pending claims for the same /64 or ID: **hash priority [P]**, not timestamps (timestamps are sender-controlled).
5. **Convergence interval**, then activation. For unregistered miners the MOBu is proposed by the subordinate-side proposer (§4.4).
6. **Wait** the post-admission period before mining.

A KWC at capacity rejects new joins; newcomers wait or try another KWC. Each chain accepts only as many new requests as it has free slots.

**Failover [P]** (M3a.1 E3; off by default). A miner whose KWC cannot issue entropy for 2 consecutive rounds moves, by the self-service path (§3.2, about 450 s), to a KWC with spare capacity that can. At the v0.6 tie-break it cut the longest gap in a regional blackout under the all-online rule from 165.6 to 45.7 minutes. At the v0.7 defaults the longest gaps with and without it are close (18.7 and 17.8 minutes under the all-online rule; 6.1 minutes either way at t = 21 of 40), and in 31 days of realistic churn it moved about 58,000 miners under the all-online rule (about 830 at t = 21 of 40) for the same issuance. It stays off by default (architect, 2026-10-21); M4 revisits it with correlated outages.

### 3.2 Uniqueness [D]

- One active session per /64 (unregistered). One active mining channel per ID (registered).
- A miner may not join a KWC in which its own ID is a witness. If it does, its blocks are rejected even if valid.
- Joining a different KWC while still counted live elsewhere is treated as parallel mining: rejected, and grounds for a ban.
- Moving to another KWC is allowed at any time, but takes the grace epoch plus admission (a few minutes).
- **Quick return [D].** A miner that shut down with its own offline request and comes back within the grace epoch is re-admitted at once.
- **Self-service offline:** before rejoining, a miner may query peers; if still listed online, it broadcasts its own signed offline request, waits one epoch, then rejoins.
- **Replay protection:** a signed join request names its target KWC; any other KWC ignores it. Only a request signed by the candidate key **for a different KWC** counts as evidence of duplication.

### 3.3 PoW-ID header [D]

Fixed contents for the round, including: candidate public key, reward wallet, /64, KWC ID, difficulty, version, **previous PoW-ID block hash (mandatory, so entropy refreshes every round)**, height. No free-data or extra-nonce field.

### 3.4 Entropy lock [D]

1. Miner builds header `H`, signs it with the candidate key: `σ_m = Sign(sk_m, "GB/PoW-ID/miner" ‖ H)`.
2. Miner sends `(H, σ_m)` to **every** KWC member. Each member verifies it and signs.

**Default [P]: threshold BLS entropy** (architect, 2026-10-21; M3a.1 E3). Each KWC holds a threshold BLS key, set up whenever its membership changes (a new key epoch), with t = ⌈2n/3⌉ of the n members seated at key setup: 27 of 40. Key setup costs each member 40 messages sent and 78 received, 51.8 kB (M2 C5); the shares are refreshed every 30 days [P] (§12).

3. Each member returns its share `σ_i = SignShare(sk_i, "GB/entropy" ‖ H ‖ σ_m)`. Any t shares combine into the KWC's unique group signature `σ_K`, and `E = SHA256("GB/entropy-out" ‖ σ_K)`.
4. The entropy is the same whichever t members sign, so withholding a share cannot change it: this closes the online-set grinding lever (§8 S11(f), §12). Once t members have signed, a member that does not sign delays nothing, and entropy needs no One Chance. Fewer than t reachable members stop the KWC's entropy: n − t + 1 members refusing or unreachable (14 of 40 at t = 27) block it, and t colluding members can produce it without the others (§12, M4).
5. `E` is committed into the header. The header is then immutable for the round.

**The remaining limit (M3a.1 E3, in the reference run at the final defaults, 1d9ce91).** A 10.88-hour blackout of the largest region (about 38 % of witness nodes) can still pause the chain. At t = ⌈2n/3⌉, 3 of 41 scripted blackouts had a gap over 10 minutes; the three longest were 13.0, 12.1 and 11.3 minutes, all in blackouts of as_east. Their traces (`results/sim/reference/traces/`) show two causes. At the blackout's start, the round in progress keeps the difficulty set from the miners counted before it: one round of difficulty lag (9,143 miners counted, about 540 able to get entropy, an expected wait of 514 s; the pause lasted 782 s). Hours into the blackout, in the evening UTC, almost no KWC can issue entropy: 0–4 miners could get it, none in half the samples, although the difficulty had followed the count down to 1–2 miners. The all-online rule's longest gap was 12.6 minutes; with failover, 15.1 minutes (all-online) and 16.6 minutes (t = ⌈2n/3⌉). The comparison t = 21 of 40 [P] kept every gap under 6.2 minutes, at a security cost (§12). (In the 0b711f5 run, before the final defaults, the longest were 16.3 and 19.5 minutes.)

**Comparison: every online member (the rule until v0.6).**

- Each member signs `σ_i = Sign(sk_i, "GB/entropy" ‖ H ‖ σ_m)` and shares `σ_i` with all members and the miner; `E = SHA256("GB/entropy-out" ‖ AggregateBLS(σ_m, σ_i for every online member i) ‖ signer_bitfield)`.
- The aggregate **must include every online member's signature**. A missing online signature triggers One Chance (§4.5). At least the quorum (27 of 40) must actually sign: members recorded offline during the round do not count towards it (M3a.1 found that the simulator had issued entropy with fewer signers; ASSUMPTIONS SIM-43).

**Optimistic verification [P].** A witness verifies the entropy (the group signature, or under the comparison the aggregate) once. Only if that fails does it verify the shares one by one to find the bad signature. The honest case therefore costs one aggregate verification per miner per round; M4/M5 load models use this.

Rules:

- **One header per miner per round.** Two headers signed by the same miner key in the same round are proof of equivocation and grounds for a ban.
- **Abandoning a header** means waiting for the next PoW-ID block before starting again.
- **Proof of possession:** the miner's signature on the PoWit, verified with the public key in the PoW-ID block body, domain-separated from other signatures.

### 3.5 Hash chain [D]

- **Start rule (r) [D], the default** (architect, 2026-10-21; selected by M3a.1 E1): a miner's chain starts at `t0`, its entropy issue time as its KWC attests it: the clock time at which the last required signature was made (under a threshold, the t-th), or at which a record offline completed the required set, rounded up to a whole second, and never before `timestamp(previous PoW-ID block) + 1 s`. The entropy depends on the header only; `t0` is attested separately, by the KWC's signature over `"GB/t0" ‖ header_digest ‖ t0` (Appendix A). A miner whose entropy arrives late starts late: no missed step is ever computed.
- **Start rule (a), comparison:** the chain always starts at `timestamp(previous PoW-ID block) + 1 s`, regardless of when `E` arrives; a miner whose entropy arrives late computes the missed steps immediately (catch-up). It was the default until v0.6. M3a found that catch-up costs issuance (−17 % on a fixed target) and, after a stall, sends out bursts of blocks with old timestamps; under a PoWit deadline those blocks are late everywhere.
- **Start rule (b), comparison only:** the hash chain starts when the miner's entropy arrives locally. This is the rejected alternative, kept only for comparison in S13.
- **Known risk of rule (r) [O]:** t or more colluding members of a KWC could attest an earlier `t0` than the real issue time (backdate it), which recreates catch-up for the miners they serve. Tier 1 assumes they do not (ASSUMPTIONS SIM-37). M4 tests it as scenario S30.
- In every rule, `t0` below is that rule's start.
- `h_0 = SHA256("GB/chain" ‖ prev_hash ‖ height ‖ t0 ‖ nonce=0 ‖ E ‖ header_digest)`
- `h_{n+1} = SHA256("GB/chain" ‖ h_n ‖ prev_hash ‖ height ‖ (t0+n+1) ‖ nonce=n+1 ‖ E ‖ header_digest)`
- **Fixed field order, fixed-length encodings, domain tags [P].** The older "sort fields by hexadecimal value before hashing" rule is tested separately (scenario S14), not used in the simulator.
- The miner wins at the first `n` with `h_n < target`. Block timestamp = `t0 + n`.

### 3.6 Witnessing and signing [D]

Every member recomputes the chain independently. An honest member signs the PoWit **only when its own clock reaches `t0 + N`**, where `N` is the winning step: on time, at its own clock's `t0 + N` [D]. δ is the validators' tolerance (§3.7), not a margin for signing early. The miner collects at least the quorum, aggregates, and broadcasts the PoW-ID block with its PoWit (also to the CAC).

**Entropy signing [D].** A member signs a miner's entropy request once it has received and validated the header's parent, whatever its own preferred tip.

### 3.7 Global validation [D]

Validators check: quorum signatures with bitfield and proof of possession; full chain recomputation from inputs; `timestamp = t0 + N`; the block was not received before its own timestamp (local clock, tolerance δ); serialization of issuance; uniqueness. A new ID becomes active only after confirmation depth.

**PoWit arrival deadline [D]: D = 10 s with the soft scope** (architect, 2026-10-22, applying M3a.1 E1's criteria as written to the 0b711f5 run). A node that receives a PoW-ID block's PoWit (the quorum signatures) more than D seconds after the block's timestamp does not extend that block, but still accepts a longer chain through it. **D is measured by each node on its own local clock against the block's attested timestamp (`t0 + N`); no global clock is assumed.**

E1's criteria, fixed in advance, picked no deadline in the corrected M3a.1 run, D = 10 s in the 0b711f5 run, and **no deadline in the reference run at the final defaults (1d9ce91), for the architect**. In the corrected run (after the late-block fix, ASSUMPTIONS SIM-25), 6 of the 15 criterion-4 reruns of the D = 10 s configuration, those with the P1 correction, reorganised 7–8 blocks below confirmation, which failed criterion 1. In 0b711f5 and 1d9ce91 every run and rerun of both finalists, (r) first-seen literal with D = 10 s soft and without a deadline, was clean, and the two tie on interval (0.32 and 0.34 s from 30 s with their best H5-passing variant, P2 ×1.1). In 0b711f5 they tied on orphans too (1.86 % and 1.84 %), and the simplicity order fixed in advance put D = 10 s first. In 1d9ce91, whose first stage runs under P2 ×1.1 rather than P2 ×1.5, they do not: 1.85 % (±0.07) with D = 10 s against 1.77 % (±0.09) without, a paired difference outside the tie band, so criterion 5 selects no deadline. D = 10 s cuts a 10 % withholder's amplification from 0.154 to 0.001 (0.148 to 0.001 in 0b711f5), wherever the withholder sits: in the supplementary set W1 (E1's finalists, 5 seeds), 0.154 to 0.0004 with the withholder spread over the regions and 0.144 to 0.000 with it all in oc, the smallest region and among the farthest from the network's miners. In one of W1's five runs with the withholder in oc, D = 10 s reorganised the chain 7 blocks below confirmation (one ID voided), like the corrected run's P1 reorganisations. Start rule (r) had already removed catch-up, the deadline's first purpose (M3a); M3b tests adversarial withholding under D = 10 s and explains these reorganisations.

**Clocks.** The deadline tolerates unsynchronised clocks. In C6 (every node's clock skewed with σ = 1 s or 2 s, δ = 2 s, honest witnesses signing on time; the default model skews 99 % of nodes with σ = 0.05 s and 1 % with 2 s) no block was late or early at the panel majority, and the deadline split no validator: under the soft scope a node whose clock disagrees with the others' only declines to extend a block it judged late, and follows a longer chain through it (no validator split for good in C3 at D = 5, 10, 30 and 60 s, nor in any E1 run of the finalists). The permanent early-block rule (below) is less tolerant: with every clock at σ = 2 s and δ = 2 s, 2.1 % of panel receptions were early and 16 of the 101 panel validators rejected a final-chain block for good and split off (none at σ = 1 s or with δ = 10 s, and none with re-evaluation; the same 16 in the 0b711f5 run, where C6 did not yet report splits). The permanent deadline scope, in which the block and every chain through it stay invalid at that node for good, is a comparison only: it split 63 of the 101 panel validators off the network chain in M3a, and again at every D in C3 of the reference run.

**Early blocks [D].** A node that receives a block before its own timestamp (local clock, tolerance δ) rejects it for good. Re-evaluating it once the node's clock passes timestamp − δ is a comparison.

**Confirmation [D].** Confirmation depth k means k blocks built on top of the block (k = 6, §2).

**IDs in replaced blocks are void [D].** If the chain switches to a branch without a block whose ID had passed confirmation depth, that ID is void: it leaves the active list, the committee, its seat and any pending placement. This is neither a ban nor a penalty (no ban record, no offence). The miner returns to being a candidate, and a replaced block counts again if the chain returns to it.

**Fork choice**, in this order (every block must carry a valid PoWit):

1. The longest valid chain.
2. At the same height, the block this node received first, on its own clock (first-seen) [D] (architect, 2026-10-21; selected by M3a.1 E1).
3. Then the lower block hash.

**Comparison:** v0.6's tie-break, the earlier timestamp (the smaller winning step N) and then the lower block hash. Under first-seen a miner never switches to a later block at the same height, so the [P] same-parent header rule (§3.4) changes nothing; in E1 the two header rules gave identical results.

The confirmation depth stays as in §2. M1 finds that same-step ties end about 1.66% of rounds at the 30 s target.

**Known caveat** (architect, 2026-10-22): under the defaults the honest win share differs by region. In the reference run at the final defaults (1d9ce91, C1), each region's miners won between 0.981 (af, 95 % CI 0.968–0.994) and 1.023 (na_west) of their share of mining time at 1,000 miners, and between 0.981 (as_south, 0.971–0.992) and 1.032 (na_east, 1.016–1.048) at 100,000; a tagged group spread over the regions in proportion wins its share (0.986–1.010 at every size). The deficit is accepted for now; M3b explains it (tie losses against literal sit-outs, correlated by region) and tests mitigations (ASSUMPTIONS SIM-51).

### 3.8 Difficulty [D]

- **Variant B (default): count-based.** Per-hash success probability `p = factor / (N × target interval)`.
- **The count N [D], issued entropy** (architect, 2026-10-21; selected by M3a.1 E1/E2): the miners whose KWC issued entropy for the round, that is mining, online and with a header on the round's parent. Each KWC commits the number it issued in its entropy attestations; the block carries the sum over KWCs with Merkle roots of the headers, validators recompute it, and an inflated count is attributable to the members who signed it (ASSUMPTIONS SIM-36). The block mined in a round sets the next round's difficulty, a lag of one round. Comparisons: the miners past their post-admission wait (the default until v0.6), all Globally Active miners, and the count committed a confirmation depth before the parent (lagged; it removes the fork storms of the unlagged count but fails H5, with a hopping gain of 1.036 at m = 4).
- **The correction [D], P2 ×1.1** (architect, 2026-10-22): every 144 blocks the factor is multiplied by the last 144 intervals' mean ÷ the target interval, and kept within ×0.9 to ×1.1. Its worst hopping gain is 1.017, within H5's locked 1.02 (0b711f5 and 1d9ce91), and it is the best H5-passing variant of both E1 finalists (0.32 and 0.34 s from 30 s). In the reference run at the final defaults (1d9ce91) it held the realistic interval at 30.21 s (0.69 % long, daily SD 0.87 %) and sat at its bound in 21–27 % of 144-block windows in honest runs (57–66 % with a 10 % withholder); P2 ×1.5 never reached its bound in 0b711f5. P2 ×1.5, the default set on 2026-10-21, misses H5 there (1.0208 > 1.02) and is recorded as failing; it is reported as a comparison only, with no correction, P1 (the factor set to that ratio each block), P2 ×1.25 (1.022, also failing H5) and B-exact (1.021, failing). In the corrected M3a.1 run all three P2 bounds had passed H5 (1.012–1.016).
- **Variant A (comparison): Bitcoin-style retarget** from recent block times over a window of K = 144 PoW-ID blocks, each retarget clamped to a factor of 4 in either direction.

### 3.9 Issuance rate [D default, P variant]

- **Default:** fixed target, one PoW-ID block per 30 s.
- **Adaptive variant (optional):** the rate may rise with demand, but is capped at `R ≤ registered_IDs / (k × T_min)`, where `k` is a safety factor.
- The cap counts **registered** IDs, not active IDs, because an attacker can inflate the active count by coming online. It is recalculated only at **fixed checkpoints**, every `T_min` by default (§2).
- With checkpoints every `T_min`, `k = 7/6`. M1 reports `k` for the other checkpoint intervals in the §2 sweep.
- **Guarantee:** With the safety factor k for the chosen checkpoint interval, a 100%-capture attacker cannot reach 51% in less than T_min, at any start phase.
- **Why `k` is needed:** an attacker that chooses when to start relative to the public checkpoint schedule reaches majority sooner than `T_min` under the cap without `k` (M1: 0.857 `T_min` for checkpoints every `T_min`).
- The cap may set the rate **below** the fixed 30 s target. That is its purpose.

### 3.10 After winning

The winning miner is treated as offline once its PoW-ID block passes confirmation depth, freeing its slot. A new ID may mine transactions before it is allocated to a WC.

### 3.11 Registration index and canonical active list [D]

- **Registration index.** Every registered ID has one. Genesis IDs are numbered first, in their published order; each new ID then takes the next index, in PoW-ID confirmation order.
- **Canonical active list.** All registered IDs in index order, excluding banned and deactivated IDs.
- Every node builds both from chain data alone. Allocation (§4.2) and the committee lottery (§4.3) use them.

---

## 4. Witness layer

### 4.1 Duty [D]

Every registered ID is a witness node in exactly one WC, permanently on duty. A WC leads one KWC and is a subordinate in three others; each node therefore watches about 1,000 miners. Registered IDs must keep witnessing whether or not they mine.

### 4.2 Allocation [D]

Allocation is deterministic and publicly recomputable from chain data. Not even an ID's owner knows its placement when minting. The rationale is in `docs/ARCHITECTURE.md` §8.

**Beacon.** The beacon for an ID is the hash of the PoW-ID block a fixed delay after the ID's confirmation: 6 blocks by default, equal to the confirmation depth (§2). A returning ID whose seat was vacated (§4.7) uses the hash of the block the same delay after its re-activation is confirmed. Genesis IDs are allocated first, in registration-index order, using a public launch seed in place of a beacon.

**When placement takes effect [D].** At the ID's beacon block (its confirmation, or its re-activation's confirmation, plus the allocation beacon delay), with no further delay, whether or not an Allocation Committee Block has appeared. Every node computes the placement from chain data.

**Seats: inside-out Fisher–Yates insertion.**

1. Seats are numbered 0, 1, 2, …; WC `w` is seats `10w` to `10w + 9`.
2. IDs are inserted in registration-index order (§3.11). When an ID is inserted while `n` seats are filled (seats 0 to `n − 1`), compute `j = SHA256("GB/alloc" ‖ ID ‖ beacon) mod (n + 1)`, reading the hash as a 256-bit integer.
   - If `j = n`, the ID takes seat `n`.
   - Otherwise the ID in seat `j` moves to seat `n`, and the new ID takes seat `j`.
3. A WC is **active** when all 10 of its seats are filled, so the number of active WCs is `W = ⌊filled seats / 10⌋`. The partial tail WC is **pending**: its members may mine (§3.10) but do not witness until it fills.
4. **Removal** (a ban, or a continuous absence longer than the long-absence threshold L, §4.6–§4.7): the ID in the last filled seat moves into the vacated seat. If that empties a seat of the last active WC, that WC returns to pending, `W` decreases by one, and the KWC ring is recomputed. Going offline or being deactivated does not remove an ID from its seat (§4.7).
5. **Re-insertion.** A returning ID whose seat was vacated is re-inserted by step 2, with the re-activation beacon above. An ID that returns within L never left its seat.

**KWCs: a Golomb-ruler ring.** KWC `w` has leader WC `w` and subordinate WCs `(w + 1)`, `(w + 4)` and `(w + 6)`, all mod `W`. The 30-node comparison uses `(w + 1)` and `(w + 3)`.

- Each WC leads exactly one KWC and is a subordinate in exactly three.
- There are **no mutual monitoring pairs** (two WCs must not monitor each other in different KWCs), and any two KWCs share at most one WC.
- These properties need `W ≥ 13` (`W ≥ 7` for the comparison), which holds from genesis.

**Consequences.**

- Each new ID changes the membership of one existing WC and moves one existing ID into the pending tail WC. Chain membership is therefore not fixed. In exchange, every WC's composition tracks the attacker's share of the population, not its share of recent issuance.
- This replaces the per-ID formula `chain(ID) = f(SHA256(ID ‖ beacon))` and the older rule that a vacancy is filled by a newly registered ID.

**Role of the CAC.** The Allocation Committee records and attests the placement in its block. Placement does not wait for that record (above), so a stalled committee cannot delay it. Every node recomputes the record and rejects a mismatch. The committee cannot choose a placement, and rejecting a block cannot produce a different one.

**[P, optional test]** Diversity constraints within a KWC (for example, no two members from the same /48 or the same network operator).

### 4.3 Chain Allocation Committee [D]

600 members, first in, first out. 66% approval for an Allocation Committee Block, meaning at least two-thirds of members, rounded up: ⌈2n/3⌉ (400 of 600). Leader rotates. A leader that gets two conflicting blocks approved has both rejected. The committee's block is a record and attestation: no placement waits for it (§4.2).

**Seat lottery [D].** For every 10th PoW-Tx block B, the new CAC member is the ID at position SHA256("GB/cac" || hash of block B+k) mod N_active in the canonical active list, where N_active is the length of that list. If that ID is already a member, take the next position, wrapping around, until a non-member is found. The oldest member leaves (first in, first out, unchanged). k = 3 [O] (using a later block limits the influence of whoever mines block B). Every node computes the committee from chain data; nothing extra is broadcast.

- **Timing.** The new member is drawn before the oldest member leaves, so the oldest member counts as a member during the draw. The canonical active list (§3.11) is read as of block B, so it is fixed before beacon block B+k exists.
- **Modulo bias.** Hashes are 256-bit, so each position's probability differs from 1/N_active by a relative amount below N_active / 2²⁵⁶. That is below 10⁻⁶⁸ at 10⁹ IDs, and negligible far beyond.
- **Result.** An attacker's expected share of committee seats equals its share of active IDs. Mining power plays no part.

**Departures [D].** A member that is banned or deactivated leaves the committee at once. An extra lottery draw fills its seat at the next 10th PoW-Tx block, using the same next-position rule. **[P] Extra-draw position:** SHA256("GB/cac" ‖ hash of block B+k ‖ i), where i = 1, 2, … counts the extra draws from the same beacon; the regular draw keeps its formula above.

**Duties of the leader's block.**

- A record and attestation of the placement of new and re-inserted IDs (§4.2); placement takes effect without it.
- Committee joins and leaves, which every node can recompute from the lottery.
- Bans already approved by KWCs, compiled into Master Blacklisting Blocks, one for registered and one for unregistered miners.

**Stalling.** An attacker holding enough seats can stall Allocation Committee Blocks. A stall delays the committee's record and its compilation of approved bans; it cannot delay or change any placement, which takes effect at each ID's beacon block (§4.2). M4 tests the impact.

**Comparison rule (v0.2).** The miner of every 10th PoW-Tx block joins; if it is already a member, the seat goes to the next 10th-block miner who is not. It is kept only as a labelled comparison in M1.

### 4.4 Proposers [D]

Each KWC has two rotating proposers, changing every 2 PoW-Tx blocks: a **master** drawn from all 40 members, and a **subordinate-only** proposer drawn from the three subordinate WCs. If a proposer fails to act, the next in rotation does.

**Proposer rights [D].**

- **Master proposer:** MOBr, offline requests for registered miners, and ban proposals.
- **Subordinate-only proposer:** MOBu and offline requests for unregistered miners. As the one exception to the master's ban rights, it may propose bans of leader-chain members who refuse to witness newcomers.
- **The miner:** its own self-service offline request (§3.2).

### 4.5 One Chance [D]

If an online member's signature is missing from a miner's aggregate, every other member relays the miner's original signed request to that member. The member gets one chance to sign and send its signature to the miner and all members; members relay it to the miner, who gets one chance to include it. A member that still refuses is penalized; a miner that still omits it is refused entry. A proposer that issues a false blacklist or offline request gets One Chance itself; repeating it gets it banned by the next proposer, using the first request embedded in the second as proof. Each exchange has a time limit of *n* blocks.

- **Wait [O].** The default wait before One Chance is judged to have failed is one grace epoch, swept over 1, 2 and 5 rounds (§2). One Chance is the intended mechanism for members that disappear without announcing it.
- **Client requirement [D].** A node shutting down normally sends its own offline request first, so only crashes and power cuts are unannounced.

### 4.6 Ban principle [D]

- **Network-level bans require verifiable evidence** (for example, two conflicting signed statements, a signed false statement, or documented refusal through One Chance).
- **Bans are only for proven misbehaviour.** Being offline is not misbehaviour: offline beyond the allowance leads to deactivation (§4.7), never a ban.
- **Miner complaints may trigger a check but never decide a ban.** Witnesses vote (66% of the KWC, meaning at least two-thirds rounded up: 27 of 40) only on behaviour they observed themselves after One Chance.
- **Voting pool.** The full 40-member KWC votes on bans, MOBu/MOBr and offline requests. The subordinate-only proposer (§4.4) only proposes.
- **Unreachable is not refusal [D], the default.** A member that cannot be reached is treated as offline under the allowance, without a penalty. A ban requires verifiable evidence: equivocation, or a signed refusal while demonstrably online. A member that is online but cut off by a network partition counts as unreachable. The strict alternatives (the §4.8 ladder on a failed One Chance, with or without a yearly reset, and an immediate ban) are comparisons only.
- **Conflicting decisions [D].** If two conflicting KWC decisions are both approved, both are rejected. A member who signed both has produced equivocation evidence and may be banned.
- **Witness double-signing [D].** A witness that signs PoWits for two different blocks from the same miner at the same height has produced equivocation evidence, and is banned under the rules above.

### 4.7 Offline handling [D rules, O values]

Grace epoch; self-service offline requests; forced signing via One Chance for members that withhold while online.

**Offline beyond the allowance leads to deactivation, never a ban.**

- A deactivated ID stops counting as active and leaves the canonical active list (§3.11).
- **Seat rule [D].** Going offline or being deactivated does not change any seat. A deactivated ID keeps its WC seat as an **offline member** of its KWC: it does not sign, and quorums are met by online members.
- A seat is vacated (swap-with-last, §4.2) **only** on a ban, or after a continuous absence longer than the **long-absence threshold L** (§2: 30 days [O], which is 86,400 PoW-ID blocks at 30 s).
- On return an ID waits the **re-activation wait** and counts as active again. If its seat was vacated, it is re-inserted by §4.2, with the beacon of the block 6 blocks after its re-activation is confirmed.
- The durations (deactivation threshold, re-activation wait, L) are [O] and swept (§2).
- Only bans and absences longer than L change KWC compositions (§10 H6). Absent members that keep their seats reduce how many members are online to meet quorums; M1 section H maps the honest uptime this requires.

**Offline points ladder [D]** (architect, 2026-10-21; M3a.1 E4). Every offline period of at least n = 24 hours earns points, whatever its cause: 1, 1, 2, 3, 4, 5, then 5 for each further period. A two-thirds decision of one of the member's KWCs records them, at the first round one can decide. At 20 points the member loses its seat, with a 30-day cooldown before it may take a seat again [D]. The steps, the cap and the reset after 12 months of block time are as tested [O]; blacklisting at the cap is the comparison.

In E4 (10,000 miners, 290,000 IDs, every region including scheduled load-shedding, 365 days, under "unreachable is not refusal" and all-online entropy), n = 1 h and n = 6 h fail H7 in every region. The highest yearly loss rates are 947 % (seat loss) and 1,213 % (blacklist) of honest IDs at n = 1 h, and 151 % and 209 % at n = 6 h. At n = 24 h H7 holds in every region (highest 0.51 % and 0.52 % a year, in Africa), and no member reaches 20 points from scheduled load-shedding alone. Rerun under the threshold entropy default (efab187), n = 24 h with seat loss again keeps H7 in every region (0.34–0.50 % a year, highest in Africa; 0.51 % with a blacklist), and n = 1 h and 6 h again fail everywhere.

### 4.8 Penalties [D]

Lesser offences: 24-hour suspension, then 30 days, then permanent. A suspended ID still performs witness duty but earns nothing. Proven fraud may be permanent immediately. A banned member's signature is excluded from entropy aggregates.

---

## 5. Transaction layer (minimal model)

Longest chain; one ID = one channel at 1 hash/s. Modeled only to the extent needed for: CAC seat assignment, active-ID share, pool concentration, and a selfish-mining sanity check against active-ID share.

---

## 6. Economic inputs (not design decisions)

Example values per PoW-Tx block: miner 1 Shamsi + ~80% of fees; each signing leader-WC member 0.1 Shamsi + 1.9% of fees (at least 7); CAC leader 0.1 Shamsi + 1% of fees when applicable; public rewards pool (Murphy) 1 Shamsi. **PoW-ID blocks carry no reward.** Token price, hosting prices and bandwidth prices are external parameters.

A [P] proposal to separate PoWit validity from payment is recorded in §12 for M4; the payment rule above is unchanged.

---

## 7. Threat model

The adversary may: obtain arbitrary compute; acquire large IPv6 allocations; lease cloud infrastructure across many providers and autonomous systems; perform temporary routing hijacks; flood admissions; sustain long-term spending; run many witness IDs and collude; bribe; operate botnets or rent residential proxies; flood specific home nodes offline; manipulate clocks and time sources; control a share of a newcomer's peers; behave irrationally (non-profit motives).

The adversary cannot: break SHA-256 or BLS12-381 security; forge signatures.

---

## 8. Scenario catalogue

**A. Issuance and time**

- **S1** Baseline honest network (sanity check: share ∝ /64 share).
- **S2** Well-funded datacenter attacker; attacker share 5–60%; honest growth and churn swept.
- **S3** Patient accumulator over multi-year horizons.
- **S4** Botnet / residential proxies: free nodes with high churn (100k, 500k, 1M devices).
- **S5** Restart attack: fixed-for-duration entropy (old design) versus per-round entropy (current). Realistic restart cost C = grace epoch + convergence interval + post-admission wait (450 s at the §2 defaults).
- **S6** Difficulty hopping: Variant A versus Variant B.
- **S7** Adaptive issuance manipulation, with and without the `T_min` cap.
- **S8** Unregistered-slot exhaustion and honest demand surges.
- **S9** Honest churn, patience and dropout, and the offline-handling sweep: deactivation threshold, re-activation wait and long-absence threshold L (honest attrition).
- **S10** Genesis size sweep, including a fraction of genesis IDs secretly controlled by one party.

**B. Entropy and pacing integrity**

- **S11** Grinding: (a) old flow with miner signing last using non-unique signatures; (b) subset selection; (c) colluding last witness; (d) header equivocation; (e) current BLS all-online-members flow; (f) the online-set lever: c colluding members choose among up to 2^c entropies by withholding, measured under the strict rule, under the default "unreachable is not refusal" rule (§4.6) and under threshold BLS entropy (§12). Expected: advantage in (a)–(c), detection in (d), none in (e); for (f), an advantage that ban rules can only price and threshold BLS entropy removes.
- **S12** Early signing: fraction of KWCs capable of signing without honest members versus attacker share; detection via timestamp checks (§3.7); clock skew sweep.
- **S13** Targeted delay of entropy: start rule (a) versus (b).
- **S14** Sorted-hash collision test (separate unit test).

**C. Witness layer**

- **S15** KWC stall and capture probabilities versus attacker share: exact calculation plus simulation over allocation and network growth.
- **S16** Intermittent stalling that stays within the offline allowance.
- **S17** Targeted flooding of honest witnesses and One Chance griefing; the strict rule versus the §4.6 default.
- **S18** Stalling proposers trapping honest miners as "online"; self-service recovery time.
- **S19** Lazy onboarding by chains; effect of the subordinate-only proposer.
- **S20** Lazy subordinates that sign without recomputing.
- **S21** CAC capture and stalling over multi-year runs, under the seat lottery (§4.3), with the v0.2 rule as a comparison. Primary measures: exact entries into each state per year, and the share of time spent in it. The M1 brief's independent-composition estimate is reported only as a labelled comparison.
- **S22** Correlated operators: a deliberately "ugly" operator graph (shared cloud regions, one hidden owner of many IDs).
- **S23** Regional partition plus provider outage plus miner stampede at peak demand; client reroute backoff policy.
- **S24** Eclipsed newcomers, with a varying fraction of poisoned peer-discovery sources.
- **S25** Prefix-hijack replay attempting to frame an honest /64 as a duplicate.
- **S26** Spoofed-source framing, with and without a mandatory handshake.

**D. Cost**

- **S27** Real-hardware benchmark: persistent sessions per server, memory, CPU and bandwidth at protocol message rates.
- **S28** Money cost model: attacker cost per /64-year and per ID; break-even against rewards; bribery cost priced as detection probability × (loss + future rewards).

**E. Transaction-layer sanity**

- **S29** Pool concentration and selfish-mining thresholds measured against active-ID share.

**F. Start-rule integrity (M4)**

- **S30** Backdated start under rule (r) (§3.5): t or more colluding members of a KWC attest an earlier `t0` than the real issue time. Measure the gain of the miners they serve against the backdate and the colluding share, at both thresholds (t = 27 and t = 21 of 40), and test countermeasures.

**G. Threshold entropy (M4)**

- **S31** Blocking and capture (§3.4): n − t + 1 members refusing or unreachable stop a KWC's entropy (14 at t = 27, 20 at t = 21), and t colluding members produce it without the others. Measure both sides at both thresholds, with M3's household profiles.

---

## 9. Metrics (every run records config, seed and git commit)

- Attacker share of **issued** and **active** IDs over time; time to 33%, 51%, 67%.
- **Amplification factor:** attacker issuance share ÷ attacker /64 share.
- Honest expected wait to first ID; honest dropout.
- Honest attrition: IDs lost without misbehaviour.
- Fraction of KWCs stallable / capturable; witness share reachable from each region.
- Detection latency, reroute time, failed observer lookups, ambiguity window, recovery cost.
- Fork / orphan rate; deviation of issuance rate from target.
- Per-node load on a household witness (CPU, bandwidth, messages).
- Attack cost: money to affect X% of the network at size N; money per honest victim.
- Concentration over time (Gini coefficient, Nakamoto coefficient).

---

## 10. Pre-registered hypotheses

**Thresholds locked before M3, 2026-10-09** (architect). Results are published whether each passes or fails.

- **H1 Linearity.** For every protocol-compliant attacker strategy in S1–S4, the amplification factor's 95% confidence interval lies within [0.95, 1.05].
- **H2 Time floor.** With 100% issuance capture, time to 51% of active IDs matches the analytic value `(0.51/0.49) × H_active / R` within ±5%.
- **H3 No grinding.** Under S11(e), the attacker's advantage over an honest miner has a 95% confidence interval that includes 1 and whose upper end is at most 1.02, with at least 10,000 trials.
- **H4 No restart advantage.** Under per-round entropy (S5 current), the restart strategy's advantage factor is ≤ 1.02.
- **H5 Hopping.** Under Variant B, including the correction M3 proposes, the hopping gain is ≤ 2%; report Variant A's gain.
- **H6 Witness capture.** At attacker share ≤ 25%, the expected number of distinct episodes in which a KWC can sign without honest members, over 10 years at 100,000 KWCs, is < 1, for the unregistered quorum under the v0.4 seat rule with L = 30 days and M3's household profiles. Stall fractions are reported without a threshold.
  - *Meaning:* the attacker's seats alone meet the quorum. Such a KWC still cannot make an early-signed block valid beyond δ, because §3.7 rejects a block received before its own timestamp. Its powers are stalling and censoring miners in that KWC; entropy grinding needs every seat.
  - *Primary measure [D]:* **distinct episodes**, the expected number of times a KWC enters the state (entries into the state). Raw compositions in the state are reported as an upper bound.
  - *Refresh model:* compositions form when an ID is inserted (a new ID, or a returning ID whose seat was vacated: about 4.7 compositions each) and when an ID is removed (a ban, or an absence longer than L: about 4.6 each). Going offline within L changes no composition (§4.7). The v0.2 count (about R/10 new KWCs per year, plus 4 per ban replacement) is reported as a comparison.
  - *Ban rate:* 0 per year in the base case; 1% and 5% of registered IDs per year as labelled sensitivities.
  - Report for both quorums: the unregistered quorum governs PoW-ID, the registered quorum governs PoW-Tx.
- **H7 Honest attrition.** IDs lost per year without misbehaviour ≤ 1%, under both M3 regional profiles (reliable and unreliable grid), for the chosen offline parameters.
- **H8 Griefing.** Under the §4.6 [P] rule, zero griefing bans of honest witnesses; report the cost per victim under the strict rule.
- **H9 Cost.** Report the money required for an attacker to hold 33% and 51% of active IDs for one year at 1M and 10M honest active IDs (no threshold; reported).

---

## 11. Methodology requirements

1. **Two-tier modelling.**
   - *Tier 1:* protocol-level discrete-event simulation with real message exchange, regional latency distributions and churn, at reduced scale (up to ~100,000 miners).
   - *Tier 2:* aggregate statistical model at full scale (millions), **validated against Tier 1** wherever their scales overlap.
2. **Real cryptography** for micro-tests (SHA-256, BLS12-381); simulators may substitute statistically equivalent abstractions only after the equivalence is tested and documented.
3. **Analytical ground truth first.** Anything with a closed form is computed exactly, and the simulator must match it before being trusted beyond it.
4. **Determinism.** Every run is seeded; config, seed and commit are recorded; results are bit-for-bit reproducible.
5. **No silent assumptions.** Every modelling assumption is listed in `ASSUMPTIONS.md` with its source (this spec section, a paper, or an external data source).
6. **Outputs** are machine-readable (CSV / JSON / Parquet) plus plots. Any front end reads outputs only and never recomputes.
7. **Implementation language:** Rust, for the simulation and the Stage 3 testnet. Protocol-core logic is shared between them.
8. **Tier 1 simulator conventions [D]** (architect, 2026-10-12 and 2026-10-14):
   - Additive, behaviour-preserving changes to `gb-protocol` and `gb-config` are allowed, with the M1 tables, the M2 tables and the test vectors pinned byte for byte.
   - Crypto modes: real, the M2 C6 stand-in, and a fast mode used at 10⁴–10⁵ miners only after the V0 gate shows the three agree.
   - Latency comes from RIPE Atlas measurements, aggregated to regions and credited.
   - A reference run of `gb sim` is committed in `results/sim/reference/`.
   - A check whose analytic reference does not model the simulated rule is reported as **reference-limited**, not as a failure, and no new reference replaces it: the V2 old-design restart checks and the V3 Variant A checks at m = 4.
   - The zero-latency orphan reference (C2) is E[H_K − 1], where K is the number of miners tied at the earliest step and H_K the K-th harmonic number. This was changed after the check failed against E[K − 1] in M3a (ASSUMPTIONS SIM-30).

---

## 12. Open questions

- PoW-Tx block interval.
- Post-admission wait, convergence interval, clock tolerance δ.
- Offline deactivation and re-activation durations.
- Registered / unregistered capacity split.
- Mining-API streaming: format and whether mandatory.
- Witness beneficiary details.
- Key compromise and revocation (for example, a pre-registered backup key): post-Stage 1.
- Diversity constraints in allocation.
- Witness Chain decentralization trigger for Stage 4 (published criteria).
- How often a household ID is absent per year and how long absences last (§4.7); M3 household downtime profiles supply them.
- **The leader-WC requirement is the main stall risk for registered KWCs.** The registered PoWit needs 7 of the leader WC's 10 members as well as 21 of the 30 subordinates; M1 section H shows this fails far more often under honest downtime than any 27 of 40. M4 compares it with a single 27-of-40 rule, together with the forced-signing (§4.5) and ban (§4.6) rules.
- **[P] Separate validity from payment (M4).** Any 27 of 40 signatures make a PoWit valid, with no separate leader-WC requirement. A leader-WC member is paid only if its own signature is in the PoWit. The reward share of a leader member who did not sign is never generated: it is not created and does not go to the miner, so no one gains by omitting signatures. M4 compares this rule with 7 + 21 on liveness (M1 section H), on the incentive for leaders to stay online and sign, and on total issuance per block. The payment rule of §6 is unchanged until then.
- **[P] Raising the attacker share that ID issuance can tolerate without relying on bans (M2/M4):**
  - (a) **Split rule:** a lower quorum for PoWits only, for example 51%, since validators recompute every PoWit (§3.7). M1 section G maps it.
  - (b) **Larger KWCs**, for example 60 or 100 seats. M1 section I maps the trade-off.
  - (c) **Threshold BLS entropy:** any quorum of members produce the same signature, which removes the single-member entropy stall and entropy grinding. M2 measures the key-setup cost. **M3a.1 E3 (liveness):** the test fixed in advance was no gap over 10 minutes between final-chain blocks in any of 41 regional blackouts of 10.88 hours (the day-9.19 event, and one in each of the 8 regions over 5 seeds). At the v0.6 tie-break, the default when E3 ran, every entropy rule failed it, mostly through sit-out stalls (ASSUMPTIONS SIM-47). Rerun at the v0.7 defaults (an informational run of the configuration before its commit), t = 21 of 40 passed, with and without failover (longest gap 6.1 minutes). t = ⌈2n/3⌉ failed narrowly (1 of 41 blackouts; 16.3 minutes without failover, 14.8 with), and so did the all-online rule (17.8 minutes in 1 of 41; with failover, 18.7 minutes in 2 of 41). Every remaining long gap starts with a blackout of as_east, the largest region, while the difficulty still counts its miners. The security cost of t = 21 of 40 (E3_security.csv): an attacker with 25 % of seats holds at least 21 of a KWC's 40 with probability 1.7 × 10⁻⁴, against 1.9 × 10⁻⁸ for 27; at 33 %, 8.4 × 10⁻³ against 8.5 × 10⁻⁶. Since v0.7, t = ⌈2n/3⌉ is the [P] default (§3.4) and t = 21 of 40 stays [P]. In the reference run at the final defaults (1d9ce91), t = 21 of 40 passed again (longest gap 6.2 minutes); t = ⌈2n/3⌉ failed (3 of 41 blackouts; 13.0 minutes without failover, 16.6 with), and so did the all-online rule (1 of 41, 12.6 minutes; 15.1 with failover). §3.4 gives the causes.
    - **[P] Share refresh (architect, 2026-10-19).** Every 30 days the shares of each KWC's threshold key are re-randomised (proactive secret sharing): the group public key stays the same, and shares stolen one by one over time expire at the next refresh, so an attacker needs t shares from a single 30-day epoch. M4 measures the cost per member.
  - Decisions that validators cannot recompute (bans, MOBu/MOBr, offline requests, committee records) remain bound by the one-third limit and rely on the ban rules.
- **Online-set grinding lever (found in M2 C1(c′)).** Entropy comes from every online member, so c colluding members can choose among up to 2^c entropies by deciding who withholds; M2 measured 1.88× for one member. The default "unreachable is not refusal" rule (§4.6) makes withholding nearly free. Threshold BLS entropy, the [P] default since v0.7 (§3.4), removes it; M4 measures the lever (S11(f)) under the all-online comparison, with the strict and the default unreachable rules.
- **Threshold entropy in M4.** The blocking side of the threshold next to the capture side: n − t + 1 refusing or unreachable members stop a KWC's entropy (14 at t = 27, 20 at t = 21), while t colluding members produce it alone (27 or 21; scenario S31); and backdated starts (S30) at both thresholds.
- **[P] Threshold-BLS randomness beacon (M4).** Replace the block-hash beacons used for seat placement (§4.2) and the committee lottery (§4.3) with a threshold-BLS randomness beacon, and compare it with the current beacons.
- **[P] Witness peer topology (option 2).** No persistent witness-to-witness connections: the miner relays routine messages. Members connect on demand, through the global directory of member addresses, only for One Chance, decisions (MOBu/MOBr, offline requests, bans), proposer duties and catch-up. Online status comes from the network record, not from heartbeats: a member counts as online until an offline request is recorded, by the member itself or by its KWC proposer after One Chance fails. Address changes are announced network-wide and update the directory. M1 section I estimates the on-demand connections; M3/M4 measure how often a member that disappears without announcing it stalls its KWCs' entropy until One Chance fails (about one grace epoch). The alternative is persistent connections among all members of a node's KWCs.

---

## 13. Stage 3 controlled public testnet: decisions recorded for planning

These decisions are out of scope for Stage 1 code. They are recorded so the simulators and `gb-protocol` do not contradict them.

- **Purpose.** Traction, plus a live test of the core thesis: many miners competing under the pacing at a fixed issuance rate. It is not a test of witness decentralisation.
- **Witnesses [D].** Run by the foundation, and stated publicly. Each server runs 3 witness nodes as one WC, with a quorum of 2 of 3, real BLS entropy from all online members and real One Chance. Miners are public.
- **Miner identity [D].**
  - IPv6: one slot per /64.
  - IPv4: at most K slots per public IPv4 address (default K = 1, adjustable), each tied to an email-verified account, with one active session per account and a captcha at sign-up.
  - Every miner's address type and /64 are recorded, so a strict one-slot-per-/64 rule can be evaluated later from real data.
  - Mainnet keeps one slot per /64 (unregistered) or per ID (registered).
- **Rewards [D].** Each PoW-ID block pays 0.1 Shamsi testnet credits (105,120 per year at 30 s), with a published cap. This is a testnet-only exception to §6, where PoW-ID blocks carry no reward.
  - Wording [D]: "testnet credits, no guaranteed value, redeemable at mainnet subject to terms and KYC."
  - Blocks record a random account ID generated at sign-up, never an email; a private database maps account IDs to emails.
  - Mainnet IDs are issued only through KYC at genesis.
- **Code [D].** The miner client is open source; the node and WC code stay private until Stage 4.
- **Before public launch [D].** A private staging run: about 10 nodes in containers on one server, with injected delay and loss via `tc netem`. This replaces the separate live mini-testnet idea; M3's simulator covers network effects at scale.

---

## Appendix A. Wire formats [P]

Byte layouts that `gb-protocol` and its test vectors follow. They are proposed, and can change before M3.

- **Integers** are big-endian at fixed widths. Every hash input starts with a domain tag (an ASCII string, written below in quotes).
- **Header.** §3.3 lists the header's fields, not their order; this is the canonical [P] order:

  | Field | Bytes |
  |---|---|
  | version | 4 (u32) |
  | height | 8 (u64) |
  | previous PoW-ID block hash | 32 |
  | candidate public key (BLS, G1 compressed) | 48 |
  | reward wallet | 32 |
  | address type: 0 = IPv6 /64, 1 = IPv4 | 1 |
  | address: the /64 prefix, or the IPv4 address left-padded with zeros | 8 |
  | KWC ID (index of its leader WC) | 8 (u64) |
  | difficulty target (256-bit) | 32 |

  The header is 173 bytes. Mainnet uses address type 0 only; type 1 exists for the §13 testnet.
- **Digests.** `header_digest = SHA256("GB/header" ‖ header)`. Block hash = `SHA256("GB/block" ‖ header ‖ E ‖ N)`, with N as a u64; the last step of the same-height tie-break compares block hashes as 256-bit integers.
- **Hash chain (§3.5).** `prev_hash` 32 bytes, height u64, timestamps u64 seconds, nonce u64, E 32 bytes, header_digest 32 bytes. A step wins when `h_n`, read as a 256-bit integer, is below the target.
- **Signer bitfield.** One bit per KWC seat in seat order: the leader WC's seats 0–9, then each subordinate WC's seats 0–9 in ring-offset order. Bit i is bit (i mod 8) of byte ⌊i/8⌋, least significant first; 40 seats take 5 bytes.
- **PoWit.** Members sign `"GB/powit" ‖ block hash`; the PoWit carries the aggregate signature (96 bytes) and the signer bitfield.
- **BLS.** BLS12-381, minimal public-key size: public keys in G1 (48 bytes), signatures in G2 (96 bytes), ciphersuite `BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_`, with proofs of possession under `BLS_POP_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_`.
- **Equivocation.** Two different headers at the same height, each signed by the same candidate key with the miner signature of §3.4, are proof of equivocation.
- **Threshold entropy (§3.4) [P], protocol version 2.** The block carries the KWC's group signature `σ_K` on `"GB/entropy" ‖ H ‖ σ_m` and its key epoch instead of the members' aggregate and signer bitfield; `E = SHA256("GB/entropy-out" ‖ σ_K)`. `vectors/protocol_v1.json` (the aggregate of every online member) is unchanged.
- **Start attestation (rule (r), §3.5) [P], protocol version 2.** The KWC signs `"GB/t0" ‖ header_digest ‖ t0`, with `t0` a u64 of seconds; the block carries `t0` and that signature. The entropy is unchanged (it depends on the header only). `vectors/protocol_v1.json` (version 1, rule (a)) is unchanged.

---

## Changelog

### v0.7 — 2026-10-22

Incorporates the architect's M3a decisions (2026-10-12), the M3a.1 decisions (2026-10-14), the share refresh (2026-10-19), the M3a.1 defaults (2026-10-21) and the decisions on the M3a.1 reference run (2026-10-22), with the M3a.1 evidence. §10 (H1–H9) is unchanged.

- **Start rule (r) [D]** (§3.5): a miner's chain starts at its KWC's attested entropy issue time; no missed step is computed. Rule (a), the default until v0.6, is the comparison. Known risk: t or more colluding members could backdate `t0`, tested in M4 as S30 (§8). Appendix A records the start attestation [P] for protocol version 2; `vectors/protocol_v1.json` is unchanged.
- **Fork choice [D]** (§3.7): at the same height, the block a node received first, then the lower block hash. v0.6's earlier timestamp then lower hash is the comparison. M3a.1 found that under v0.6's order, two blocks at the same timestamp whose later one has the lower hash leave nearly every miner sitting out under one header per height, which stalls the chain for up to tens of minutes (ASSUMPTIONS SIM-47).
- **One header per height [D]** (§3.4, M3a decision 5) stays the default; re-signing on a new parent is the [P] comparison (identical under first-seen in E1).
- **PoWit deadline [D]** (§2, §3.7): D = 10 s with the soft scope, measured by each node on its own clock against the block's attested timestamp (2026-10-22). E1's criteria as written picked no deadline in the corrected M3a.1 run, the default from 2026-10-21 until then, and D = 10 s at the final defaults. This supersedes M3a decision 9 (the permanent scope).
- **Difficulty [D]** (§3.8): the issued-entropy count with the P2 ×1.1 correction (2026-10-22; P2 ×1.5, the default from 2026-10-21, misses H5 at the final defaults and is a comparison). This supersedes M3a decision 6, the count after the post-admission wait. P1, P2 ×1.25 and ×1.5, and B-exact (M3a decision 13) are comparisons.
- **Known caveat** (§3.7): the honest win share differs by region under the defaults (C1); accepted for now, explained and mitigated in M3b (2026-10-22).
- **Entropy quorum** (§3.4): at least 27 of 40 members must actually sign (a clarification; the simulator had issued entropy with fewer, ASSUMPTIONS SIM-43).
- **Unreachable is not refusal [D]** (§4.6; M3a.1 decision 1): bans only on verifiable evidence; a member cut off by a partition counts as unreachable. This supersedes M3a decision 11 (the strict ladder) and the interim default in M3a decision 15.
- **IDs in replaced blocks are void [D]**, and confirmation depth k means k blocks built on top (§3.7; M3a.1 decision 3).
- **Early blocks [D]** are rejected for good (§3.7; M3a decision 10). **Entropy signing [D]** after validating the header's parent (§3.6; M3a decision 7). **PoWit signing [D]** on time, at the member's own `t0 + N` (§3.6; M3a decision 12). **Quick return [D]** within the grace epoch (§3.2; M3a.1 decision 6).
- **Entropy** (§2, §3.4, §12): threshold BLS entropy with t = ⌈2n/3⌉ of the seated members is the [P] default (2026-10-21, after the report). It closes the online-set grinding lever at the same liveness as the all-online rule: in E3 both missed the pre-set blackout test narrowly (15–19 minutes in 1 of 41 blackouts at the v0.7 defaults; at the final defaults 13.0 minutes in 3 of 41 for t = ⌈2n/3⌉ and 12.6 minutes in 1 of 41 for the all-online rule), after every rule had failed it at the v0.6 tie-break, mostly through sit-out stalls (SIM-47). t = 21 of 40 passed (longest gap 6.1 minutes) and stays [P], with its security cost. The all-online rule is the comparison. §3.4 states the remaining limit.
- **Failover [P]** (§3.1): off by default; M4 revisits it with correlated outages.
- **Offline points ladder [D]** (§4.7): n = 24 h, and seat loss with a 30-day cooldown at 20 points (2026-10-21, after the report); the steps, cap and reset stay [O]. E4 rerun under the threshold entropy default: H7 holds in every region at n = 24 h (at most 0.50 % a year).
- **[P] Threshold-BLS randomness beacon** for M4 (§12; M3a decision 15) and **[P] 30-day share refresh** of threshold keys (§12; 2026-10-19).
- **Tier 1 simulator conventions** (§11 item 8): M3a decisions 1–3 and 8, and M3a.1 decisions 4 and 5. M3a decision 4 (the repository-local git author) and 14 (the withholder measured in C3) are recorded in ASSUMPTIONS.
- **Scope** (§0): the out-of-scope list names throughput, SCA, the SOS VM and smart contracts, the BPU/BVU, rewards and token economics, and storage; PoIE bounds how fast IDs accumulate and does not prove one person per ID.
- **Scenarios** (§8): S11(f) and S17 refer to the default unreachable rule; S30 (backdated start under rule (r), at both thresholds) and S31 (threshold blocking and capture) are added for M4.

### v0.6 — 2026-10-09

Decisions by the protocol architect on the M2 results.

- **§10.** H1–H9 thresholds locked before M3, 2026-10-09: H1 and H2 unchanged; H3 CI includes 1 with upper end ≤ 1.02 and at least 10,000 trials; H4 ≤ 1.02; H5 ≤ 2% including the M3 correction; H6 < 1 episode at p ≤ 25%, 100k KWCs, L = 30 days with M3 profiles; H7 ≤ 1% per year under both regional profiles; H8 zero griefing bans under the [P] rule; H9 reported at 1M and 10M IDs.
- **§8 S11(f) and §12.** The online-set grinding lever found in M2 C1(c′): c colluding members choose among up to 2^c entropies by withholding. M4 measures it under the strict rule, the [P] rule and threshold BLS entropy; threshold BLS entropy is the leading M4 candidate.
- **§3.4 [P].** Optimistic verification of the entropy aggregate (verify once; verify shares only on failure), used in the M4/M5 load model.
- **§3.7.** Fork choice in a precise order: longest valid chain; then the earlier timestamp (smaller N); then the lower block hash [P].
- **§4.6 [D].** A witness that signs PoWits for two different blocks from the same miner at the same height has produced equivocation evidence and is banned.

### v0.5 — 2026-10-08

Decisions by the protocol architect, recorded after M1.2.

- **§2.** L = 30 days is the primary value (still [O]; sweep unchanged). New rows: One Chance wait [O] (1 grace epoch; sweep 1, 2, 5 rounds) and PoWit arrival deadline D [O] (no default yet; sweep 5, 10, 30, 60 s).
- **§4.5.** The One Chance wait, and the client requirement [D] that a node shutting down normally sends its own offline request first, so only crashes and power cuts are unannounced.
- **§3.7.** New PoWit arrival deadline: a PoW-ID block whose PoWit is not received within D seconds of its timestamp is invalid and is not extended; M3 measures the effect. Fork-choice wording clarified ("the longest valid chain, where every block must carry a valid PoWit; two valid blocks at the same height are resolved by the lower block hash [P]"); no rule change.
- **§12.** The long-absence threshold L is no longer listed as open.
- **§13 (new).** Stage 3 controlled public testnet: decisions recorded for planning, out of scope for Stage 1 code. Includes the testnet-only exception to §6 (PoW-ID blocks pay testnet credits).
- **Appendix A (new) [P].** Wire formats for `gb-protocol` and its test vectors: integer encoding, the canonical header field order (with an address type so the §13 testnet can record IPv4 miners), digests, the hash-chain encoding, the signer bitfield, the PoWit message and the BLS ciphersuite. Reason: §3.3–§3.7 name the fields and hashes but not their bytes.

### v0.4 — 2026-10-07

Decisions by the protocol architect, recorded after reviewing the M1.1 results.

- **§4.7 and §4.2 seat rule [D].**
  - Going offline or being deactivated no longer changes any seat. A deactivated ID keeps its WC seat as an offline member: it does not sign, and quorums are met by online members.
  - A seat is vacated (swap-with-last) only on a ban or after a continuous absence longer than the long-absence threshold L.
  - A returning ID whose seat was vacated is re-inserted by Fisher–Yates, with the beacon 6 blocks after its re-activation is confirmed.
  - This replaces v0.3's "deactivation vacates the seat".
  - Reason: under v0.3 every absence long enough to deactivate an ID changed about 9.3 KWC compositions (a removal and a re-insertion). Under v0.4 only absences longer than L do (M1.2 table B5).
- **§2 new row: long-absence threshold L** = 30 days [O] (86,400 PoW-ID blocks at 30 s), sweep 3, 7, 30, 90 and 365 days. The allocation beacon row now covers re-insertion. **§8 S9** adds L to the offline-handling sweep.
- **§4.2 placement takes effect without the committee [D]**, at the ID's beacon block (its confirmation plus the allocation beacon delay), with no further delay. **§4.3:** the Allocation Committee Block becomes a record and attestation only, so a stall cannot delay placement. Reason: every node computes placement from chain data, so a committee stall need not hold it up.
- **§4.3 departures [D].** A banned or deactivated member leaves the committee at once; an extra lottery draw fills its seat at the next 10th PoW-Tx block. The extra draw's position [P] is SHA256("GB/cac" ‖ hash of block B+k ‖ i), with i = 1, 2, … counting extra draws from the same beacon; the regular draw keeps its formula. Reason: a draw from the same beacon without a counter would land on the first draw's position.
- **§4.4 proposer rights [D].** The master proposer proposes MOBr, registered offline requests and bans. The subordinate-only proposer proposes MOBu and unregistered offline requests, and keeps one exception: bans of leader-chain members who refuse to witness newcomers. The miner makes its own self-service offline request.
- **§4.6 conflicting decisions [D].** If two conflicting KWC decisions are both approved, both are rejected, and a member who signed both has produced equivocation evidence and may be banned. Reason: a conflict then cancels a decision rather than enacting two (M1 section G).
- **§3.8.** The Variant B correction stays [P] with no preferred form; M3 proposes and tests one.
- **§10 H6.** The primary measure is now distinct episodes (entries into the state); raw compositions are an upper bound. Compositions follow the v0.4 seat rule. Reason: consecutive compositions share all but one seat, so counting each as an independent draw counts one episode several times.
- **§2 quorums unchanged** (27 of 40; 7 + 21; ⌈2n/3⌉ for the committee). **§12** records that the leader-WC requirement (7 of 10) is the main stall risk for registered KWCs, for M4 to compare with a single 27-of-40 rule together with the forced-signing and ban rules.
- **§12 new [P] proposals:**
  - separate PoWit validity from payment (any 27 of 40 makes a PoWit valid; an unsigned leader member's share is never generated), for M4; **§6** gains a pointer, and the payment rule is unchanged;
  - three ways to raise the attacker share that ID issuance can tolerate without relying on bans: the split rule, larger KWCs and threshold BLS entropy, for M2/M4;
  - witness peer topology option 2: no persistent witness-to-witness connections, on-demand connections through the global directory, online status from the network record.
- **§12 housekeeping.** Removed the answered questions (the re-insertion beacon, committee seats of deactivated or banned members, the Variant B form). Added the absence-duration question, the long-absence threshold, and the proposals above.
- **§7 (editorial, 2026-10-07).** Removed VRF from §7; no VRF in the design.

### v0.3 — 2026-10-06

Decisions by the protocol architect, recorded after reviewing the M1 results.

- **§2 Genesis IDs (G).**
  - Default changed from 2,100,000 to 2,900,000 [D]. Sweep changed to 1M, 2.1M, 2.9M, 5M, 10M.
  - Reason: M1 showed that with 2.1M genesis IDs the 2-year floor holds only while at least 96.2% of them stay active. With 2.9M it holds with up to 30.3% of genesis IDs inactive (footnote ¹).
- **§2 and §3.9 adaptive cap.**
  - The cap is now `registered_IDs / (k × T_min)`. The checkpoint interval defaults to `T_min` [D], with `k = 7/6` [D].
  - The guarantee sentence now reads: "With the safety factor k for the chosen checkpoint interval, a 100%-capture attacker cannot reach 51% in less than T_min, at any start phase."
  - The adaptive cap remains an optional variant; the fixed 30 s rate remains the default.
  - Reason: M1 showed that without `k` an attacker choosing its start phase reaches 51% in 0.857 `T_min` (checkpoints every `T_min`), and that `k = 7/6` restores the floor exactly.
- **§2 and §3.8 difficulty.**
  - Variant B (count-based, from the exact admitted online count) becomes the default [D]. Variant A (Bitcoin-style, K = 144, 4× clamp) is the comparison [D]. Variant B's small correction is [P], to be proposed and tested in M3.
  - Reason: M1 found that an attacker hopping in for one Variant A window earns (1 + m) times the fair rate per miner-second, while Variant B, which follows the exact admitted count rather than recent block times, gives no first-order gain.
- **§3.7.** The lower-hash tie-break stays [P] and the confirmation depth is unchanged. M3 measures the orphan rate. Reason: M1 found same-step ties end about 1.66% of rounds at 30 s.
- **§3.11 (new).** Registration index and canonical active list [D]. Reason: allocation and the committee lottery need an order every node can rebuild from chain data.
- **§4.2 allocation.**
  - The allocation algorithm proposed in `docs/ARCHITECTURE.md` §8 becomes the rule [D], numbered by the §3.11 index. It uses inside-out Fisher–Yates insertion, the Golomb ring {1, 4, 6} and swap-with-last removal.
  - The CAC publishes the placement; every node recomputes it and rejects a mismatch.
  - It replaces the per-ID formula `chain(ID) = f(SHA256(ID ‖ beacon))` and the older rule that a vacancy is filled by a newly registered ID.
  - New §2 rows: ring offsets [D]; allocation beacon delay of 6 blocks [O], sweep 3, 6, 12.
  - Deactivation vacates a seat by the removal step, and re-activation re-inserts by Fisher–Yates.
  - Reason: a per-ID hash cannot keep every WC at exactly 10 members. Append-only filling would make new WCs mirror the attacker's share of recent issuance rather than of the population.
- **§4.3 committee.**
  - The seat lottery over the canonical active list [D] replaces "the miner of every 10th PoW-Tx block joins" and the v0.2 seat-uniqueness rule [P]. The lottery's next-position rule keeps one seat per ID. Beacon offset k = 3 [O], sweep 1, 3, 6, 12.
  - The new member is drawn before the oldest leaves, from the list as of block B.
  - Added a note that 256-bit hashes make the modulo bias negligible, and a list of the leader block's duties.
  - Stalling is accepted for now because allocation is deterministic; M4 tests the impact.
  - Reason: the attacker's expected committee share now equals its share of active IDs, independent of mining power, and the committee can announce placements but not choose them.
- **§4.6–§4.7.** Offline beyond the allowance leads to deactivation, never a ban. Bans are only for proven misbehaviour, and the full 40-member KWC votes on bans, MOBu/MOBr and offline requests. Durations stay [O] and swept. Reason: honest household downtime must not cost an ID.
- **§2 capacity.** Footnote ² notes that 200 registered miners per leader WC cannot be reached network-wide. The split stays a sweep. Reason: each ID mines at most one channel, so a KWC averages at most 10 registered miners.
- **§8.**
  - S5 now defines the realistic restart cost C = grace epoch + convergence interval + post-admission wait (450 s). Reason: needed for the restart analysis.
  - S12 and S21 updated to the new wording and measures.
- **§10 H6.**
  - "Capable of early signing" is renamed "capable of signing without honest members", keeping the §3.7 explanation.
  - Results are reported for both quorums: unregistered for PoW-ID, registered for PoW-Tx.
  - The ban rate is 0 per year in the base case, with 1% and 5% as sensitivities.
  - Compositions are counted under the §4.2 allocation, including deactivation, with the v0.2 count as a comparison.
  - Reason: §3.7 already rejects early-signed blocks beyond δ, and the adopted allocation changes how compositions form.
- **§0 scope** updated for the committee lottery, and **§12** gained four open questions.

### v0.2 — 2026-10-05

Decisions by the protocol architect, recorded after the M0/M1 plan review.

- **§0 C2.** The time floor now reads "(T/(1−T)) × H_active / R" instead of "the existing active base divided by the issuance rate". Reason: for a 100%-capture attacker the exact bound is (T/(1−T)) × H_active / R. That is about 0.49 × H_active / R at 33%, so the old wording overstated the floor below 50%.
- **§2 Genesis IDs (G).**
  - Default changed from 1,000,000 [O] to 2,100,000 [D]. Sweep changed to 0.5M, 1M, 2M, 2.1M, 3M.
  - Reason, in footnote ¹: with a 30 s rate, about 2.02M active IDs are needed before a 100%-capture attacker takes 2 years to reach 51%. 2.1M adds a margin, and the floor depends on genesis IDs staying active.
- **§2 PoWit quorums.** Added the 30-node comparison values: registered ≥7 of 10 leader and ≥14 of 20 subordinates; unregistered any ≥20 of 30.
- **§2 new rows.**
  - Adaptive-cap checkpoint interval [O], sweep T_min/4, T_min/2, T_min.
  - CAC approval threshold ⌈2n/3⌉ [D].
  - CAC seat uniqueness [P].
- **§2 sweeps added.**
  - Offline deactivation threshold: 1 h, 6 h, 24 h, 3 d, 7 d.
  - Re-activation wait: 1 d, 7 d, 14 d, 30 d.
  - Capacity split per leader WC (registered/unregistered): 200/50, 100/150, 15/235.
- **§3.5.** Defined start rule (b): the chain starts when the miner's entropy arrives locally. It is the rejected alternative, kept only for comparison in S13.
- **§3.7.** Added the [P] same-height tie-break: keep the lower block hash. First-seen is the comparison in M3.
- **§3.9.**
  - The cap counts registered IDs (not active IDs) and is recalculated at fixed checkpoints. The interval is [O].
  - The cap may set the rate below the fixed 30 s target.
  - Added a note on the safety factor k needed so the floor holds at the worst start phase (k = 7/6 for checkpoint interval T_min).
- **§4.3.**
  - "66%" means at least two-thirds rounded up, ⌈2n/3⌉ (400 of 600).
  - Added the [P] rule that one ID cannot hold two CAC seats at once; a member's 10th-block win passes to the next 10th-block miner who is not a member.
- **§4.6.** "66% of the KWC" means at least two-thirds rounded up: 27 of 40.
- **§10 H6.**
  - Added the refresh model: every KWC composition ever formed counts as an independent draw. About R/10 new WCs form per year, plus replacements after bans.
  - Results are reported for both the registered and the unregistered quorum.
- **§11.** Added item 7: implementation language is Rust, for the simulation and the Stage 3 testnet, with protocol-core logic shared between them.

### v0.1

Initial compiled specification.
