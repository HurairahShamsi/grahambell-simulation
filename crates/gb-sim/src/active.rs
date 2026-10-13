//! The canonical active list (SPEC §3.11) as a Fenwick tree over registration indices. An ID
//! joins or leaves, and the ID at a position of the list is found, in O(log n), so the
//! committee lottery reads the list without copying it.

/// Registration indices of the active IDs, in index order.
#[derive(Debug, Clone, Default)]
pub(crate) struct ActiveSet {
    /// Fenwick tree of presence counts, 1-based (`tree[0]` is unused).
    tree: Vec<u32>,
    present: Vec<bool>,
    len: usize,
}

impl ActiveSet {
    /// Active IDs.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Adds registration index `i`; false if it was already active.
    pub(crate) fn insert(&mut self, i: usize) -> bool {
        if i >= self.present.len() {
            self.grow(i + 1);
        }
        if self.present[i] {
            return false;
        }
        self.present[i] = true;
        self.len += 1;
        self.add(i, 1);
        true
    }

    /// Removes registration index `i`; false if it was not active.
    pub(crate) fn remove(&mut self, i: usize) -> bool {
        if !self.present.get(i).copied().unwrap_or(false) {
            return false;
        }
        self.present[i] = false;
        self.len -= 1;
        self.add(i, -1);
        true
    }

    /// The registration index at position `k` of the list (0-based); `k` must be below `len`.
    pub(crate) fn nth(&self, k: usize) -> usize {
        debug_assert!(k < self.len);
        let n = self.present.len();
        let mut pos = 0;
        let mut remaining = k as u32 + 1;
        let mut step = n.next_power_of_two();
        while step > 0 {
            let next = pos + step;
            if next <= n && self.tree[next] < remaining {
                pos = next;
                remaining -= self.tree[next];
            }
            step /= 2;
        }
        pos
    }

    /// The active registration indices, ascending.
    pub(crate) fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.present
            .iter()
            .enumerate()
            .filter(|(_, p)| **p)
            .map(|(i, _)| i)
    }

    fn add(&mut self, i: usize, delta: i32) {
        let mut j = i + 1;
        while j < self.tree.len() {
            self.tree[j] = self.tree[j].wrapping_add_signed(delta);
            j += j & j.wrapping_neg();
        }
    }

    /// Room for indices below `min_len` (at least doubling), with the tree rebuilt in O(n).
    fn grow(&mut self, min_len: usize) {
        let n = min_len.max(2 * self.present.len()).max(64);
        self.present.resize(n, false);
        self.tree = vec![0; n + 1];
        for j in 1..=n {
            self.tree[j] += u32::from(self.present[j - 1]);
            let parent = j + (j & j.wrapping_neg());
            if parent <= n {
                self.tree[parent] += self.tree[j];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{Keyed, Purpose};
    use std::collections::BTreeSet;

    #[test]
    fn it_matches_a_sorted_set_under_random_changes() {
        let mut keyed = Keyed::new(5, "active");
        let mut set = ActiveSet::default();
        let mut model = BTreeSet::new();
        for step in 0..20_000u64 {
            let w = keyed.words(Purpose::Setup, 0, step);
            let i = (w[0] % 3_000) as usize;
            if w[1] % 3 == 0 {
                assert_eq!(set.remove(i), model.remove(&i));
            } else {
                assert_eq!(set.insert(i), model.insert(i));
            }
            assert_eq!(set.len(), model.len());
            if !model.is_empty() {
                let k = (w[2] % model.len() as u64) as usize;
                assert_eq!(set.nth(k), *model.iter().nth(k).unwrap());
            }
        }
        assert!(set.iter().eq(model.iter().copied()));
    }
}
