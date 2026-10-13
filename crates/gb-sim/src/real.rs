//! Real cryptography for the simulator's real mode, and the M2 test-vector check (B4).
//!
//! Real mode runs the protocol path with real BLS12-381 signatures and real SHA-256: every
//! slot's entropy is the aggregate of its own signature and every recorded-online member's
//! (SPEC §3.4), PoWits are real aggregates, and each broadcast block is checked by
//! `gb_protocol::validate::validate` and by `validate_facts`, which must agree. The same
//! functions rebuild every entry of `protocol_v1.json` in [`vectors_check`].

use crate::sim::Sim;
use gb_protocol::allocation::insertion_position;
use gb_protocol::chain::{ChainInputs, StartRule, chain_start};
use gb_protocol::committee::lottery_position;
use gb_protocol::encoding::{Address, Header, block_hash};
use gb_protocol::entropy::{
    Bitfield, Keypair, SignatureBytes, aggregate, entropy_aggregate, member_message, miner_message,
    powit_message,
};
use gb_protocol::hash::{Hash, tagged, to_hex};
use gb_protocol::issuance::variant_b_target;
use gb_protocol::validate::{
    Block, BlockFacts, ChainCheck, ForkCandidate, KwcKeys, QuorumRule, Reception, Rejection, Rules,
    Tip, fork_choice, validate, validate_facts, witness_equivocators,
};
use serde_json::Value;

/// The committed M2 test vectors.
pub const VECTORS: &str = include_str!("../../gb-protocol/vectors/protocol_v1.json");

/// A slot's real entropy for one round.
#[derive(Debug, Clone)]
pub struct RealEntropy {
    /// E.
    pub entropy: Hash,
    /// The miner's signature on the header.
    pub miner_signature: SignatureBytes,
    /// The entropy aggregate.
    pub aggregate: SignatureBytes,
    /// Members whose signatures are in it (KWC seat order).
    pub signers: Bitfield,
}

/// A witness's key in real mode.
pub fn witness_key(seed: u64, w: u32) -> Keypair {
    let ikm = tagged(
        b"GB/sim-witness-key",
        &[&seed.to_be_bytes(), &w.to_be_bytes()],
    );
    Keypair::from_seed(&ikm).unwrap_or_else(|_| fallback_key())
}

/// A miner's candidate key for its `epoch`-th ID in real mode.
pub fn candidate_key(seed: u64, m: u32, epoch: u32) -> Keypair {
    let ikm = tagged(
        b"GB/sim-candidate-key",
        &[&seed.to_be_bytes(), &m.to_be_bytes(), &epoch.to_be_bytes()],
    );
    Keypair::from_seed(&ikm).unwrap_or_else(|_| fallback_key())
}

fn fallback_key() -> Keypair {
    // `from_seed` fails only for an input shorter than 32 bytes, which never happens here.
    match Keypair::from_seed(&[7; 32]) {
        Ok(k) => k,
        Err(_) => unreachable_key(),
    }
}

#[allow(clippy::panic)]
fn unreachable_key() -> Keypair {
    panic!("BLS key generation failed on a fixed 32-byte seed")
}

/// The entropy lock (SPEC §3.4): the miner signs the header, every member in `signers` signs
/// `header ‖ σ_m`, and E is derived from the aggregate.
pub fn entropy_lock(
    miner: &Keypair,
    header: &Header,
    members: &[&Keypair],
    signers: &Bitfield,
) -> Option<(SignatureBytes, Vec<SignatureBytes>, SignatureBytes, Hash)> {
    let sig_m = miner.sign(&miner_message(header));
    let msg = member_message(header, &sig_m);
    let sigs: Vec<SignatureBytes> = signers
        .signers()
        .into_iter()
        .map(|i| members[i].sign(&msg))
        .collect();
    let (agg, e) = entropy_aggregate(&sig_m, &sigs, signers).ok()?;
    Some((sig_m, sigs, agg, e))
}

/// A PoWit (SPEC §3.6): the given seats sign `"GB/powit" ‖ block hash`.
pub fn powit(
    members: &[&Keypair],
    seats: &[usize],
    hash: &Hash,
) -> Option<(Bitfield, SignatureBytes)> {
    let msg = powit_message(hash);
    let sigs: Vec<SignatureBytes> = seats.iter().map(|i| members[*i].sign(&msg)).collect();
    Some((
        Bitfield::from_seats(members.len(), seats),
        aggregate(&sigs).ok()?,
    ))
}

/// A verdict name: "valid" or the rejection.
pub fn verdict_name(r: Result<Hash, Rejection>) -> String {
    match r {
        Ok(_) => "valid".to_string(),
        Err(e) => format!("{e:?}"),
    }
}

