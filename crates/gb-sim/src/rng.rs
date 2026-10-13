//! Random numbers by key (ARCHITECTURE §6).
//!
//! Every draw is addressed by `(purpose, entity, index)`: the ChaCha20 stream seeded by
//! [`gb_runlog::rng_stream`] is read at stream `purpose ⊕ entity` and block `index`, and one
//! 64-byte block gives eight 64-bit words. The value of a draw therefore depends only on the
//! seed and its key, never on the order in which the simulation asks for draws. That makes
//! lazy evaluation possible (a node's arrival time for a block is computed only when needed)
//! and gives common random numbers across the jobs of a sweep.
//!
//! The block is computed directly, one at a time, with exactly the words `rand_chacha`'s
//! `ChaCha20Rng` gives at that stream and position (a test compares them); the RNG itself would
//! compute four blocks and re-key for every draw.

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
    key: [u32; 8],
}

impl Keyed {
    /// The family for `label` under the master `seed`.
    pub fn new(seed: u64, label: &str) -> Keyed {
        let seed = gb_runlog::rng_stream(seed, label).get_seed();
        let mut key = [0u32; 8];
        for (word, bytes) in key.iter_mut().zip(seed.chunks_exact(4)) {
            *word = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        Keyed { key }
    }

    /// Eight 64-bit words for the key `(purpose, entity, index)`.
    pub fn words(&mut self, purpose: Purpose, entity: u64, index: u64) -> [u64; 8] {
        let stream = (u64::from(purpose as u8) << 56) ^ (entity & 0x00ff_ffff_ffff_ffff);
        let block = chacha20_block(&self.key, index, stream);
        let mut out = [0u64; 8];
        for (i, word) in out.iter_mut().enumerate() {
            *word = u64::from(block[2 * i]) | (u64::from(block[2 * i + 1]) << 32);
        }
        out
    }

    /// Eight uniforms in `[0, 1)` for the key.
    pub fn uniforms(&mut self, purpose: Purpose, entity: u64, index: u64) -> [f64; 8] {
        self.words(purpose, entity, index).map(unit)
    }
}

/// One ChaCha20 block (20 rounds) in the layout `rand_chacha` uses: the constants, the key, a
/// 64-bit block counter and a 64-bit stream number, each little-endian.
fn chacha20_block(key: &[u32; 8], counter: u64, stream: u64) -> [u32; 16] {
    let input: [u32; 16] = [
        0x6170_7865,
        0x3320_646e,
        0x7962_2d32,
        0x6b20_6574,
        key[0],
        key[1],
        key[2],
        key[3],
        key[4],
        key[5],
        key[6],
        key[7],
        counter as u32,
        (counter >> 32) as u32,
        stream as u32,
        (stream >> 32) as u32,
    ];
    let mut s = input;
    let quarter = |s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize| {
        s[a] = s[a].wrapping_add(s[b]);
        s[d] = (s[d] ^ s[a]).rotate_left(16);
        s[c] = s[c].wrapping_add(s[d]);
        s[b] = (s[b] ^ s[c]).rotate_left(12);
        s[a] = s[a].wrapping_add(s[b]);
        s[d] = (s[d] ^ s[a]).rotate_left(8);
        s[c] = s[c].wrapping_add(s[d]);
        s[b] = (s[b] ^ s[c]).rotate_left(7);
    };
    for _ in 0..10 {
        quarter(&mut s, 0, 4, 8, 12);
        quarter(&mut s, 1, 5, 9, 13);
        quarter(&mut s, 2, 6, 10, 14);
        quarter(&mut s, 3, 7, 11, 15);
        quarter(&mut s, 0, 5, 10, 15);
        quarter(&mut s, 1, 6, 11, 12);
        quarter(&mut s, 2, 7, 8, 13);
        quarter(&mut s, 3, 4, 9, 14);
    }
    for (out, inp) in s.iter_mut().zip(input) {
        *out = out.wrapping_add(inp);
    }
    s
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
    fn blocks_equal_the_words_of_rand_chacha() {
        use rand_core::Rng;
        let positions = [
            0u64,
            1,
            2,
            3,
            4,
            5,
            255,
            1 << 20,
            (1 << 32) - 1,
            1 << 32,
            u64::MAX / 2,
        ];
        for seed in [0u64, 1, 20_261_005, u64::MAX] {
            for label in ["gb-sim", "pin", "x"] {
                let mut keyed = Keyed::new(seed, label);
                let mut rng = gb_runlog::rng_stream(seed, label);
                for purpose in [Purpose::NodeAttributes, Purpose::Gossip, Purpose::Growth] {
                    for entity in [0u64, 7, 1 << 24, (1 << 56) - 1, u64::MAX] {
                        for index in positions {
                            let stream =
                                (u64::from(purpose as u8) << 56) ^ (entity & 0x00ff_ffff_ffff_ffff);
                            rng.set_stream(stream);
                            rng.set_word_pos(u128::from(index) * 16);
                            let expected: Vec<u64> = (0..8).map(|_| rng.next_u64()).collect();
                            assert_eq!(keyed.words(purpose, entity, index).to_vec(), expected);
                        }
                    }
                }
            }
        }
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
