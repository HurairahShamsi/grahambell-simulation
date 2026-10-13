//! The event queue (ARCHITECTURE §3): timestamped events in a deterministic order, by time and
//! then by sequence number (the order in which they were scheduled).
//!
//! Most events go to a binary heap. Events scheduled a constant delay after the current time
//! (timers such as One Chance retries) can go to a FIFO lane instead: their times never
//! decrease in scheduling order, so a lane is already sorted, and pushing or popping costs
//! O(1). Lanes share the heap's sequence numbers, so the order of events is exactly the same
//! as with the heap alone.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};

/// Simulated time in microseconds since the start of a run.
pub type Micros = u64;

/// Microseconds per second.
pub const SECOND: Micros = 1_000_000;

/// Seconds to microseconds, rounded to the nearest microsecond (negative values become 0).
pub fn micros(seconds: f64) -> Micros {
    if seconds <= 0.0 {
        0
    } else if seconds >= (u64::MAX / 2) as f64 / SECOND as f64 {
        u64::MAX / 2
    } else {
        libm::round(seconds * SECOND as f64) as Micros
    }
}

/// Microseconds to seconds.
pub fn seconds(t: Micros) -> f64 {
    t as f64 / SECOND as f64
}

struct Entry<E> {
    time: Micros,
    seq: u64,
    event: E,
}

impl<E> PartialEq for Entry<E> {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time && self.seq == other.seq
    }
}

impl<E> Eq for Entry<E> {}

impl<E> PartialOrd for Entry<E> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<E> Ord for Entry<E> {
    // Reversed, so the binary max-heap pops the earliest (time, seq) first.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .time
            .cmp(&self.time)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

/// A priority queue of events.
pub struct Queue<E> {
    heap: BinaryHeap<Entry<E>>,
    lanes: Vec<VecDeque<Entry<E>>>,
    seq: u64,
    now: Micros,
    processed: u64,
}

impl<E> Default for Queue<E> {
    fn default() -> Self {
        Queue {
            heap: BinaryHeap::new(),
            lanes: Vec::new(),
            seq: 0,
            now: 0,
            processed: 0,
        }
    }
}

impl<E> Queue<E> {
    /// Schedules `event` at `time`; a time in the past is moved to now.
    pub fn push(&mut self, time: Micros, event: E) {
        let time = time.max(self.now);
        self.heap.push(Entry {
            time,
            seq: self.seq,
            event,
        });
        self.seq += 1;
    }

    /// Schedules `event` at `time` in FIFO lane `lane`. Meant for events whose times never
    /// decrease in scheduling order within the lane (a constant delay after now); an event
    /// that would break the lane's order goes to the heap instead, so the order of events is
    /// always the same as with [`Queue::push`].
    pub fn push_lane(&mut self, lane: usize, time: Micros, event: E) {
        let time = time.max(self.now);
        if self.lanes.len() <= lane {
            self.lanes.resize_with(lane + 1, VecDeque::new);
        }
        let entry = Entry {
            time,
            seq: self.seq,
            event,
        };
        let queue = &mut self.lanes[lane];
        if queue.back().is_some_and(|last| last.time > time) {
            self.heap.push(entry);
        } else {
            queue.push_back(entry);
        }
        self.seq += 1;
    }

    /// Where the earliest event waits: `None` for the heap, `Some(lane)` for a lane.
    fn earliest(&self) -> Option<Option<usize>> {
        let mut best = self.heap.peek().map(|e| ((e.time, e.seq), None));
        for (i, lane) in self.lanes.iter().enumerate() {
            if let Some(e) = lane.front() {
                if best.is_none_or(|(key, _)| (e.time, e.seq) < key) {
                    best = Some(((e.time, e.seq), Some(i)));
                }
            }
        }
        best.map(|(_, place)| place)
    }

    /// The next event and its time, advancing the clock.
    pub fn pop(&mut self) -> Option<(Micros, E)> {
        let entry = match self.earliest()? {
            None => self.heap.pop()?,
            Some(lane) => self.lanes[lane].pop_front()?,
        };
        self.now = entry.time;
        self.processed += 1;
        Some((entry.time, entry.event))
    }

    /// The time of the next event.
    pub fn peek_time(&self) -> Option<Micros> {
        match self.earliest()? {
            None => self.heap.peek().map(|e| e.time),
            Some(lane) => self.lanes[lane].front().map(|e| e.time),
        }
    }

    /// The current time.
    pub fn now(&self) -> Micros {
        self.now
    }

    /// Events processed so far.
    pub fn processed(&self) -> u64 {
        self.processed
    }

    /// Events waiting.
    pub fn len(&self) -> usize {
        self.heap.len() + self.lanes.iter().map(VecDeque::len).sum::<usize>()
    }

    /// True when no event is waiting.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_come_out_by_time_then_scheduling_order() {
        let mut q = Queue::default();
        q.push(5, 'c');
        q.push(1, 'a');
        q.push(5, 'd');
        q.push(1, 'b');
        let order: Vec<char> = std::iter::from_fn(|| q.pop().map(|(_, e)| e)).collect();
        assert_eq!(order, vec!['a', 'b', 'c', 'd']);
        assert_eq!(q.now(), 5);
        q.push(2, 'e');
        assert_eq!(q.pop(), Some((5, 'e')), "past times move to now");
        assert_eq!(q.processed(), 5);
    }

    #[test]
    fn lanes_keep_the_order_of_a_single_heap() {
        // The same schedule through the heap alone and through heap plus lanes, with ties.
        let schedule: Vec<(Option<usize>, Micros)> = vec![
            (None, 30),
            (Some(0), 10),
            (Some(1), 30),
            (Some(0), 30),
            (None, 10),
            (Some(1), 30),
            (Some(0), 20),
            (None, 30),
        ];
        let mut single = Queue::default();
        let mut laned = Queue::default();
        for (i, (lane, t)) in schedule.iter().enumerate() {
            single.push(*t, i);
            match lane {
                Some(l) => laned.push_lane(*l, *t, i),
                None => laned.push(*t, i),
            }
        }
        assert_eq!(laned.len(), schedule.len());
        let a: Vec<_> = std::iter::from_fn(|| single.pop()).collect();
        let b: Vec<_> = std::iter::from_fn(|| laned.pop()).collect();
        assert_eq!(a, b);
        assert_eq!(single.processed(), laned.processed());
        assert!(laned.is_empty());
    }

    #[test]
    fn time_conversions_round_trip() {
        assert_eq!(micros(1.5), 1_500_000);
        assert_eq!(micros(-3.0), 0);
        assert_eq!(seconds(2_500_000), 2.5);
    }
}
