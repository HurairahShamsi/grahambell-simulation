//! Rounds: candidate generation, entropy and PoWit timing, and the miner's decision.
//!
//! **Fast mode (exact).** Every miner slot's chain on a parent has independent steps that win
//! with probability `p = T / 2²⁵⁶` (`gb_protocol::chain::step_win_probability`). The first step
//! with any success among `r` slots is geometric with `1 − (1 − p)^r`, the number of slots
//! succeeding at it is binomial conditioned on at least one, and those slots are uniform among
//! the remaining ones; the others carry on, memoryless. Drawing steps this way is the same law
//! as one geometric draw per slot (the `PerMiner` sampler, the reference). Eligibility
//! (online, admitted, KWC able to sign) is applied to each drawn candidate, which leaves the
//! law of the eligible slots' steps unchanged.
//!
//! **Stand-in and real modes** compute every slot's real SHA-256 chain (`ChainInputs`) with
//! stand-in or real BLS entropy and take its first winning step.

use crate::chain::{Candidate, Group, INF, Node};
use crate::engine::{Micros, SECOND, micros, seconds};
use crate::household::{bit, pick};
use crate::job::{ForkRule, Sampler, Strategy};
use crate::rng::{Cursor, Purpose};
use crate::samplers::{any_success, geometric_failures, zero_truncated_binomial};
use crate::sim::{Ev, Sim};
use crate::world::{NONE, Phase};
use gb_config::protocol::{EntropySigners, EquivocationScope, StartRule as ConfigStartRule};
use gb_config::sim::CryptoMode;
use gb_protocol::chain::{ChainInputs, StartRule, chain_start, step_win_probability};
use gb_protocol::encoding::{Address, Header, block_hash};
use gb_protocol::entropy::entropy_threshold;
use gb_protocol::hash::Hash;
use gb_protocol::validate::{
    BlockFacts, ChainCheck, QuorumRule, Reception, Rules, Tip, check_powit_quorum, prefer,
    validate_facts,
};
use std::cmp::Ordering;

/// What the chain generators return: the sorted (step, slot) list, and each slot's entropy,
/// header and real entropy record.
type ChainList = (
    Vec<(u64, u32)>,
    Vec<Hash>,
    Vec<Header>,
    Vec<Option<crate::real::RealEntropy>>,
);

/// Steps searched at most by the chain generators of the stand-in and real modes.
const CHAIN_CAP: u64 = 600;

/// What a drawn slot's checks found up to its entropy (`Sim::prepare`); under start rule (r)
/// a candidate whose step comes later than its draw keeps it until its own found time.
#[derive(Debug, Clone)]
pub(crate) struct Prep {
    g: u32,
    m: u32,
    k: u32,
    step: u64,
    node: Node,
    displaced_at: Micros,
    e_ready: Micros,
    t0: u64,
    found: Micros,
    entropy_flags: Vec<bool>,
    entropy_quorum: u32,
}

/// When a miner's entropy is issued and when the miner holds it (SPEC §3.4).
#[derive(Debug, Clone)]
struct EntropyTiming {
    /// When the miner holds the entropy.
    ready: Micros,
    /// Whether an absent member held it up (all-online-members rule).
    stalled: bool,
    /// The issue time the KWC attests, on the signing member's clock, seconds: when the last
    /// required signature (or, under a threshold, the t-th) was made, or a record offline
    /// completed the required set.
    issued_s: f64,
    /// Which of the round's members' signatures it used.
    signers: Vec<bool>,
    /// Signatures it needed.
    quorum: u32,
}

/// How a round draws its candidates.
#[derive(Debug, Clone)]
pub(crate) enum Generator {
    /// Next success step over the remaining slots, then the tied winners.
    Gap {
        next_step: u64,
        remaining: u64,
        picked: Vec<u32>,
        draws: u64,
    },
    /// A precomputed list of (step, slot), sorted.
    List { items: Vec<(u64, u32)>, pos: usize },
}

