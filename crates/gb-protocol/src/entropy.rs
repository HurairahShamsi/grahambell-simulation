//! The entropy lock, BLS signatures and equivocation evidence (SPEC §3.4, Appendix A).
//!
//! BLS12-381 with minimal public-key size (public keys in G1, 48 bytes; signatures in G2,
//! 96 bytes), ciphersuite `BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_`, implemented by
//! `blst`.
//!
//! 1. The miner signs `"GB/PoW-ID/miner" ‖ H`.
//! 2. Every online member signs `"GB/entropy" ‖ H ‖ σ_m`.
//! 3. `E = SHA256("GB/entropy-out" ‖ AggregateBLS(σ_m, σ_i …) ‖ signer_bitfield)`.
//!
//! BLS signatures are unique (one valid signature per key and message), so nobody can grind E
//! by re-signing. Aggregating signatures of distinct keys is safe against rogue keys because
//! every key carries a proof of possession, checked when the key is registered.

use crate::encoding::{HEADER_LEN, Header};
use crate::error::ProtocolError;
use crate::hash::{Hash, tagged};
use blst::BLST_ERROR;
use blst::min_pk::{AggregatePublicKey, AggregateSignature, PublicKey, SecretKey, Signature};
use gb_config::protocol::EntropySigners;

/// Signature ciphersuite (domain separation tag).
pub const SIG_DST: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";
/// Proof-of-possession ciphersuite.
pub const POP_DST: &[u8] = b"BLS_POP_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";
/// Message tag of the miner's header signature.
pub const MINER_TAG: &[u8] = b"GB/PoW-ID/miner";
/// Message tag of member entropy signatures.
pub const ENTROPY_TAG: &[u8] = b"GB/entropy";
/// Domain tag of the entropy output E.
pub const ENTROPY_OUT_TAG: &[u8] = b"GB/entropy-out";
/// Message tag of PoWit signatures.
pub const POWIT_TAG: &[u8] = b"GB/powit";

/// Compressed public key (G1).
pub type PublicKeyBytes = [u8; 48];
/// Compressed signature (G2).
pub type SignatureBytes = [u8; 96];

fn bls(error: BLST_ERROR) -> ProtocolError {
    ProtocolError::Bls(format!("{error:?}"))
}

/// A BLS key pair.
#[derive(Debug, Clone)]
pub struct Keypair {
    secret: SecretKey,
    /// Compressed public key.
    pub public: PublicKeyBytes,
}

impl Keypair {
    /// Derives a key pair from 32 bytes of input keying material (IETF KeyGen).
    pub fn from_seed(ikm: &[u8; 32]) -> Result<Keypair, ProtocolError> {
        let secret = SecretKey::key_gen(ikm, &[]).map_err(bls)?;
        let public = secret.sk_to_pk().to_bytes();
        Ok(Keypair { secret, public })
    }

    /// Signs `message` with the protocol ciphersuite.
    pub fn sign(&self, message: &[u8]) -> SignatureBytes {
        self.secret.sign(message, SIG_DST, &[]).to_bytes()
    }

    /// Proof of possession: a signature on the public key under the PoP ciphersuite.
    pub fn proof_of_possession(&self) -> SignatureBytes {
        self.secret.sign(&self.public, POP_DST, &[]).to_bytes()
    }

    /// The secret key bytes (big-endian scalar); for test vectors only.
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.secret.to_bytes()
    }
}

fn public_key(bytes: &PublicKeyBytes) -> Result<PublicKey, ProtocolError> {
    let pk = PublicKey::from_bytes(bytes).map_err(bls)?;
    pk.validate().map_err(bls)?;
    Ok(pk)
}

fn signature(bytes: &SignatureBytes) -> Result<Signature, ProtocolError> {
    Signature::from_bytes(bytes).map_err(bls)
}

/// Verifies one signature.
pub fn verify(public: &PublicKeyBytes, message: &[u8], sig: &SignatureBytes) -> bool {
    match (public_key(public), signature(sig)) {
        (Ok(pk), Ok(s)) => {
            s.verify(true, message, SIG_DST, &[], &pk, false) == BLST_ERROR::BLST_SUCCESS
        }
        _ => false,
    }
}

/// Verifies a proof of possession.
pub fn verify_proof_of_possession(public: &PublicKeyBytes, proof: &SignatureBytes) -> bool {
    match (public_key(public), signature(proof)) {
        (Ok(pk), Ok(s)) => {
            s.verify(true, public, POP_DST, &[], &pk, false) == BLST_ERROR::BLST_SUCCESS
        }
        _ => false,
    }
}

