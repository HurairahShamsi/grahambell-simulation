//! Registration index, canonical active list, seats, the KWC ring and the seat rule (SPEC
//! §3.11, §4.2, §4.7).
//!
//! - **Insertion** (inside-out Fisher–Yates): with `n` seats filled,
//!   `j = SHA256("GB/alloc" ‖ ID ‖ beacon) mod (n + 1)`. If `j = n` the ID takes seat `n`;
//!   otherwise the ID in seat `j` moves to seat `n` and the new ID takes seat `j`.
//! - **Removal** (a ban, or an absence longer than L): the ID in the last seat moves into the
//!   vacated seat.
//! - **KWC ring:** KWC `w` has leader WC `w` and subordinates `(w + offset) mod W`.

use crate::hash::{Hash, mod_u256, tagged};
use std::collections::BTreeMap;

/// Domain tag of the insertion position.
pub const ALLOC_TAG: &[u8] = b"GB/alloc";

/// An ID: the hash of its winning PoW-ID block.
pub type IdHash = Hash;

/// Seat position for an ID inserted while `filled` seats are taken.
pub fn insertion_position(id: &IdHash, beacon: &Hash, filled: u64) -> u64 {
    mod_u256(&tagged(ALLOC_TAG, &[id, beacon]), filled + 1)
}

/// The WCs of KWC `w` on a ring of `wcs` active WCs: the leader `w`, then
/// `(w + o) mod W` for each ring offset `o`, in that order.
pub fn kwc_wcs(w: u64, wcs: u64, offsets: &[u64]) -> impl Iterator<Item = u64> + '_ {
    std::iter::once(w).chain(offsets.iter().map(move |o| (w + o) % wcs.max(1)))
}

/// The KWC ring on `wcs` active WCs: entry `w` is `[w, (w + o₁) mod W, …]`.
pub fn golomb_ring(wcs: u64, offsets: &[u64]) -> Vec<Vec<u64>> {
    (0..wcs)
        .map(|w| kwc_wcs(w, wcs, offsets).collect())
        .collect()
}

/// Status of a registered ID (SPEC §3.11, §4.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdStatus {
    /// Active.
    Active,
    /// Deactivated after being offline beyond the allowance (keeps its seat until L passes).
    Deactivated,
    /// Banned.
    Banned,
}

/// The registration index: every registered ID in registration order (genesis first, then
/// PoW-ID confirmation order).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registry {
    entries: Vec<(IdHash, IdStatus)>,
}

impl Registry {
    /// Registers an ID and returns its registration index.
    pub fn register(&mut self, id: IdHash) -> usize {
        self.entries.push((id, IdStatus::Active));
        self.entries.len() - 1
    }

    /// Sets the status of the ID with registration index `index`.
    pub fn set_status(&mut self, index: usize, status: IdStatus) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.1 = status;
        }
    }

    /// The ID with registration index `index`.
    pub fn id(&self, index: usize) -> Option<IdHash> {
        self.entries.get(index).map(|e| e.0)
    }

    /// The canonical active list: registration indices of active IDs, in index order
    /// (banned and deactivated IDs excluded).
    pub fn canonical_active_list(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.1 == IdStatus::Active)
            .map(|(i, _)| i)
            .collect()
    }
}

/// SPEC §4.7 seat rule (v0.4): a seat is vacated only on a ban, or after a continuous absence
/// longer than `long_absence_s`. Going offline or being deactivated changes no seat.
pub fn vacates_seat(
    banned: bool,
    absent_since: Option<u64>,
    now: u64,
    long_absence_s: u64,
) -> bool {
    banned || absent_since.is_some_and(|since| now.saturating_sub(since) > long_absence_s)
}

/// Seats and the KWC ring (SPEC §4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allocation {
    seats: Vec<IdHash>,
    seat_of: BTreeMap<IdHash, usize>,
    wc_size: usize,
    offsets: Vec<u64>,
}

impl Allocation {
    /// An empty allocation with WCs of `wc_size` seats and the given ring offsets.
    pub fn new(wc_size: usize, offsets: &[u64]) -> Allocation {
        Allocation {
            seats: Vec::new(),
            seat_of: BTreeMap::new(),
            wc_size,
            offsets: offsets.to_vec(),
        }
    }