impl Sim<'_> {
    /// Opens the round on block `parent`.
    pub(crate) fn open_group(&mut self, parent: u32, now: Micros) {
        let g = self.groups.len() as u32;
        let block = &self.blocks[parent as usize];
        let t0 = chain_start(block.timestamp, StartRule::A);
        let height = block.height + 1;
        let target = block.child_target;
        let p = step_win_probability(&target);
        let slots = self.miners.len() as u32;
        // The round exists before its candidates are drawn, so that drawing them can use the
        // round's KWC member lists (`round_members`).
        self.groups.push(Group {
            parent,
            height,
            t0,
            target,
            p,
            slots,
            generator: Generator::List {
                items: Vec::new(),
                pos: 0,
            },
            extras: Vec::new(),
            held: None,
            entropies: Vec::new(),
            headers: Vec::new(),
            real: Vec::new(),
            closed: false,
            horizon_at: INF,
            first_recorded: false,
            eligible: self.counts.after_wait,
            kwc_members: Vec::new(),
            issued: None,
            transient: Vec::new(),
            origin: 0,
            refresh_at: 0,
        });
        let generator = match self.spec.crypto {
            CryptoMode::Fast => match self.spec.sampler {
                Sampler::Gap => Generator::Gap {
                    next_step: 0,
                    remaining: u64::from(slots),
                    picked: Vec::new(),
                    draws: 0,
                },
                Sampler::PerMiner => {
                    let mut items = Vec::with_capacity(slots as usize);
                    for m in 0..slots {
                        let u = self.keyed.uniforms(
                            Purpose::Round,
                            (u64::from(g) << 24) | u64::from(m),
                            0,
                        );
                        let n = geometric_failures(1.0 - u[0], p);
                        if n < 100_000 {
                            items.push((n, m));
                        }
                    }
                    items.sort();
                    Generator::List { items, pos: 0 }
                }
            },
            CryptoMode::StandIn | CryptoMode::Real => {
                let (items, entropies, headers, real) =
                    self.chain_list(g, parent, height, t0, &target);
                let group = &mut self.groups[g as usize];
                group.entropies = entropies;
                group.headers = headers;
                group.real = real;
                Generator::List { items, pos: 0 }
            }
        };
        self.groups[g as usize].generator = generator;
        // Slots drawn individually (S5 restarters) and removed from the generator.
        let mut extras = Vec::new();
        for m in 0..slots {
            match self.spec_strategy(m) {
                Some(Strategy::Restart {
                    old_design: true, ..
                }) => {
                    if self.miners[m as usize].phase != Phase::Mining {
                        continue;
                    }
                    let mut next = self.miners[m as usize].next_success;
                    let mut draw = 0;
                    while next != u64::MAX && next < t0 {
                        self.advance_connection_sequence(m, t0.saturating_sub(1), draw);
                        draw += 1;
                        next = self.miners[m as usize].next_success;
                    }
                    if next != u64::MAX {
                        extras.push((next - t0, m));
                    }
                }
                Some(Strategy::Restart {
                    old_design: false,
                    window_s,
                }) => {
                    if self.miners[m as usize].phase != Phase::Mining {
                        continue;
                    }
                    let u = self.keyed.uniforms(
                        Purpose::Strategy,
                        (u64::from(g) << 24) | u64::from(m),
                        1,
                    );
                    let n = geometric_failures(1.0 - u[0], p);
                    if n as f64 > window_s {
                        // It sees a slow chain this round and restarts (it cannot do better).
                        self.restart(now, m);
                    } else {
                        extras.push((n, m));
                    }
                }
                _ => {}
            }
        }
        extras.sort_by(|a, b| b.cmp(a));
        self.groups[g as usize].extras = extras;
        self.schedule_wake(now, g);
    }

    /// The members of KWC `k` in round `g`: read from the allocation when the round first needs
    /// them and kept for the rest of the round. A miner's entropy, its PoWit and the checks of
    /// its block then refer to one member list even when a placement changes the KWC during the
    /// round; in the protocol a round's KWC is fixed by the chain it builds on.
    pub(crate) fn round_members(&mut self, g: u32, k: u32) -> Vec<u32> {
        if let Some((_, members)) = self.groups[g as usize]
            .kwc_members
            .iter()
            .find(|(x, _)| *x == k)
        {
            return members.clone();
        }
        let members = self.kwc_members(k);
        self.groups[g as usize]
            .kwc_members
            .push((k, members.clone()));
        members
    }

    /// Real SHA-256 chains for every slot (stand-in or real entropy).
    fn chain_list(
        &mut self,
        g: u32,
        parent: u32,
        height: u64,
        t0: u64,
        target: &Hash,
    ) -> ChainList {
        let slots = self.miners.len() as u32;
        let mut items = Vec::new();
        let mut headers = Vec::with_capacity(slots as usize);
        let mut entropies = Vec::with_capacity(slots as usize);
        let mut real = Vec::with_capacity(slots as usize);
        let prev = self.blocks[parent as usize].hash;
        for m in 0..slots {
            let k = self.miners[m as usize].kwc;
            let header = self.header_for(m, height, prev, k, target);
            let (entropy, r) = if self.spec.crypto == CryptoMode::Real && k != NONE {
                let members = self.round_members(g, k);
                let rec = crate::real::entropy_for(self, m, &header, &members);
                (rec.entropy, Some(rec))
            } else {
                (self.standin_entropy(g, m), None)
            };
            entropies.push(entropy);
            real.push(r);
            headers.push(header);
            let inputs = ChainInputs::new(&header, entropy, t0);
            let p = step_win_probability(target);
            if let Some((n, _)) = inputs.winning_step(target, CHAIN_CAP) {
                items.push((n, m));
                if self.spec.record_rounds && self.res.slot_steps.len() < 200_000 {
                    self.res.slot_steps.push(n as f64 * p);
                }
            } else if self.spec.record_rounds && self.res.slot_steps.len() < 200_000 {
                self.res.slot_steps.push(CHAIN_CAP as f64 * p);
            }
        }
        items.sort();
        (items, entropies, headers, real)
    }

    /// Stand-in entropy E for slot `m` in round `g` (32 uniform bytes, M2 C6).
    pub(crate) fn standin_entropy(&mut self, g: u32, m: u32) -> Hash {
        let w = self
            .keyed
            .words(Purpose::Content, (u64::from(g) << 24) | u64::from(m), 0);
        let mut e = [0u8; 32];
        for (i, word) in w.iter().take(4).enumerate() {
            e[i * 8..i * 8 + 8].copy_from_slice(&word.to_be_bytes());
        }
        e
    }

    /// The header slot `m` signs at `height` on `prev` (SPEC §3.3, Appendix A).
    pub(crate) fn header_for(
        &mut self,
        m: u32,
        height: u64,
        prev: Hash,
        kwc: u32,
        target: &Hash,
    ) -> Header {
        let epoch = self.miners[m as usize].key_epoch;
        let candidate_pk = if self.spec.crypto == CryptoMode::Real {
            crate::real::candidate_key(self.spec.seed, m, epoch).public
        } else {
            let w = self
                .keyed
                .words(Purpose::Content, (1 << 40) | u64::from(m), u64::from(epoch));
            let mut pk = [0u8; 48];
            for (i, word) in w.iter().take(6).enumerate() {
                pk[i * 8..i * 8 + 8].copy_from_slice(&word.to_be_bytes());
            }
            pk
        };
        let mut wallet = [0u8; 32];
        wallet[..4].copy_from_slice(&m.to_be_bytes());
        let mut prefix = [0u8; 8];
        prefix[..2].copy_from_slice(&[0x20, 0x01]);
        prefix[4..].copy_from_slice(&m.to_be_bytes());
        Header {
            version: gb_protocol::PROTOCOL_VERSION,
            height,
            prev_hash: prev,
            candidate_pk,
            reward_wallet: wallet,
            address: Address::Ipv6Prefix64(prefix),
            kwc_id: u64::from(kwc),
            target: *target,
        }
    }

    /// True for slots whose winning steps are drawn individually (S5 restarters): the shared
    /// generator's draws for them are thinned out, so each slot has exactly one chain.
    fn individual(&self, m: u32) -> bool {
        matches!(self.spec_strategy(m), Some(Strategy::Restart { .. }))
    }

    /// The next (step, winners) of a round, without slots drawn individually.
    fn next_draw(&mut self, g: u32) -> Option<(u64, Vec<u32>)> {
        loop {
            let (step, winners, from_generator) = self.next_raw_draw(g)?;
            let kept: Vec<u32> = if from_generator {
                let (keep, thin): (Vec<u32>, Vec<u32>) =
                    winners.into_iter().partition(|m| !self.individual(*m));
                for _ in thin {
                    self.drop(9);
                }
                keep
            } else {
                winners
            };
            if !kept.is_empty() {
                return Some((step, kept));
            }
        }
    }

    /// The next (step, winners) of a round, and whether they came from the shared generator.
    fn next_raw_draw(&mut self, g: u32) -> Option<(u64, Vec<u32>, bool)> {
        let p = self.groups[g as usize].p;
        let slots = self.groups[g as usize].slots;
        // Extras (individually drawn slots) merge with the generator by step.
        let extra_step = self.groups[g as usize].extras.last().map(|e| e.0);
        let held = self.groups[g as usize].held.take();
        let gen_next = if let Some(h) = held {
            Some(h)
        } else {
            match &mut self.groups[g as usize].generator {
                Generator::Gap {
                    next_step,
                    remaining,
                    picked,
                    draws,
                } => {
                    if *remaining == 0 || p <= 0.0 {
                        None
                    } else {
                        let mut cursor = Cursor::new(
                            &mut self.keyed,
                            Purpose::Round,
                            (u64::from(g) << 24) | 0xff_ffff,
                            *draws,
                        );
                        let q = any_success(p, *remaining);
                        let gap = geometric_failures(cursor.uniform_positive(), q);
                        let step = next_step.saturating_add(gap);
                        let k = zero_truncated_binomial(cursor.uniform(), *remaining, p);
                        let mut winners = Vec::with_capacity(k as usize);
                        while (winners.len() as u64) < k {
                            let m = pick(cursor.word(), slots as usize) as u32;
                            if !picked.contains(&m) && !winners.contains(&m) {
                                winners.push(m);
                            }
                        }
                        *draws = cursor.next_block();
                        picked.extend(&winners);
                        *remaining -= k;
                        *next_step = step + 1;
                        Some((step, winners))
                    }
                }
                Generator::List { items, pos } => {
                    if *pos >= items.len() {
                        None
                    } else {
                        let step = items[*pos].0;
                        let mut winners = Vec::new();
                        while *pos < items.len() && items[*pos].0 == step {
                            winners.push(items[*pos].1);
                            *pos += 1;
                        }
                        Some((step, winners))
                    }
                }
            }
        };
        // Merge by step: the generator's draw waits in `held` while extras come first.
        match (gen_next, extra_step) {
            (None, None) => None,
            (Some((step, winners)), None) => Some((step, winners, true)),
            (None, Some(_)) => self.pop_extras(g).map(|(s, w)| (s, w, false)),
            (Some((step, winners)), Some(es)) => {
                if es < step {
                    self.groups[g as usize].held = Some((step, winners));
                    self.pop_extras(g).map(|(s, w)| (s, w, false))
                } else {
                    Some((step, winners, true))
                }
            }
        }
    }

    /// Pops every extra at the smallest step.
    fn pop_extras(&mut self, g: u32) -> Option<(u64, Vec<u32>)> {
        let (step, m) = self.groups[g as usize].extras.pop()?;
        let mut winners = vec![m];
        while self.groups[g as usize]
            .extras
            .last()
            .is_some_and(|e| e.0 == step)
        {
            if let Some(e) = self.groups[g as usize].extras.pop() {
                winners.push(e.1);
            }
        }
        Some((step, winners))
    }

    /// Schedules the round's next wake: shortly before its next candidate could broadcast.
    fn schedule_wake(&mut self, now: Micros, g: u32) {
        let Some((step, winners)) = self.next_draw(g) else {
            // Out of draws: a round left without a block anyone extends draws again, a round
            // later, the slots it dropped while their miners could not mine (SIM-48).
            if !self.groups[g as usize].closed && !self.groups[g as usize].transient.is_empty() {
                self.q.push(
                    now + micros(self.p.interval_s as f64),
                    Ev::GroupRefresh { g },
                );
            }
            return;
        };
        self.pending.insert(g, (step, winners));
        let t0 = self.groups[g as usize].t0;
        let slack = self.max_skew + self.p.signing_tolerance + 1.0;
        let at = micros(((t0 + step) as f64 - slack).max(0.0));
        self.q.push(at.max(now), Ev::GroupWake { g });
    }

    pub(crate) fn on_group_wake(&mut self, now: Micros, g: u32) {
        if self.groups[g as usize].closed {
            return;
        }
        let Some((step, winners)) = self.pending.remove(&g) else {
            return;
        };
        let height = self.groups[g as usize].height;
        let horizon = self.horizon_reached(height + 1, now);
        if horizon && self.groups[g as usize].horizon_at == INF {
            self.groups[g as usize].horizon_at = now;
        }
        let beyond = self.groups[g as usize].horizon_at != INF;
        if beyond && (!self.spec.audit || self.horizon_reached(height + 3, now)) {
            let group = &mut self.groups[g as usize];
            group.closed = true;
            // Candidates keep their own member lists; the round needs none any more.
            group.kwc_members = Vec::new();
            group.transient = Vec::new();
            return;
        }
        if !self.groups[g as usize].first_recorded {
            self.groups[g as usize].first_recorded = true;
            self.res.rounds += 1;
            let eligible = self.groups[g as usize].eligible;
            self.res.round_slots += eligible as f64;
            if winners.len() >= 2 {
                self.res.tie_rounds += 1;
            }
            if self.spec.record_rounds && self.res.nmin.len() < 50_000 {
                self.res.nmin.push(step);
                self.res.round_r.push(eligible);
                self.res.round_p.push(self.groups[g as usize].p);
                self.res.tie_size.push(winners.len() as u64);
            }
        }
        for m in winners {
            if let Some(c) = self.materialise(now, g, m, step) {
                self.candidates[c as usize].beyond_horizon = beyond;
            }
        }
        self.schedule_wake(now, g);
    }

    /// True when every panel node accepted a block at height ≥ `height` long enough ago that
    /// any household has it too. A block a node holds late (its PoWit after the deadline) does
    /// not count at that node: the node does not extend it, so blocks at that height can still
    /// win there (until 2026-10-21 such blocks counted, and a height whose blocks were all late
    /// ended its round, stalling the chain until the late block's own miner extended it).
    pub(crate) fn horizon_reached(&self, height: u64, now: Micros) -> bool {
        if self.settled_ids.len() as u64 > height {
            return true;
        }
        let margin = self.d_max;
        (0..self.panel.len())
            .filter(|i| !self.panel_split[*i])
            .all(|i| {
                self.unsettled.iter().any(|b| {
                    let block = &self.blocks[*b as usize];
                    block.height >= height
                        && block.panel[i] != INF
                        && !block.panel_late[i]
                        && block.panel[i].saturating_add(margin) <= now
                })
            })
    }

    /// Unsettled blocks at `height`.
    fn blocks_at(&self, height: u64) -> Vec<u32> {
        self.unsettled
            .iter()
            .copied()
            .filter(|b| self.blocks[*b as usize].height == height)
            .collect()
    }

    /// A round that ran out of draws (SIM-48): if no block at its height or above is held on
    /// time by any validator, so that nobody would extend one, its slots dropped while their
    /// miners could not mine are drawn again, from now. Without this, a round whose every slot
    /// was drawn during an outage, when nobody could mine, stayed without candidates for good
    /// and stopped the chain, whereas a miner that becomes able to mine joins the round.
    pub(crate) fn on_group_refresh(&mut self, now: Micros, g: u32) {
        let (closed, height, t0, p) = {
            let grp = &self.groups[g as usize];
            (grp.closed, grp.height, grp.t0, grp.p)
        };
        if closed || self.groups[g as usize].transient.is_empty() {
            return;
        }
        // Its height settled, or its parent lost: the round can never matter again.
        let parent = self.groups[g as usize].parent;
        let parent_lost =
            self.blocks[parent as usize].settled && !self.blocks[parent as usize].canonical;
        if parent_lost || (height as usize) < self.settled_ids.len() {
            self.groups[g as usize].transient = Vec::new();
            return;
        }
        let extendable = |b: u32| {
            let block = &self.blocks[b as usize];
            !block.withheld
                && block
                    .panel
                    .iter()
                    .zip(&block.panel_late)
                    .any(|(at, late)| *at != INF && !*late)
        };
        if self.blocks_from(height).into_iter().any(extendable) {
            // A block someone extends stays so (or settles), so the round never needs a refresh.
            self.groups[g as usize].transient = Vec::new();
            return;
        }
        let now_step = (now / SECOND).saturating_sub(t0);
        let slots = std::mem::take(&mut self.groups[g as usize].transient);
        match &mut self.groups[g as usize].generator {
            Generator::Gap {
                next_step,
                remaining,
                picked,
                ..
            } => {
                let mut back = vec![false; self.miners.len()];
                for m in &slots {
                    back[*m as usize] = true;
                }
                picked.retain(|m| !back[*m as usize]);
                *remaining += slots.len() as u64;
                *next_step = (*next_step).max(now_step);
            }
            Generator::List { .. } => {
                if self.spec.crypto != CryptoMode::Fast {
                    // Stand-in and real modes compute each slot's chain once (V0 only).
                    return;
                }
                let refresh = u64::from(self.res.round_refreshes as u32) + 1;
                let mut fresh = Vec::with_capacity(slots.len());
                for m in &slots {
                    let u = self.keyed.uniforms(
                        Purpose::Round,
                        (u64::from(g) << 24) | u64::from(*m),
                        refresh,
                    );
                    let n = geometric_failures(1.0 - u[0], p);
                    if n < 100_000 {
                        fresh.push((now_step + n, *m));
                    }
                }
                if let Generator::List { items, pos } = &mut self.groups[g as usize].generator {
                    let mut rest: Vec<(u64, u32)> = items.split_off(*pos);
                    rest.extend(fresh);
                    rest.sort();
                    items.extend(rest);
                }
            }
        }
        let grp = &mut self.groups[g as usize];
        grp.origin = now_step;
        grp.refresh_at = now;
        self.res.round_refreshes += 1;
        self.schedule_wake(now, g);
    }

    /// Unsettled blocks at `height` or above.
    fn blocks_from(&self, height: u64) -> Vec<u32> {
        self.unsettled
            .iter()
            .copied()
            .filter(|b| self.blocks[*b as usize].height >= height)
            .collect()
    }

    /// Turns a drawn (slot, step) into a candidate if the slot really mines this round, and
    /// schedules its decision. Returns the candidate.
    ///
    /// Under start rule (r) a miner's chain starts at its own attested entropy issue time,
    /// which can be later than the round's start, so its step can be found later than the
    /// draw order suggests. Its remaining checks then wait for its own found time
    /// (`Ev::Found`), as they would for a draw at that time under rule (a).
    pub(crate) fn materialise(&mut self, now: Micros, g: u32, m: u32, step: u64) -> Option<u32> {
        let prep = self.prepare(now, g, m, step)?;
        if self.spec.start_rule == ConfigStartRule::R {
            let slack = self.max_skew + self.p.signing_tolerance + 1.0;
            let at = micros(((prep.t0 + step) as f64 - slack).max(0.0));
            if at > now {
                self.res.deferred += 1;
                let d = match self.deferred_free.pop() {
                    Some(d) => {
                        self.deferred[d as usize] = Some(prep);
                        d
                    }
                    None => {
                        self.deferred.push(Some(prep));
                        (self.deferred.len() - 1) as u32
                    }
                };
                self.q.push(at, Ev::Found { d });
                return None;
            }
        }
        self.complete(now, prep, false)
    }

    /// Start rule (r): a deferred candidate reaches its own found time.
    pub(crate) fn on_found(&mut self, now: Micros, d: u32) {
        let Some(prep) = self.deferred[d as usize].take() else {
            return;
        };
        self.deferred_free.push(d);
        let g = prep.g;
        if self.groups[g as usize].closed {
            // Blocks two heights on are everywhere: this round is over.
            self.drop(6);
            return;
        }
        let beyond = self.groups[g as usize].horizon_at != INF;
        if let Some(c) = self.complete(now, prep, true) {
            self.candidates[c as usize].beyond_horizon = beyond;
        }
    }

    /// The checks of a drawn slot up to its entropy: whether it mines this round, on which
    /// block, and when its chain starts and it finds its step.
    fn prepare(&mut self, now: Micros, g: u32, m: u32, step: u64) -> Option<Prep> {
        let (parent, height, t0_round, origin, refresh_at) = {
            let grp = &self.groups[g as usize];
            (grp.parent, grp.height, grp.t0, grp.origin, grp.refresh_at)
        };
        // After a refresh (SIM-48), rule (r) counts a slot's chain from the refresh: its next
        // success after the refresh is as likely at every step as a fresh chain's first.
        let step = if self.spec.start_rule == ConfigStartRule::R {
            step.saturating_sub(origin)
        } else {
            step
        };
        let miner = self.miners[m as usize].clone();
        if !matches!(miner.phase, Phase::Mining | Phase::Waiting) || miner.idle || miner.kwc == NONE
        {
            self.drop(0);
            if miner.phase != Phase::Gone {
                self.groups[g as usize].transient.push(m);
            }
            return None;
        }
        let node = self.node_of(miner.household);
        // When the miner has the parent.
        let (a_parent, late) = self.accept(&node, parent);
        if a_parent == INF || late {
            self.drop(1);
            return None;
        }
        // The miner signs its header for this height when it first holds a block at the
        // parent's height and is mining; it signs on the best such block it holds then. Under
        // the literal rule (one header per height) it cannot sign again at this height: if a
        // better block arrives before it finds its step, it sits out until the next block.
        // Under the [P] rule it re-signs on its preferred tip.
        let mut level: Vec<(u32, Micros)> = Vec::new();
        for s in self.blocks_at(height - 1) {
            let (a_s, late_s) = self.accept(&node, s);
            if a_s != INF && !late_s {
                level.push((s, a_s));
            }
        }
        let first = level.iter().map(|x| x.1).min().unwrap_or(a_parent);
        let sign_time = first.max(miner.mine_at);
        // The header it signed at `sign_time` is on the best block it held then, which may
        // not be this round's parent (the parent may have arrived later).
        let mut committed: Option<(u32, Micros)> = None;
        let mut displaced_at = INF;
        for &(s, a_s) in &level {
            if a_s <= sign_time
                && committed.is_none_or(|(c, a_c)| self.prefer_block(s, a_s, c, a_c))
            {
                committed = Some((s, a_s));
            }
            if s != parent && self.prefer_block(s, a_s, parent, a_parent) {
                displaced_at = displaced_at.min(a_s);
            }
        }
        if self.spec.equivocation == EquivocationScope::SameHeight
            && committed.is_some_and(|(c, _)| c != parent)
        {
            // Its header at this height is on another block: that round has this slot.
            self.drop(2);
            return None;
        }
        let a_parent = if self.spec.equivocation == EquivocationScope::SameHeight {
            sign_time.max(a_parent)
        } else {
            a_parent
        };
        let mut start = a_parent.max(miner.mine_at).max(refresh_at);
        let hh = &self.hh[miner.household as usize];
        if !hh.online() {
            let back = hh.back_at(now);
            if !miner.ghost || back == u64::MAX {
                self.drop(3);
                self.groups[g as usize].transient.push(m);
                return None;
            }
            start = start.max(back);
        }
        let k = miner.kwc;
        let Some(timing) = self.entropy_ready(&node, m, k, start, parent, g) else {
            self.res.quorum_failures += 1;
            self.drop(4);
            self.groups[g as usize].transient.push(m);
            return None;
        };
        if timing.stalled {
            self.res.stalled_candidates += 1;
        }
        let t0 = if self.spec.start_rule == ConfigStartRule::R {
            // Rule (r): steps count from the attested issue time, rounded up to a whole second.
            let issued = timing.issued_s.max(0.0).ceil() as u64;
            let t0 = chain_start(
                self.blocks[parent as usize].timestamp,
                StartRule::R { issued },
            );
            if t0 > t0_round {
                self.res.late_starts += 1;
                self.res.late_start_s_sum += (t0 - t0_round) as f64;
            }
            t0
        } else {
            t0_round
        };
        let found = (micros((t0 + step) as f64)).max(timing.ready);
        Some(Prep {
            g,
            m,
            k,
            step,
            node,
            displaced_at,
            e_ready: timing.ready,
            t0,
            found,
            entropy_flags: timing.signers,
            entropy_quorum: timing.quorum,
        })
    }

    /// The checks of a drawn slot from its found time on, and its candidate. `deferred`: the
    /// checks run at the candidate's own found time (start rule (r)), so the miner's state and
    /// the blocks at the parent's height are read again.
    fn complete(&mut self, now: Micros, prep: Prep, deferred: bool) -> Option<u32> {
        let Prep {
            g,
            m,
            k,
            step,
            node,
            mut displaced_at,
            e_ready,
            t0,
            found,
            entropy_flags,
            entropy_quorum,
        } = prep;
        let (parent, height, target) = {
            let grp = &self.groups[g as usize];
            (grp.parent, grp.height, grp.target)
        };
        if deferred {
            let miner = &self.miners[m as usize];
            if !matches!(miner.phase, Phase::Mining | Phase::Waiting)
                || miner.idle
                || miner.kwc != k
            {
                self.drop(0);
                return None;
            }
            let hh = &self.hh[miner.household as usize];
            if !hh.online() && (!miner.ghost || hh.back_at(now) == u64::MAX) {
                self.drop(3);
                return None;
            }
            // A better block at the parent's height broadcast since the draw ends the round
            // for it too, if it arrived before the step was found.
            let (a_parent, _) = self.accept(&node, parent);
            for s in self.blocks_at(height - 1) {
                if s == parent {
                    continue;
                }
                let (a_s, late_s) = self.accept(&node, s);
                if a_s != INF && !late_s && self.prefer_block(s, a_s, parent, a_parent) {
                    displaced_at = displaced_at.min(a_s);
                }
            }
        }
        if displaced_at <= found {
            if self.spec.equivocation == EquivocationScope::SameHeight {
                self.res.sat_out += 1;
            }
            self.drop(5);
            return None;
        }
        // A block the miner accepted before finding its step that beats its candidate (a
        // longer chain, or a better block at this height under the fork rule) ended the round
        // for it; a worse block at this height does not, since its own block would win.
        let timestamp = t0 + step;
        for x in self.blocks_from(height) {
            let (a_x, late_x) = self.accept(&node, x);
            if a_x != INF && !late_x && a_x <= found && self.beats_step(x, timestamp, height) {
                self.drop(6);
                return None;
            }
        }
        let entropy_members = self.round_members(g, k);
        let Some((broadcast, signers)) = self.powit_time(&node, m, k, t0, step, e_ready, g) else {
            self.res.quorum_failures += 1;
            self.drop(7);
            return None;
        };
        let prev = self.blocks[parent as usize].hash;
        let header = self.header_for(m, height, prev, k, &target);
        // Stand-in and real modes computed this slot's chain from its header at the start of
        // the round; a slot whose header has changed since (it re-admitted to another KWC, or
        // won and took a new key) has no chain on that header and sits the round out.
        if let Some(original) = self.groups[g as usize].headers.get(m as usize) {
            if *original != header {
                self.drop(0);
                return None;
            }
        }
        let entropy = match self.groups[g as usize].entropies.get(m as usize) {
            Some(e) => *e,
            None => self.standin_entropy(g, m),
        };
        let hash = block_hash(&header, &entropy, step);
        let candidate = Candidate {
            miner: m,
            group: g,
            step,
            t0,
            timestamp,
            broadcast,
            header,
            entropy,
            hash,
            beyond_horizon: false,
            done: false,
            release_at: INF,
            signers,
            members: entropy_members,
            entropy_signers: entropy_flags,
            entropy_quorum,
            uses: 0,
        };
        let c = self.new_candidate(candidate);
        let uses = self.candidates[c as usize].uses;
        self.q.push(broadcast.max(now), Ev::Decide { c, uses });
        Some(c)
    }

    /// Stores a new candidate, in a slot that is done if one is free.
    fn new_candidate(&mut self, mut candidate: Candidate) -> u32 {
        match self.free_candidates.pop() {
            Some(c) => {
                candidate.uses = self.candidates[c as usize].uses.wrapping_add(1);
                self.candidates[c as usize] = candidate;
                c
            }
            None => {
                self.candidates.push(candidate);
                (self.candidates.len() - 1) as u32
            }
        }
    }

    /// Marks candidate `c` done and frees its slot (its member lists go with it).
    fn retire(&mut self, c: u32) {
        let cand = &mut self.candidates[c as usize];
        cand.done = true;
        cand.members = Vec::new();
        cand.signers = Vec::new();
        cand.entropy_signers = Vec::new();
        self.free_candidates.push(c);
    }

    /// One-way direct delay between two households, keyed by (round, slot, member, leg).
    fn direct(&mut self, a: &Node, b: &Node, g: u32, m: u32, leg: u64) -> f64 {
        let entity = (u64::from(g) << 24) ^ u64::from(m) ^ (b.key << 32);
        let u = self
            .keyed
            .uniforms(Purpose::Direct, entity & 0x00ff_ffff_ffff_ffff, leg);
        self.net
            .direct_delay(a.region, a.access, b.region, b.access, u[0], 1.0 - u[1])
    }

    /// When the miner's entropy is issued and when the miner holds it (SPEC §3.4). `None`
    /// when the KWC cannot issue it.
    ///
    /// All online members (the protocol rule): every recorded-online member must have the
    /// parent and sign; a member that disappeared without announcing it holds the entropy
    /// until it returns or One Chance records it offline. `None` when fewer recorded-online
    /// members than the quorum remain.
    ///
    /// Threshold (the M3a.1 experiment E3): the seated members reachable when the miner asks
    /// sign once they hold the parent, and any t of them give the same group signature, so
    /// absent members are not waited for; one that returns mid-round signs from the next
    /// round. `None` when fewer than t can sign.
    fn entropy_ready(
        &mut self,
        node: &Node,
        m: u32,
        k: u32,
        start: Micros,
        parent: u32,
        g: u32,
    ) -> Option<EntropyTiming> {
        let members = self.round_members(g, k);
        if let Some(rule) = self.threshold_rule() {
            let seated = members
                .iter()
                .filter(|w| !self.witnesses[**w as usize].removed())
                .count();
            let t = entropy_threshold(seated, rule).unwrap_or(seated);
            let mut signed: Vec<(Micros, f64, usize)> = Vec::with_capacity(members.len());
            for (i, w) in members.iter().enumerate() {
                if self.witnesses[*w as usize].removed() || !self.witness_reachable(*w) {
                    continue;
                }
                let h = self.witnesses[*w as usize].household;
                let member = self.node_of(h);
                let out = self.direct(node, &member, g, m, 2 * i as u64);
                let back = self.direct(&member, node, g, m, 2 * i as u64 + 1);
                let (a_p, _) = self.accept(&member, parent);
                if a_p == INF {
                    continue;
                }
                let at = (start + micros(out)).max(a_p);
                signed.push((at + micros(back), seconds(at) + member.skew, i));
            }
            if t == 0 || signed.len() < t {
                return None;
            }
            // The miner holds the entropy once t signatures have arrived; the KWC attests the
            // time the t-th was made.
            signed.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
            let mut flags = vec![false; members.len()];
            for s in signed.iter().take(t) {
                flags[s.2] = true;
            }
            let ready = signed[t - 1].0.max(start);
            let mut clocks: Vec<f64> = signed.iter().map(|s| s.1).collect();
            clocks.sort_by(f64::total_cmp);
            return Some(EntropyTiming {
                ready,
                stalled: false,
                issued_s: clocks[t - 1],
                signers: flags,
                quorum: t as u32,
            });
        }
        let recorded: Vec<u32> = members
            .iter()
            .copied()
            .filter(|w| {
                let wit = &self.witnesses[*w as usize];
                wit.recorded && !wit.removed()
            })
            .collect();
        if recorded.len() < self.p.quorum_unregistered {
            return None;
        }
        let mut ready = start;
        let mut stalled = false;
        let mut issued_s = seconds(start);
        let mut flags = vec![false; members.len()];
        let mut signers = 0;
        for (i, w) in recorded.iter().enumerate() {
            let h = self.witnesses[*w as usize].household;
            let member = self.node_of(h);
            let out = self.direct(node, &member, g, m, 2 * i as u64);
            let back = self.direct(&member, node, g, m, 2 * i as u64 + 1);
            let (signed, clock, signs) = if self.witness_reachable(*w) {
                let (a_p, _) = self.accept(&member, parent);
                if a_p == INF {
                    continue;
                }
                let at = (start + micros(out)).max(a_p);
                (at, seconds(at) + member.skew, true)
            } else {
                stalled = true;
                let wit = &self.witnesses[*w as usize];
                let back_at = self.witness_back_at(*w, start);
                let record = if wit.ghost { wit.check_at } else { INF };
                if record < back_at {
                    // Recorded offline: a KWC decision completes the required set, without
                    // this member's signature.
                    (record, seconds(record), false)
                } else {
                    (back_at, seconds(back_at) + member.skew, true)
                }
            };
            if signed == INF {
                return None;
            }
            if signs {
                signers += 1;
                if let Some(at) = members.iter().position(|x| x == w) {
                    flags[at] = true;
                }
            }
            ready = ready.max(signed + micros(back));
            issued_s = issued_s.max(clock);
        }
        // Members recorded offline while the miner waited leave the online set; if fewer than
        // the quorum remain to sign, the KWC cannot issue entropy this round (SPEC §3.4).
        if signers < self.p.quorum_unregistered {
            return None;
        }
        Some(EntropyTiming {
            ready,
            stalled,
            issued_s,
            signers: flags,
            quorum: self.p.quorum_unregistered as u32,
        })
    }

    /// The threshold entropy rule of the run, if it uses one.
    pub(crate) fn threshold_rule(&self) -> Option<EntropySigners> {
        match self.spec.entropy_signers {
            EntropySigners::AllOnlineMembers => None,
            rule => Some(rule),
        }
    }

    /// PoWit signatures a block needs: the unregistered quorum (27 of 40), or just over half
    /// of the seats under the majority threshold (21 of 40; split quorum: just over half for
    /// witness signatures, two-thirds for decisions).
    pub(crate) fn powit_quorum(&self) -> usize {
        match self.spec.entropy_signers {
            EntropySigners::ThresholdMajority => self.p.unregistered.seats() / 2 + 1,
            _ => self.p.quorum_unregistered,
        }
    }

    /// When the miner holds a PoWit quorum (SPEC §3.6): each online member signs once its own
    /// clock reaches `t0 + N` (minus the signing tolerance of the run's rule) and it has the
    /// entropy; the miner aggregates the first quorum to arrive.
    #[allow(clippy::too_many_arguments)]
    fn powit_time(
        &mut self,
        node: &Node,
        m: u32,
        k: u32,
        t0: u64,
        step: u64,
        e_ready: Micros,
        g: u32,
    ) -> Option<(Micros, Vec<u32>)> {
        let members = self.round_members(g, k);
        let mut arrivals: Vec<(Micros, u32)> = Vec::with_capacity(members.len());
        for (i, w) in members.iter().enumerate() {
            if self.witnesses[*w as usize].removed() {
                continue;
            }
            let h = self.witnesses[*w as usize].household;
            let member = self.node_of(h);
            let out = self.direct(node, &member, g, m, 100 + 2 * i as u64);
            let back = self.direct(&member, node, g, m, 101 + 2 * i as u64);
            let mut sign = (e_ready + micros(out)).max(micros(
                ((t0 + step) as f64 - self.p.signing_tolerance - member.skew).max(0.0),
            ));
            if !self.witness_reachable(*w) {
                let back_at = self.witness_back_at(*w, e_ready);
                if back_at == u64::MAX {
                    continue;
                }
                sign = sign.max(back_at);
            }
            arrivals.push((sign + micros(back), *w));
        }
        let quorum = self.powit_quorum();
        if arrivals.len() < quorum {
            return None;
        }
        arrivals.sort();
        let at = arrivals[quorum - 1].0 + 1_000;
        Some((at, arrivals.iter().take(quorum).map(|a| a.1).collect()))
    }

    fn drop(&mut self, reason: usize) {
        if self.res.drops.len() < crate::job::DROP_REASONS.len() {
            self.res.drops.resize(crate::job::DROP_REASONS.len(), 0);
        }
        self.res.drops[reason] += 1;
    }

    /// True when block `x` beats a candidate with `timestamp` at `height` whose hash is not
    /// known yet (at the same timestamp it counts as beaten: the hash decides at random).
    fn beats_step(&self, x: u32, timestamp: u64, height: u64) -> bool {
        let block = &self.blocks[x as usize];
        if block.height > height {
            return true;
        }
        if block.height < height {
            return false;
        }
        match self.spec.fork_rule {
            ForkRule::V06 => block.timestamp <= timestamp,
            ForkRule::FirstSeen | ForkRule::LowerHashOnly => true,
        }
    }

    /// True when block `x` beats candidate `c` at a node that saw `x` first.
    fn beats(&self, x: u32, c: &Candidate, height: u64) -> bool {
        let block = &self.blocks[x as usize];
        if block.height > height {
            return true;
        }
        if block.height < height {
            return false;
        }
        match self.spec.fork_rule {
            ForkRule::V06 => {
                prefer((block.timestamp, &block.hash), (c.timestamp, &c.hash)) == Ordering::Less
            }
            ForkRule::FirstSeen => true,
            ForkRule::LowerHashOnly => block.hash < c.hash,
        }
    }

    pub(crate) fn on_decide(&mut self, now: Micros, c: u32, uses: u32) {
        if self.candidates[c as usize].uses != uses || self.candidates[c as usize].done {
            return;
        }
        let cand = self.candidates[c as usize].clone();
        let m = cand.miner;
        let h = self.miners[m as usize].household;
        if !self.hh[h as usize].online() {
            let back = self.hh[h as usize].back_at(now);
            if self.miners[m as usize].ghost && back != u64::MAX {
                self.q.push(back, Ev::Decide { c, uses });
            } else {
                self.retire(c);
            }
            return;
        }
        let node = self.node_of(h);
        let height = self.groups[cand.group as usize].height;
        for x in self.blocks_from(height) {
            let (a_x, late) = self.accept(&node, x);
            if a_x != INF && !late && a_x <= now && self.beats(x, &cand, height) {
                self.res.abandoned += 1;
                self.drop(8);
                self.retire(c);
                return;
            }
        }
        if self.spec_strategy(m) == Some(Strategy::Withhold) {
            self.res.withheld += 1;
            self.withheld.push(c);
            self.schedule_release(c);
            return;
        }
        self.broadcast(now, c, false);
    }

    fn broadcast(&mut self, now: Micros, c: u32, withheld: bool) {
        self.candidates[c as usize].done = true;
        let cand = self.candidates[c as usize].clone();
        let m = cand.miner;
        // The block must pass the protocol's checks at its own miner.
        let parent = self.groups[cand.group as usize].parent;
        let pb = &self.blocks[parent as usize];
        let tip = Tip {
            hash: pb.hash,
            height: pb.height,
            timestamp: pb.timestamp,
        };
        let seats = self.p.unregistered.seats();
        let on: Vec<bool> = cand
            .members
            .iter()
            .map(|w| cand.signers.contains(w))
            .collect();
        let bits = crate::world::bitfield_of(&on);
        let entropy_bits = crate::world::bitfield_of(&cand.entropy_signers);
        let facts = BlockFacts {
            header: &cand.header,
            entropy: cand.entropy,
            entropy_signers: &entropy_bits,
            winning_step: cand.step,
            timestamp: cand.timestamp,
            powit_signers: &bits,
        };
        let rules = Rules {
            clock_tolerance_s: self.spec.delta,
            powit_deadline_s: None,
            start_rule: if self.spec.start_rule == ConfigStartRule::R {
                StartRule::R { issued: cand.t0 }
            } else {
                StartRule::A
            },
            max_steps: CHAIN_CAP.max(1_000_000),
        };
        let check = if self.spec.crypto == CryptoMode::Fast {
            ChainCheck::Trusted
        } else {
            ChainCheck::Recompute
        };
        let reception = Reception {
            block_at: (cand.timestamp as f64).max(now as f64 / SECOND as f64),
            powit_at: now as f64 / SECOND as f64,
        };
        // The entropy needed the quorum of online members or, under a threshold, t of the
        // seated members; the PoWit needs its own quorum.
        let quorum = if cand.entropy_quorum as usize == self.p.quorum_unregistered {
            self.p.unregistered
        } else {
            QuorumRule::Pool {
                seats,
                quorum: cand.entropy_quorum as usize,
            }
        };
        let powit_rule = QuorumRule::Pool {
            seats,
            quorum: self.powit_quorum(),
        };
        let verdict = validate_facts(&facts, &tip, &quorum, &reception, &rules, &|_| true, check)
            .and_then(|hash| check_powit_quorum(&powit_rule, &bits).map(|()| hash));
        if verdict.is_err() && seats == on.len() {
            self.res.self_invalid += 1;
        }
        if self.spec.crypto == CryptoMode::Real {
            crate::real::validate_real(self, c, &tip);
        }
        self.add_block(now, c, withheld);
        self.retire(c);
        // An old-design restarter draws its next win after this one.
        if let Some(Strategy::Restart {
            old_design: true,
            window_s,
        }) = self.spec_strategy(m)
        {
            self.advance_connection_sequence(
                m,
                cand.timestamp,
                1_000 + u64::from(self.miners[m as usize].conn),
            );
            let next = self.miners[m as usize].next_success;
            if (next.saturating_sub(now / SECOND)) as f64 > window_s {
                self.restart(now, m);
            }
        }
        // Withholders release when they see the next honest block at their height.
        if !withheld {
            let pending = self.withheld.clone();
            for w in pending {
                self.schedule_release(w);
            }
        }
    }

    /// Schedules a withheld block's release: when its miner accepts another block at its
    /// height (the next honest block), or after two rounds if none comes.
    fn schedule_release(&mut self, c: u32) {
        let cand = self.candidates[c as usize].clone();
        let height = self.groups[cand.group as usize].height;
        let node = self.node_of(self.miners[cand.miner as usize].household);
        let mut release = cand.broadcast + micros(2.0 * self.p.interval_s as f64);
        for x in self.blocks_at(height) {
            if self.blocks[x as usize].withheld {
                continue;
            }
            let (a, _) = self.accept(&node, x);
            if a != INF {
                release = release.min(a);
            }
        }
        if release < self.candidates[c as usize].release_at {
            self.candidates[c as usize].release_at = release;
            let uses = self.candidates[c as usize].uses;
            self.q.push(release, Ev::Release { c, uses });
        }
    }

    pub(crate) fn on_release(&mut self, now: Micros, c: u32, uses: u32) {
        if self.candidates[c as usize].uses != uses
            || self.candidates[c as usize].release_at != now
            || !self.withheld.contains(&c)
        {
            return;
        }
        self.withheld.retain(|x| *x != c);
        self.broadcast(now, c, true);
    }

    /// S6: moves hopping groups between rest, idle and active windows at window boundaries of
    /// the network chain.
    pub(crate) fn apply_hops(&mut self, now: Micros) {
        let height = if self.blocks.is_empty() {
            0
        } else {
            self.network_height()
        };
        for (i, spec) in self.spec.groups.iter().enumerate() {
            let Strategy::Hop {
                window_blocks,
                idle_windows,
                rest_windows,
            } = spec.strategy
            else {
                continue;
            };
            let g = i + 1;
            // A cycle is `rest_windows` windows away (the first ones let Variant A settle at the
            // honest slots), then `idle_windows` counted but idle, then one active window.
            let window = height / u64::from(window_blocks.max(1));
            let cycle = u64::from(rest_windows) + u64::from(idle_windows) + 1;
            let pos = window % cycle;
            let state = if pos < u64::from(rest_windows) {
                0
            } else if pos < u64::from(rest_windows) + u64::from(idle_windows) {
                1
            } else {
                2
            };
            if self.hop_state[g] == state && now > 0 {
                continue;
            }
            let slots: Vec<u32> = (0..self.miners.len() as u32)
                .filter(|m| self.miners[*m as usize].group as usize == g)
                .collect();
            let olds: Vec<_> = slots.iter().map(|m| self.miner_status(*m)).collect();
            self.hop_state[g] = state;
            for (m, old) in slots.iter().zip(olds) {
                self.miners[*m as usize].idle = state == 1;
                self.commit_miner(now, *m, old);
            }
            for m in slots {
                let h = self.miners[m as usize].household;
                if state == 0 {
                    self.set_cause(now, h, bit::SCRIPT, true, true, u64::MAX);
                } else {
                    self.set_cause(now, h, bit::SCRIPT, false, true, now);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::JobSpec;
    use gb_config::Config;

    fn small(cfg: &Config) -> JobSpec {
        let mut spec = JobSpec::from_config(cfg, "round-test", "T", 7).idealised();
        spec.miners = 4;
        spec.registered_ids = 400;
        spec
    }

    /// The first member of miner 0's KWC in round 0 disappears at time 0 without announcing
    /// it: under the all-online-members rule its entropy waits for the One Chance record.
    fn stalled_round(rule: ConfigStartRule) -> (Prep, Micros) {
        let cfg = Config::default();
        let mut spec = small(&cfg);
        spec.start_rule = rule;
        spec.entropy_signers = EntropySigners::AllOnlineMembers;
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let k = sim.miners[0].kwc;
        let w = sim.round_members(0, k)[0];
        let h = sim.witnesses[w as usize].household;
        sim.set_cause(0, h, bit::CRASH, true, false, u64::MAX);
        let record = sim.witnesses[w as usize].check_at;
        (sim.prepare(0, 0, 0, 5).unwrap(), record)
    }

    #[test]
    fn under_rule_r_a_late_entropy_starts_the_chain_late_and_catches_up_nothing() {
        // Rule (a): the chain starts one second after genesis whenever the entropy comes, so
        // the miner computes the missed steps at once (catch-up) and its timestamp is early.
        let (a, record) = stalled_round(ConfigStartRule::A);
        assert_eq!(a.t0, 1);
        assert_eq!(a.e_ready, record);
        assert_eq!(a.found, record);
        assert!(micros((a.t0 + 5) as f64) < a.found);
        // Rule (r): steps count from the attested issue time (the record, here), so the step
        // is found five seconds after it and the timestamp says so.
        let (r, record) = stalled_round(ConfigStartRule::R);
        assert_eq!(r.t0, (record as f64 / SECOND as f64).ceil() as u64);
        assert_eq!(r.found, micros((r.t0 + 5) as f64));
        assert!(r.found >= r.e_ready);
    }

    #[test]
    fn rule_r_defers_a_late_candidate_to_its_own_found_time() {
        let cfg = Config::default();
        let mut spec = small(&cfg);
        spec.start_rule = ConfigStartRule::R;
        // A member away without announcing it delays the entropy (all-online-members rule).
        spec.entropy_signers = EntropySigners::AllOnlineMembers;
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let k = sim.miners[0].kwc;
        let w = sim.round_members(0, k)[0];
        let h = sim.witnesses[w as usize].household;
        sim.set_cause(0, h, bit::CRASH, true, false, u64::MAX);
        assert_eq!(sim.materialise(0, 0, 0, 5), None);
        assert_eq!(sim.res.deferred, 1);
        let mut found = None;
        for _ in 0..10_000 {
            match sim.q.pop() {
                Some((at, Ev::Found { d })) => {
                    found = Some((at, d));
                    break;
                }
                Some(_) => {}
                None => break,
            }
        }
        let (at, d) = found.expect("no deferred candidate");
        let prep = sim.deferred[d as usize].clone().unwrap();
        // It waits until shortly before its own found time.
        assert!(at < prep.found && prep.found - at < 5 * SECOND);
        let c = sim.complete(prep.found, prep, true).unwrap();
        let cand = &sim.candidates[c as usize];
        assert_eq!(cand.timestamp, cand.t0 + 5);
        assert!(cand.t0 > 1);
    }

    /// Miner 0's entropy in round 0 with the first `absent` members of its KWC away without
    /// announcing it, under `rule`; also the members and which ones are away.
    fn entropy_with_absences(
        rule: EntropySigners,
        absent: usize,
    ) -> (Option<EntropyTiming>, Vec<u32>, Vec<bool>) {
        let cfg = Config::default();
        let mut spec = small(&cfg);
        spec.entropy_signers = rule;
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let k = sim.miners[0].kwc;
        let members = sim.round_members(0, k);
        for w in members.iter().take(absent) {
            let h = sim.witnesses[*w as usize].household;
            sim.set_cause(0, h, bit::CRASH, true, false, u64::MAX);
        }
        let away: Vec<bool> = members.iter().map(|w| !sim.witness_reachable(*w)).collect();
        let node = sim.node_of(sim.miners[0].household);
        (sim.entropy_ready(&node, 0, k, 0, 0, 0), members, away)
    }

    #[test]
    fn threshold_entropy_waits_for_no_absent_member_and_needs_t_reachable_ones() {
        // 13 of 40 away: 27 reachable, so the two-thirds threshold issues at once, from
        // reachable members only.
        let (timing, members, away) = entropy_with_absences(EntropySigners::ThresholdTwoThirds, 13);
        assert_eq!(members.len(), 40);
        assert_eq!(away.iter().filter(|a| **a).count(), 13);
        let t = timing.unwrap();
        assert!(!t.stalled);
        assert_eq!((t.quorum, t.ready), (27, 0));
        let flags = t.signers;
        assert_eq!(flags.iter().filter(|f| **f).count(), 27);
        assert!(flags.iter().zip(&away).all(|(f, a)| !(*f && *a)));
        // The same absences stall the entropy under the all-online-members rule.
        let (all, _, _) = entropy_with_absences(EntropySigners::AllOnlineMembers, 13);
        assert!(all.unwrap().stalled);
        // One more away: 26 reachable, below t.
        let (none, _, _) = entropy_with_absences(EntropySigners::ThresholdTwoThirds, 14);
        assert!(none.is_none());
    }

    #[test]
    fn all_online_entropy_needs_the_quorum_left_after_absent_members_are_recorded_offline() {
        // 13 of 40 crashed for good: once they are recorded offline, 27 online members sign,
        // and the entropy records exactly those signatures.
        let (timing, _, away) = entropy_with_absences(EntropySigners::AllOnlineMembers, 13);
        let t = timing.unwrap();
        assert!(t.stalled);
        assert_eq!(t.signers.iter().filter(|f| **f).count(), 27);
        assert!(t.signers.iter().zip(&away).all(|(f, a)| !(*f && *a)));
        // 14 crashed: recording them offline leaves 26 online members, below the quorum, so
        // the KWC cannot issue entropy (until 2026-10-16 the entropy completed with 26).
        let (none, _, _) = entropy_with_absences(EntropySigners::AllOnlineMembers, 14);
        assert!(none.is_none());
    }

    #[test]
    fn threshold_entropy_fails_below_t_and_a_returning_member_signs_again() {
        let cfg = Config::default();
        let mut spec = small(&cfg);
        spec.entropy_signers = EntropySigners::ThresholdMajority;
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let k = sim.miners[0].kwc;
        let members = sim.round_members(0, k);
        let node = sim.node_of(sim.miners[0].household);
        let mut down = Vec::new();
        for w in &members {
            if members
                .iter()
                .filter(|x| sim.witness_reachable(**x))
                .count()
                < 21
            {
                break;
            }
            let h = sim.witnesses[*w as usize].household;
            if sim.hh[h as usize].online() {
                sim.set_cause(0, h, bit::CRASH, true, false, u64::MAX);
                down.push(h);
            }
        }
        // Fewer than 21 of 40 reachable: no entropy this round.
        assert!(sim.entropy_ready(&node, 0, k, 0, 0, 0).is_none());
        assert_eq!(sim.powit_quorum(), 21);
        // The last member back: the next request has 21 again.
        let h = *down.last().unwrap();
        sim.set_cause(SECOND, h, bit::CRASH, false, false, SECOND);
        sim.open_group(0, SECOND);
        let t = sim.entropy_ready(&node, 0, k, SECOND, 0, 1).unwrap();
        assert_eq!(t.quorum, 21);
    }
}