/// `"GB/PoW-ID/miner" ‖ H`.
pub fn miner_message(header: &Header) -> Vec<u8> {
    [MINER_TAG, &header.encode()[..]].concat()
}

/// `"GB/entropy" ‖ H ‖ σ_m`.
pub fn member_message(header: &Header, miner_signature: &SignatureBytes) -> Vec<u8> {
    [ENTROPY_TAG, &header.encode()[..], &miner_signature[..]].concat()
}

/// `"GB/powit" ‖ block hash`.
pub fn powit_message(block_hash: &Hash) -> Vec<u8> {
    [POWIT_TAG, &block_hash[..]].concat()
}

/// Signer bitfield: one bit per KWC seat, in seat order; bit i is bit (i mod 8) of byte ⌊i/8⌋,
/// least significant first (Appendix A).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Bitfield {
    seats: usize,
    bytes: Vec<u8>,
}

impl Bitfield {
    /// An empty bitfield for `seats` seats.
    pub fn new(seats: usize) -> Bitfield {
        Bitfield {
            seats,
            bytes: vec![0; seats.div_ceil(8)],
        }
    }

    /// A bitfield with the listed seats set.
    pub fn from_seats(seats: usize, set: &[usize]) -> Bitfield {
        let mut b = Bitfield::new(seats);
        for s in set {
            b.set(*s);
        }
        b
    }

    /// Decodes `bytes` for `seats` seats; unused high bits must be zero.
    pub fn from_bytes(seats: usize, bytes: &[u8]) -> Result<Bitfield, ProtocolError> {
        let b = Bitfield {
            seats,
            bytes: bytes.to_vec(),
        };
        let clean = (seats..bytes.len() * 8).all(|i| !b.raw_get(i));
        if bytes.len() != seats.div_ceil(8) || !clean {
            return Err(ProtocolError::Bitfield {
                bits: bytes.len() * 8,
                seats,
            });
        }
        Ok(b)
    }

    fn raw_get(&self, i: usize) -> bool {
        self.bytes[i / 8] >> (i % 8) & 1 == 1
    }

    /// Marks seat `i` as signed (ignored if out of range).
    pub fn set(&mut self, i: usize) {
        if i < self.seats {
            self.bytes[i / 8] |= 1 << (i % 8);
        }
    }

    /// True when seat `i` signed.
    pub fn get(&self, i: usize) -> bool {
        i < self.seats && self.raw_get(i)
    }

    /// Seats covered.
    pub fn seats(&self) -> usize {
        self.seats
    }

    /// Signed seats, in order.
    pub fn signers(&self) -> Vec<usize> {
        (0..self.seats).filter(|i| self.raw_get(*i)).collect()
    }

    /// Number of signed seats.
    pub fn count(&self) -> usize {
        self.signers().len()
    }

