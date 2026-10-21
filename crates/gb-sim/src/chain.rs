//! Blocks, difficulty, per-node views, the validator panel and the network chain.
//!
//! - **Views.** A node accepts a block when it has received it and its parent, and the block
//!   passes the node's timing checks (`check_received` for early blocks, `check_powit_deadline`
//!   for late PoWits, on the node's own clock). Arrival times are drawn lazily by key.
//! - **Panel.** 101 ideal validators, spread over regions like the population, stand for the
//!   network: the network chain is the longest chain that more than half of them hold (their
//!   tips and the tips' ancestors). An ID passes confirmation depth when 6 blocks are built on
//!   its block on the network chain, and placements take effect at the beacon block
//!   (SPEC §4.2).
//! - **Fork choice** at every node uses the gb-protocol comparator of the run's rule.

use crate::engine::{Micros, SECOND, micros};
use crate::job::ForkRule;
use crate::rng::Purpose;
use crate::sim::{Ev, Sim};
use crate::world::{NONE, Phase};
use gb_config::protocol::{DifficultyCountRule, DifficultyVariant, EquivocationScope};
use gb_protocol::encoding::Header;
use gb_protocol::hash::{Hash, tagged};
use gb_protocol::issuance::{
    PPM, count_based_target, update_correction_ppm, variant_a_retarget, variant_b_target,
};
use gb_protocol::validate::{
    ForkCandidate, accepts_chain_through_late_block, check_powit_deadline, check_received,
    fork_choice, fork_choice_first_seen, fork_choice_lower_hash, reevaluates_early_block,
};
use std::cmp::Ordering;

/// The E3 diagnosis of one gap between final-chain blocks (broadcast at `from` and `to`) from
/// the samples taken in it: each sample's cause (see [`crate::job::GAP_CAUSES`]), their
/// shares, the most common one (the first of the order on a tie), and the mean expected time
/// to a block with the miners holding entropy alone.
pub(crate) fn diagnose_gap(
    from: Micros,
    to: Micros,
    samples: &[(Micros, u32, u32, f64)],
    interval_s: f64,
) -> crate::job::GapDiagnosis {
    let mut counts = [0u64; 4];
    let (mut expected_sum, mut expected_n) = (0.0, 0u64);
    for (_, mining, with_entropy, p) in samples.iter().filter(|s| s.0 >= from && s.0 <= to) {
        let cause = if *mining == 0 {
            0
        } else if *with_entropy == 0 {
            1
        } else {
            // 1 − (1 − p)^n, accurately for small p.
            let win = -((-p).ln_1p() * f64::from(*with_entropy)).exp_m1();
            let expected = if win > 0.0 { 1.0 / win } else { f64::INFINITY };
            expected_sum += expected;
            expected_n += 1;
            if expected > 4.0 * interval_s { 2 } else { 3 }
        };
        counts[cause] += 1;
    }
    let total: u64 = counts.iter().sum();
    let mut cause = 3u8;
    let mut most = 0u64;
    for (i, c) in counts.iter().enumerate() {
        if *c > most {
            most = *c;
            cause = i as u8;
        }
    }
    crate::job::GapDiagnosis {
        start_s: from as f64 / SECOND as f64,
        length_s: to.saturating_sub(from) as f64 / SECOND as f64,
        cause,
        shares: counts.map(|c| {
            if total > 0 {
                c as f64 / total as f64
            } else {
                0.0
            }
        }),
        expected_interval_s: if expected_n > 0 {
            expected_sum / expected_n as f64
        } else {
            f64::NAN
        },
        samples: total,
    }
}

/// Not accepted (never received, or invalid at that node).
pub(crate) const INF: Micros = u64::MAX;

/// Hashes the 64-bit node keys of [`OwnCache`] with one multiplication. The keys are
/// distinct integers, and the map is only looked up, never iterated, so the hash cannot
/// change a result.
#[derive(Debug, Default)]
pub(crate) struct NodeKeyHasher(u64);

impl std::hash::Hasher for NodeKeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(8) ^ u64::from(*b)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        }
    }

    fn write_u64(&mut self, x: u64) {
        self.0 = x.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

/// What a node's acceptance of one block is known to be (see [`OwnCache`]).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Seen {
    /// The node's own reception: `judge` on its arrival, before the parent is taken into
    /// account. A pure function of the node and the block.
    own: Option<(Micros, bool)>,
    /// The full acceptance, with the parent chain, and the settled height it was computed
    /// at. It can change only when a block settles, which always raises the settled height.
    full: Option<(usize, Micros, bool)>,
}

/// Nodes' acceptances of a block, by node key, kept while the block is unsettled. Every value
/// is what the computation would give again, so caching it changes no result.
pub(crate) type OwnCache =
    std::collections::HashMap<u64, Seen, std::hash::BuildHasherDefault<NodeKeyHasher>>;

/// A PoW-ID block in the simulation.
#[derive(Debug, Clone)]
pub(crate) struct Block {
    pub parent: u32,
    pub height: u64,
    pub timestamp: u64,
    pub hash: Hash,
    pub miner: u32,
    pub group: u8,
    pub region: u8,
    pub household: u32,
    pub broadcast: Micros,
    pub withheld: bool,
    pub child_target: Hash,
    pub factor_ppm: u64,
    pub panel: Vec<Micros>,
    pub panel_late: Vec<bool>,
    /// Nodes' own receptions of this block, kept while it is unsettled.
    pub own: OwnCache,
    pub settled: bool,
    pub canonical: bool,
    pub confirmed: bool,
    pub hop_active: bool,
    pub beyond_horizon: bool,
    /// The witness registered from this block once it passed confirmation depth (`NONE`
    /// before, or after its ID was voided by a reorganisation).
    pub registered: u32,
    /// The round this block was mined in (`NONE` for genesis).
    pub round: u32,
    /// Under the lagged issued-entropy count: the count of its own round that the block
    /// commits (0 before it is set, and for genesis).
    pub committed: u64,
}

/// A view: the node whose acceptance is computed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Node {
    pub key: u64,
    pub household: u32,
    pub region: usize,
    pub access: f64,
    pub skew: f64,
}

/// A round: the candidates on one parent.
#[derive(Debug, Clone)]
pub(crate) struct Group {
    pub parent: u32,
    pub height: u64,
    pub t0: u64,
    pub target: Hash,
    pub p: f64,
    pub slots: u32,
    pub generator: crate::round::Generator,
    pub extras: Vec<(u64, u32)>,
    pub held: Option<(u64, Vec<u32>)>,
    pub entropies: Vec<Hash>,
    pub headers: Vec<Header>,
    pub real: Vec<Option<crate::real::RealEntropy>>,
    pub closed: bool,
    pub horizon_at: Micros,
    pub first_recorded: bool,
    pub eligible: u64,
    /// Member lists of the KWCs this round has used, fixed at first use (`round_members`).
    pub kwc_members: Vec<(u32, Vec<u32>)>,
    /// Miners whose KWC issued entropy for this round (count rule `IssuedEntropy`), counted
    /// when the round's first block is added.
    pub issued: Option<u64>,
}

/// A miner's candidate block, between its winning step and its decision to broadcast.
#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub miner: u32,
    pub group: u32,
    pub step: u64,
    /// Where its chain started: the round's start under rule (a), its own attested entropy
    /// issue time under rule (r).
    pub t0: u64,
    pub timestamp: u64,
    pub broadcast: Micros,
    pub header: Header,
    pub entropy: Hash,
    pub hash: Hash,
    pub beyond_horizon: bool,
    pub done: bool,
    pub release_at: Micros,
    pub signers: Vec<u32>,
    pub members: Vec<u32>,
    pub entropy_signers: Vec<bool>,
    /// Signatures its entropy needed: the quorum of online members, or the threshold t.
    pub entropy_quorum: u32,
    /// Uses of this candidate slot (slots are reused once done); events carry it.
    pub uses: u32,
}

