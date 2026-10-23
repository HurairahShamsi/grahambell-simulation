//! Supplementary job sets, not part of the reference run: each runs only when named
//! (`gb sim --only W1`, `--only W2`).
//!
//! W1, where the withholder sits (architect, 2026-10-23): E1's two finalists ((r), first seen,
//! literal, with D = 10 s soft and without a deadline) and C3's withholder runs at D = none and
//! D = 10 s soft, at the final defaults, with the 10 % withholder placed three ways: as in the
//! reference (by the slot order that places the regions, mostly as_east; ASSUMPTIONS SIM-51),
//! spread over the regions in proportion to their miners, and all in one small, distant
//! region. The jobs placed as in the reference are those of the reference run under other
//! names, so they reproduce its figures.
//!
//! W2, E3's blackout traces (architect, 2026-10-23): E3's scripted blackout runs at the final
//! defaults, traced, and every final-chain block and every round's sample from an hour before
//! the blackout to an hour after each of the three longest pauses, with what held each pause
//! up. The runs are the reference run's under other names: the trace only observes.

use super::c::{c3_job, region_names};
use super::e::{ForkConfig, e1_job, e1_seeds, e3_jobs};
use super::{Ctx, SectionOut, Table, amplification, div, f, n, r};
use crate::job::{ForkRule, JobResult, JobSpec, OutageSampleRow};
use crate::network::{Latency, RIPE_MATRIX};
use gb_config::protocol::{DeadlineScope, EquivocationScope, StartRule};

const W1: &str = "W1";

/// The small, distant region: oc, the smallest (0.6 % of miners) and, after sa_east, the
/// farthest from the network's miners (their median round trip, `W1` summary).
const DISTANT: &str = "oc";

/// Where the withholder's slots are.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Placement {
    /// As in the reference run.
    Reference,
    /// Spread over the regions in proportion to their miners.
    Spread,
    /// All in [`DISTANT`].
    Distant,
}

impl Placement {
    fn name(self) -> &'static str {
        match self {
            Placement::Reference => "as-reference",
            Placement::Spread => "spread",
            Placement::Distant => DISTANT,
        }
    }
}

const PLACEMENTS: [Placement; 3] = [Placement::Reference, Placement::Spread, Placement::Distant];

/// E1's two finalists at the final defaults: D = 10 s soft, then no deadline.
fn finalists() -> [ForkConfig; 2] {
    [Some(10.0), None].map(|deadline| ForkConfig {
        start: StartRule::R,
        tie: ForkRule::FirstSeen,
        header: EquivocationScope::SameHeight,
        deadline,
    })
}

/// C3's withholder runs: D = 10 s soft, and no deadline (as C3 runs it, with the permanent
/// scope, which then never applies).
fn c3_deadlines() -> [(Option<f64>, DeadlineScope); 2] {
    [
        (Some(10.0), DeadlineScope::NotExtendedOnly),
        (None, DeadlineScope::Permanent),
    ]
}

fn distant_region(ctx: &Ctx<'_>) -> usize {
    let regions = region_names(ctx);
    regions
        .iter()
        .position(|r| r == DISTANT)
        .unwrap_or(regions.len() - 1)
}

fn place(ctx: &Ctx<'_>, mut j: JobSpec, p: Placement) -> JobSpec {
    j.id = format!("{}-W1-{}", j.id, p.name());
    j.section = W1.to_string();
    match p {
        Placement::Reference => {}
        Placement::Spread => j.groups_by_region = true,
        Placement::Distant => j.groups_in_region = Some(distant_region(ctx) as u8),
    }
    j
}

pub(crate) fn w1_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    for p in PLACEMENTS {
        for c in finalists() {
            for s in 0..e1_seeds(ctx) {
                jobs.push(place(ctx, e1_job(ctx, &c, true, s, ""), p));
            }
        }
        for (d, scope) in c3_deadlines() {
            jobs.push(place(ctx, c3_job(ctx, d, scope, true), p));
        }
    }
    jobs
}