    /// Inserts an ID with its beacon (a new ID, or a returning ID whose seat was vacated, with
    /// the re-activation beacon). Returns the seat it takes, or `None` if it is already seated.
    pub fn insert(&mut self, id: IdHash, beacon: &Hash) -> Option<usize> {
        if self.seat_of.contains_key(&id) {
            return None;
        }
        let n = self.seats.len();
        let j = insertion_position(&id, beacon, n as u64) as usize;
        if j == n {
            self.seats.push(id);
        } else {
            let displaced = self.seats[j];
            self.seats[j] = id;
            self.seats.push(displaced);
            self.seat_of.insert(displaced, n);
        }
        self.seat_of.insert(id, j);
        Some(j)
    }

    /// Removes an ID by swap-with-last; returns false if it holds no seat.
    pub fn remove(&mut self, id: &IdHash) -> bool {
        let Some(seat) = self.seat_of.remove(id) else {
            return false;
        };
        let last = self.seats.len() - 1;
        if seat != last {
            let moved = self.seats[last];
            self.seats[seat] = moved;
            self.seat_of.insert(moved, seat);
        }
        self.seats.pop();
        true
    }

    /// The seat of an ID.
    pub fn seat(&self, id: &IdHash) -> Option<usize> {
        self.seat_of.get(id).copied()
    }

    /// Filled seats.
    pub fn filled(&self) -> usize {
        self.seats.len()
    }

    /// Active WCs: `⌊filled / wc_size⌋` (the partial tail WC is pending).
    pub fn active_wcs(&self) -> u64 {
        (self.seats.len() / self.wc_size) as u64
    }

    /// Members of WC `w` in seat order.
    pub fn wc_members(&self, w: u64) -> &[IdHash] {
        let start = w as usize * self.wc_size;
        &self.seats[start..start + self.wc_size]
    }

    /// The KWC ring on the active WCs.
    pub fn ring(&self) -> Vec<Vec<u64>> {
        golomb_ring(self.active_wcs(), &self.offsets)
    }

    /// Members of KWC `w` in seat order: the leader WC's seats, then each subordinate WC's in
    /// ring-offset order (the bitfield order of Appendix A).
    pub fn kwc_members(&self, w: u64) -> Vec<IdHash> {
        kwc_wcs(w, self.active_wcs(), &self.offsets)
            .flat_map(|wc| self.wc_members(wc).to_vec())
            .collect()
    }

    /// The ID in seat `seat`, if filled.
    pub fn id_at(&self, seat: usize) -> Option<&IdHash> {
        self.seats.get(seat)
    }

