# How GrahamBell works

A one-page overview of the protocol that Stage 1 simulates. [SPEC.md](SPEC.md) (v0.7) is the authoritative description and [ASSUMPTIONS.md](ASSUMPTIONS.md) records every modelling choice; each rule below links to its SPEC section and carries its status there: **[D]** decided by the protocol architect, **[P]** proposed and still being compared, **[O]** open and swept in the simulations.

## What GrahamBell is

GrahamBell is a blockchain design whose security rests on time rather than on computing power or money. Identities (IDs) are issued one at a time at a fixed pace, about one every 30 seconds for the whole network, through a capped proof-of-work race in which every participant hashes at one hash per second, whatever its hardware. Influence therefore grows with the share of the network a participant keeps online over time, not with the machines or money it can bring at once, and a mature network cannot be overtaken quickly. The design calls this Proof of Infrastructure Endurance (PoIE); Stage 1 tests whether it holds up ([§0](SPEC.md#0-what-stage-1-must-establish)).

## One round, step by step

A round runs from one identity-issuance (PoW-ID) block to the next, 30 seconds on average.

1. **Header.** A miner (an admitted IPv6 /64) builds its header for the next height and signs it; it signs one header per height. [D] ([§3.3](SPEC.md#33-pow-id-header-d), [§3.4](SPEC.md#34-entropy-lock-d))
2. **Witness entropy.** Its witness group, a King Witness Chain (KWC) of 40 registered IDs, signs the header with a threshold key: any 27 members give the same group signature, which becomes an unpredictable entropy, so no work can be done in advance and no member can steer it by withholding. [P] ([§3.4](SPEC.md#34-entropy-lock-d)); the comparison is every online member signing.
3. **Sequential chain.** The miner hashes one step per second from the moment its KWC issued the entropy (start rule (r)), each step depending on the last. [D] ([§3.5](SPEC.md#35-hash-chain-d))
4. **PoWit.** When a step's hash is below the target, the members recompute the chain and sign a Proof of Witness (PoWit) when their own clocks reach that step. [D] ([§3.6](SPEC.md#36-witnessing-and-signing-d))
5. **Block.** Validators recompute the chain and check signatures, timestamp and timing, each on its own clock; a block received before its timestamp is declined until the node's clock reaches it, then reconsidered [D]. There is no PoWit arrival deadline by default [D] (a 10 s deadline is [P], for M3b's adversarial withholding). The ID becomes active once 6 blocks are built on it; IDs in replaced blocks are void, not penalised. [D] ([§3.7](SPEC.md#37-global-validation-d))

## Issuance, fork choice and difficulty

- **Pacing:** one hash per second per admitted /64 (per ID for registered miners), one session per /64, one PoW-ID block per 30 s (about 1.05M IDs a year). [D] ([§2](SPEC.md#2-parameters), [§3.1](SPEC.md#31-admission-lifecycle-d), [§3.9](SPEC.md#39-issuance-rate-d-default-p-variant))
- **Fork choice:** the longest valid chain; at the same height, the block a node saw first, then the lower hash. [D] ([§3.7](SPEC.md#37-global-validation-d))
- **Difficulty:** each hash wins with probability factor ÷ (N × 30 s), where N counts the miners whose KWC issued entropy for the round and the factor (P2, bounded ×0.9–×1.1) corrects from the last 144 block times. [D] ([§3.8](SPEC.md#38-difficulty-d))

## Witness chains and penalties

- **Hardware:** witness nodes are meant to run at home on an always-on device; mining runs on anything. Measured hardware and bandwidth requirements will be published after the M5 household benchmarks.
- **Seats:** every registered ID is a witness in one Witness Chain (WC) of 10; a KWC is a leading WC and three subordinate WCs, with no two WCs watching each other. Seats come from a deterministic shuffle seeded by a later block, so nobody chooses where an ID lands. [D] ([§4.1](SPEC.md#41-duty-d), [§4.2](SPEC.md#42-allocation-d))
- **One Chance:** a member that does not sign is asked again; one that cannot be reached is recorded offline, never banned ("unreachable is not refusal"). Bans need verifiable evidence, such as two conflicting signatures. [D] ([§4.5](SPEC.md#45-one-chance-d), [§4.6](SPEC.md#46-ban-principle-d))
- **Offline:** a long absence leads to deactivation, never a ban. Every offline period of 24 hours or more earns points, and at 20 points a member loses its seat for 30 days [D]. ([§4.7](SPEC.md#47-offline-handling-d-rules-o-values), [§4.8](SPEC.md#48-penalties-d))

## Network and threat model in the simulations

- **Network:** latency from RIPE Atlas measurements across 8 regions, packet loss, regional partitions, clock skew, and household power cuts, load-shedding and internet-provider outages ([ASSUMPTIONS](ASSUMPTIONS.md) SIM-4 to SIM-15).
- **Attacker:** no stake is involved. The attacker shares tested: 5–100 % of admitted /64s in M1's time-to-threshold grid, 5–60 % in scenario S2, up to 25 % for witness capture (H6), 10, 25 and 33 % for the seat tails (E3). The worst case wins every new ID and stays online all the time. Witness collusion is limited by quorums (27 of 40) and by every block being recomputed by validators ([§7](SPEC.md#7-threat-model), [§10](SPEC.md#10-pre-registered-hypotheses)).

## What this is not

PoIE does not prove that one person holds one ID; it bounds how fast IDs can accumulate. This repository does not cover throughput (TPS), the Single Chain Architecture (SCA), the Shamsi Operating System (SOS) VM and smart contracts, the software Block Processing Unit and Block Validation Unit, rewards and token economics, or storage. Those are separate designs, not tested in Stage 1 ([§0, in scope and out of scope](SPEC.md#in-scope)).

## Known open risks and where they are tested

- **Subnet and provider concentration:** a /48 holds 65,536 /64s, and a few providers serve many households. Admission caps per /48 and per provider are planned (M3b), with their cost in M5.
- **Sleeper accumulation:** an attacker that gathers IDs quietly for years, then switches behaviour at once (M3b).
- **Regional blackouts beyond a third of a KWC:** they stall issuance, and M3a.1's E3 measures them. In a deliberate stress test, an entire region holding about 38% of witnesses loses power for about 11 hours. Issuance pauses at the blackout's start while difficulty catches up with the miners who remain: across 100 simulated blackouts, the median pause was about 7.6 minutes, over 10 minutes in 41%, over 30 minutes in 6%, and the longest 49 minutes. Block production then resumes automatically at ~30 s. This is protocol behaviour, not a simulator artefact. (The figures are the supplementary section W3, reproducible with `gb sim --only W3`: [results](../results/sim/supplement-w3/).) A threshold of 21 of 40 kept every gap under 9.4 minutes, at a security cost ([§3.4](SPEC.md#34-entropy-lock-d), [§12](SPEC.md#12-open-questions)). M3b tests within-round difficulty relaxation, which would cap such pauses; M4 continues with the witness layer.
- **Threshold entropy, both sides:** 14 members refusing or unreachable stop a KWC's entropy at the default threshold of 27, and 27 colluding members produce it alone; M4 measures both (scenario S31), and backdated starts at both thresholds.
- **Rotation of IDs:** an attacker that deactivates and re-activates a pool of IDs around committee lotteries and seat beacons, to inflate its share at the moments that count (M3b).
- **Threshold-BLS share theft over time:** a 30-day share refresh is proposed [P] and costed in M4 ([§12](SPEC.md#12-open-questions)).
- **Regional win share:** under the defaults, honest miners in some regions win slightly less than their share of mining time and others slightly more (at 100,000 miners as_south about 2 % less and na_east about 3 % more, C1); accepted for now as a known caveat, explained and mitigated in M3b ([§3.7](SPEC.md#37-global-validation-d)).
- **Backdated starts under rule (r):** t colluding members could attest an earlier start; M4 scenario S30 ([§3.5](SPEC.md#35-hash-chain-d)).