/// The withholder's share of slot time in each region, over `runs`.
fn withholder_regions(runs: &[&JobResult], regions: usize) -> Vec<f64> {
    let mut slot = vec![0.0; regions];
    for res in runs {
        if let Some(row) = res.slot_s_by_group_region.get(1) {
            for (total, s) in slot.iter_mut().zip(row) {
                *total += s;
            }
        }
    }
    let all: f64 = slot.iter().sum();
    slot.iter().map(|s| div(*s, all)).collect()
}

/// The regions holding at least 0.5 % of the withholder's slot time, largest first.
fn regions_text(shares: &[f64], names: &[String]) -> String {
    let mut v: Vec<(f64, &String)> = shares.iter().copied().zip(names).collect();
    v.sort_by(|a, b| b.0.total_cmp(&a.0));
    v.iter()
        .filter(|(s, _)| *s >= 0.005)
        .map(|(s, name)| format!("{name} {} %", r(100.0 * s, 0)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Each region's median round trip to the network's miners, in ms (RIPE Atlas medians weighted
/// by the regions' shares of miners).
fn median_rtt_to_miners(ctx: &Ctx<'_>) -> Vec<f64> {
    let names = region_names(ctx);
    let shares = &ctx.cfg.sim.network.region_shares;
    let Ok(latency) = Latency::from_ripe(RIPE_MATRIX, &names) else {
        return vec![f64::NAN; names.len()];
    };
    (0..names.len())
        .map(|a| {
            (0..names.len())
                .map(|b| shares[b] * 2_000.0 * latency.one_way(a, b, 0.5))
                .sum()
        })
        .collect()
}

/// One row of `W1_withholder_placement.csv`; returns the summary's part for it.
fn push_row(
    table: &mut Table,
    setup: &str,
    p: Placement,
    deadline: Option<f64>,
    runs: &[&JobResult],
) -> String {
    let deadline = if deadline.is_some() {
        "D = 10 s soft"
    } else {
        "no deadline"
    };
    let (amp, se, _) = amplification(runs, 1);
    let (lo, hi) = super::ci95(amp, se);
    let withheld: u64 = runs.iter().map(|x| x.withheld).sum();
    let won: u64 = runs.iter().map(|x| x.withheld_won).sum();
    table.push(vec![
        setup.to_string(),
        p.name().to_string(),
        deadline.to_string(),
        n(runs.len() as u64),
        n(withheld),
        n(won),
        f(div(won as f64, withheld as f64)),
        f(amp),
        f(se),
        f(lo),
        f(hi),
        n(runs.iter().map(|x| x.deep_reorgs).sum()),
        n(runs.iter().map(|x| x.max_reorg).max().unwrap_or(0)),
    ]);
    format!("{deadline} {} (SE {})", r(amp, 3), r(se, 3))
}

pub(crate) fn w1_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> SectionOut {
    let mut out = SectionOut::default();
    let names = region_names(ctx);
    let seeds = e1_seeds(ctx) as usize;
    let mut table = Table::new(
        "W1_withholder_placement.csv",
        &[
            "setup",
            "placement",
            "deadline",
            "runs",
            "withheld",
            "withheld_on_final_chain",
            "success_rate",
            "withholder_amplification",
            "se",
            "ci95_low",
            "ci95_high",
            "reorgs_below_confirmation",
            "deepest_reorg",
        ],
    );
    let mut placed = Table::new(
        "W1_withholder_regions.csv",
        &["placement", "region", "withholder_slot_share"],
    );
    let (mut e1_lines, mut c3_lines) = (Vec::new(), Vec::new());
    let mut i = 0;
    for p in PLACEMENTS {
        let start = i;
        let mut e1_parts = Vec::new();
        for c in finalists() {
            let runs: Vec<&JobResult> = results[i..i + seeds].iter().collect();
            i += seeds;
            e1_parts.push(push_row(&mut table, "E1 finalist", p, c.deadline, &runs));
        }
        let mut c3_parts = Vec::new();
        for (d, _) in c3_deadlines() {
            c3_parts.push(push_row(&mut table, "C3", p, d, &[&results[i]]));
            i += 1;
        }
        let runs: Vec<&JobResult> = results[start..i].iter().collect();
        let shares = withholder_regions(&runs, names.len());
        for (name, s) in names.iter().zip(&shares) {
            placed.push(vec![p.name().to_string(), name.clone(), f(*s)]);
        }
        let label = format!(
            "{} ({})",
            match p {
                Placement::Reference => "as in the reference",
                Placement::Spread => "spread over the regions",
                Placement::Distant => "all in one small, distant region",
            },
            regions_text(&shares, &names)
        );
        e1_lines.push(format!("{label}: {}", e1_parts.join(", ")));
        c3_lines.push(format!("{label}: {}", c3_parts.join(", ")));
    }
    out.tables.push(table);
    out.tables.push(placed);
    let rtt = median_rtt_to_miners(ctx);
    let farthest = (0..names.len())
        .max_by(|a, b| rtt[*a].total_cmp(&rtt[*b]))
        .unwrap_or(0);
    let distant = distant_region(ctx);
    let as_east = names
        .iter()
        .position(|x| x == "as_east")
        .map_or(f64::NAN, |k| rtt[k]);
    out.summary = format!(
        "## W1 — where the withholder sits (supplementary)\n\nNot part of the reference run (architect, 2026-10-23); run with `gb sim --only W1`. E1's two finalists and C3's withholder runs at the final defaults, with the 10 % withholder placed as in the reference run, spread over the regions in proportion to their miners, or all in {}, the smallest region ({} % of miners) and among the farthest from the network's miners (median round trip {} ms; the farthest, {}, {} ms; as_east {} ms). The runs placed as in the reference are the reference run's E1 and C3 withholder runs under other names. Withholder amplification, with and without the PoWit deadline (`W1_withholder_placement.csv`; the withholder's slot time by region in `W1_withholder_regions.csv`):\n\n- E1's finalists, (r) first seen literal, {seeds} {} each: {}\n- C3, one seed: {}\n",
        names[distant],
        r(100.0 * ctx.cfg.sim.network.region_shares[distant], 1),
        r(rtt[distant], 0),
        names[farthest],
        r(rtt[farthest], 0),
        r(as_east, 0),
        if seeds == 1 { "seed" } else { "seeds" },
        e1_lines.join("; "),
        c3_lines.join("; ")
    );
    out
}

// ---------------------------------------------------------------------- W2

const W2: &str = "W2";

/// The traces start this long before a blackout and end this long after the pause.
const MARGIN_S: f64 = 3_600.0;

/// The pauses W2 traces, longest first.
const PAUSES: usize = 3;

/// E3's scripted blackout runs (the day-9.19 event and one blackout per region over the
/// seeds) under the configured entropy rule and failover, traced (W2).
pub(crate) fn w2_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let signers = ctx.cfg.entropy.signers.value;
    let failover = ctx.cfg.admission.failover_after_rounds.value;
    e3_jobs(ctx)
        .into_iter()
        .filter(|j| {
            !j.scripted_blackouts.is_empty()
                && j.entropy_signers == signers
                && j.failover_after_rounds == failover
        })
        .map(|mut j| {
            j.id = format!("{}-W2", j.id);
            j.section = W2.to_string();
            j.trace_blocks = true;
            j
        })
        .collect()
}

/// A pause between consecutive final-chain blocks that overlaps a scripted blackout: the run,
/// the outage record, and the trace row of the block that ended it.
struct Pause {
    run: usize,
    outage: usize,
    row: usize,
    length_s: f64,
}

/// What held a sample up, as E3's gap diagnosis classes it, and the expected wait for a block
/// with the miners holding entropy at the tip's difficulty (NaN when no miner holds it).
fn sample_state(s: &OutageSampleRow, interval_s: f64) -> (&'static str, f64) {
    if s.miners_active == 0 {
        return ("no miners", f64::NAN);
    }
    if s.miners_able == 0 {
        return ("no entropy", f64::NAN);
    }
    // 1 − (1 − p)^n, accurately for small p.
    let win = -((-s.p_step).ln_1p() * f64::from(s.miners_able)).exp_m1();
    let wait = if win > 0.0 { 1.0 / win } else { f64::INFINITY };
    let state = if wait > 4.0 * interval_s {
        "difficulty too high"
    } else {
        "other"
    };
    (state, wait)
}

const STATES: [&str; 4] = ["no miners", "no entropy", "difficulty too high", "other"];

/// The expected time to the first block over `[from, to]` (truncated at `to`), each sample's
/// state holding from its time to the next sample's (the first's from `from`), with
/// `slots(sample)` slots each succeeding with the sample's per-step probability every second.
fn expected_wait(
    inside: &[&OutageSampleRow],
    from: f64,
    to: f64,
    slots: impl Fn(&OutageSampleRow) -> u32,
) -> f64 {
    if inside.is_empty() {
        return to - from;
    }
    let (mut expected, mut log_survival) = (0.0, 0.0f64);
    for (i, s) in inside.iter().enumerate() {
        let a = if i == 0 { from } else { s.time_s };
        let b = inside.get(i + 1).map_or(to, |x| x.time_s).min(to);
        if b <= a {
            continue;
        }
        let rate = f64::from(slots(s)) * -(-s.p_step).ln_1p();
        let dt = b - a;
        let survival = log_survival.exp();
        expected += if rate > 0.0 {
            survival * -(-rate * dt).exp_m1() / rate
        } else {
            survival * dt
        };
        log_survival -= rate * dt;
    }
    expected
}

/// What each part of a pause was (seconds), by the sample in force: no miner able to get
/// entropy (or none mining), every able miner's slot held by the skip rule (SIM-48), able
/// miners out of the round for protocol reasons (sitting out under one header per height,
/// without the parent, or with a candidate in flight), and able miners still to be drawn.
fn pause_split(inside: &[&OutageSampleRow], from: f64, to: f64) -> [f64; 4] {
    let mut split = [0.0; 4];
    for (i, s) in inside.iter().enumerate() {
        let a = if i == 0 { from } else { s.time_s };
        let b = inside.get(i + 1).map_or(to, |x| x.time_s).min(to);
        if b <= a {
            continue;
        }
        let k = if s.miners_active == 0 || s.miners_able == 0 {
            0
        } else if s.tip_pool_able > 0 {
            3
        } else if s.tip_skipped_able > 0 {
            1
        } else {
            2
        };
        split[k] += b - a;
    }
    split
}

fn reason_name(reason: u8) -> &'static str {
    match reason {
        0 => "not mining",
        1 => "offline",
        _ => "no entropy",
    }
}

fn how_name(how: u8, left_s: f64, pause_to: f64) -> &'static str {
    if left_s >= pause_to {
        return "the pause's block (next round)";
    }
    match how {
        0 => "refresh",
        1 => "dropped: a block at its height held on time",
        2 => "round settled or closed",
        _ => "run ended",
    }
}

pub(crate) fn w2_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> SectionOut {
    let mut out = SectionOut::default();
    let names = region_names(ctx);
    let interval = ctx.cfg.issuance.pow_id_target_interval_s.value;
    let mut pauses = Vec::new();
    for (run, res) in results.iter().enumerate() {
        for (outage, o) in res.outages.iter().enumerate() {
            if o.kind != 2 {
                continue;
            }
            let end = o.start_s + o.duration_s;
            for (row, b) in res.block_trace.iter().enumerate() {
                if b.gap_s > 0.0 && b.broadcast_s > o.start_s && b.broadcast_s - b.gap_s < end {
                    pauses.push(Pause {
                        run,
                        outage,
                        row,
                        length_s: b.gap_s,
                    });
                }
            }
        }
    }
    pauses.sort_by(|a, b| b.length_s.total_cmp(&a.length_s));
    pauses.truncate(PAUSES);
    let mut gaps = Table::new(
        "W2_pauses.csv",
        &[
            "rank",
            "run",
            "region",
            "blackout_start_s",
            "blackout_s",
            "pause_start_since_blackout_start_s",
            "pause_s",
            "count_before",
            "count_during",
            "count_after",
            "miners_able_before",
            "miners_able_during_min",
            "miners_able_during_mean",
            "miners_able_after",
            "expected_wait_during_s",
            "pause_over_expected_wait",
            "mean_interval_next_10_s",
            "share_no_miners",
            "share_no_entropy",
            "share_difficulty_too_high",
            "share_other",
            "samples",
            "cause",
            "no_entropy_s",
            "all_able_held_by_skip_rule_s",
            "able_out_for_protocol_reasons_s",
            "able_still_to_draw_s",
            "skipped_slots_able_again",
            "held_slot_s",
            "expected_blocks_held",
            "expected_length_s",
            "expected_length_without_skip_rule_s",
            "skip_rule_share",
        ],
    );
    let mut lines = Vec::new();
    for (k, p) in pauses.iter().enumerate() {
        let rank = k + 1;
        let res = &results[p.run];
        let o = &res.outages[p.outage];
        let start = o.start_s;
        let end_row = &res.block_trace[p.row];
        let (pause_from, pause_to) = (end_row.broadcast_s - end_row.gap_s, end_row.broadcast_s);
        let (from, to) = (start - MARGIN_S, pause_to + MARGIN_S);
        let mut blocks = Table::new(
            &format!("traces/E3_pause{rank}_blocks.csv"),
            &[
                "height",
                "timestamp_s",
                "broadcast_s",
                "since_blackout_start_s",
                "gap_s",
                "count",
                "p_step",
                "factor",
                "kwcs",
                "kwcs_able",
                "miners_active",
                "miners_able",
            ],
        );
        for b in res
            .block_trace
            .iter()
            .filter(|b| b.broadcast_s >= from && b.broadcast_s <= to)
        {
            blocks.push(vec![
                n(b.height),
                n(b.timestamp_s),
                f(b.broadcast_s),
                f(b.broadcast_s - start),
                f(b.gap_s),
                n(b.count),
                f(b.p_step),
                f(b.factor),
                n(u64::from(b.kwcs)),
                n(u64::from(b.kwcs_able)),
                n(u64::from(b.miners_active)),
                n(u64::from(b.miners_able)),
            ]);
        }
        let mut samples = Table::new(
            &format!("traces/E3_pause{rank}_samples.csv"),
            &[
                "time_s",
                "since_blackout_start_s",
                "in_pause",
                "miners_active",
                "miners_able",
                "p_step",
                "expected_wait_s",
                "state",
                "tip_pool",
                "tip_pool_able",
                "tip_skipped",
                "tip_skipped_able",
            ],
        );
        let mut counts = [0u64; 4];
        let (mut able_min, mut able_max, mut able_sum, mut wait_sum, mut waits) =
            (u32::MAX, 0u32, 0u64, 0.0, 0u64);
        let mut within: Vec<&OutageSampleRow> = Vec::new();
        for s in res
            .outage_samples
            .iter()
            .filter(|s| s.outage as usize == p.outage && s.time_s >= from && s.time_s <= to)
        {
            let (state, wait) = sample_state(s, interval);
            let inside = s.time_s >= pause_from && s.time_s <= pause_to;
            if inside {
                if let Some(i) = STATES.iter().position(|x| *x == state) {
                    counts[i] += 1;
                }
                able_min = able_min.min(s.miners_able);
                able_max = able_max.max(s.miners_able);
                able_sum += u64::from(s.miners_able);
                if wait.is_finite() {
                    wait_sum += wait;
                    waits += 1;
                }
                within.push(s);
            }
            samples.push(vec![
                f(s.time_s),
                f(s.time_s - start),
                n(u64::from(inside)),
                n(u64::from(s.miners_active)),
                n(u64::from(s.miners_able)),
                f(s.p_step),
                f(wait),
                state.to_string(),
                n(u64::from(s.tip_pool)),
                n(u64::from(s.tip_pool_able)),
                n(u64::from(s.tip_skipped)),
                n(u64::from(s.tip_skipped_able)),
            ]);
        }
        // The skipped slots of the pause's round whose miner could mine again during it.
        let mut skipped = Table::new(
            &format!("traces/E3_pause{rank}_skipped.csv"),
            &[
                "miner",
                "reason",
                "skipped_since_blackout_start_s",
                "able_since_blackout_start_s",
                "left_since_blackout_start_s",
                "rejoined_by",
                "held_in_pause_s",
            ],
        );
        let (mut held_slot_s, mut held_slots) = (0.0, 0u64);
        for row in res.skip_rows.iter().filter(|x| {
            x.round_height == end_row.height && x.able_s < pause_to && x.left_s > pause_from
        }) {
            let held = (row.left_s.min(pause_to) - row.able_s.max(pause_from)).max(0.0);
            held_slot_s += held;
            held_slots += 1;
            skipped.push(vec![
                n(u64::from(row.miner)),
                reason_name(row.reason).to_string(),
                f(row.skipped_s - start),
                f(row.able_s - start),
                f(row.left_s - start),
                how_name(row.how, row.left_s, pause_to).to_string(),
                f(held),
            ]);
        }
        let p_mean = div(
            within.iter().map(|s| s.p_step).sum::<f64>(),
            within.len() as f64,
        );
        let blocks_held: f64 = {
            // Each sample's held slots for the time its state holds.
            let mut e = 0.0;
            for (i, s) in within.iter().enumerate() {
                let a = if i == 0 { pause_from } else { s.time_s };
                let b = within
                    .get(i + 1)
                    .map_or(pause_to, |x| x.time_s)
                    .min(pause_to);
                if b > a {
                    e += f64::from(s.tip_skipped_able) * -(-s.p_step).ln_1p() * (b - a);
                }
            }
            e
        };
        let split = pause_split(&within, pause_from, pause_to);
        let expected = expected_wait(&within, pause_from, pause_to, |s| s.tip_pool_able);
        let without = expected_wait(&within, pause_from, pause_to, |s| {
            s.tip_pool_able + s.tip_skipped_able
        });
        let skip_share = div(expected - without, expected);
        let inside: u64 = counts.iter().sum();
        let before = p.row.checked_sub(1).map(|i| &res.block_trace[i]);
        let after = res.block_trace.get(p.row + 1);
        let next: Vec<f64> = res.block_trace[p.row + 1..]
            .iter()
            .take(10)
            .map(|b| b.gap_s)
            .collect();
        let next_mean = div(next.iter().sum(), next.len() as f64);
        let wait = div(wait_sum, waits as f64);
        let able_mean = div(able_sum as f64, inside as f64);
        let share = |i: usize| div(counts[i] as f64, inside as f64);
        let cause = (0..4)
            .max_by_key(|i| counts[*i])
            .map_or("none", |i| STATES[i]);
        let region = names
            .get(usize::from(o.region))
            .cloned()
            .unwrap_or_default();
        gaps.push(vec![
            n(rank as u64),
            res.id.clone(),
            region.clone(),
            f(start),
            f(o.duration_s),
            f(pause_from - start),
            f(p.length_s),
            before.map_or(String::new(), |b| n(b.count)),
            n(end_row.count),
            after.map_or(String::new(), |b| n(b.count)),
            before.map_or(String::new(), |b| n(u64::from(b.miners_able))),
            if inside > 0 {
                n(u64::from(able_min))
            } else {
                String::new()
            },
            f(able_mean),
            n(u64::from(end_row.miners_able)),
            f(wait),
            f(div(p.length_s, wait)),
            f(next_mean),
            f(share(0)),
            f(share(1)),
            f(share(2)),
            f(share(3)),
            n(inside),
            cause.to_string(),
            f(split[0]),
            f(split[1]),
            f(split[2]),
            f(split[3]),
            n(held_slots),
            f(held_slot_s),
            f(blocks_held),
            f(expected),
            f(without),
            f(skip_share),
        ]);
        lines.push(format!(
            "{rank}. `{}` ({region}): a pause of {} s {}. The round's difficulty was set from a count of {} miners{}; during the pause {}–{able_max} miners could get entropy (mean {}), so that difficulty implied an expected wait of {} s ({} target intervals) and the pause lasted {} times that. The block that ended it counted {} miners, and the next 10 blocks came {} s apart on average. Samples during the pause ({inside}): no miners {} %, no entropy {} %, difficulty too high {} %, other {} % (most: {cause}). Of the pause, {} s had no miner able to get entropy, {} s had every able miner's slot held by the skip rule (SIM-48), {} s had able miners out of the round for protocol reasons, and {} s had able miners still to be drawn. {held_slots} skipped slots' miners could mine again during it, holding back {} slot-seconds ({} expected blocks at the tip's difficulty, per-step probability {}): with the skip rule the expected pause on this path is {} s, without it {} s, so the skip rule accounts for {} % of it and protocol behaviour for the rest.",
            res.id,
            r(p.length_s, 0),
            if pause_from < start {
                format!("that began {} s before the blackout", r(start - pause_from, 0))
            } else {
                format!("from {} s after the blackout began", r(pause_from - start, 0))
            },
            end_row.count,
            before.map_or(String::new(), |b| format!(
                " (the block before the pause had {} miners able to get entropy)",
                b.miners_able
            )),
            if inside > 0 { able_min } else { 0 },
            r(able_mean, 0),
            r(wait, 0),
            r(wait / interval, 1),
            r(div(p.length_s, wait), 2),
            after.map_or("–".to_string(), |b| b.count.to_string()),
            r(next_mean, 1),
            r(100.0 * share(0), 0),
            r(100.0 * share(1), 0),
            r(100.0 * share(2), 0),
            r(100.0 * share(3), 0),
            r(split[0], 0),
            r(split[1], 0),
            r(split[2], 0),
            r(split[3], 0),
            r(held_slot_s, 0),
            r(blocks_held, 2),
            r(p_mean, 4),
            r(expected, 0),
            r(without, 0),
            r(100.0 * skip_share, 0),
        ));
        out.tables.push(blocks);
        out.tables.push(samples);
        out.tables.push(skipped);
    }
    out.tables.push(gaps);
    out.summary = format!(
        "## W2 — E3's blackout traces (supplementary)\n\nNot part of the reference run (architect, 2026-10-23); run with `gb sim --only W2`. E3's {} scripted blackout runs under the configured entropy rule and failover, traced: the reference run's runs under other names (the trace only observes). For each of the {PAUSES} longest pauses between final-chain blocks during a blackout, every final-chain block (`traces/E3_pause<k>_blocks.csv`: its difficulty count, per-step success probability and correction factor, and the KWCs and miners able to issue and get entropy when it was broadcast), every round's sample (`traces/E3_pause<k>_samples.csv`, from the blackout's start: miners mining and those able to get entropy, the tip's difficulty, the expected wait, E3's diagnosis of what held it up, and the slots of the round on the tip still to be drawn and skipped, with those whose miner could mine) from an hour before the blackout to an hour after the pause, and the skipped slots of the pause's round whose miner could mine again during it (`traces/E3_pause<k>_skipped.csv`: when each was skipped, when its miner could mine again, and when it rejoined); the pauses in `W2_pauses.csv`. A round's difficulty is set from the count taken when the previous round's block arrived (SPEC §3.8), so a round open when a blackout begins keeps the difficulty of the miners counted before it. A slot drawn while its miner cannot mine is skipped, and is drawn again only when the round runs out of draws (a refresh, SIM-48) or at the next round: that skip rule is the simulator's, whereas under the protocol a miner whose KWC can issue entropy again asks for it. The expected pause without the skip rule counts those miners as mining from when they could.\n\n{}\n",
        results.len(),
        lines.join("\n")
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(cfg: &gb_config::Config) -> Ctx<'_> {
        Ctx {
            cfg,
            quick: false,
            seed: cfg.run.seed,
            days_scale: 1.0,
            replicate_scale: 1.0,
            k: cfg.run.monte_carlo.tolerance_standard_errors,
        }
    }

    /// A job's specification without its name and section.
    fn unnamed(j: &JobSpec) -> String {
        let mut j = j.clone();
        j.id.clear();
        j.section.clear();
        serde_json::to_string(&j).unwrap()
    }

    #[test]
    fn the_jobs_placed_as_in_the_reference_are_its_withholder_runs_under_other_names() {
        let cfg = gb_config::Config::default();
        let ctx = ctx(&cfg);
        let reference: Vec<String> = super::super::e::e1_stage1_jobs(&ctx)
            .iter()
            .chain(&super::super::c::c3_jobs(&ctx))
            .filter(|j| !j.groups.is_empty())
            .map(unnamed)
            .collect();
        let jobs = w1_jobs(&ctx);
        let per = jobs.len() / PLACEMENTS.len();
        assert_eq!(per, 2 * e1_seeds(&ctx) as usize + 2);
        for j in &jobs[..per] {
            assert!(
                reference.contains(&unnamed(j)),
                "{} is not a reference run",
                j.id
            );
        }
        for j in &jobs[per..] {
            assert!(!reference.contains(&unnamed(j)), "{}", j.id);
            assert!(
                j.groups_by_region || j.groups_in_region.is_some(),
                "{}",
                j.id
            );
        }
    }

    #[test]
    fn w2_traces_e3s_blackout_runs_at_the_defaults_under_other_names() {
        let cfg = gb_config::Config::default();
        let ctx = ctx(&cfg);
        let reference: Vec<String> = e3_jobs(&ctx).iter().map(unnamed).collect();
        let jobs = w2_jobs(&ctx);
        // The day-9.19 event and a blackout of each of the 8 regions over 5 seeds.
        assert_eq!(jobs.len(), 41);
        for j in &jobs {
            assert!(j.trace_blocks, "{}", j.id);
            let mut plain = j.clone();
            plain.trace_blocks = false;
            assert!(
                reference.contains(&unnamed(&plain)),
                "{} is not an E3 run",
                j.id
            );
        }
    }

    #[test]
    fn tracing_blocks_only_observes_the_run() {
        let cfg = gb_config::Config::default();
        let ctx = Ctx {
            quick: true,
            ..ctx(&cfg)
        };
        let mut traced_spec = w2_jobs(&ctx).pop().unwrap();
        traced_spec.miners = 100;
        let mut plain_spec = traced_spec.clone();
        plain_spec.trace_blocks = false;
        let traced = crate::run_job(&traced_spec, &cfg).unwrap();
        let plain = crate::run_job(&plain_spec, &cfg).unwrap();
        assert!(!traced.block_trace.is_empty());
        assert!(!traced.outage_samples.is_empty());
        assert!(plain.block_trace.is_empty() && plain.outage_samples.is_empty());
        assert!(plain.skip_rows.is_empty());
        let mut stripped = traced.clone();
        stripped.block_trace.clear();
        stripped.outage_samples.clear();
        stripped.skip_rows.clear();
        assert_eq!(
            serde_json::to_string(&stripped).unwrap(),
            serde_json::to_string(&plain).unwrap()
        );
        let out = w2_analyse(&ctx, &[traced]);
        let names: Vec<&str> = out.tables.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"traces/E3_pause1_blocks.csv"), "{names:?}");
        assert!(names.contains(&"traces/E3_pause1_samples.csv"), "{names:?}");
        assert!(names.contains(&"traces/E3_pause1_skipped.csv"), "{names:?}");
        assert!(names.contains(&"W2_pauses.csv"), "{names:?}");
    }

    #[test]
    fn the_daily_curves_only_observe_the_run() {
        let cfg = gb_config::Config::default();
        let ctx = ctx(&cfg);
        let mut sampled = super::super::e::e2_jobs(&ctx)
            .into_iter()
            .find(|j| j.availability_curve)
            .unwrap();
        sampled.miners = 100;
        sampled.registered_ids = 1_300;
        sampled.days = 1.5;
        let mut plain_spec = sampled.clone();
        plain_spec.availability_curve = false;
        let with = crate::run_job(&sampled, &cfg).unwrap();
        let plain = crate::run_job(&plain_spec, &cfg).unwrap();
        // Every 10 minutes for the half day after the first.
        let samples: f64 = with.availability_by_hour.iter().map(|h| h[0]).sum();
        assert!((60.0..=80.0).contains(&samples), "{samples}");
        assert!(plain.availability_by_hour.is_empty());
        let mut stripped = with.clone();
        stripped.availability_by_hour.clear();
        assert_eq!(
            serde_json::to_string(&stripped).unwrap(),
            serde_json::to_string(&plain).unwrap()
        );
    }
}