    /// The encoded bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Aggregates signatures.
pub fn aggregate(signatures: &[SignatureBytes]) -> Result<SignatureBytes, ProtocolError> {
    if signatures.is_empty() {
        return Err(ProtocolError::Empty("no signatures to aggregate"));
    }
    let parsed: Vec<Signature> = signatures.iter().map(signature).collect::<Result<_, _>>()?;
    let refs: Vec<&Signature> = parsed.iter().collect();
    Ok(AggregateSignature::aggregate(&refs, true)
        .map_err(bls)?
        .to_signature()
        .to_bytes())
}

fn aggregate_keys(keys: &[&PublicKeyBytes]) -> Result<PublicKey, ProtocolError> {
    if keys.is_empty() {
        return Err(ProtocolError::Empty("no public keys to aggregate"));
    }
    let parsed: Vec<PublicKey> = keys
        .iter()
        .map(|k| public_key(k))
        .collect::<Result<_, _>>()?;
    let refs: Vec<&PublicKey> = parsed.iter().collect();
    Ok(AggregatePublicKey::aggregate(&refs, false)
        .map_err(bls)?
        .to_public_key())
}

/// `E = SHA256("GB/entropy-out" ‖ aggregate ‖ bitfield)`.
pub fn entropy_output(aggregate: &SignatureBytes, signers: &Bitfield) -> Hash {
    tagged(ENTROPY_OUT_TAG, &[aggregate, signers.as_bytes()])
}

/// The entropy aggregate a miner builds: σ_m plus the listed members' signatures, and E.
pub fn entropy_aggregate(
    miner_signature: &SignatureBytes,
    member_signatures: &[SignatureBytes],
    signers: &Bitfield,
) -> Result<(SignatureBytes, Hash), ProtocolError> {
    let mut all = vec![*miner_signature];
    all.extend_from_slice(member_signatures);
    let agg = aggregate(&all)?;
    Ok((agg, entropy_output(&agg, signers)))
}

/// Checks an entropy aggregate: it must combine the miner's signature on the header and the
/// signature of every member marked in `signers` (keys in seat order, proofs of possession
/// already checked) on `"GB/entropy" ‖ H ‖ σ_m`.
pub fn verify_entropy(
    header: &Header,
    miner_signature: &SignatureBytes,
    member_keys: &[PublicKeyBytes],
    signers: &Bitfield,
    aggregate_signature: &SignatureBytes,
) -> bool {
    if signers.seats() != member_keys.len() || signers.count() == 0 {
        return false;
    }
    let miner_pk = match public_key(&header.candidate_pk) {
        Ok(pk) => pk,
        Err(_) => return false,
    };
    let chosen: Vec<&PublicKeyBytes> = signers.signers().iter().map(|i| &member_keys[*i]).collect();
    let (members, agg) = match (aggregate_keys(&chosen), signature(aggregate_signature)) {
        (Ok(m), Ok(a)) => (m, a),
        _ => return false,
    };
    let miner_msg = miner_message(header);
    let member_msg = member_message(header, miner_signature);
    verify(&header.candidate_pk, &miner_msg, miner_signature)
        && agg.aggregate_verify(
            true,
            &[&miner_msg, &member_msg],
            SIG_DST,
            &[&miner_pk, &members],
            false,
        ) == BLST_ERROR::BLST_SUCCESS
}

/// Verifies an aggregate signature by the members marked in `signers` on one message (used for
/// the PoWit).
pub fn verify_same_message(
    member_keys: &[PublicKeyBytes],
    signers: &Bitfield,
    message: &[u8],
    aggregate_signature: &SignatureBytes,
) -> bool {
    if signers.seats() != member_keys.len() || signers.count() == 0 {
        return false;
    }
    let chosen: Vec<&PublicKeyBytes> = signers.signers().iter().map(|i| &member_keys[*i]).collect();
    let parsed: Result<Vec<PublicKey>, _> = chosen.iter().map(|k| public_key(k)).collect();
    match (parsed, signature(aggregate_signature)) {
        (Ok(keys), Ok(agg)) => {
            let refs: Vec<&PublicKey> = keys.iter().collect();
            agg.fast_aggregate_verify(true, message, SIG_DST, &refs) == BLST_ERROR::BLST_SUCCESS
        }
        _ => false,
    }
}

/// A header with the miner's signature on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedHeader {
    /// The encoded header.
    pub header: [u8; HEADER_LEN],
    /// The miner's signature on `"GB/PoW-ID/miner" ‖ header`.
    pub signature: SignatureBytes,
}

/// True when `a` and `b` prove equivocation: two different headers at the same height, both
/// validly signed by the same candidate key (SPEC §3.4, one header per miner per round).
pub fn is_equivocation(a: &SignedHeader, b: &SignedHeader) -> bool {
    let (Ok(ha), Ok(hb)) = (Header::decode(&a.header), Header::decode(&b.header)) else {
        return false;
    };
    ha != hb
        && ha.height == hb.height
        && ha.candidate_pk == hb.candidate_pk
        && verify(&ha.candidate_pk, &miner_message(&ha), &a.signature)
        && verify(&hb.candidate_pk, &miner_message(&hb), &b.signature)
}

