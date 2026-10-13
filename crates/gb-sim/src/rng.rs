//! Random numbers by key (ARCHITECTURE §6).
//!
//! Every draw is addressed by `(purpose, entity, index)`: a ChaCha20 stream seeded from
//! [`gb_runlog::rng_stream`] is positioned at stream `purpose ⊕ entity` and block `index`, and
//! one 64-byte block gives eight 64-bit words. The value of a draw therefore depends only on
//! the seed and its key, never on the order in which the simulation asks for draws. That makes
//! lazy evaluation possible (a node's arrival time for a block is computed only when needed)
//! and gives common random numbers across the jobs of a sweep.

use rand_chacha::ChaCha20Rng;
use rand_core::Rng;

/// What a draw is for; part of the stream number, so purposes never share streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Purpose {
    /// Static node attributes (region, device, skew, access delay).
    NodeAttributes = 1,
    /// Household outage processes.
    Household = 2,
    /// Shared outage processes (feeder groups, providers, regions).
    SharedOutage = 3,
    /// Gossip arrival of a block at a node.
    Gossip = 4,
    /// Direct messages between two nodes.
    Direct = 5,
    /// Candidate generation of a round.
    Round = 6,
    /// Stand-in entropy, keys and block contents.
    Content = 7,
    /// Overlay calibration.
    Overlay = 8,
    /// Admission choices and timings.
    Admission = 9,
    /// Strategy decisions.
    Strategy = 10,
    /// Allocation of genesis IDs and other world set-up.
    Setup = 11,
    /// Sampling of KWCs and other measurements.
    Sampling = 12,
    /// Growth: arrivals and patience.
    Growth = 13,
}

/// Keyed random access to one ChaCha20 stream family.
#[derive(Debug, Clone)]
pub struct Keyed {
    rng: ChaCha20Rng,
}

impl Keyed {
    /// The family for `label` under the master `seed`.
    pub fn new(seed: u64, label: &str) -> Keyed {
        Keyed {
            rng: gb_runlog::rng_stream(seed, label),
        }
    }

    /// Eight 64-bit words for the key `(purpose, entity, index)`.
    pub fn words(&mut self, purpose: Purpose, entity: u64, index: u64) -> [u64; 8] {
        let stream = (u64::from(purpose as u8) << 56) ^ (entity & 0x00ff_ffff_ffff_ffff);
        self.rng.set_stream(stream);
        self.rng.set_word_pos(u128::from(index) * 16);
        let mut out = [0u64; 8];
        for word in &mut out {
            *word = self.rng.next_u64();
        }
        out
    }

    /// Eight uniforms in `[0, 1)` for the key.
    pub fn uniforms(&mut self, purpose: Purpose, entity: u64, index: u64) -> [f64; 8] {
        self.words(purpose, entity, index).map(unit)
    }
}

/// A uniform in `[0, 1)` from the top 53 bits of a word (as `gb_analytic::mc::uniform`).
pub fn unit(word: u64) -> f64 {
    (word >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// A uniform in `(0, 1]` from a word, safe to pass to a logarithm.
pub fn unit_positive(word: u64) -> f64 {
    1.0 - unit(word)
}

/// A sequential source over consecutive keyed blocks, for code that needs an unknown number of
/// draws for one key (it implements [`rand_core::TryRng`] so the `gb_analytic::mc` samplers
/// accept it).
#[derive(Debug)]
pub struct Cursor<'a> {
    keyed: &'a mut Keyed,
    purpose: Purpose,
    entity: u64,
    block: u64,
    buffer: [u64; 8],
    used: usize,
}

impl<'a> Cursor<'a> {
    /// A cursor starting at block `first_block` of `(purpose, entity)`.
    pub fn new(keyed: &'a mut Keyed, purpose: Purpose, entity: u64, first_block: u64) -> Self {
        Cursor {
            keyed,
            purpose,
            entity,
            block: first_block,
            buffer: [0; 8],
            used: 8,
        }
    }

    /// The next word.
    pub fn word(&mut self) -> u64 {
        if self.used == 8 {
            self.buffer = self.keyed.words(self.purpose, self.entity, self.block);
            self.block += 1;
            self.used = 0;
        }
        let w = self.buffer[self.used];
        self.used += 1;
        w
    }

    /// The next uniform in `[0, 1)`.
    pub fn uniform(&mut self) -> f64 {
        unit(self.word())
    }

    /// The next uniform in `(0, 1]`.
    pub fn uniform_positive(&mut self) -> f64 {
        unit_positive(self.word())
    }

    /// The block the cursor will read next, so a caller can resume a sequence later.
    pub fn next_block(&self) -> u64 {
        if self.used == 8 {
            self.block
        } else {
            self.block + 1
        }
    }
}

impl rand_core::TryRng for Cursor<'_> {
    type Error = core::convert::Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok((self.word() >> 32) as u32)
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        Ok(self.word())
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        for chunk in dst.chunks_mut(8) {
            let bytes = self.word().to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_depend_only_on_their_key() {
        let mut a = Keyed::new(7, "test");
        let mut b = Keyed::new(7, "test");
        let x = a.words(Purpose::Gossip, 3, 9);
        let _ = b.words(Purpose::Gossip, 1, 1);
        let _ = b.words(Purpose::Direct, 3, 9);
        assert_eq!(b.words(Purpose::Gossip, 3, 9), x);
        assert_ne!(a.words(Purpose::Gossip, 3, 10), x);
        assert_ne!(a.words(Purpose::Direct, 3, 9), x);
        assert_ne!(Keyed::new(8, "test").words(Purpose::Gossip, 3, 9), x);
    }

    #[test]
    fn draws_are_pinned() {
        // Pins the keyed derivation so a dependency change to ChaCha20 random access would be
        // caught; value recorded from this implementation.
        let mut k = Keyed::new(20_261_005, "pin");
        assert_eq!(k.words(Purpose::Round, 42, 7)[0], PINNED);
    }

    const PINNED: u64 = 3_107_084_630_820_225_675;

    #[test]
    fn cursor_walks_blocks_in_order() {
        let mut k = Keyed::new(1, "c");
        let expected: Vec<u64> = k
            .words(Purpose::Round, 5, 0)
            .into_iter()
            .chain(k.words(Purpose::Round, 5, 1))
            .collect();
        let mut k2 = Keyed::new(1, "c");
        let mut c = Cursor::new(&mut k2, Purpose::Round, 5, 0);
        let got: Vec<u64> = (0..16).map(|_| c.word()).collect();
        assert_eq!(got, expected);
        assert_eq!(c.next_block(), 2);
    }

    #[test]
    fn units_are_in_range() {
        assert_eq!(unit(0), 0.0);
        assert!(unit(u64::MAX) < 1.0);
        assert_eq!(unit_positive(0), 1.0);
        assert!(unit_positive(u64::MAX) > 0.0);
    }
}