    /// The ring offsets.
    pub fn offsets(&self) -> &[u64] {
        &self.offsets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::tagged;

    fn id(i: u64) -> IdHash {
        tagged(b"test-id", &[&i.to_be_bytes()])
    }

    fn beacon(i: u64) -> Hash {
        tagged(b"test-beacon", &[&i.to_be_bytes()])
    }

    /// Each KWC's composition: its WC indices in ascending order, each with sorted members.
    fn compositions(a: &Allocation) -> Vec<Vec<(u64, Vec<IdHash>)>> {
        a.ring()
            .into_iter()
            .map(|kwc| {
                let mut record: Vec<(u64, Vec<IdHash>)> = kwc
                    .iter()
                    .map(|w| {
                        let mut m = a.wc_members(*w).to_vec();
                        m.sort_unstable();
                        (*w, m)
                    })
                    .collect();
                record.sort_unstable();
                record
            })
            .collect()
    }

    fn changed(before: &[Vec<(u64, Vec<IdHash>)>], after: &[Vec<(u64, Vec<IdHash>)>]) -> u64 {
        after
            .iter()
            .enumerate()
            .filter(|(w, k)| before.get(*w) != Some(*k))
            .count() as u64
    }

    #[test]
    fn insertion_and_removal_keep_a_consistent_seat_map() {
        let mut a = Allocation::new(10, &[1, 4, 6]);
        for i in 0..300 {
            a.insert(id(i), &beacon(i));
        }
        assert_eq!(a.insert(id(5), &beacon(999)), None, "already seated");
        for i in (0..300).step_by(7) {
            assert!(a.remove(&id(i)));
        }
        assert!(!a.remove(&id(0)));
        for s in 0..a.filled() {
            let who = a.seats[s];
            assert_eq!(a.seat(&who), Some(s));
        }
        assert_eq!(a.filled(), 300 - 300usize.div_ceil(7));
    }

    #[test]
    fn insertion_position_is_the_hash_modulo_filled_plus_one() {
        let n = 12_345;
        let j = insertion_position(&id(1), &beacon(1), n);
        assert_eq!(
            j,
            mod_u256(&tagged(ALLOC_TAG, &[&id(1), &beacon(1)]), n + 1)
        );
        assert!(j <= n);
    }

    #[test]
    fn ring_matches_m1_and_meets_every_requirement() {
        // Reference: M1's ring construction and its independent ring checker.
        for wcs in [13u64, 50, 400] {
            let ring = golomb_ring(wcs, &[1, 4, 6]);
            assert_eq!(ring, gb_analytic::witness::golomb_ring(wcs, &[1, 4, 6]));
            assert_eq!(gb_analytic::witness::ring_violations(&ring), 0);
        }
    }

    #[test]
    fn composition_rates_reproduce_m1() {
        // M1 (section B, B-allocation-compositions): 4.7 changed compositions per insertion and
        // 4.6 per removal, within 2%, with 2,000 IDs then 1,000 insertions and 1,000 removals.
        let mut a = Allocation::new(10, &[1, 4, 6]);
        for i in 0..2_000 {
            a.insert(id(i), &beacon(i));
        }
        let mut before = compositions(&a);
        let (mut inserted, mut removed) = (0u64, 0u64);
        for i in 2_000..3_000 {
            a.insert(id(i), &beacon(i));
            let after = compositions(&a);
            inserted += changed(&before, &after);
            before = after;
        }
        for i in 0..1_000u64 {
            // Remove pseudo-randomly chosen seated IDs.
            let seat = mod_u256(&beacon(10_000 + i), a.filled() as u64) as usize;
            let who = a.seats[seat];
            a.remove(&who);
            let after = compositions(&a);
            removed += changed(&before, &after);
            before = after;
        }
        let (per_insert, per_remove) = (inserted as f64 / 1_000.0, removed as f64 / 1_000.0);
        assert!((per_insert - 4.7).abs() <= 0.02 * 4.7, "{per_insert}");
        assert!((per_remove - 4.6).abs() <= 0.02 * 4.6, "{per_remove}");
    }

    #[test]
    fn kwc_members_follow_seat_order() {
        let mut a = Allocation::new(10, &[1, 4, 6]);
        for i in 0..200 {
            a.insert(id(i), &beacon(i));
        }
        let members = a.kwc_members(3);
        assert_eq!(members.len(), 40);
        assert_eq!(&members[..10], a.wc_members(3));
        assert_eq!(&members[10..20], a.wc_members(4));
        assert_eq!(&members[30..40], a.wc_members(9));
    }

    #[test]
    fn seats_are_vacated_only_by_bans_and_long_absences() {
        let l = 30 * 86_400;
        assert!(vacates_seat(true, None, 0, l));
        assert!(
            !vacates_seat(false, Some(100), 100 + l, l),
            "exactly L is not longer than L"
        );
        assert!(vacates_seat(false, Some(100), 101 + l, l));
        assert!(!vacates_seat(false, None, 10 * l, l));
    }

    #[test]
    fn canonical_active_list_excludes_banned_and_deactivated_ids() {
        let mut r = Registry::default();
        for i in 0..5 {
            r.register(id(i));
        }
        r.set_status(1, IdStatus::Deactivated);
        r.set_status(3, IdStatus::Banned);
        assert_eq!(r.canonical_active_list(), vec![0, 2, 4]);
        assert_eq!(r.id(2), Some(id(2)));
    }
}
