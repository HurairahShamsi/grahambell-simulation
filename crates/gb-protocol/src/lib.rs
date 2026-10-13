//! GrahamBell protocol core (SPEC §3–§4, Appendix A).
//!
//! Pure, deterministic functions with no I/O, clock or network access. The M3/M4 simulators
//! call them directly, and so will the Stage 3 node, so a rule changes in one place. Test
//! vectors in `vectors/` pin the outputs (see [`vectors`]).
//!
//! | Module | SPEC |
//! |---|---|
//! | [`encoding`] — header bytes, digests, block hash | §3.3, Appendix A |
//! | [`entropy`] — BLS signatures, proofs of possession, entropy E, equivocation | §3.4 |
//! | [`chain`] — the hash chain, start rules, the winning step | §3.5 |
//! | [`validate`] — the signing condition and global validation | §3.6–§3.7 |
//! | [`issuance`] — difficulty and issuance rate | §3.8–§3.9 |
//! | [`allocation`] — registration index, seats, the KWC ring, the seat rule | §3.11, §4.2, §4.7 |
//! | [`committee`] — the committee lottery | §4.3 |
//! | [`offline`] — One Chance outcomes and penalties | §4.5–§4.8 |
//!
//! Cryptography comes only from `blst` (BLS12-381) and `sha2` (SHA-256); this crate contains
//! no hand-written cryptographic arithmetic.

pub mod allocation;
pub mod chain;
pub mod committee;
pub mod encoding;
pub mod entropy;
pub mod error;
pub mod hash;
pub mod issuance;
pub mod offline;
pub mod validate;
pub mod vectors;

pub use error::ProtocolError;

/// Protocol version: the first header field (Appendix A). Simulator outputs record it.
pub const PROTOCOL_VERSION: u32 = 1;
