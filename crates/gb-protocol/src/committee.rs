//! The Chain Allocation Committee seat lottery (SPEC §4.3).
//!
//! For every 10th PoW-Tx block B, the new member is the ID at position
//! `SHA256("GB/cac" ‖ hash of block B+k) mod N_active` of the canonical active list; a draw
//! that lands on a member moves to the next position, wrapping around. The new member is
//! drawn before the oldest leaves. A banned or deactivated member leaves at once, and an extra
//! draw fills its seat at the next 10th block, at `SHA256("GB/cac" ‖ hash ‖ i)` with
//! `i = 1, 2, …` \[P\].

use crate::hash::{Hash, mod_u256, tagged};
use std::collections::VecDeque;

/// Domain tag of lottery positions.
pub const CAC_TAG: &[u8] = b"GB/cac";

/// Lottery position for draw `draw` (0 = the regular draw, i ≥ 1 = extra draws) from beacon
/// `beacon` in a list of `active` IDs.
pub fn lottery_position(beacon: &Hash, draw: u64, active: u64) -> u64 {
    let h = if draw == 0 {
        tagged(CAC_TAG, &[beacon])
    } else {
        tagged(CAC_TAG, &[beacon, &draw.to_be_bytes()])
    };
    mod_u256(&h, active)
}

/// A first-in first-out committee of registration indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committee {
    size: usize,
    members: VecDeque<usize>,
    pending_extra: u64,
}

impl Committee {
    /// An empty committee of `size` seats.
    pub fn new(size: usize) -> Committee {
        Committee {
            size,
            members: VecDeque::new(),
            pending_extra: 0,
        }
    }

    /// Members, oldest first.
    pub fn members(&self) -> &VecDeque<usize> {
        &self.members
    }

    /// Seats waiting for an extra draw.
    pub fn pending_extra_draws(&self) -> u64 {
        self.pending_extra
    }

    fn pick(&self, len: usize, at: &impl Fn(usize) -> usize, start: u64) -> Option<usize> {
        (0..len)
            .map(|step| at((start as usize + step) % len))
            .find(|id| !self.members.contains(id))
    }

    /// The refresh at a 10th PoW-Tx block: the regular draw joins before the oldest member
    /// leaves (once the committee is full), then one extra draw per departed member. `active`
    /// is the canonical active list as of block B; `beacon` is the hash of block B+k.
    pub fn refresh(&mut self, active: &[usize], beacon: &Hash) {
        self.refresh_with(active.len(), |i| active[i], beacon);
    }

    /// [`Committee::refresh`] on a canonical active list given by its length and its entry at
    /// each position, for callers that keep the list in another structure.
    pub fn refresh_with(&mut self, len: usize, at: impl Fn(usize) -> usize, beacon: &Hash) {
        if len == 0 {
            return;
        }
        let n = len as u64;
        if let Some(joiner) = self.pick(len, &at, lottery_position(beacon, 0, n)) {
            self.members.push_back(joiner);
            // The oldest leaves once the committee, counting seats awaiting extra draws, is
            // over size.
            if self.members.len() + self.pending_extra as usize > self.size {
                self.members.pop_front();
            }
        }
        for i in 1..=self.pending_extra {
            if self.members.len() >= self.size {
                break;
            }
            if let Some(joiner) = self.pick(len, &at, lottery_position(beacon, i, n)) {
                self.members.push_back(joiner);
            }
        }
        self.pending_extra = 0;
    }

    /// A banned or deactivated member leaves at once; an extra draw refills the seat at the
    /// next refresh.
    pub fn depart(&mut self, member: usize) {
        if let Some(pos) = self.members.iter().position(|m| *m == member) {
            self.members.remove(pos);
            self.pending_extra += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beacon(i: u64) -> Hash {
        tagged(b"test-beacon", &[&i.to_be_bytes()])
    }

    #[test]
    fn regular_and_extra_draws_use_different_positions() {
        let b = beacon(1);
        assert_eq!(
            lottery_position(&b, 0, 1_000),
            mod_u256(&tagged(CAC_TAG, &[&b]), 1_000)
        );
        let positions: std::collections::BTreeSet<u64> =
            (0..5).map(|i| lottery_position(&b, i, u64::MAX)).collect();
        assert_eq!(positions.len(), 5);
    }

    #[test]
    fn committee_fills_then_slides_first_in_first_out() {
        let active: Vec<usize> = (0..100).collect();
        let mut c = Committee::new(10);
        for i in 0..10 {
            c.refresh(&active, &beacon(i));
        }
        assert_eq!(c.members().len(), 10);
        let oldest = c.members()[0];
        c.refresh(&active, &beacon(10));
        assert_eq!(c.members().len(), 10);
        assert!(!c.members().contains(&oldest));
        let unique: std::collections::BTreeSet<usize> = c.members().iter().copied().collect();
        assert_eq!(
            unique.len(),
            10,
            "the next-position rule keeps one seat per ID"
        );
    }

    #[test]
    fn departures_leave_at_once_and_extra_draws_refill() {
        let active: Vec<usize> = (0..100).collect();
        let mut c = Committee::new(10);
        for i in 0..10 {
            c.refresh(&active, &beacon(i));
        }
        let leaving = c.members()[4];
        c.depart(leaving);
        assert_eq!(c.members().len(), 9);
        assert_eq!(c.pending_extra_draws(), 1);
        let oldest = c.members()[0];
        c.refresh(&active, &beacon(50));
        assert_eq!(c.members().len(), 10);
        assert_eq!(c.pending_extra_draws(), 0);
        assert!(
            !c.members().contains(&oldest),
            "the oldest still leaves at the refresh"
        );
    }

    #[test]
    fn a_draw_on_a_member_moves_to_the_next_position() {
        // With 3 active IDs and 2 members, the joiner is the one non-member wherever it lands.
        let active = vec![10, 20, 30];
        let mut c = Committee::new(3);
        c.members.push_back(10);
        c.members.push_back(30);
        c.refresh(&active, &beacon(3));
        assert!(c.members().contains(&20));
    }
}