/// Real entropy for slot `m` with the KWC `members` (simulator real mode).
pub(crate) fn entropy_for(
    sim: &mut Sim<'_>,
    m: u32,
    header: &Header,
    members: &[u32],
) -> RealEntropy {
    let seed = sim.spec.seed;
    let epoch = sim.miners[m as usize].key_epoch;
    let miner = candidate_key(seed, m, epoch);
    let keys: Vec<Keypair> = members.iter().map(|w| witness_key(seed, *w)).collect();
    let refs: Vec<&Keypair> = keys.iter().collect();
    let on: Vec<usize> = members
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            let wit = &sim.witnesses[**w as usize];
            wit.recorded && !wit.banned
        })
        .map(|(i, _)| i)
        .collect();
    let signers = Bitfield::from_seats(members.len(), &on);
    match entropy_lock(&miner, header, &refs, &signers) {
        Some((sig_m, _, agg, e)) => RealEntropy {
            entropy: e,
            miner_signature: sig_m,
            aggregate: agg,
            signers,
        },
        None => RealEntropy {
            entropy: [0; 32],
            miner_signature: [0; 96],
            aggregate: [0; 96],
            signers,
        },
    }
}

/// Real mode: builds the real block of candidate `c` and checks that `validate()` and
/// `validate_facts` agree on it at two receptions (on time, and after a 10 s deadline).
pub(crate) fn validate_real(sim: &mut Sim<'_>, c: u32, tip: &Tip) {
    let cand = sim.candidates[c as usize].clone();
    let g = cand.group as usize;
    let Some(Some(rec)) = sim.groups[g].real.get(cand.miner as usize).cloned() else {
        return;
    };
    let seed = sim.spec.seed;
    let members = sim.kwc_members(cand.kwc);
    let keys: Vec<Keypair> = members.iter().map(|w| witness_key(seed, *w)).collect();
    let refs: Vec<&Keypair> = keys.iter().collect();
    let seats: Vec<usize> = members
        .iter()
        .enumerate()
        .filter(|(_, w)| cand.signers.contains(w))
        .map(|(i, _)| i)
        .collect();
    let hash = block_hash(&cand.header, &rec.entropy, cand.step);
    let Some((bits, agg)) = powit(&refs, &seats, &hash) else {
        return;
    };
    let block = Block {
        header: cand.header,
        miner_signature: rec.miner_signature,
        entropy_aggregate: rec.aggregate,
        entropy_signers: rec.signers.clone(),
        winning_step: cand.step,
        timestamp: cand.timestamp,
        powit_aggregate: agg,
        powit_signers: bits.clone(),
    };
    let Ok(kwc) = KwcKeys::new(
        &keys
            .iter()
            .map(|k| (k.public, k.proof_of_possession()))
            .collect::<Vec<_>>(),
    ) else {
        return;
    };
    let quorum = sim.p.unregistered;
    let rules = Rules {
        clock_tolerance_s: sim.spec.delta,
        powit_deadline_s: Some(10.0),
        start_rule: StartRule::A,
        max_steps: 1_000_000,
    };
    let ts = cand.timestamp as f64;
    for reception in [
        Reception {
            block_at: ts,
            powit_at: ts + 1.0,
        },
        Reception {
            block_at: ts,
            powit_at: ts + 10.5,
        },
    ] {
        let full = validate(&block, tip, &kwc, &quorum, &reception, &rules, &|_| true);
        let facts = BlockFacts {
            header: &block.header,
            entropy: block.entropy(),
            entropy_signers: &block.entropy_signers,
            winning_step: block.winning_step,
            timestamp: block.timestamp,
            powit_signers: &block.powit_signers,
        };
        let light = validate_facts(
            &facts,
            tip,
            &quorum,
            &reception,
            &rules,
            &|_| true,
            ChainCheck::Recompute,
        );
        sim.res.validated += 1;
        if full != light {
            sim.res.validate_mismatches += 1;
        }
    }
}

/// One compared field of the test vectors.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct VectorField {
    /// JSON path of the field.
    pub field: String,
    /// Value in `protocol_v1.json`.
    pub expected: String,
    /// Value rebuilt through the simulator's path.
    pub rebuilt: String,
    /// Equal.
    pub matches: bool,
}