/// Signatures a threshold entropy needs from the `seated` members of a KWC (the M3a.1
/// experiment E3; architect, 2026-10-14): ⌈2n/3⌉ under the two-thirds rule and ⌊n/2⌋ + 1
/// under the majority comparison. Any t members give the same group signature, so the
/// entropy does not depend on which ones sign. `None` under the all-online-members rule,
/// which needs every member recorded online.
pub fn entropy_threshold(seated: usize, signers: EntropySigners) -> Option<usize> {
    match signers {
        EntropySigners::AllOnlineMembers => None,
        EntropySigners::ThresholdTwoThirds => Some((2 * seated).div_ceil(3)),
        EntropySigners::ThresholdMajority => Some(seated / 2 + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_entropy_needs_two_thirds_or_just_over_half_of_the_seated_members() {
        use EntropySigners::{AllOnlineMembers, ThresholdMajority, ThresholdTwoThirds};
        assert_eq!(entropy_threshold(40, ThresholdTwoThirds), Some(27));
        assert_eq!(entropy_threshold(40, ThresholdMajority), Some(21));
        assert_eq!(entropy_threshold(30, ThresholdTwoThirds), Some(20));
        assert_eq!(entropy_threshold(31, ThresholdTwoThirds), Some(21));
        assert_eq!(entropy_threshold(31, ThresholdMajority), Some(16));
        assert_eq!(entropy_threshold(40, AllOnlineMembers), None);
    }
    use crate::encoding::tests::sample;

    fn keys(n: u8) -> Vec<Keypair> {
        (0..n)
            .map(|i| Keypair::from_seed(&[i + 1; 32]).unwrap())
            .collect()
    }

    fn header_for(miner: &Keypair) -> Header {
        let mut h = sample();
        h.candidate_pk = miner.public;
        h
    }

    #[test]
    fn signatures_are_unique_and_verify() {
        let k = &keys(1)[0];
        let a = k.sign(b"message");
        assert_eq!(a, k.sign(b"message"), "BLS signatures are deterministic");
        assert!(verify(&k.public, b"message", &a));
        assert!(!verify(&k.public, b"other", &a));
        assert!(verify_proof_of_possession(
            &k.public,
            &k.proof_of_possession()
        ));
        // A signature under the message ciphersuite is not a valid proof of possession.
        assert!(!verify_proof_of_possession(&k.public, &k.sign(&k.public)));
    }

    #[test]
    fn entropy_aggregate_verifies_and_rejects_tampering() {
        let miner = Keypair::from_seed(&[99; 32]).unwrap();
        let members = keys(5);
        let header = header_for(&miner);
        let sig_m = miner.sign(&miner_message(&header));
        let msg = member_message(&header, &sig_m);
        let sigs: Vec<SignatureBytes> = members.iter().map(|k| k.sign(&msg)).collect();
        let pks: Vec<PublicKeyBytes> = members.iter().map(|k| k.public).collect();
        let all = Bitfield::from_seats(5, &[0, 1, 2, 3, 4]);
        let (agg, e) = entropy_aggregate(&sig_m, &sigs, &all).unwrap();
        assert!(verify_entropy(&header, &sig_m, &pks, &all, &agg));
        assert_eq!(e, entropy_output(&agg, &all));
        // Claiming a member that did not sign fails.
        let (agg4, _) = entropy_aggregate(&sig_m, &sigs[..4], &all).unwrap();
        assert!(!verify_entropy(&header, &sig_m, &pks, &all, &agg4));
        // The honest four-member aggregate verifies with the matching bitfield.
        let four = Bitfield::from_seats(5, &[0, 1, 2, 3]);
        assert!(verify_entropy(&header, &sig_m, &pks, &four, &agg4));
        // A different header fails.
        let mut other = header;
        other.kwc_id += 1;
        assert!(!verify_entropy(&other, &sig_m, &pks, &all, &agg));
    }

    #[test]
    fn powit_aggregate_verifies_for_the_marked_signers_only() {
        let members = keys(4);
        let pks: Vec<PublicKeyBytes> = members.iter().map(|k| k.public).collect();
        let msg = powit_message(&[5; 32]);
        let sigs: Vec<SignatureBytes> = members[..3].iter().map(|k| k.sign(&msg)).collect();
        let agg = aggregate(&sigs).unwrap();
        assert!(verify_same_message(
            &pks,
            &Bitfield::from_seats(4, &[0, 1, 2]),
            &msg,
            &agg
        ));
        assert!(!verify_same_message(
            &pks,
            &Bitfield::from_seats(4, &[0, 1, 3]),
            &msg,
            &agg
        ));
    }

    #[test]
    fn two_headers_at_one_height_signed_by_one_key_are_equivocation() {
        let miner = Keypair::from_seed(&[7; 32]).unwrap();
        let a = header_for(&miner);
        let mut b = a;
        b.reward_wallet[0] ^= 1;
        let sign = |h: &Header| SignedHeader {
            header: h.encode(),
            signature: miner.sign(&miner_message(h)),
        };
        assert!(is_equivocation(&sign(&a), &sign(&b)));
        assert!(
            !is_equivocation(&sign(&a), &sign(&a)),
            "the same header twice"
        );
        let mut c = b;
        c.height += 1;
        assert!(!is_equivocation(&sign(&a), &sign(&c)), "different heights");
        let forged = SignedHeader {
            header: b.encode(),
            signature: miner.sign(b"something else"),
        };
        assert!(!is_equivocation(&sign(&a), &forged), "invalid signature");
    }

    #[test]
    fn bitfield_layout_is_least_significant_bit_first() {
        let b = Bitfield::from_seats(40, &[0, 9, 39]);
        assert_eq!(b.as_bytes(), &[0b0000_0001, 0b0000_0010, 0, 0, 0b1000_0000]);
        assert_eq!(b.signers(), vec![0, 9, 39]);
        assert_eq!(Bitfield::from_bytes(40, b.as_bytes()).unwrap(), b);
        assert!(
            Bitfield::from_bytes(10, &[0, 0b0000_0100]).is_err(),
            "bit 10 of 10 seats"
        );
    }
}