impl Sim<'_> {
    // ------------------------------------------------------------------ genesis

    pub(crate) fn start_chain(&mut self) {
        let hash = tagged(b"GB/sim-genesis-block", &[&self.spec.seed.to_be_bytes()]);
        let n = self.count_for_difficulty();
        let target = match self.spec.frozen_target {
            Some(t) => t,
            None => match self.spec.difficulty {
                DifficultyVariant::CountBased => {
                    count_based_target(n, self.p.interval_s, self.spec.correction.as_ref(), PPM)
                }
                DifficultyVariant::BitcoinRetarget => variant_b_target(n, self.p.interval_s),
            },
        };
        let panel = self.panel.len();
        self.blocks.push(Block {
            parent: NONE,
            height: 0,
            timestamp: 0,
            hash,
            miner: NONE,
            group: 0,
            region: 0,
            household: NONE,
            broadcast: 0,
            withheld: false,
            child_target: target,
            factor_ppm: PPM,
            panel: vec![0; panel],
            panel_late: vec![false; panel],
            own: OwnCache::default(),
            settled: true,
            canonical: true,
            confirmed: true,
            hop_active: false,
            beyond_horizon: false,
            registered: NONE,
            round: NONE,
            committed: 0,
        });
        self.settled_ids.push(0);
        self.network_tip = 0;
        self.open_group(0, 0);
        self.q
            .push(micros(self.p.interval_s as f64), Ev::PanelUpdate);
    }

    /// Variant B's admitted online count under the run's count rule (SPEC §3.8). Under the
    /// issued-entropy rule this is the count before any round (genesis); rounds have their own
    /// ([`Sim::issued_count`]).
    pub(crate) fn count_for_difficulty(&self) -> u64 {
        match self.spec.count_rule {
            DifficultyCountRule::AfterPostAdmissionWait
            | DifficultyCountRule::IssuedEntropy
            | DifficultyCountRule::IssuedEntropyLagged => self.counts.after_wait,
            DifficultyCountRule::AllGloballyActive => self.counts.all,
        }
        .max(1)
    }

    /// Count rule `IssuedEntropy` (the M3a.1 experiment E2): the miners whose KWC issued
    /// entropy for round `g`, counted when the round's first block is added (at `now`) and
    /// kept for the round, so each branch has its own count. A miner counts when it mines
    /// (past its post-admission wait, not idle) with its household online, its KWC can
    /// issue entropy, and its header is on the round's parent: under one header per height a
    /// miner that signed on another block at that height sat the round out, and under the
    /// [P] rule a miner mines on the best block it holds. The block mined in round `g` sets
    /// the difficulty of the next round, so the count lags one round. Tier 1 takes the count
    /// as known exactly (ASSUMPTIONS SIM-36).
    pub(crate) fn issued_count(&mut self, g: u32, now: Micros) -> u64 {
        if let Some(n) = self.groups[g as usize].issued {
            return n;
        }
        let parent = self.groups[g as usize].parent;
        let height = self.blocks[parent as usize].height;
        let siblings: Vec<u32> = if height == 0 {
            Vec::new()
        } else {
            self.unsettled
                .iter()
                .copied()
                .filter(|b| *b != parent && self.blocks[*b as usize].height == height)
                .collect()
        };
        let mut able: Vec<Option<bool>> = vec![None; self.alloc.active_wcs() as usize];
        let mut n = 0u64;
        for m in 0..self.miners.len() as u32 {
            let (h, k, mine_at) = {
                let miner = &self.miners[m as usize];
                if miner.phase != Phase::Mining || miner.idle || miner.kwc == NONE {
                    continue;
                }
                (miner.household, miner.kwc, miner.mine_at)
            };
            if mine_at > now || !self.hh[h as usize].online() {
                continue;
            }
            let Some(slot) = able.get_mut(k as usize) else {
                continue;
            };
            let can = match *slot {
                Some(x) => x,
                None => {
                    let x = self.kwc_can_issue(k);
                    able[k as usize] = Some(x);
                    x
                }
            };
            if !can {
                continue;
            }
            if !siblings.is_empty() {
                let node = self.node_of(h);
                let mut level: Vec<(u32, Micros)> = Vec::new();
                for s in std::iter::once(parent).chain(siblings.iter().copied()) {
                    let (a_s, late) = self.accept(&node, s);
                    if a_s != INF && !late && a_s <= now {
                        level.push((s, a_s));
                    }
                }
                let Some(first) = level.iter().map(|x| x.1).min() else {
                    continue;
                };
                let by = if self.spec.equivocation == EquivocationScope::SameHeight {
                    first.max(mine_at)
                } else {
                    now
                };
                let mut chosen: Option<(u32, Micros)> = None;
                for &(s, a_s) in &level {
                    if a_s <= by && chosen.is_none_or(|(c, a_c)| self.prefer_block(s, a_s, c, a_c))
                    {
                        chosen = Some((s, a_s));
                    }
                }
                if chosen.map(|x| x.0) != Some(parent) {
                    continue;
                }
            }
            n += 1;
        }
        self.groups[g as usize].issued = Some(n);
        n
    }

    // ------------------------------------------------------------------ views

    /// The node of a household.
    pub(crate) fn node_of(&self, h: u32) -> Node {
        let hh = &self.hh[h as usize];
        Node {
            key: u64::from(h),
            household: h,
            region: hh.region as usize,
            access: hh.access,
            skew: hh.skew,
        }
    }

    /// The node of panel validator `i`.
    pub(crate) fn panel_node(&self, i: usize) -> Node {
        let v = &self.panel[i];
        Node {
            key: (1 << 24) - 1 - i as u64,
            household: NONE,
            region: v.region,
            access: v.access,
            skew: v.skew,
        }
    }

    /// When `node` receives block `b` (true time), before its own timing checks.
    fn arrival(&mut self, node: &Node, b: u32) -> Micros {
        let block = &self.blocks[b as usize];
        if node.household != NONE && node.household == block.household {
            return block.broadcast;
        }
        let (src, sent) = (block.region as usize, block.broadcast);
        let u = self.keyed.uniforms(
            Purpose::Gossip,
            (u64::from(b) << 24) | (node.key & 0xff_ffff),
            0,
        );
        let delay = self.net.gossip_delay(src, node.region, node.access, u[0]);
        self.net.deliver(sent, src, node.region, delay)
    }

    /// When `node` accepts block `b` as valid, or [`INF`]; also whether its PoWit was late
    /// there (a late block is never extended; under the soft scope a longer chain through it
    /// is still accepted).
    pub(crate) fn accept(&mut self, node: &Node, b: u32) -> (Micros, bool) {
        let block = &self.blocks[b as usize];
        if block.settled {
            return (if block.canonical { 0 } else { INF }, false);
        }
        let epoch = self.settled_ids.len();
        let seen = block.own.get(&node.key).copied().unwrap_or_default();
        if let Some((at_epoch, at, late)) = seen.full {
            if at_epoch == epoch {
                return (at, late);
            }
        }
        let parent = block.parent;
        let (parent_at, _) = self.accept(node, parent);
        let mut own = seen.own;
        let result = if parent_at == INF {
            (INF, false)
        } else {
            let (at, late) = match own {
                Some(o) => o,
                None => {
                    let arrived = self.arrival(node, b);
                    let o = self.judge(node, b, arrived);
                    own = Some(o);
                    o
                }
            };
            if at == INF {
                (INF, late)
            } else {
                (at.max(parent_at), late)
            }
        };
        let fresh = self.blocks[b as usize]
            .own
            .insert(
                node.key,
                Seen {
                    own,
                    full: Some((epoch, result.0, result.1)),
                },
            )
            .is_none();
        if fresh {
            self.own_entries += 1;
            if self.own_entries > self.p.cache_entries {
                self.clear_acceptance_caches();
            }
        }
        result
    }

    /// Empties every unsettled block's acceptance cache, which then refills as needed
    /// (`sim.acceptance_cache_entries`). Cached values are what recomputing them gives, so
    /// this bounds memory, as in a fork storm, and changes no result.
    fn clear_acceptance_caches(&mut self) {
        for &b in &self.unsettled {
            self.blocks[b as usize].own = OwnCache::default();
        }
        self.own_entries = 0;
    }

    /// The node's timing checks on a block that arrived at `arrived` (true time): returns the
    /// acceptance time (or [`INF`]) and whether the PoWit was late.
    fn judge(&self, node: &Node, b: u32, arrived: Micros) -> (Micros, bool) {
        let block = &self.blocks[b as usize];
        if block.household == node.household && node.household != NONE {
            return (arrived, false);
        }
        let clock = arrived as f64 / SECOND as f64 + node.skew;
        let mut at = arrived;
        if check_received(clock, block.timestamp, self.spec.delta).is_err() {
            if !reevaluates_early_block(self.spec.early_block) {
                return (INF, false);
            }
            // Re-requested once the node's clock passes timestamp − δ.
            let ready = block.timestamp as f64 - self.spec.delta - node.skew;
            at = micros(ready + 0.2).max(arrived);
        }
        // The PoWit travels with the block.
        let late = check_powit_deadline(clock, block.timestamp, self.spec.deadline).is_err();
        if late && !accepts_chain_through_late_block(self.spec.deadline_scope) {
            return (INF, true);
        }
        (at, late)
    }

    /// True when `a` is preferred to `b` at a node that saw them at `a_seen`, `b_seen`.
    pub(crate) fn prefer_block(&self, a: u32, a_seen: Micros, b: u32, b_seen: Micros) -> bool {
        let (x, y) = (&self.blocks[a as usize], &self.blocks[b as usize]);
        let fa = ForkCandidate {
            height: x.height,
            timestamp: x.timestamp,
            hash: x.hash,
        };
        let fb = ForkCandidate {
            height: y.height,
            timestamp: y.timestamp,
            hash: y.hash,
        };
        let order = match self.spec.fork_rule {
            ForkRule::V06 => fork_choice(&fa, &fb),
            ForkRule::FirstSeen => fork_choice_first_seen(&fa, a_seen as f64, &fb, b_seen as f64),
            ForkRule::LowerHashOnly => fork_choice_lower_hash(&fa, &fb),
        };
        order == Ordering::Less
    }

    // ------------------------------------------------------------------ blocks

    /// Records a broadcast block and opens its round.
    pub(crate) fn add_block(&mut self, now: Micros, c: u32, withheld: bool) -> u32 {
        let cand = self.candidates[c as usize].clone();
        let parent = self.groups[cand.group as usize].parent;
        let miner = cand.miner;
        let h = self.miners[miner as usize].household;
        let id = self.blocks.len() as u32;
        let height = self.blocks[parent as usize].height + 1;
        let hop_active = self
            .hop_state
            .get(self.miners[miner as usize].group as usize)
            .copied()
            == Some(2);
        self.blocks.push(Block {
            parent,
            height,
            timestamp: cand.timestamp,
            hash: cand.hash,
            miner,
            group: self.miners[miner as usize].group,
            region: self.hh[h as usize].region,
            household: h,
            broadcast: now,
            withheld,
            child_target: [0; 32],
            factor_ppm: PPM,
            panel: Vec::new(),
            panel_late: Vec::new(),
            own: OwnCache::default(),
            settled: false,
            canonical: false,
            confirmed: false,
            hop_active,
            beyond_horizon: cand.beyond_horizon,
            registered: NONE,
            round: cand.group,
            committed: 0,
        });
        self.unsettled.push(id);
        self.watch_fork_storm(now, height);
        self.set_difficulty(id);
        self.panel_accept(id);
        self.res.blocks += 1;
        if self.res.hash_uniforms.len() < 50_000 {
            let top = u64::from_be_bytes(cand.hash[..8].try_into().unwrap_or([0; 8]));
            self.res.hash_uniforms.push(crate::rng::unit(top));
        }
        if self.res.wins_by_slot.len() < self.miners.len() {
            self.res.wins_by_slot.resize(self.miners.len(), 0);
        }
        self.res.wins_by_slot[miner as usize] += 1;
        // The panel has the block once every panel node has received it.
        let latest = self.blocks[id as usize]
            .panel
            .iter()
            .copied()
            .filter(|t| *t != INF)
            .max()
            .unwrap_or(now);
        self.q.push(latest + 1, Ev::PanelUpdate);
        self.open_group(id, now);
        id
    }

    /// Records how many blocks are unsettled, and at one height, after a block at `height` is
    /// added. Once more than `sim.world.fork_storm_unsettled` are unsettled the run stops, as
    /// a fork storm (ASSUMPTIONS SIM-29): the event loop ends before the next event.
    fn watch_fork_storm(&mut self, now: Micros, height: u64) {
        let width = self
            .unsettled
            .iter()
            .filter(|b| self.blocks[**b as usize].height == height)
            .count();
        self.res.max_unsettled = self.res.max_unsettled.max(self.unsettled.len() as u64);
        self.res.max_fork_width = self.res.max_fork_width.max(width as u64);
        if self.unsettled.len() > self.p.fork_storm_unsettled && self.res.fork_storm_s.is_none() {
            self.res.fork_storm_s = Some(crate::engine::seconds(now));
            self.stop_at = now;
            self.stopped = true;
        }
    }

    /// The timestamp of the ancestor of `b` at `height`.
    fn ancestor_timestamp(&self, b: u32, height: u64) -> u64 {
        let id = self.ancestor(b, height);
        self.blocks[id as usize].timestamp
    }

    /// The ancestor of `b` at `height` (or `b` itself).
    pub(crate) fn ancestor(&self, b: u32, height: u64) -> u32 {
        let mut x = b;
        loop {
            let block = &self.blocks[x as usize];
            if block.height <= height {
                return x;
            }
            if block.settled && block.canonical {
                return self.settled_ids[height as usize];
            }
            x = block.parent;
        }
    }

    /// The difficulty a block passes to its children (SPEC §3.8).
    fn set_difficulty(&mut self, b: u32) {
        let block = &self.blocks[b as usize];
        let parent = &self.blocks[block.parent as usize];
        let height = block.height;
        let k = u64::from(self.spec.window_blocks.max(1));
        let window =
            (height >= k).then(|| block.timestamp - self.ancestor_timestamp(b, height - k));
        let parent_factor = parent.factor_ppm;
        let parent_target = parent.child_target;
        let (target, factor) = if let Some(t) = self.spec.frozen_target {
            (t, PPM)
        } else {
            match self.spec.difficulty {
                DifficultyVariant::CountBased => {
                    let round = self.blocks[b as usize].round;
                    let now = self.blocks[b as usize].broadcast;
                    let n = match self.spec.count_rule {
                        DifficultyCountRule::IssuedEntropy if round != NONE => {
                            self.issued_count(round, now).max(1)
                        }
                        DifficultyCountRule::IssuedEntropyLagged if round != NONE => {
                            // The block commits its own round's count; the next round uses
                            // the count committed a confirmation depth before it, shared by
                            // every branch that forked since.
                            let own = self.issued_count(round, now).max(1);
                            self.blocks[b as usize].committed = own;
                            let lag = self.p.confirmation_depth;
                            let earlier = if height >= lag {
                                self.blocks[self.ancestor(b, height - lag) as usize].committed
                            } else {
                                0
                            };
                            if earlier > 0 {
                                earlier
                            } else {
                                self.count_for_difficulty()
                            }
                        }
                        _ => self.count_for_difficulty(),
                    };
                    let factor = match &self.spec.correction {
                        Some(c) => update_correction_ppm(
                            c,
                            parent_factor,
                            height,
                            window,
                            self.p.interval_s,
                        ),
                        None => PPM,
                    };
                    (
                        count_based_target(
                            n,
                            self.p.interval_s,
                            self.spec.correction.as_ref(),
                            factor,
                        ),
                        factor,
                    )
                }
                DifficultyVariant::BitcoinRetarget => {
                    if height % k == 0 {
                        let clamp = self
                            .spec
                            .clamp
                            .map_or(1_000_000, |c| c.round().max(1.0) as u64);
                        let w = window.unwrap_or(k * self.p.interval_s);
                        (
                            variant_a_retarget(&parent_target, w, k, self.p.interval_s, clamp),
                            PPM,
                        )
                    } else {
                        (parent_target, PPM)
                    }
                }
            }
        };
        let block = &mut self.blocks[b as usize];
        block.child_target = target;
        block.factor_ppm = factor;
        if self.spec.correction.is_some() && height % k == 0 && self.res.factor_ppm.len() < 100_000
        {
            self.res.factor_ppm.push(factor);
        }
    }

    /// Acceptance times of a new block at every panel node.
    fn panel_accept(&mut self, b: u32) {
        let parent = self.blocks[b as usize].parent;
        let n = self.panel.len();
        let mut times = Vec::with_capacity(n);
        let mut lates = Vec::with_capacity(n);
        let (mut early, mut late_count) = (0u64, 0u64);
        for i in 0..n {
            let node = self.panel_node(i);
            let parent_at = {
                let pb = &self.blocks[parent as usize];
                if pb.settled {
                    if pb.canonical { 0 } else { INF }
                } else {
                    pb.panel[i]
                }
            };
            let arrived = self.arrival(&node, b);
            let (at, late) = self.judge(&node, b, arrived);
            let clock = arrived as f64 / SECOND as f64 + node.skew;
            if check_received(clock, self.blocks[b as usize].timestamp, self.spec.delta).is_err() {
                early += 1;
            }
            if late {
                late_count += 1;
            }
            let t = if at == INF || parent_at == INF {
                INF
            } else {
                at.max(parent_at)
            };
            times.push(t);
            lates.push(late);
        }
        self.res.panel_pairs += n as u64;
        self.res.early_pairs += early;
        self.res.late_pairs += late_count;
        if 2 * early > n as u64 {
            self.res.early_majority += 1;
        }
        let region = self.blocks[b as usize].region as usize;
        if self.blocks[b as usize].broadcast >= self.p.warmup {
            self.res.blocks_by_region[region] += 1;
            if 2 * late_count > n as u64 {
                self.res.late_majority_by_region[region] += 1;
            }
        }
        if 2 * late_count > n as u64 {
            self.res.late_majority += 1;
        }
        let block = &mut self.blocks[b as usize];
        block.panel = times;
        block.panel_late = lates;
    }

    /// The tip of panel node `i` at `t`, from the stored acceptance times.
    fn panel_tip(&self, i: usize, t: Micros) -> u32 {
        let mut best = *self.settled_ids.last().unwrap_or(&0);
        let mut best_seen = 0;
        for &b in &self.unsettled {
            let block = &self.blocks[b as usize];
            let at = block.panel[i];
            if at == INF || at > t || block.panel_late[i] {
                continue;
            }
            if self.prefer_block(b, at, best, best_seen) {
                best = b;
                best_seen = at;
            }
        }
        best
    }

    // ------------------------------------------------------------------ network chain

    /// The network tip: the highest block on the chains (tips and their ancestors) of more than
    /// half of the panel nodes that are not split off. A block that only a minority holds, such
    /// as the tip of a partitioned region, is never on the network chain.
    fn majority_tip(&self, tips: &[u32]) -> u32 {
        let settled = *self.settled_ids.last().unwrap_or(&0);
        let base = self.blocks[settled as usize].height;
        // Each voting node's chain above the settled height, lowest block first.
        let chains: Vec<Vec<u32>> = tips
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.panel_split[*i])
            .map(|(_, t)| {
                let mut chain = Vec::new();
                let mut x = *t;
                while self.blocks[x as usize].height > base {
                    chain.push(x);
                    x = self.blocks[x as usize].parent;
                }
                chain.reverse();
                chain
            })
            .collect();
        let need = chains.len() / 2 + 1;
        let mut tip = settled;
        for level in 0.. {
            let mut counts: Vec<(u32, usize)> = Vec::new();
            for chain in &chains {
                if let Some(b) = chain.get(level) {
                    match counts.iter_mut().find(|(x, _)| x == b) {
                        Some(c) => c.1 += 1,
                        None => counts.push((*b, 1)),
                    }
                }
            }
            match counts.iter().find(|(_, c)| *c >= need) {
                Some((b, _)) => tip = *b,
                None => break,
            }
        }
        tip
    }

    /// Moves the network tip to the majority tip of `tips`, unless that is an ancestor of the
    /// current tip: a majority that has only become unsure (no majority above some block, as
    /// while large regions are partitioned) has not chosen another branch, so nothing is
    /// reorganised.
    fn update_network_tip(&mut self, now: Micros, tips: &[u32]) {
        let new_tip = self.majority_tip(tips);
        if new_tip != self.network_tip && !self.is_ancestor(new_tip, self.network_tip) {
            self.switch_network_tip(now, new_tip);
        }
    }

    /// True when `a` is `b` or one of its ancestors.
    fn is_ancestor(&self, a: u32, b: u32) -> bool {
        let height = self.blocks[a as usize].height;
        height <= self.blocks[b as usize].height && self.ancestor(b, height) == a
    }

    pub(crate) fn on_panel_update(&mut self, now: Micros) {
        let n = self.panel.len();
        let tips: Vec<u32> = (0..n).map(|i| self.panel_tip(i, now)).collect();
        self.update_network_tip(now, &tips);
        self.settle(now, &tips);
        self.confirm(now);
        self.place_pending();
        self.apply_hops(now);
        if now >= self.invariants_due {
            self.check_invariants();
            self.invariants_due = now + 86_400 * SECOND;
        }
        if self.panel_update_at <= now || self.panel_update_at == u64::MAX {
            self.panel_update_at = now + micros(self.p.interval_s as f64);
            self.q.push(self.panel_update_at, Ev::PanelUpdate);
        }
    }

    fn switch_network_tip(&mut self, _now: Micros, new_tip: u32) {
        let old = self.network_tip;
        // Common ancestor.
        let (mut a, mut b) = (old, new_tip);
        while a != b {
            let (ha, hb) = (
                self.blocks[a as usize].height,
                self.blocks[b as usize].height,
            );
            if ha >= hb {
                a = self.parent_or_settled(a);
            } else {
                b = self.parent_or_settled(b);
            }
        }
        let fork = self.blocks[a as usize].height;
        let depth = self.blocks[old as usize].height.saturating_sub(fork);
        if depth > 0 && fork < self.blocks[old as usize].height {
            self.res.max_reorg = self.res.max_reorg.max(depth);
            if fork < self.confirmed_height {
                self.res.deep_reorgs += 1;
                self.confirmed_height = fork;
                // IDs in the replaced blocks are void (architect, 2026-10-14): their blocks
                // can be confirmed again if the chain comes back to them.
                let mut x = old;
                let mut replaced = Vec::new();
                while self.blocks[x as usize].height > fork {
                    if self.blocks[x as usize].registered != NONE {
                        replaced.push(x);
                    }
                    x = self.parent_or_settled(x);
                }
                for x in replaced {
                    let w = self.blocks[x as usize].registered;
                    let miner = self.blocks[x as usize].miner;
                    self.blocks[x as usize].registered = NONE;
                    self.blocks[x as usize].confirmed = false;
                    self.void_id(w, miner);
                }
            }
        }
        self.network_tip = new_tip;
    }

    fn parent_or_settled(&self, b: u32) -> u32 {
        let block = &self.blocks[b as usize];
        if block.parent == NONE {
            b
        } else {
            block.parent
        }
    }

    /// The height of the network tip.
    pub(crate) fn network_height(&self) -> u64 {
        self.blocks[self.network_tip as usize].height
    }

    /// The network chain's block at `height`.
    pub(crate) fn network_block_at(&self, height: u64) -> Option<u32> {
        if height > self.network_height() {
            return None;
        }
        Some(self.ancestor(self.network_tip, height))
    }

    /// Settles heights deep below the panel's tips: their network-chain block is final and
    /// competing blocks are orphans. A height settles when a majority of the panel nodes that
    /// are not split off hold the network chain's block there, `settle_depth` blocks below the
    /// network tip, and no partition is open. A node that can never accept that block (it
    /// rejected it or an ancestor for good, as with a late PoWit under the literal deadline
    /// rule) is counted as split and no longer votes; a node merely still on another branch
    /// keeps voting and follows the network chain once it holds it.
    fn settle(&mut self, now: Micros, tips: &[u32]) {
        if self
            .net
            .partition_open(now.saturating_sub(self.d_max))
            .is_some()
            || self.net.partition_open(now).is_some()
        {
            return;
        }
        loop {
            let next = self.settled_ids.len() as u64;
            let active: Vec<usize> = (0..tips.len()).filter(|i| !self.panel_split[*i]).collect();
            if active.is_empty() {
                return;
            }
            // Depth is measured on the network chain: a node that rejected a block for good
            // stays behind on its own branch and must not hold settling back.
            if self.network_height() < next + self.p.settle_depth {
                return;
            }
            let Some(chosen) = self.network_block_at(next) else {
                return;
            };
            let disagreeing: Vec<usize> = active
                .iter()
                .copied()
                .filter(|i| self.ancestor(tips[*i], next) != chosen)
                .collect();
            if 2 * disagreeing.len() >= active.len() {
                return;
            }
            // A validator that disagrees only because it is still on another branch follows the
            // network chain once it holds it, and keeps voting. Only one that can never accept
            // the chosen block (it rejected it, or an ancestor, for good, as with a late PoWit
            // under the permanent deadline) is split off.
            let chosen_panel = &self.blocks[chosen as usize].panel;
            let split: Vec<usize> = disagreeing
                .into_iter()
                .filter(|i| chosen_panel.get(*i).copied() == Some(INF))
                .collect();
            for i in split {
                self.panel_split[i] = true;
            }
            self.settled_ids.push(chosen);
            for &b in &self.unsettled {
                let block = &mut self.blocks[b as usize];
                if block.height == next {
                    block.settled = true;
                    block.canonical = b == chosen;
                    block.panel = Vec::new();
                    block.panel_late = Vec::new();
                    self.own_entries = self.own_entries.saturating_sub(block.own.len());
                    block.own = OwnCache::default();
                }
            }
            // Descendants of orphans can never be canonical: they settle as orphans too.
            let mut changed = true;
            while changed {
                changed = false;
                for &b in &self.unsettled {
                    let parent = self.blocks[b as usize].parent;
                    let orphaned = {
                        let pb = &self.blocks[parent as usize];
                        pb.settled && !pb.canonical
                    };
                    let block = &mut self.blocks[b as usize];
                    if orphaned && !block.settled {
                        block.settled = true;
                        block.canonical = false;
                        block.panel = Vec::new();
                        block.panel_late = Vec::new();
                        self.own_entries = self.own_entries.saturating_sub(block.own.len());
                        block.own = OwnCache::default();
                        changed = true;
                    }
                }
            }
            let blocks = &self.blocks;
            self.unsettled.retain(|b| !blocks[*b as usize].settled);
        }
    }

    /// Confirms blocks at confirmation depth on the network chain: their winners become
    /// registered IDs, to be placed at their beacon block.
    fn confirm(&mut self, now: Micros) {
        let tip_height = self.network_height();
        let depth = self.p.confirmation_depth;
        while self.confirmed_height + depth < tip_height {
            let h = self.confirmed_height + 1;
            let Some(b) = self.network_block_at(h) else {
                break;
            };
            if !self.blocks[b as usize].confirmed {
                self.blocks[b as usize].confirmed = true;
                self.confirmed.push(b);
                self.register_winner(now, b);
                self.after_confirmation(now, b);
            }
            self.confirmed_height = h;
        }
    }

    /// V1: the attacker's share of active IDs after each confirmed block.
    fn after_confirmation(&mut self, _now: Micros, b: u32) {
        let Some(threshold) = self.spec.threshold else {
            return;
        };
        let group = self.blocks[b as usize].group;
        if group == 1 {
            self.v1_attacker += 1;
        } else {
            // An honest ID is active with probability f (M1 A-3, realistic online fraction).
            let f = self.spec.honest_id_online.unwrap_or(1.0);
            let u = self
                .keyed
                .uniforms(Purpose::Sampling, 7_000 + u64::from(b), 0);
            if f >= 1.0 || u[0] < f {
                self.res.v1_honest_active += 1;
            }
        }
        let genesis_active = (self.spec.honest_id_online.unwrap_or(1.0)
            * self.spec.threshold_genesis as f64)
            .round() as u64;
        let active = genesis_active + self.v1_attacker + self.res.v1_honest_active;
        // Integer comparison of attacker/active ≥ threshold (threshold as a decimal).
        if (self.v1_attacker as f64) >= threshold * active as f64 - 1e-9 {
            self.res.threshold_blocks = Some(self.blocks[b as usize].height);
            self.res.v1_attacker_blocks = self.v1_attacker;
            self.stopped = true;
        }
    }

    // ------------------------------------------------------------------ the end

    /// Final measurements from the network chain at the end of the run.
    pub(crate) fn finish(&mut self, now: Micros) {
        self.check_invariants();
        self.advance_area(now);
        // Let the last blocks reach the panel before reading the final chain.
        let horizon = now + self.d_max;
        let n = self.panel.len();
        let tips: Vec<u32> = (0..n).map(|i| self.panel_tip(i, horizon)).collect();
        let tip = self.majority_tip(&tips);
        let top = self.blocks[tip as usize].height;
        let last = top.saturating_sub(12);
        // Canonical chain by height.
        let mut chain = vec![NONE; (top + 1) as usize];
        let mut x = tip;
        loop {
            let block = &self.blocks[x as usize];
            chain[block.height as usize] = x;
            if block.height == 0 {
                break;
            }
            if block.settled && block.canonical {
                for (h, id) in self.settled_ids.iter().enumerate() {
                    if (h as u64) <= block.height {
                        chain[h] = *id;
                    }
                }
                break;
            }
            x = block.parent;
        }
        let groups = self.res.ids_by_group.len();
        let mut per_height: std::collections::BTreeMap<u64, Vec<u32>> = Default::default();
        for (id, block) in self.blocks.iter().enumerate().skip(1) {
            if block.height <= last && block.broadcast >= self.p.warmup {
                per_height.entry(block.height).or_default().push(id as u32);
            }
        }
        for (height, ids) in &per_height {
            let canonical = chain[*height as usize];
            if ids.len() >= 2 {
                self.res.fork_heights += 1;
            }
            for id in ids {
                let block = &self.blocks[*id as usize];
                if *id == canonical {
                    self.res.canonical += 1;
                    self.res.ids_by_group[block.group as usize] += 1;
                    self.res.ids_by_region[block.region as usize] += 1;
                    if block.household != NONE {
                        let device = self.hh[block.household as usize].device as usize;
                        self.res.ids_by_group_device[block.group as usize][device] += 1;
                        self.res.ids_by_group_region[block.group as usize]
                            [block.region as usize] += 1;
                    }
                    let day = (block.broadcast / (86_400 * SECOND)) as usize;
                    if self.res.daily_ids.len() <= day {
                        self.res.daily_ids.resize(day + 1, 0);
                        self.res.daily_ids_by_group.resize(day + 1, vec![0; groups]);
                    }
                    self.res.daily_ids[day] += 1;
                    self.res.daily_ids_by_group[day][block.group as usize] += 1;
                    if self.res.daily_ids_by_region.len() <= day {
                        self.res
                            .daily_ids_by_region
                            .resize(day + 1, vec![0; self.res.ids_by_region.len()]);
                    }
                    self.res.daily_ids_by_region[day][block.region as usize] += 1;
                    let half = (block.broadcast / (43_200 * SECOND)) as usize;
                    if self.res.halfday_ids.len() <= half {
                        self.res.halfday_ids.resize(half + 1, 0);
                    }
                    self.res.halfday_ids[half] += 1;
                    if block.withheld {
                        self.res.withheld_won += 1;
                    }
                    if block.hop_active {
                        self.res.hop_ids += 1;
                    }
                    if block.beyond_horizon {
                        self.res.horizon_hits += 1;
                    }
                    let parent = &self.blocks[block.parent as usize];
                    let interval = block.timestamp.saturating_sub(parent.timestamp) as f64;
                    self.res.interval_sum_s += interval;
                    self.res.interval_sq_sum_s += interval * interval;
                    self.res.interval_count += 1;
                } else {
                    self.res.orphans += 1;
                    if canonical != NONE && block.hash < self.blocks[canonical as usize].hash {
                        self.res.loser_lower_hash += 1;
                    }
                    if !block.withheld
                        && ids
                            .iter()
                            .any(|i| self.blocks[*i as usize].withheld && *i == canonical)
                    {
                        self.res.displaced_by_withheld += 1;
                    }
                }
            }
        }
        // Windows of K blocks on the final chain, and global pauses.
        let k = u64::from(self.spec.window_blocks.max(1));
        let mut previous: Option<u64> = None;
        let mut h = k;
        while h <= last {
            let id = chain[h as usize];
            if id != NONE {
                let ts = self.blocks[id as usize].timestamp;
                if let Some(p) = previous {
                    self.res.window_s.push(ts.saturating_sub(p) as f64);
                }
                previous = Some(ts);
            }
            h += k;
        }
        let mut last_broadcast: Option<Micros> = None;
        let mut gaps: Vec<(Micros, Micros)> = Vec::new();
        for height in 1..=last {
            let id = chain[height as usize];
            if id == NONE {
                continue;
            }
            let b = self.blocks[id as usize].broadcast;
            if let Some(p) = last_broadcast {
                if p >= self.p.warmup && b > p + 120 * SECOND {
                    self.res.pauses += 1;
                    self.res.pause_s += (b - p) as f64 / SECOND as f64;
                }
                if p >= self.p.warmup {
                    gaps.push((p, b));
                    let gap = b.saturating_sub(p) as f64 / SECOND as f64;
                    if gap > self.res.max_gap_s {
                        self.res.max_gap_s = gap;
                        self.res.max_gap_start_s = p as f64 / SECOND as f64;
                    }
                }
            }
            last_broadcast = Some(b);
        }
        // The longest gap that overlaps each tracked outage (E3), and for scripted blackouts
        // the cause of every gap over 10 minutes that overlaps it.
        let interval = self.p.interval_s as f64;
        for (i, record) in self.res.outages.iter_mut().enumerate() {
            let from = micros(record.start_s);
            let to = micros(record.start_s + record.duration_s);
            record.max_gap_s = gaps
                .iter()
                .filter(|(p, b)| *b > from && *p < to)
                .map(|(p, b)| b.saturating_sub(*p) as f64 / SECOND as f64)
                .fold(0.0, f64::max);
            if record.kind != 2 {
                continue;
            }
            let samples = self
                .outage_diag
                .get(i)
                .map(|d| d.samples.as_slice())
                .unwrap_or(&[]);
            for (p, b) in gaps.iter().filter(|(p, b)| *b > from && *p < to) {
                if b.saturating_sub(*p) <= 600 * SECOND {
                    continue;
                }
                record.gaps.push(diagnose_gap(*p, *b, samples, interval));
            }
        }
        self.res.permanent_split_panel_nodes =
            self.panel_split.iter().filter(|x| **x).count() as u64;
        let split = &self.panel_split;
        self.res.split_panel_nodes = tips
            .iter()
            .enumerate()
            .filter(|(i, t)| {
                if split[*i] {
                    return true;
                }
                let h = self.blocks[**t as usize].height.min(top);
                chain.get(h as usize).copied() != Some(self.ancestor(**t, h))
            })
            .count() as u64;
        self.res.waiting_newcomers = self
            .miners
            .iter()
            .filter(|m| m.newcomer && !m.first_id && m.phase != Phase::Gone)
            .count() as u64;
        // A6: every newcomer's arrival and wait to a first ID.
        let day = 86_400.0 * SECOND as f64;
        for miner in self.miners.iter().filter(|m| m.newcomer).take(200_000) {
            let wait = (miner.first_id_at != u64::MAX)
                .then(|| miner.first_id_at.saturating_sub(miner.joined_at) as f64 / day);
            self.res
                .newcomers
                .push((miner.joined_at as f64 / day, wait));
        }
        // Witness ID-time by region and device.
        let end = now;
        for w in &self.witnesses {
            let hh = &self.hh[w.household as usize];
            let from = w.registered_at.max(self.p.warmup);
            if end > from && w.seat != NONE {
                self.res.witness_id_s[hh.region as usize][hh.device as usize] +=
                    (end - from) as f64 / SECOND as f64;
            }
        }
        // A run stopped by a fork storm keeps only the complete days before it in its daily
        // series: its last, partial day would read as a fall in issuance.
        if self.res.fork_storm_s.is_some() {
            let days = (now / (86_400 * SECOND)) as usize;
            self.res.daily_ids.truncate(days);
            self.res.daily_ids_by_group.truncate(days);
            self.res.daily_ids_by_region.truncate(days);
            self.res.daily_slot_s_by_group.truncate(days);
            self.res.daily_slot_s_by_region.truncate(days);
            self.res.daily_kwc.truncate(days);
            self.res
                .halfday_ids
                .truncate((now / (43_200 * SECOND)) as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::JobSpec;
    use gb_config::protocol::EquivocationScope;
    use gb_protocol::issuance::variant_a_retarget;

    fn spec(cfg: &gb_config::Config) -> JobSpec {
        let mut spec = JobSpec::from_config(cfg, "chain-test", "T", 7).idealised();
        spec.miners = 4;
        spec.registered_ids = 400;
        spec
    }

    /// Adds a block on `parent` that every panel node holds from time 0, with its difficulty.
    fn block(sim: &mut Sim<'_>, parent: u32, timestamp: u64, tag: u8) -> u32 {
        let n = sim.panel.len();
        let height = sim.blocks[parent as usize].height + 1;
        sim.blocks.push(Block {
            parent,
            height,
            timestamp,
            hash: [tag; 32],
            miner: NONE,
            group: 0,
            region: 0,
            household: NONE,
            broadcast: 0,
            withheld: false,
            child_target: [0; 32],
            factor_ppm: PPM,
            panel: vec![0; n],
            panel_late: vec![false; n],
            own: OwnCache::default(),
            settled: false,
            canonical: false,
            confirmed: false,
            hop_active: false,
            beyond_horizon: false,
            registered: NONE,
            round: NONE,
            committed: 0,
        });
        let id = (sim.blocks.len() - 1) as u32;
        sim.unsettled.push(id);
        sim.set_difficulty(id);
        id
    }

    /// Panel tips: `n` nodes on each block, 101 in all.
    fn tips(groups: &[(u32, usize)]) -> Vec<u32> {
        let tips: Vec<u32> = groups
            .iter()
            .flat_map(|(b, n)| std::iter::repeat_n(*b, *n))
            .collect();
        assert_eq!(tips.len(), 101);
        tips
    }

    /// Genesis, branch A (3 blocks) and a longer branch B (4 blocks) from genesis.
    fn two_branches(sim: &mut Sim<'_>) -> ([u32; 3], [u32; 4]) {
        sim.start_chain();
        let a1 = block(sim, 0, 31, 1);
        let a2 = block(sim, a1, 62, 2);
        let a3 = block(sim, a2, 93, 3);
        let b1 = block(sim, 0, 40, 4);
        let b2 = block(sim, b1, 80, 5);
        let b3 = block(sim, b2, 120, 6);
        let b4 = block(sim, b3, 160, 7);
        ([a1, a2, a3], [b1, b2, b3, b4])
    }

    #[test]
    fn a_fork_storm_stops_the_run_once_too_many_blocks_are_unsettled() {
        let mut cfg = gb_config::Config::default();
        cfg.sim.world.fork_storm_unsettled = 100;
        let spec = spec(&cfg);
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start_chain();
        let a1 = block(&mut sim, 0, 31, 1);
        sim.watch_fork_storm(SECOND, 1);
        for tag in 2..=3 {
            block(&mut sim, 0, 30 + u64::from(tag), tag);
            sim.watch_fork_storm(5 * SECOND, 1);
        }
        let mut parent = a1;
        for i in 0..97u64 {
            parent = block(&mut sim, parent, 62 + 31 * i, 9);
            sim.watch_fork_storm(6 * SECOND, 2 + i);
        }
        assert!(!sim.stopped);
        assert_eq!(sim.res.max_fork_width, 3);
        assert_eq!(sim.res.max_unsettled, 100);
        // The 101st unsettled block is one more than the limit allows.
        block(&mut sim, 0, 40, 4);
        sim.watch_fork_storm(7 * SECOND, 1);
        assert!(sim.stopped);
        assert_eq!(sim.res.fork_storm_s, Some(7.0));
        assert_eq!(sim.stop_at, 7 * SECOND);
        assert_eq!(sim.res.max_fork_width, 4);
        assert_eq!(sim.res.max_unsettled, 101);
        // The first storm is the one recorded.
        block(&mut sim, 0, 41, 5);
        sim.watch_fork_storm(9 * SECOND, 1);
        assert_eq!(sim.res.fork_storm_s, Some(7.0));
    }

    /// A miner holds block P (from 10 s), then a better block S at the same height (from
    /// 20 s). Under one header per height it signed on P, so S's round is not its round; under
    /// the [P] rule it may sign again on S.
    fn round_on_the_later_better_block(scope: EquivocationScope) -> u64 {
        let cfg = gb_config::Config::default();
        let mut spec = spec(&cfg);
        spec.equivocation = scope;
        // No PoWit deadline: the later block would be late under D = 10 s. The v0.6 tie-break,
        // under which a later block at the same height can be the better one.
        spec.deadline = None;
        spec.fork_rule = ForkRule::V06;
        spec.start_rule = gb_config::protocol::StartRule::A;
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let p = block(&mut sim, 0, 8, 1);
        let s = block(&mut sim, 0, 5, 2);
        sim.blocks[p as usize].broadcast = 10 * SECOND;
        sim.blocks[s as usize].broadcast = 20 * SECOND;
        assert!(sim.prefer_block(s, 20 * SECOND, p, 10 * SECOND));
        let g = sim.groups.len() as u32;
        sim.open_group(s, 30 * SECOND);
        sim.materialise(30 * SECOND, g, 0, 100);
        sim.res.drops.get(2).copied().unwrap_or(0)
    }

    #[test]
    fn under_one_header_per_height_a_miner_does_not_join_a_later_better_round() {
        assert_eq!(
            round_on_the_later_better_block(EquivocationScope::SameHeight),
            1
        );
        assert_eq!(
            round_on_the_later_better_block(EquivocationScope::SameParent),
            0
        );
    }

    #[test]
    fn a_reorg_below_confirmation_voids_the_replaced_id_without_a_ban_or_penalty() {
        let cfg = gb_config::Config::default();
        let spec = spec(&cfg);
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let a1 = block(&mut sim, 0, 31, 1);
        let a2 = block(&mut sim, a1, 62, 2);
        let b1 = block(&mut sim, 0, 40, 3);
        let b2 = block(&mut sim, b1, 80, 4);
        let b3 = block(&mut sim, b2, 120, 5);
        sim.blocks[a1 as usize].miner = 0;
        sim.blocks[a1 as usize].confirmed = true;
        sim.network_tip = a2;
        sim.confirmed_height = 2;
        sim.register_winner(SECOND, a1);
        let w = sim.blocks[a1 as usize].registered;
        assert_ne!(w, NONE);
        assert!(sim.miners[0].first_id);
        let reg = sim.witnesses[w as usize].reg as usize;
        sim.insert_seat(w, &[7; 32]);
        assert_ne!(sim.witnesses[w as usize].seat, NONE);
        assert!(sim.active.iter().any(|r| r == reg));
        // The network chain moves to branch B, below the confirmed block A1.
        sim.update_network_tip(2 * SECOND, &tips(&[(b3, 101)]));
        assert_eq!(sim.network_tip, b3);
        assert_eq!(sim.res.deep_reorgs, 1);
        assert_eq!(sim.res.voided_ids, 1);
        let wit = &sim.witnesses[w as usize];
        assert!(wit.void && !wit.banned && wit.offences.is_empty());
        assert_eq!(wit.seat, NONE);
        assert!(!sim.active.iter().any(|r| r == reg));
        assert!(!sim.registry.canonical_active_list().contains(&reg));
        assert!(sim.placements.iter().all(|(_, x)| *x != w));
        // Not a ban and not a penalty: no ban record, no penalty, no offence.
        assert_eq!(sim.res.bans.iter().flatten().sum::<u64>(), 0);
        assert_eq!(sim.res.penalties.iter().flatten().sum::<u64>(), 0);
        // The miner slot is a candidate again, and the block can be confirmed again.
        assert!(!sim.miners[0].first_id);
        assert!(!sim.blocks[a1 as usize].confirmed);
        assert_eq!(sim.blocks[a1 as usize].registered, NONE);
        sim.check_invariants();
        assert_eq!(sim.res.invariant_failures, 0);
    }

    #[test]
    fn a_run_stopped_by_a_fork_storm_keeps_only_its_complete_days() {
        let cfg = gb_config::Config::default();
        let spec = spec(&cfg);
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start_chain();
        sim.res.daily_ids = vec![1; 4];
        sim.res.daily_kwc = vec![[1, 1, 1]; 4];
        sim.res.halfday_ids = vec![1; 7];
        sim.res.fork_storm_s = Some(2.6 * 86_400.0);
        sim.finish(micros(2.6 * 86_400.0));
        assert_eq!(sim.res.daily_ids.len(), 2);
        assert_eq!(sim.res.daily_kwc.len(), 2);
        assert_eq!(sim.res.halfday_ids.len(), 5);
    }

    #[test]
    fn a_longer_branch_held_by_a_minority_is_not_the_network_chain() {
        let cfg = gb_config::Config::default();
        let spec = spec(&cfg);
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        let ([a1, a2, a3], [.., b4]) = two_branches(&mut sim);
        // B4 is the most common tip (40 of 101), but 61 nodes hold A1 and A2.
        let t = tips(&[(b4, 40), (a3, 35), (a2, 26)]);
        assert_eq!(sim.majority_tip(&t), a2);
        // Without a majority above genesis the network tip is genesis.
        let t = tips(&[(b4, 50), (a1, 51)]);
        assert_eq!(sim.majority_tip(&t), a1);
        let t = tips(&[(b4, 50), (a1, 50), (0, 1)]);
        assert_eq!(sim.majority_tip(&t), 0);
    }

    #[test]
    fn losing_the_majority_view_is_not_a_reorg() {
        let cfg = gb_config::Config::default();
        let spec = spec(&cfg);
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        let ([a1, _, a3], [.., b4]) = two_branches(&mut sim);
        sim.update_network_tip(0, &tips(&[(a3, 101)]));
        assert_eq!(sim.network_tip, a3);
        // Only A1 is still held by a majority: the network has not chosen another branch.
        sim.update_network_tip(0, &tips(&[(a3, 50), (b4, 30), (a1, 21)]));
        assert_eq!(sim.network_tip, a3);
        assert_eq!(sim.res.max_reorg, 0);
    }

    #[test]
    fn a_majority_on_another_branch_reorganises_and_counts_confirmed_ids() {
        let cfg = gb_config::Config::default();
        let spec = spec(&cfg);
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        let ([.., a3], [.., b4]) = two_branches(&mut sim);
        sim.update_network_tip(0, &tips(&[(a3, 101)]));
        sim.confirmed_height = 2;
        sim.update_network_tip(0, &tips(&[(b4, 60), (a3, 41)]));
        assert_eq!(sim.network_tip, b4);
        // Fork at genesis: three blocks abandoned, two of them confirmed.
        assert_eq!(sim.res.max_reorg, 3);
        assert_eq!(sim.res.deep_reorgs, 1);
        assert_eq!(sim.confirmed_height, 0);
    }

    #[test]
    fn a_validator_that_can_never_accept_a_settled_block_is_split_for_good() {
        let cfg = gb_config::Config::default();
        let spec = spec(&cfg);
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start_chain();
        // A chain of 14 blocks; validators 0, 1 and 2 rejected its first block for good, so
        // they hold none of it.
        let mut b = 0;
        let mut chain = Vec::new();
        for i in 0..14u64 {
            b = block(&mut sim, b, 30 * (i + 1), i as u8 + 1);
            chain.push(b);
        }
        for x in &chain {
            for v in 0..3 {
                sim.blocks[*x as usize].panel[v] = INF;
            }
        }
        let tips: Vec<u32> = (0..sim.panel.len())
            .map(|v| if v < 3 { 0 } else { b })
            .collect();
        sim.update_network_tip(0, &tips);
        sim.settle(0, &tips);
        assert_eq!(sim.settled_ids.len(), 3);
        assert_eq!(sim.panel_split.iter().filter(|x| **x).count(), 3);
        sim.finish(0);
        assert_eq!(sim.res.permanent_split_panel_nodes, 3);
        assert!(sim.res.split_panel_nodes >= 3);
    }

    #[test]
    fn difficulty_follows_each_branch() {
        let cfg = gb_config::Config::default();
        let mut spec = spec(&cfg);
        spec.difficulty = DifficultyVariant::BitcoinRetarget;
        spec.window_blocks = 2;
        spec.clamp = Some(4.0);
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start_chain();
        // Branch A keeps the 30 s pace; branch B is three times slower.
        let a1 = block(&mut sim, 0, 30, 1);
        let a2 = block(&mut sim, a1, 60, 2);
        let a3 = block(&mut sim, a2, 90, 3);
        let b1 = block(&mut sim, 0, 90, 4);
        let b2 = block(&mut sim, b1, 180, 5);
        let target = |b: u32| sim.blocks[b as usize].child_target;
        assert_eq!(target(a2), variant_a_retarget(&target(a1), 60, 2, 30, 4));
        assert_eq!(target(b2), variant_a_retarget(&target(b1), 180, 2, 30, 4));
        assert_ne!(target(a2), target(b2));
        // Height 3 is not a retarget height: A3 passes on A2's target.
        assert_eq!(target(a3), target(a2));
    }

    /// Two blocks at height 1: S reaches every miner at once, P (the better block, with the
    /// earlier timestamp) 100 s later. Returns the issued-entropy counts of the rounds on P
    /// and on S, read at 200 s.
    fn issued_on_a_fork(scope: EquivocationScope) -> (u64, u64) {
        let cfg = gb_config::Config::default();
        let mut spec = spec(&cfg);
        spec.count_rule = DifficultyCountRule::IssuedEntropy;
        spec.equivocation = scope;
        spec.deadline = None;
        // The v0.6 tie-break, under which the later block P (earlier timestamp) is the better one.
        spec.fork_rule = ForkRule::V06;
        spec.start_rule = gb_config::protocol::StartRule::A;
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let s = block(&mut sim, 0, 2, 1);
        let p = block(&mut sim, 0, 1, 2);
        sim.blocks[p as usize].broadcast = 100 * SECOND;
        sim.open_group(p, 100 * SECOND);
        let gp = (sim.groups.len() - 1) as u32;
        sim.open_group(s, 100 * SECOND);
        let gs = (sim.groups.len() - 1) as u32;
        (
            sim.issued_count(gp, 200 * SECOND),
            sim.issued_count(gs, 200 * SECOND),
        )
    }

    #[test]
    fn a_height_every_validator_holds_late_does_not_end_the_round_below_it() {
        let cfg = gb_config::Config::default();
        let spec = spec(&cfg);
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let b = block(&mut sim, 0, 1, 1);
        let later = 10_000 * SECOND;
        // On time everywhere: no later block at height 1 can win, so the round on genesis ends.
        assert!(sim.horizon_reached(1, later));
        // Late at every validator: none extends it, so blocks at height 1 can still win.
        let n = sim.blocks[b as usize].panel_late.len();
        sim.blocks[b as usize].panel_late = vec![true; n];
        assert!(!sim.horizon_reached(1, later));
        // An on-time child makes the late block an ancestor of the chain the validators hold.
        let c = block(&mut sim, b, 2, 2);
        assert_eq!(sim.blocks[c as usize].height, 2);
        assert!(sim.horizon_reached(1, later));
    }

    #[test]
    fn a_gap_is_put_down_to_the_cause_that_covers_most_of_its_samples() {
        let s = SECOND;
        // At p = 1/3000, 100 miners expect a block in about 30.5 s and 10 in about 300 s.
        let p = 1.0 / 3_000.0;
        let samples = vec![
            (10 * s, 0, 0, p),
            (40 * s, 50, 0, p),
            (70 * s, 50, 0, p),
            (100 * s, 50, 10, p),
            (130 * s, 100, 100, p),
            (2_000 * s, 100, 100, p),
        ];
        let d = diagnose_gap(0, 1_000 * s, &samples, 30.0);
        assert_eq!(d.samples, 5);
        assert_eq!(crate::job::GAP_CAUSES[d.cause as usize], "no entropy");
        assert_eq!(d.shares, [0.2, 0.4, 0.2, 0.2]);
        assert!((d.expected_interval_s - (300.45 + 30.50) / 2.0).abs() < 0.1);
        assert_eq!((d.start_s, d.length_s), (0.0, 1_000.0));
    }

    #[test]
    fn the_lagged_count_is_the_one_committed_a_confirmation_depth_before_the_parent() {
        let cfg = gb_config::Config::default();
        let mut spec = spec(&cfg);
        spec.count_rule = DifficultyCountRule::IssuedEntropyLagged;
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        // A chain of 8 blocks, the block at height h committing the count 100 + h.
        let mut chain = vec![0u32];
        for h in 1..=8u64 {
            let b = block(&mut sim, chain[chain.len() - 1], h, h as u8);
            sim.blocks[b as usize].committed = 100 + h;
            chain.push(b);
        }
        // A branch forked at height 6, also 8 blocks long.
        let mut branch = vec![chain[5]];
        for h in 6..=8u64 {
            let b = block(&mut sim, branch[branch.len() - 1], h + 10, 50 + h as u8);
            branch.push(b);
        }
        let expected = count_based_target(102, sim.p.interval_s, None, PPM);
        for tip in [chain[8], branch[3]] {
            let parent = sim.blocks[tip as usize].parent;
            sim.open_group(parent, 0);
            let g = (sim.groups.len() - 1) as u32;
            sim.blocks[tip as usize].round = g;
            sim.set_difficulty(tip);
            // Both tips use the count committed at height 8 − 6 = 2, which they share, and
            // each commits its own round's count for the blocks 6 above it.
            assert_eq!(sim.blocks[tip as usize].child_target, expected);
            let own = sim.issued_count(g, 0).max(1);
            assert_eq!(sim.blocks[tip as usize].committed, own);
        }
    }

    #[test]
    fn the_issued_entropy_count_leaves_out_miners_that_sat_the_round_out() {
        // One header per height: every miner signed on S, the first block it held, so none of
        // them gets entropy for the round on P.
        assert_eq!(issued_on_a_fork(EquivocationScope::SameHeight), (0, 4));
        // [P] same parent: each miner mines on the better block, P.
        assert_eq!(issued_on_a_fork(EquivocationScope::SameParent), (4, 0));
        // At an unforked height every mining miner counts, unless its KWC cannot issue.
        let cfg = gb_config::Config::default();
        let mut spec = spec(&cfg);
        spec.count_rule = DifficultyCountRule::IssuedEntropy;
        let mut sim = Sim::build(&spec, &cfg).unwrap();
        sim.start();
        let b = block(&mut sim, 0, 1, 3);
        sim.open_group(b, 0);
        let g = (sim.groups.len() - 1) as u32;
        let k = sim.miners[0].kwc;
        let w = sim.kwc_members(k)[0];
        let h = sim.witnesses[w as usize].household;
        // A member away without announcing it stalls every KWC it sits in (all-online-members
        // rule; its WC is in four KWCs).
        sim.set_cause(0, h, crate::household::bit::CRASH, true, false, u64::MAX);
        let stalled = (0..4)
            .filter(|m| sim.kwc_members(sim.miners[*m].kwc).contains(&w))
            .count() as u64;
        assert!(stalled >= 1);
        assert_eq!(sim.issued_count(g, 10 * SECOND), 4 - stalled);
    }
}