fn vector_seed(label: &str, i: u64) -> Hash {
    tagged(b"GB/vectors", &[label.as_bytes(), &i.to_be_bytes()])
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Rebuilds every derived field of `protocol_v1.json` (keys, encodings, digests, signatures,
/// chain steps, verdicts, positions, targets) through the simulator's real-mode functions and
/// compares them one by one (M3 B4). Inputs such as seeds and header fields enter through the
/// rebuilt values.
pub fn vectors_check() -> Result<Vec<VectorField>, String> {
    let file: Value = serde_json::from_str(VECTORS).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut cmp = |field: &str, expected: Option<&Value>, rebuilt: String| {
        let expected = expected.map(text).unwrap_or_default();
        out.push(VectorField {
            field: field.to_string(),
            matches: expected == rebuilt,
            expected,
            rebuilt,
        });
    };
    let members: Vec<Keypair> = (0..4)
        .map(|i| Keypair::from_seed(&vector_seed("member", i)))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let miner = Keypair::from_seed(&vector_seed("miner", 0)).map_err(|e| e.to_string())?;
    for (i, k) in std::iter::once(&miner).chain(members.iter()).enumerate() {
        let key = &file["keys"][i];
        cmp(
            &format!("keys[{i}].secret"),
            Some(&key["secret"]),
            to_hex(&k.secret_bytes()),
        );
        cmp(
            &format!("keys[{i}].public"),
            Some(&key["public"]),
            to_hex(&k.public),
        );
        cmp(
            &format!("keys[{i}].proof_of_possession"),
            Some(&key["proof_of_possession"]),
            to_hex(&k.proof_of_possession()),
        );
    }
    let mut target = [0u8; 32];
    target[0] = 0x20;
    let header = |address: Address| Header {
        version: gb_protocol::PROTOCOL_VERSION,
        height: 10,
        prev_hash: vector_seed("prev", 0),
        candidate_pk: miner.public,
        reward_wallet: vector_seed("wallet", 0),
        address,
        kwc_id: 7,
        target,
    };
    let h6 = header(Address::Ipv6Prefix64([0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 1]));
    let h4 = header(Address::Ipv4([192, 0, 2, 7]));
    for (i, h) in [h6, h4].iter().enumerate() {
        let j = &file["headers"][i];
        cmp(
            &format!("headers[{i}].encoding"),
            Some(&j["encoding"]),
            to_hex(&h.encode()),
        );
        cmp(
            &format!("headers[{i}].header_digest"),
            Some(&j["header_digest"]),
            to_hex(&h.digest()),
        );
    }
    let refs: Vec<&Keypair> = members.iter().collect();
    let all = Bitfield::from_seats(4, &[0, 1, 2, 3]);
    let (sig_m, sigs, agg, e) =
        entropy_lock(&miner, &h6, &refs, &all).ok_or("entropy lock failed")?;
    let ent = &file["entropy"];
    cmp(
        "entropy.header_digest",
        Some(&ent["header_digest"]),
        to_hex(&h6.digest()),
    );
    cmp(
        "entropy.miner_signature",
        Some(&ent["miner_signature"]),
        to_hex(&sig_m),
    );
    for (i, s) in sigs.iter().enumerate() {
        cmp(
            &format!("entropy.member_signatures[{i}]"),
            Some(&ent["member_signatures"][i]),
            to_hex(s),
        );
    }
    cmp(
        "entropy.signer_bitfield",
        Some(&ent["signer_bitfield"]),
        to_hex(all.as_bytes()),
    );
    cmp("entropy.aggregate", Some(&ent["aggregate"]), to_hex(&agg));
    cmp("entropy.entropy", Some(&ent["entropy"]), to_hex(&e));
    let tip = Tip {
        hash: h6.prev_hash,
        height: 9,
        timestamp: 1_000,
    };
    let t0 = chain_start(tip.timestamp, StartRule::A);
    let inputs = ChainInputs::new(&h6, e, t0);
    let ch = &file["chain"];
    cmp("chain.t0", Some(&ch["t0"]), t0.to_string());
    for (i, (_, h)) in inputs.steps().take(5).enumerate() {
        cmp(
            &format!("chain.first_steps[{i}]"),
            Some(&ch["first_steps"][i]),
            to_hex(&h),
        );
    }
    let (n, h_n) = inputs
        .winning_step(&target, 100_000)
        .ok_or("no winning step")?;
    cmp(
        "chain.winning_step",
        Some(&ch["winning_step"]),
        n.to_string(),
    );
    cmp(
        "chain.winning_hash",
        Some(&ch["winning_hash"]),
        to_hex(&h_n),
    );
    cmp(
        "chain.timestamp",
        Some(&ch["timestamp"]),
        (t0 + n).to_string(),
    );
    let bhash = block_hash(&h6, &e, n);
    cmp("chain.block_hash", Some(&ch["block_hash"]), to_hex(&bhash));
    let (pbits, pagg) = powit(&refs, &[0, 1, 2], &bhash).ok_or("PoWit failed")?;
    cmp(
        "powit.signer_bitfield",
        Some(&file["powit"]["signer_bitfield"]),
        to_hex(pbits.as_bytes()),
    );
    cmp(
        "powit.aggregate",
        Some(&file["powit"]["aggregate"]),
        to_hex(&pagg),
    );
    let block = Block {
        header: h6,
        miner_signature: sig_m,
        entropy_aggregate: agg,
        entropy_signers: all.clone(),
        winning_step: n,
        timestamp: t0 + n,
        powit_aggregate: pagg,
        powit_signers: pbits,
    };
    let kwc = KwcKeys::new(
        &members
            .iter()
            .map(|k| (k.public, k.proof_of_possession()))
            .collect::<Vec<_>>(),
    )
    .map_err(|e| e.to_string())?;
    let quorum = QuorumRule::Pool {
        seats: 4,
        quorum: 3,
    };
    let rules = Rules {
        clock_tolerance_s: 2.0,
        powit_deadline_s: Some(10.0),
        start_rule: StartRule::A,
        max_steps: 1_000_000,
    };
    let ts = block.timestamp as f64;
    let mut wrong_time = block.clone();
    wrong_time.timestamp += 1;
    let cases = [
        (&block, ts, ts + 1.0),
        (&block, ts - 2.5, ts),
        (&block, ts, ts + 10.5),
        (&wrong_time, ts + 1.0, ts + 1.0),
    ];
    for (i, (b, block_at, powit_at)) in cases.iter().enumerate() {
        let verdict = verdict_name(validate(
            b,
            &tip,
            &kwc,
            &quorum,
            &Reception {
                block_at: *block_at,
                powit_at: *powit_at,
            },
            &rules,
            &|_| true,
        ));
        cmp(
            &format!("validation[{i}].verdict"),
            Some(&file["validation"][i]["verdict"]),
            verdict,
        );
    }
    let mut second = block.clone();
    second.header.reward_wallet = vector_seed("wallet", 1);
    let second_hash = second.hash();
    let (sbits, sagg) = powit(&refs, &[1, 2, 3], &second_hash).ok_or("PoWit failed")?;
    second.powit_signers = sbits;
    second.powit_aggregate = sagg;
    let we = &file["witness_equivocation"];
    cmp(
        "witness_equivocation.second_block_hash",
        Some(&we["second_block_hash"]),
        to_hex(&second_hash),
    );
    cmp(
        "witness_equivocation.second_powit_aggregate",
        Some(&we["second_powit_aggregate"]),
        to_hex(&second.powit_aggregate),
    );
    cmp(
        "witness_equivocation.equivocating_seats",
        Some(&we["equivocating_seats"]),
        serde_json::to_string(&witness_equivocators(&block, &second, &kwc)).unwrap_or_default(),
    );
    let fork = |height: u64, timestamp: u64, label: &str| ForkCandidate {
        height,
        timestamp,
        hash: vector_seed(label, 0),
    };
    let forks = [
        (fork(11, 2_000, "fork-a"), fork(10, 1_000, "fork-b")),
        (fork(10, 1_005, "fork-c"), fork(10, 1_006, "fork-d")),
        (fork(10, 1_005, "fork-e"), fork(10, 1_005, "fork-f")),
    ];
    for (i, (a, b)) in forks.iter().enumerate() {
        let preferred = if fork_choice(a, b) == std::cmp::Ordering::Greater {
            "b"
        } else {
            "a"
        };
        cmp(
            &format!("fork_choice[{i}].preferred"),
            Some(&file["fork_choice"][i]["preferred"]),
            preferred.to_string(),
        );
    }
    for (i, (index, filled)) in [(0u64, 0u64), (1, 9), (2, 999), (3, 2_899_999)]
        .iter()
        .enumerate()
    {
        let position = insertion_position(
            &vector_seed("id", *index),
            &vector_seed("beacon", *index),
            *filled,
        );
        cmp(
            &format!("allocation[{i}].position"),
            Some(&file["allocation"][i]["position"]),
            position.to_string(),
        );
    }
    for draw in 0..4u64 {
        let position = lottery_position(&vector_seed("cac-beacon", 0), draw, 2_900_000);
        cmp(
            &format!("committee[{draw}].position"),
            Some(&file["committee"][draw as usize]["position"]),
            position.to_string(),
        );
    }
    for (i, (m, interval)) in [(1_000u64, 30u64), (2_900_000, 30)].iter().enumerate() {
        cmp(
            &format!("issuance[{i}].target"),
            Some(&file["issuance"][i]["target"]),
            to_hex(&variant_b_target(*m, *interval)),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_simulator_path_reproduces_every_test_vector() {
        let fields = vectors_check().unwrap();
        // Every derived field of the file; the rest are inputs, descriptions or signer choices.
        assert_eq!(fields.len(), 60);
        let failing: Vec<&VectorField> = fields.iter().filter(|f| !f.matches).collect();
        assert!(failing.is_empty(), "{failing:?}");
    }
}
