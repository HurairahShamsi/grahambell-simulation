//! Honest-network experiments C1–C7 and A6 (realistic latency, loss, skew and households).

use super::{
    Chart, ChartPanel, ChartSeries, Ctx, SectionOut, Table, amplification, count_check, div, f,
    mc_check, mean_se, n, quantile, r, storm_day, storm_note,
};
use crate::job::{ForkRule, GroupSpec, JobResult, JobSpec, Strategy};
use gb_analytic::exact::decimal;
use gb_analytic::quorum_tradeoff::QuorumLayout;
use gb_analytic::validation::Check;
use gb_config::protocol::{
    CountBasedCorrection, DeadlineScope, DifficultyCountRule, DifficultyVariant, EarlyBlockRule,
    EquivocationScope, StrictOffencePenalty, UnreachableRule, WitnessSigning,
};
use gb_config::sim::LatencySource;
use num_traits::ToPrimitive;

const C1: &str = "C1";
const C2: &str = "C2";
const C3: &str = "C3";
const C4: &str = "C4";
const C5: &str = "C5";
const C6: &str = "C6";
const C7: &str = "C7";
const A6: &str = "A6";

pub(crate) fn realistic(
    ctx: &Ctx<'_>,
    id: &str,
    section: &str,
    rep: u64,
    miners: u32,
    registered: u64,
    days: f64,
) -> JobSpec {
    let mut j = ctx.job(id, section, rep);
    j.miners = miners;
    j.registered_ids = registered;
    j.days = days + j.warmup_days;
    if ctx.quick {
        j.warmup_days = 0.1;
        j.days = days + 0.1;
    }
    j
}

fn tags() -> Vec<GroupSpec> {
    vec![
        GroupSpec {
            label: "x=0.1".to_string(),
            share: 0.1,
            strategy: Strategy::Honest,
            always_online: false,
        },
        GroupSpec {
            label: "x=0.5".to_string(),
            share: 0.5,
            strategy: Strategy::Honest,
            always_online: false,
        },
    ]
}

pub(crate) fn region_names(ctx: &Ctx<'_>) -> Vec<String> {
    ctx.cfg.sim.network.regions.clone()
}

// ---------------------------------------------------------------------- C1

fn c1_sizes(ctx: &Ctx<'_>) -> Vec<u32> {
    ctx.pick(vec![1_000, 10_000, 100_000], vec![300])
}

pub(crate) fn c1_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    let seeds = ctx.reps(2, 1);
    let registered = ctx.pick(290_000, 2_900);
    let days = ctx.days(30.0, 0.5);
    for m in c1_sizes(ctx) {
        for s in 0..seeds {
            let mut j = realistic(
                ctx,
                &format!("C1-m{m}-literal-s{s}"),
                C1,
                s,
                m,
                registered,
                days,
            );
            j.groups = tags();
            jobs.push(j);
        }
    }
    let m = ctx.pick(10_000, 300);
    for s in 0..seeds {
        let mut j = realistic(
            ctx,
            &format!("C1-m{m}-same-parent-s{s}"),
            C1,
            s,
            m,
            registered,
            days,
        );
        j.groups = tags();
        j.equivocation = EquivocationScope::SameParent;
        jobs.push(j);
    }
    let mut ideal = ctx.job("C1-idealised-control", C1, 0).idealised();
    ideal.miners = ctx.pick(1_000, 200);
    ideal.registered_ids = ctx.pick(2_900, 1_300);
    ideal.days = ctx.days(10.0, 0.5);
    ideal.groups = tags();
    jobs.push(ideal);
    jobs
}

/// The tagged groups' blocks and mining time by region and by device (M3a.1: the honest 10 %
/// group's deficit). For each group and each region (or device): the group's share of its own
/// slot time there and the network's, its blocks per slot-second there relative to the whole
/// network's, and its amplification within that region (or device).
fn group_breakdown(runs: &[&JobResult], m: u32, regions: &[String], table: &mut Table) {
    let devices = [
        "always_on".to_string(),
        "desktop".to_string(),
        "laptop".to_string(),
    ];
    let total_ids: f64 = runs.iter().map(|r| r.canonical as f64).sum();
    let total_slot: f64 = runs
        .iter()
        .map(|r| r.slot_s_by_group.iter().sum::<f64>())
        .sum();
    for (by, names, ids_of, slot_of) in [
        (
            "region",
            regions,
            (|r: &JobResult| &r.ids_by_group_region) as fn(&JobResult) -> &Vec<Vec<u64>>,
            (|r: &JobResult| &r.slot_s_by_group_region) as fn(&JobResult) -> &Vec<Vec<f64>>,
        ),
        (
            "device",
            &devices[..],
            |r: &JobResult| &r.ids_by_group_device,
            |r: &JobResult| &r.slot_s_by_group_device,
        ),
    ] {
        for (g, label) in [(1usize, "x=0.1"), (2, "x=0.5"), (0, "untagged")] {
            let cell = |x: usize| {
                let ids: f64 = runs
                    .iter()
                    .map(|r| {
                        ids_of(r)
                            .get(g)
                            .and_then(|v| v.get(x))
                            .copied()
                            .unwrap_or(0) as f64
                    })
                    .sum();
                let slot: f64 = runs
                    .iter()
                    .map(|r| {
                        slot_of(r)
                            .get(g)
                            .and_then(|v| v.get(x))
                            .copied()
                            .unwrap_or(0.0)
                    })
                    .sum();
                let all_ids: f64 = runs
                    .iter()
                    .map(|r| {
                        ids_of(r)
                            .iter()
                            .map(|v| v.get(x).copied().unwrap_or(0) as f64)
                            .sum::<f64>()
                    })
                    .sum();
                let all_slot: f64 = runs
                    .iter()
                    .map(|r| {
                        slot_of(r)
                            .iter()
                            .map(|v| v.get(x).copied().unwrap_or(0.0))
                            .sum::<f64>()
                    })
                    .sum();
                (ids, slot, all_ids, all_slot)
            };
            let group_slot: f64 = (0..names.len()).map(|x| cell(x).1).sum();
            for (x, name) in names.iter().enumerate() {
                let (ids, slot, all_ids, all_slot) = cell(x);
                table.push(vec![
                    n(u64::from(m)),
                    label.to_string(),
                    by.to_string(),
                    name.clone(),
                    f(ids),
                    f(div(slot, group_slot)),
                    f(div(all_slot, total_slot)),
                    f(div(div(ids, slot), div(total_ids, total_slot))),
                    f(div(div(ids, all_ids), div(slot, all_slot))),
                ]);
            }
        }
    }
}

pub(crate) fn c1_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> SectionOut {
    let mut out = SectionOut::default();
    let seeds = ctx.reps(2, 1) as usize;
    let sizes = c1_sizes(ctx);
    let mut table = Table::new(
        "C1_amplification.csv",
        &[
            "miners",
            "rule",
            "group",
            "slot_share_nominal",
            "final_chain_ids",
            "amplification",
            "se",
            "ci95_low",
            "ci95_high",
            "day_batches",
        ],
    );
    let regions = region_names(ctx);
    let mut by_region = Table::new(
        "C1_amplification_by_region.csv",
        &[
            "miners",
            "region",
            "final_chain_ids",
            "id_share",
            "slot_share",
            "amplification",
            "se",
            "ci95_low",
            "ci95_high",
        ],
    );
    let mut summary = Vec::new();
    let mut chart_points: Vec<(String, Vec<(f64, f64)>)> = vec![
        ("x = 0.1".to_string(), Vec::new()),
        ("x = 0.5".to_string(), Vec::new()),
    ];
    let row = |runs: &[&JobResult],
               m: u32,
               rule: &str,
               table: &mut Table,
               summary: &mut Vec<String>,
               chart: &mut Vec<(String, Vec<(f64, f64)>)>| {
        for (g, share) in [(1usize, 0.1), (2, 0.5)] {
            let (amp, se, batches) = amplification(runs, g);
            let ids: u64 = runs
                .iter()
                .map(|r| r.ids_by_group.get(g).copied().unwrap_or(0))
                .sum();
            let (lo, hi) = super::ci95(amp, se);
            table.push(vec![
                n(u64::from(m)),
                rule.to_string(),
                format!("x={share}"),
                f(share),
                n(ids),
                f(amp),
                f(se),
                f(lo),
                f(hi),
                n(batches),
            ]);
            summary.push(format!(
                "{m} miners, {rule}, x = {share}: amplification {} (95 % CI {}–{}){}",
                r(amp, 4),
                r(lo, 4),
                r(hi, 4),
                storm_note(runs)
            ));
            if rule == "literal" {
                chart[g - 1].1.push((f64::from(m), amp));
            }
        }
    };
    let mut breakdown = Table::new(
        "C1_group_breakdown.csv",
        &[
            "miners",
            "group",
            "by",
            "value",
            "final_chain_ids",
            "group_slot_share",
            "network_slot_share",
            "win_rate_vs_network",
            "amplification_within",
        ],
    );
    let mut i = 0;
    for m in &sizes {
        let runs: Vec<&JobResult> = results[i..i + seeds].iter().collect();
        i += seeds;
        row(
            &runs,
            *m,
            "literal",
            &mut table,
            &mut summary,
            &mut chart_points,
        );
        group_breakdown(&runs, *m, &regions, &mut breakdown);
        // By region, pooled over seeds.
        for (rg, name) in regions.iter().enumerate() {
            let mut daily = Vec::new();
            let (mut ids, mut all_ids, mut slot, mut all_slot) = (0.0, 0.0, 0.0, 0.0);
            for res in &runs {
                let days = res
                    .daily_ids_by_region
                    .len()
                    .min(res.daily_slot_s_by_region.len());
                for d in 0..days {
                    let gi = res.daily_ids_by_region[d].get(rg).copied().unwrap_or(0) as f64;
                    let ti: f64 = res.daily_ids_by_region[d].iter().map(|x| *x as f64).sum();
                    let gs = res.daily_slot_s_by_region[d]
                        .get(rg)
                        .copied()
                        .unwrap_or(0.0);
                    let ts: f64 = res.daily_slot_s_by_region[d].iter().sum();
                    if ti > 0.0 && ts > 0.0 && gs > 0.0 {
                        daily.push((gi / ti) / (gs / ts));
                    }
                    ids += gi;
                    all_ids += ti;
                    slot += gs;
                    all_slot += ts;
                }
            }
            let amp = div(div(ids, all_ids), div(slot, all_slot));
            let (_, se) = mean_se(&daily);
            let (lo, hi) = super::ci95(amp, se);
            by_region.push(vec![
                n(u64::from(*m)),
                name.clone(),
                f(ids),
                f(div(ids, all_ids)),
                f(div(slot, all_slot)),
                f(amp),
                f(se),
                f(lo),
                f(hi),
            ]);
        }
    }
    let runs: Vec<&JobResult> = results[i..i + seeds].iter().collect();
    i += seeds;
    row(
        &runs,
        ctx.pick(10_000, 300),
        "same-parent [P]",
        &mut table,
        &mut summary,
        &mut chart_points,
    );
    let control = &results[i];
    for (g, share) in [(1usize, 0.1), (2, 0.5)] {
        let (amp, se, batches) = amplification(&[control], g);
        out.checks.push(mc_check(
            C1,
            &format!("C1-idealised-control-x{share}"),
            "idealised network: a tagged group's amplification vs 1",
            1.0,
            amp,
            se,
            batches,
            ctx.k,
            0.0,
        ));
    }
    out.tables.push(table);
    out.tables.push(by_region);
    out.tables.push(breakdown);
    out.charts.push(Chart {
        file: "C1_amplification.png".to_string(),
        title: "C1 — honest control: amplification of a tagged group".to_string(),
        subtitle: "realistic network and households, literal one-header rule; H1 asks the CI to lie in [0.95, 1.05]".to_string(),
        panels: vec![ChartPanel {
            title: "amplification by network size".to_string(),
            x_label: "miners".to_string(),
            y_label: "ID share ÷ slot share".to_string(),
            log_x: true,
            series: chart_points.into_iter().map(|(label, points)| ChartSeries { label, points }).collect(),
            references: vec![("1".to_string(), 1.0), ("0.95".to_string(), 0.95), ("1.05".to_string(), 1.05)],
        }],
    });
    out.summary = format!(
        "## C1 — S1 honest baseline (H1 control)\n\nA tagged group of honest slots wins IDs in proportion to its mining slot-time (`C1_amplification.csv`, by region in `C1_amplification_by_region.csv`):\n\n{}\n",
        summary
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    out
}

// ---------------------------------------------------------------------- C2

const LOSSES: [f64; 4] = [0.0, 0.005, 0.02, 0.05];

fn c2_rules() -> [(ForkRule, &'static str); 3] {
    [
        (ForkRule::V06, "v0.6"),
        (ForkRule::FirstSeen, "first-seen"),
        (ForkRule::LowerHashOnly, "lower-hash-only"),
    ]
}

pub(crate) fn c2_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    let m = ctx.pick(10_000, 300);
    let registered = ctx.pick(29_000, 2_900);
    let days = ctx.days(10.0, 0.5);
    for (rule, name) in c2_rules() {
        for (scope, sname) in [
            (EquivocationScope::SameHeight, "literal"),
            (EquivocationScope::SameParent, "same-parent"),
        ] {
            for loss in ctx.pick(LOSSES.to_vec(), vec![0.0, 0.05]) {
                let mut j = realistic(
                    ctx,
                    &format!("C2-{name}-{sname}-loss{loss}"),
                    C2,
                    0,
                    m,
                    registered,
                    days,
                );
                j.fork_rule = rule;
                j.equivocation = scope;
                j.loss = loss;
                jobs.push(j);
            }
        }
    }
    let mut stylised = realistic(ctx, "C2-v0.6-literal-stylised", C2, 0, m, registered, days);
    stylised.latency = LatencySource::Stylised;
    jobs.push(stylised);
    let mut part = realistic(
        ctx,
        "C2-v0.6-literal-partitions",
        C2,
        0,
        m,
        registered,
        days,
    );
    part.partition_rate_per_region_per_day = 1.0;
    part.partition_length_s = 600.0;
    jobs.push(part);
    let mut ideal = ctx.job("C2-idealised", C2, 0).idealised();
    ideal.miners = ctx.pick(1_000, 200);
    ideal.registered_ids = ctx.pick(2_900, 1_300);
    ideal.days = ctx.days(10.0, 0.5);
    jobs.push(ideal);
    jobs
}

fn fork_row(label: &[String], res: &JobResult) -> Vec<String> {
    let mut row = label.to_vec();
    row.extend([
        n(res.canonical),
        n(res.orphans),
        f(div(res.orphans as f64, res.canonical as f64)),
        n(res.fork_heights),
        f(div(res.fork_heights as f64, res.canonical as f64)),
        n(res.loser_lower_hash),
        f(div(res.loser_lower_hash as f64, res.orphans as f64)),
        n(res.sat_out),
        n(res.max_reorg),
        n(res.deep_reorgs),
        f(div(res.interval_sum_s, res.interval_count as f64)),
        n(res.pauses),
        f(res.pause_s),
        storm_day(res),
    ]);
    row
}

const FORK_COLUMNS: [&str; 14] = [
    "final_chain_blocks",
    "orphans",
    "orphan_rate",
    "fork_heights",
    "fork_rate",
    "orphan_had_lower_hash",
    "share_lower_hash_loser",
    "sat_out",
    "max_reorg",
    "reorgs_deeper_than_confirmation",
    "mean_interval_s",
    "pauses_over_120s",
    "pause_seconds",
    "fork_storm_day",
];

pub(crate) fn c2_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> SectionOut {
    let mut out = SectionOut::default();
    let mut header = vec!["rule", "header_rule", "loss", "latency", "partitions"];
    header.extend(FORK_COLUMNS);
    let mut table = Table::new("C2_fork_choice.csv", &header);
    let mut i = 0;
    let mut summary = Vec::new();
    let mut chart: Vec<ChartSeries> = Vec::new();
    for (_, name) in c2_rules() {
        for sname in ["literal", "same-parent"] {
            let mut pts = Vec::new();
            for loss in ctx.pick(LOSSES.to_vec(), vec![0.0, 0.05]) {
                let res = &results[i];
                i += 1;
                table.push(fork_row(
                    &[
                        name.to_string(),
                        sname.to_string(),
                        f(loss),
                        "ripe_atlas".to_string(),
                        "none".to_string(),
                    ],
                    res,
                ));
                pts.push((
                    100.0 * loss,
                    100.0 * div(res.orphans as f64, res.canonical as f64),
                ));
                if loss == 0.0 {
                    summary.push(format!(
                        "{name}, {sname}: orphan rate {} %, fork heights {} %, lower-hash loser {} % of orphans, max reorg {}, deeper than confirmation {}",
                        r(100.0 * div(res.orphans as f64, res.canonical as f64), 2),
                        r(100.0 * div(res.fork_heights as f64, res.canonical as f64), 2),
                        r(100.0 * div(res.loser_lower_hash as f64, res.orphans as f64), 1),
                        res.max_reorg,
                        res.deep_reorgs
                    ));
                }
            }
            if sname == "literal" {
                chart.push(ChartSeries {
                    label: name.to_string(),
                    points: pts,
                });
            }
        }
    }
    let stylised = &results[i];
    table.push(fork_row(
        &[
            "v0.6".to_string(),
            "literal".to_string(),
            "0".to_string(),
            "stylised".to_string(),
            "none".to_string(),
        ],
        stylised,
    ));
    let part = &results[i + 1];
    table.push(fork_row(
        &[
            "v0.6".to_string(),
            "literal".to_string(),
            "0".to_string(),
            "ripe_atlas".to_string(),
            "1/region/day, 10 min".to_string(),
        ],
        part,
    ));
    let ideal = &results[i + 2];
    table.push(fork_row(
        &[
            "v0.6".to_string(),
            "literal".to_string(),
            "0".to_string(),
            "idealised".to_string(),
            "none".to_string(),
        ],
        ideal,
    ));
    // Zero latency: the K winners tied at the earliest step decide one after another, and a
    // miner that already holds a better block does not broadcast, so the blocks broadcast are
    // the records (successive minima) of the hash order: H_K of them in expectation, H_K the
    // harmonic number. Orphans per round are E[H_K − 1]. Until 2026-10-14 this check used
    // E[K − 1], which assumes every tied winner broadcasts; it failed in the M3a reference run
    // (ASSUMPTIONS SIM-30).
    let m = u64::from(ctx.pick(1_000u32, 200));
    let expected: f64 = (2..=12u64)
        .map(|k| {
            let harmonic: f64 = (1..=k).map(|i| 1.0 / i as f64).sum();
            (harmonic - 1.0) * gb_analytic::ties::tie_size_probability(m, 30.0, k)
        })
        .sum();
    let per_round = div(ideal.orphans as f64, ideal.canonical as f64);
    out.checks.push(mc_check(
        C2,
        "C2-idealised-orphans",
        &format!("zero latency: orphans per final-chain block vs E[H_K − 1] (K tied winners, M1 F1; H_K the harmonic number; {m} slots)"),
        expected,
        per_round,
        (expected / ideal.canonical.max(1) as f64).sqrt(),
        ideal.canonical,
        ctx.k,
        0.0,
    ));
    out.tables.push(table);
    out.charts.push(Chart {
        file: "C2_orphan_rate.png".to_string(),
        title: "C2 — orphan rate by fork-choice rule".to_string(),
        subtitle: "realistic network and households, literal one-header rule".to_string(),
        panels: vec![ChartPanel {
            title: "orphans per final-chain block".to_string(),
            x_label: "message loss (%)".to_string(),
            y_label: "orphan rate (%)".to_string(),
            log_x: false,
            series: chart,
            references: Vec::new(),
        }],
    });
    out.summary = format!(
        "## C2 — fork choice under latency\n\nOrphan and fork rates for the v0.6 rule, first-seen and lower-hash-only (`C2_fork_choice.csv`; no loss below):\n\n{}\n",
        summary
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    out
}

// ---------------------------------------------------------------------- C3

fn deadlines() -> Vec<Option<f64>> {
    vec![None, Some(5.0), Some(10.0), Some(30.0), Some(60.0)]
}

fn c3_configs() -> Vec<(Option<f64>, DeadlineScope)> {
    let mut v = vec![(None, DeadlineScope::Permanent)];
    for d in deadlines().into_iter().skip(1) {
        v.push((d, DeadlineScope::Permanent));
        v.push((d, DeadlineScope::NotExtendedOnly));
    }
    v
}

fn scope_name(s: DeadlineScope) -> &'static str {
    match s {
        DeadlineScope::Permanent => "permanent",
        DeadlineScope::NotExtendedOnly => "not_extended_only",
    }
}

pub(crate) fn c3_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    let m = ctx.pick(10_000, 300);
    let registered = ctx.pick(29_000, 2_900);
    let days = ctx.days(10.0, 0.5);
    for withhold in [false, true] {
        for (d, scope) in c3_configs() {
            let label = format!(
                "C3-{}-D{}-{}",
                if withhold { "withholder" } else { "honest" },
                d.map_or("none".to_string(), |x| x.to_string()),
                scope_name(scope)
            );
            let mut j = realistic(ctx, &label, C3, 0, m, registered, days);
            j.deadline = d;
            j.deadline_scope = scope;
            j.loss = 0.005;
            j.partition_rate_per_region_per_day = 1.0;
            j.partition_length_s = 300.0;
            if withhold {
                j.groups = vec![GroupSpec {
                    label: "withholder".to_string(),
                    share: 0.1,
                    strategy: Strategy::Withhold,
                    always_online: false,
                }];
            }
            jobs.push(j);
        }
    }
    jobs
}

pub(crate) fn c3_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> SectionOut {
    let mut out = SectionOut::default();
    let configs = c3_configs();
    let regions = region_names(ctx);
    let mut table = Table::new(
        "C3_powit_deadline.csv",
        &[
            "deadline_s",
            "scope",
            "final_chain_blocks",
            "blocks_late_at_panel_majority",
            "late_share",
            "fork_heights",
            "fork_rate",
            "orphan_rate",
            "split_panel_nodes",
            "max_reorg",
            "reorgs_deeper_than_confirmation",
            "pauses_over_120s",
            "fork_storm_day",
        ],
    );
    let mut regional = Table::new(
        "C3_late_by_region.csv",
        &[
            "deadline_s",
            "scope",
            "region",
            "blocks",
            "late_at_majority",
            "late_share",
        ],
    );
    let mut withhold = Table::new(
        "C3_withholder.csv",
        &[
            "deadline_s",
            "scope",
            "withheld",
            "withheld_on_final_chain",
            "success_rate",
            "honest_blocks_displaced",
            "withholder_amplification",
            "se",
            "fork_storm_day",
        ],
    );
    let base_forks = div(results[0].fork_heights as f64, results[0].canonical as f64);
    let mut summary = Vec::new();
    let mut late_pts = Vec::new();
    let mut success_pts = Vec::new();
    for (i, (d, scope)) in configs.iter().enumerate() {
        let res = &results[i];
        let dl = d.map_or("none".to_string(), |x| x.to_string());
        table.push(vec![
            dl.clone(),
            scope_name(*scope).to_string(),
            n(res.canonical),
            n(res.late_majority),
            f(div(res.late_majority as f64, res.blocks as f64)),
            n(res.fork_heights),
            f(div(res.fork_heights as f64, res.canonical as f64)),
            f(div(res.orphans as f64, res.canonical as f64)),
            n(res.split_panel_nodes),
            n(res.max_reorg),
            n(res.deep_reorgs),
            n(res.pauses),
            storm_day(res),
        ]);
        for (rg, name) in regions.iter().enumerate() {
            let blocks = res.blocks_by_region.get(rg).copied().unwrap_or(0);
            let late = res.late_majority_by_region.get(rg).copied().unwrap_or(0);
            regional.push(vec![
                dl.clone(),
                scope_name(*scope).to_string(),
                name.clone(),
                n(blocks),
                n(late),
                f(div(late as f64, blocks as f64)),
            ]);
        }
        if *scope == DeadlineScope::Permanent {
            late_pts.push((
                d.unwrap_or(120.0),
                100.0 * div(res.late_majority as f64, res.blocks as f64),
            ));
        }
        if d.is_none() {
            out.checks.push(count_check(
                C3,
                "C3-no-deadline-no-late",
                "D = none: no block is rejected as late at any panel node",
                0,
                res.late_pairs,
            ));
        }
        let wres = &results[configs.len() + i];
        let (amp, se, _) = amplification(&[wres], 1);
        let success = div(wres.withheld_won as f64, wres.withheld as f64);
        withhold.push(vec![
            dl.clone(),
            scope_name(*scope).to_string(),
            n(wres.withheld),
            n(wres.withheld_won),
            f(success),
            n(wres.displaced_by_withheld),
            f(amp),
            f(se),
            storm_day(wres),
        ]);
        if *scope == DeadlineScope::Permanent {
            success_pts.push((d.unwrap_or(120.0), 100.0 * success));
        }
        summary.push(format!(
            "D = {dl} ({}): {} % of blocks late at the panel majority, fork rate {} % (no deadline: {} %), split panel nodes {}, deepest reorg {} ({} below a confirmed ID){}; withholder success {} %, amplification {}{}",
            scope_name(*scope),
            r(100.0 * div(res.late_majority as f64, res.blocks as f64), 3),
            r(100.0 * div(res.fork_heights as f64, res.canonical as f64), 2),
            r(100.0 * base_forks, 2),
            res.split_panel_nodes,
            res.max_reorg,
            res.deep_reorgs,
            storm_note(&[res]),
            r(100.0 * success, 1),
            r(amp, 3),
            storm_note(&[wres])
        ));
    }
    out.tables.push(table);
    out.tables.push(regional);
    out.tables.push(withhold);
    out.charts.push(Chart {
        file: "C3_powit_deadline.png".to_string(),
        title: "C3 — PoWit arrival deadline D".to_string(),
        subtitle: "permanent scope; x = 120 stands for no deadline".to_string(),
        panels: vec![
            ChartPanel {
                title: "honest blocks late at the panel majority".to_string(),
                x_label: "D (s)".to_string(),
                y_label: "% of blocks".to_string(),
                log_x: true,
                series: vec![ChartSeries {
                    label: "honest network".to_string(),
                    points: late_pts,
                }],
                references: Vec::new(),
            },
            ChartPanel {
                title: "withholder success".to_string(),
                x_label: "D (s)".to_string(),
                y_label: "% of withheld blocks on the final chain".to_string(),
                log_x: true,
                series: vec![ChartSeries {
                    label: "withholder (10 % of slots)".to_string(),
                    points: success_pts,
                }],
                references: Vec::new(),
            },
        ],
    });
    out.summary = format!(
        "## C3 — PoWit arrival deadline and the withholder\n\nWith partitions (one per region per day, 5 min) and 0.5 % loss (`C3_powit_deadline.csv`, `C3_late_by_region.csv`, `C3_withholder.csv`; withholder values are measurements, not checks):\n\n{}\n",
        summary
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    out
}

// ---------------------------------------------------------------------- C4

fn c4_variants() -> Vec<(
    &'static str,
    DifficultyVariant,
    Option<CountBasedCorrection>,
)> {
    vec![
        ("A (K=144, 4×)", DifficultyVariant::BitcoinRetarget, None),
        ("B", DifficultyVariant::CountBased, None),
        (
            "B+P1",
            DifficultyVariant::CountBased,
            Some(CountBasedCorrection::RecentIntervalRatio {
                window_blocks: 144,
                bound: 0.1,
            }),
        ),
        (
            "B+P2",
            DifficultyVariant::CountBased,
            Some(CountBasedCorrection::Accumulated {
                window_blocks: 144,
                bound: 0.1,
            }),
        ),
        (
            "B-exact",
            DifficultyVariant::CountBased,
            Some(CountBasedCorrection::DiscreteExact),
        ),
    ]
}

fn churns() -> [(&'static str, Option<Vec<f64>>); 2] {
    [
        ("realistic", None),
        ("laptop-heavy", Some(vec![0.1, 0.1, 0.8])),
    ]
}

fn count_rules() -> [(DifficultyCountRule, &'static str); 2] {
    [
        (DifficultyCountRule::AfterPostAdmissionWait, "after_wait"),
        (
            DifficultyCountRule::AllGloballyActive,
            "all_globally_active",
        ),
    ]
}

pub(crate) fn c4_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    let m = ctx.pick(10_000, 300);
    let registered = ctx.pick(29_000, 2_900);
    let days = ctx.days(20.0, 0.5);
    for (name, variant, correction) in c4_variants() {
        for (rule, rname) in count_rules() {
            for (churn, mix) in churns() {
                let mut j = realistic(
                    ctx,
                    &format!("C4-{name}-{rname}-{churn}"),
                    C4,
                    0,
                    m,
                    registered,
                    days,
                );
                j.difficulty = variant;
                j.correction = correction;
                j.count_rule = rule;
                j.device_mix = mix;
                jobs.push(j);
            }
        }
    }
    jobs
}

pub(crate) fn c4_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> SectionOut {
    let _ = ctx;
    let mut out = SectionOut::default();
    let mut table = Table::new(
        "C4_issuance_rate.csv",
        &[
            "difficulty",
            "count_rule",
            "churn",
            "final_chain_blocks",
            "mean_interval_s",
            "deviation_from_30s_percent",
            "ids_per_day_mean",
            "ids_per_day_sd",
            "ids_per_half_day_sd",
            "final_factor_ppm",
            "pauses_over_120s",
            "fork_storm_day",
        ],
    );
    let mut i = 0;
    let mut summary = Vec::new();
    for (name, _, _) in c4_variants() {
        for (_, rname) in count_rules() {
            for (churn, _) in churns() {
                let res = &results[i];
                i += 1;
                let mean = div(res.interval_sum_s, res.interval_count as f64);
                let days: Vec<f64> = res.daily_ids.iter().skip(1).map(|x| *x as f64).collect();
                let (day_mean, _) = mean_se(&days);
                let day_sd = sd(&days);
                let halves: Vec<f64> = res.halfday_ids.iter().skip(2).map(|x| *x as f64).collect();
                table.push(vec![
                    name.to_string(),
                    rname.to_string(),
                    churn.to_string(),
                    n(res.canonical),
                    f(mean),
                    f(100.0 * (mean - 30.0) / 30.0),
                    f(day_mean),
                    f(day_sd),
                    f(sd(&halves)),
                    res.factor_ppm.last().map_or(String::new(), |x| n(*x)),
                    n(res.pauses),
                    storm_day(res),
                ]);
                if churn == "realistic" && rname == "after_wait" {
                    summary.push(format!(
                        "{name}: mean interval {} s ({} % from 30 s), daily IDs {} ± {} (SD)",
                        r(mean, 2),
                        r(100.0 * (mean - 30.0) / 30.0, 2),
                        r(day_mean, 0),
                        r(day_sd, 0)
                    ));
                }
            }
        }
    }
    out.tables.push(table);
    out.summary = format!(
        "## C4 — issuance rate under churn\n\nMean interval and stability per difficulty rule (`C4_issuance_rate.csv`; realistic churn, count after the post-admission wait):\n\n{}\n",
        summary
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    out
}

fn sd(values: &[f64]) -> f64 {
    let (mean, _) = mean_se(values);
    if values.len() < 2 {
        return f64::NAN;
    }
    (values.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (values.len() - 1) as f64).sqrt()
}

// ---------------------------------------------------------------------- C5

fn c5_rules() -> Vec<(&'static str, UnreachableRule, StrictOffencePenalty)> {
    vec![
        (
            "strict, ladder",
            UnreachableRule::Strict,
            StrictOffencePenalty::Ladder,
        ),
        (
            "strict, ladder with yearly reset [P]",
            UnreachableRule::Strict,
            StrictOffencePenalty::LadderYearlyReset,
        ),
        (
            "strict, immediate ban",
            UnreachableRule::Strict,
            StrictOffencePenalty::ImmediateBan,
        ),
        (
            "unreachable is not refusal [P]",
            UnreachableRule::UnreachableIsNotRefusal,
            StrictOffencePenalty::Ladder,
        ),
    ]
}

const WAITS: [u32; 3] = [1, 2, 5];

pub(crate) fn c5_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    let m = ctx.pick(10_000, 300);
    let registered = ctx.pick(290_000, 2_900);
    let days = ctx.days(30.0, 0.5);
    for w in WAITS {
        for (name, rule, penalty) in c5_rules() {
            let mut j = realistic(ctx, &format!("C5-W{w}-{name}"), C5, 0, m, registered, days);
            j.one_chance_wait_rounds = w;
            j.unreachable = rule;
            j.strict_penalty = penalty;
            j.switches_per_day = 0.2;
            jobs.push(j);
        }
    }
    // Mechanics: scripted crashes of one witness, nothing else going offline.
    for w in WAITS {
        let mut j = ctx.job(&format!("C5-scripted-W{w}"), C5, 0);
        j.miners = ctx.pick(300, 100);
        j.registered_ids = ctx.pick(2_900, 1_300);
        j.outages = false;
        j.one_chance_wait_rounds = w;
        j.days = 1.0;
        j.scripted_crashes = (1..=5).map(|i| (0.1 * f64::from(i), 400.0)).collect();
        jobs.push(j);
    }
    jobs
}

pub(crate) fn c5_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> SectionOut {
    let mut out = SectionOut::default();
    let regions = region_names(ctx);
    let devices = ["always_on", "desktop_on", "laptop_night"];
    let mut table = Table::new(
        "C5_one_chance.csv",
        &[
            "one_chance_wait_rounds",
            "rule",
            "witness_crashes",
            "stall_mean_s",
            "stall_p50_s",
            "stall_p90_s",
            "candidates_stalled",
            "short_glitches",
            "short_glitches_recorded_offline",
            "share_recorded",
            "penalties_per_id_year",
            "bans_per_id_year",
            "offline_records_per_id_year",
            "switch_mean_s",
            "switch_p50_s",
            "pauses_over_120s",
            "pause_seconds",
        ],
    );
    let mut loss = Table::new(
        "C5_honest_losses_by_region_device.csv",
        &[
            "one_chance_wait_rounds",
            "rule",
            "region",
            "device",
            "id_years",
            "penalties",
            "bans",
            "bans_per_id_year",
        ],
    );
    let mut summary = Vec::new();
    let mut switches = Vec::new();
    let mut i = 0;
    for w in WAITS {
        for (name, _, _) in c5_rules() {
            let res = &results[i];
            i += 1;
            let id_years: f64 = res.witness_id_s.iter().flatten().sum::<f64>()
                / ctx.cfg.model.seconds_per_year as f64;
            let penalties: u64 = res.penalties.iter().flatten().sum();
            let bans: u64 = res.bans.iter().flatten().sum();
            let records: u64 = res.offline_records.iter().flatten().sum();
            let (stall, _) = mean_se(&res.stall_samples);
            switches.extend(res.switch_s.iter().copied());
            table.push(vec![
                n(u64::from(w)),
                name.to_string(),
                n(res.witness_crashes),
                f(stall),
                f(quantile(&res.stall_samples, 0.5)),
                f(quantile(&res.stall_samples, 0.9)),
                n(res.stalled_candidates),
                n(res.short_glitches),
                n(res.short_glitches_recorded),
                f(div(
                    res.short_glitches_recorded as f64,
                    res.short_glitches as f64,
                )),
                f(div(penalties as f64, id_years)),
                f(div(bans as f64, id_years)),
                f(div(records as f64, id_years)),
                f(mean_se(&res.switch_s).0),
                f(quantile(&res.switch_s, 0.5)),
                n(res.pauses),
                f(res.pause_s),
            ]);
            for (rg, region) in regions.iter().enumerate() {
                for (dv, device) in devices.iter().enumerate() {
                    let years = res.witness_id_s[rg][dv] / ctx.cfg.model.seconds_per_year as f64;
                    let b = res.bans[rg][dv];
                    loss.push(vec![
                        n(u64::from(w)),
                        name.to_string(),
                        region.clone(),
                        (*device).to_string(),
                        f(years),
                        n(res.penalties[rg][dv]),
                        n(b),
                        f(div(b as f64, years)),
                    ]);
                }
            }
            summary.push(format!(
                "W = {w}, {name}: stalls {} s mean, glitches under the grace epoch recorded offline {} %, honest IDs lost {} % per year, global pauses {}{}",
                r(stall, 1),
                r(100.0 * div(res.short_glitches_recorded as f64, res.short_glitches as f64), 1),
                r(100.0 * div(bans as f64, id_years), 2),
                res.pauses,
                storm_note(&[res])
            ));
        }
    }
    for w in WAITS {
        let res = &results[i];
        i += 1;
        let (stall, _) = mean_se(&res.stall_samples);
        let expected = f64::from(w) * 30.0;
        out.checks.push(Check::absolute(
            C5,
            &format!("C5-scripted-stall-W{w}"),
            &format!("a scripted unannounced crash stalls its KWCs' entropy for W·30 s = {expected} s plus the vote (within 1 s)"),
            expected,
            stall,
            1.0,
        ));
    }
    if !switches.is_empty() {
        let min = switches.iter().copied().fold(f64::INFINITY, f64::min);
        out.checks.push(Check::at_least(
            C5,
            "C5-switch-minimum",
            "switching KWC takes at least the grace epoch plus admission (450 s)",
            450.0,
            min,
            0.0,
            switches.len() as u64,
        ));
        let median = quantile(&switches, 0.5);
        out.checks.push(Check::at_most(
            C5,
            "C5-switch-median",
            "median KWC switch time at most 460 s",
            460.0,
            median,
            0.0,
            switches.len() as u64,
        ));
    }
    out.tables.push(table);
    out.tables.push(loss);
    out.summary = format!(
        "## C5 — timing the SPEC promises, and honest penalties\n\nKWC switches take {} s at the median (\"a few minutes\"). Entropy stalls and penalties by One Chance wait and rule (`C5_one_chance.csv`, losses by region and device in `C5_honest_losses_by_region_device.csv`):\n\n{}\n",
        r(quantile(&switches, 0.5), 1),
        summary
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    out
}

// ---------------------------------------------------------------------- C6

const SKEWS: [f64; 5] = [0.01, 0.1, 0.5, 1.0, 2.0];
const DELTAS: [f64; 3] = [0.5, 2.0, 10.0];

fn c6_grid() -> Vec<(f64, f64, WitnessSigning, EarlyBlockRule)> {
    let mut v = Vec::new();
    for s in SKEWS {
        for d in DELTAS {
            for sign in [
                WitnessSigning::OnTime,
                WitnessSigning::EarliestWithinTolerance,
            ] {
                for early in [EarlyBlockRule::Permanent, EarlyBlockRule::ReevaluateLater] {
                    v.push((s, d, sign, early));
                }
            }
        }
    }
    v
}

pub(crate) fn c6_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    let m = ctx.pick(1_000, 200);
    let registered = ctx.pick(29_000, 2_900);
    let days = ctx.days(3.0, 0.4);
    let grid = if ctx.quick {
        c6_grid().into_iter().step_by(15).collect()
    } else {
        c6_grid()
    };
    for (s, d, sign, early) in grid {
        let mut j = realistic(
            ctx,
            &format!("C6-skew{s}-delta{d}-{sign:?}-{early:?}"),
            C6,
            0,
            m,
            registered,
            days,
        );
        j.skew_sd = s;
        j.skew_tail_fraction = 0.0;
        j.delta = d;
        j.witness_signing = sign;
        j.early_block = early;
        jobs.push(j);
    }
    let mut zero = realistic(ctx, "C6-no-skew", C6, 0, m, registered, days);
    zero.skew_sd = 0.0;
    zero.skew_tail_fraction = 0.0;
    jobs.push(zero);
    jobs
}

pub(crate) fn c6_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> SectionOut {
    let mut out = SectionOut::default();
    let grid: Vec<_> = if ctx.quick {
        c6_grid().into_iter().step_by(15).collect()
    } else {
        c6_grid()
    };
    let mut table = Table::new(
        "C6_clock_skew.csv",
        &[
            "skew_sd_s",
            "delta_s",
            "witness_signing",
            "early_block",
            "panel_pairs",
            "early_pairs",
            "early_pair_share",
            "blocks_early_at_majority",
            "blocks",
            "orphan_rate",
            "mean_interval_s",
        ],
    );
    let mut series: Vec<ChartSeries> = Vec::new();
    let mut summary = Vec::new();
    for (i, (s, d, sign, early)) in grid.iter().enumerate() {
        let res = &results[i];
        let share = div(res.early_pairs as f64, res.panel_pairs as f64);
        table.push(vec![
            f(*s),
            f(*d),
            format!("{sign:?}"),
            format!("{early:?}"),
            n(res.panel_pairs),
            n(res.early_pairs),
            f(share),
            n(res.early_majority),
            n(res.blocks),
            f(div(res.orphans as f64, res.canonical as f64)),
            f(div(res.interval_sum_s, res.interval_count as f64)),
        ]);
        if *early == EarlyBlockRule::Permanent && *d == 2.0 {
            let label = match sign {
                WitnessSigning::OnTime => "on time (default)",
                WitnessSigning::EarliestWithinTolerance => "earliest within δ (P-4)",
            };
            match series.iter_mut().find(|x| x.label == label) {
                Some(x) => x.points.push((*s, 100.0 * share)),
                None => series.push(ChartSeries {
                    label: label.to_string(),
                    points: vec![(*s, 100.0 * share)],
                }),
            }
            if *s == 1.0 || *s == 2.0 {
                summary.push(format!("skew σ = {s} s, δ = 2 s, {label}: {} % of panel receptions early, {} blocks early at the panel majority", r(100.0 * share, 3), res.early_majority));
            }
        }
    }
    let zero = &results[grid.len()];
    out.checks.push(count_check(
        C6,
        "C6-no-skew-no-early",
        "no clock skew: no honest block is received early at any panel node",
        0,
        zero.early_pairs,
    ));
    out.tables.push(table);
    out.charts.push(Chart {
        file: "C6_clock_skew.png".to_string(),
        title: "C6 — honest blocks received early (δ = 2 s, permanent rejection)".to_string(),
        subtitle: "share of panel receptions before timestamp − δ on the receiver's clock"
            .to_string(),
        panels: vec![ChartPanel {
            title: "early receptions".to_string(),
            x_label: "clock-skew standard deviation (s)".to_string(),
            y_label: "% of receptions".to_string(),
            log_x: true,
            series,
            references: Vec::new(),
        }],
    });
    out.summary = format!(
        "## C6 — clock skew and false rejections\n\n`C6_clock_skew.csv` covers skew σ × δ × signing time × early-block rule:\n\n{}\n",
        summary
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    out
}

// ---------------------------------------------------------------------- C7

const ONLINE_CHECK: [f64; 3] = [0.7, 0.8, 0.9];
const DEPARTURES: [f64; 3] = [0.0, 0.1, 0.25];

pub(crate) fn c7_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    for (i, f) in ONLINE_CHECK.iter().enumerate() {
        let mut j = ctx.job(&format!("C7-independent-f{f}"), C7, i as u64);
        j.miners = ctx.pick(300, 100);
        j.registered_ids = ctx.pick(29_000, 2_900);
        j.days = ctx.days(5.0, 0.3);
        j.warmup_days = ctx.pick(0.5, 0.05);
        j.independent_witness_online = Some(*f);
        j.kwc_sample_interval_s = 600.0;
        j.kwc_sample_size = 200;
        // Liveness under downtime alone: no honest witness is penalised (C5 measures bans).
        j.unreachable = UnreachableRule::UnreachableIsNotRefusal;
        jobs.push(j);
    }
    for (i, d) in DEPARTURES.iter().enumerate() {
        let mut j = realistic(
            ctx,
            &format!("C7-households-departures{d}"),
            C7,
            10 + i as u64,
            ctx.pick(1_000, 200),
            ctx.pick(290_000, 2_900),
            ctx.days(30.0, 0.5),
        );
        j.departures_per_year = Some(*d);
        j.kwc_sample_interval_s = 1_800.0;
        j.kwc_sample_size = 300;
        j.unreachable = UnreachableRule::UnreachableIsNotRefusal;
        jobs.push(j);
    }
    jobs
}

/// M1 section H's failure probability for a layout at signing probability `s`.
pub(crate) fn m1_fail(layout: &QuorumLayout, s: f64) -> f64 {
    let rounded = (s * 1e6).round() / 1e6;
    decimal(rounded)
        .ok()
        .and_then(|q| gb_analytic::quorum_feasibility::fail_probability(layout, &q).ok())
        .and_then(|p| p.to_f64())
        .unwrap_or(f64::NAN)
}

pub(crate) fn c7_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> SectionOut {
    let mut out = SectionOut::default();
    let unregistered = QuorumLayout::Pool {
        seats: 40,
        quorum: u64::from(ctx.cfg.quorum.unregistered_total_min.value),
    };
    let registered = QuorumLayout::TwoGroup {
        leader_seats: 10,
        leader_quorum: u64::from(ctx.cfg.quorum.registered_leader_min.value),
        subordinate_seats: 30,
        subordinate_quorum: u64::from(ctx.cfg.quorum.registered_subordinate_min.value),
    };
    let mut table = Table::new(
        "C7_quorum_liveness.csv",
        &[
            "case",
            "kwc_samples",
            "mean_online_fraction",
            "failing_unregistered",
            "se_unregistered",
            "m1_unregistered",
            "failing_registered",
            "se_registered",
            "m1_registered",
        ],
    );
    let mut summary = Vec::new();
    let daily_share = |res: &JobResult, idx: usize| -> (f64, f64) {
        let v: Vec<f64> = res
            .daily_kwc
            .iter()
            .filter(|d| d[2] > 0)
            .map(|d| d[idx] as f64 / d[2] as f64)
            .collect();
        let total = div(
            res.daily_kwc.iter().map(|d| d[idx]).sum::<u64>() as f64,
            res.daily_kwc.iter().map(|d| d[2]).sum::<u64>() as f64,
        );
        (total, mean_se(&v).1)
    };
    for (i, fr) in ONLINE_CHECK.iter().enumerate() {
        let res = &results[i];
        let samples_count = res.kwc_samples;
        let online = div(
            res.online_fraction_sum,
            (samples_count as f64 / 200.0).max(1.0),
        );
        let (u, se_u) = daily_share(res, 0);
        let (g, se_g) = daily_share(res, 1);
        // M1 section H at the realised online fraction (the nominal f differs by sampling noise).
        let (mu, mg) = (m1_fail(&unregistered, online), m1_fail(&registered, online));
        table.push(vec![
            format!("independent f={fr}"),
            n(samples_count),
            f(online),
            f(u),
            f(se_u),
            f(mu),
            f(g),
            f(se_g),
            f(mg),
        ]);
        // Sampled KWCs at one time share members, so use the binomial SE as a floor.
        let se_u2 = se_u.max((mu * (1.0 - mu) / samples_count.max(1) as f64).sqrt());
        let se_g2 = se_g.max((mg * (1.0 - mg) / samples_count.max(1) as f64).sqrt());
        out.checks.push(mc_check(C7, &format!("C7-independent-f{fr}-unregistered"), &format!("independent outages at f = {fr}: share of KWCs below 27 of 40 vs M1 fail_probability at the realised online fraction"), mu, u, se_u2, samples_count, ctx.k, 0.0));
        out.checks.push(mc_check(C7, &format!("C7-independent-f{fr}-registered"), &format!("independent outages at f = {fr}: share of KWCs below 7 + 21 vs M1 fail_probability at the realised online fraction"), mg, g, se_g2, samples_count, ctx.k, 0.0));
    }
    for (i, d) in DEPARTURES.iter().enumerate() {
        let res = &results[ONLINE_CHECK.len() + i];
        let samples_count = res.kwc_samples;
        let sample_times = (samples_count as f64 / 300.0).max(1.0);
        let online = div(res.online_fraction_sum, sample_times);
        let (u, se_u) = daily_share(res, 0);
        let (g, se_g) = daily_share(res, 1);
        let (mu, mg) = (m1_fail(&unregistered, online), m1_fail(&registered, online));
        table.push(vec![
            format!("households, departures {d}/yr"),
            n(samples_count),
            f(online),
            f(u),
            f(se_u),
            f(mu),
            f(g),
            f(se_g),
            f(mg),
        ]);
        // Against M1 H1 (published) at the realised online fraction; reference-limited, since
        // M1 assumes independent absences and household outages are correlated.
        let se_u2 = se_u.max((mu * (1.0 - mu) / samples_count.max(1) as f64).sqrt());
        let se_g2 = se_g.max((mg * (1.0 - mg) / samples_count.max(1) as f64).sqrt());
        out.checks.push(mc_check(C7, &format!("C7-households-d{d}-unregistered"), &format!("household profiles, departures {d}/yr: share of KWCs below 27 of 40 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences)"), mu, u, se_u2, samples_count, ctx.k, 0.0));
        out.checks.push(mc_check(C7, &format!("C7-households-d{d}-registered"), &format!("household profiles, departures {d}/yr: share of KWCs below 7 + 21 vs M1 H1 fail_probability at the realised online fraction (M1 assumes independent absences)"), mg, g, se_g2, samples_count, ctx.k, 0.0));
        summary.push(format!(
            "household profiles, departures {d}/yr: witnesses online {} %; KWCs below the unregistered quorum {} % (M1 at that online fraction: {} %), below 7 + 21 {} % (M1: {} %){}",
            r(100.0 * online, 1),
            r(100.0 * u, 2),
            r(100.0 * mu, 2),
            r(100.0 * g, 2),
            r(100.0 * mg, 2),
            storm_note(&[res])
        ));
    }
    out.tables.push(table);
    out.summary = format!(
        "## C7 — quorum liveness under household profiles\n\nThe share of KWCs unable to meet a quorum at a sampled instant (`C7_quorum_liveness.csv`), under the [P] \"unreachable is not refusal\" rule so that downtime alone is measured (under the strict rule honest witnesses are also banned: C5):\n\n{}\n",
        summary
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    out
}

// ---------------------------------------------------------------------- A6

pub(crate) fn a6_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    for (i, patience) in ctx.cfg.sim.growth.patience_median_days.iter().enumerate() {
        let mut j = realistic(
            ctx,
            &format!("A6-patience{patience}"),
            A6,
            20 + i as u64,
            ctx.pick(10_000, 300),
            ctx.pick(29_000, 2_900),
            ctx.days(45.0, 0.5),
        );
        j.arrivals_multiple_of_r = ctx.cfg.sim.growth.arrivals_multiple_of_r;
        j.patience_median_days = *patience;
        jobs.push(j);
    }
    // M3a.1: the wait to a first ID at 1, 2 and 3 × R arrivals, with no patience limit.
    for (i, mult) in A6_ARRIVALS.iter().enumerate() {
        let mut j = realistic(
            ctx,
            &format!("A6-arrivals{mult}R-no-patience"),
            A6,
            30 + i as u64,
            ctx.pick(10_000, 300),
            ctx.pick(29_000, 2_900),
            ctx.days(60.0, 0.5),
        );
        j.arrivals_multiple_of_r = *mult;
        j.patience_median_days = 1e9;
        jobs.push(j);
    }
    // M3a.1 (H4): V2's current-design restarters with and without the post-admission wait.
    let m = ctx.pick(1_000, 100);
    for (i, wait) in [0u32, 5].iter().enumerate() {
        let mut j = ctx.job(&format!("A6-H4-wait{wait}"), A6, 40 + i as u64);
        j.miners = m;
        j.registered_ids = ctx.pick(2_900, 1_300);
        j.outages = false;
        j.frozen_target = Some(gb_protocol::issuance::variant_b_target(u64::from(m), 30));
        j.days = ctx.days(30.0, 0.5);
        j.post_wait_rounds = Some(*wait);
        j.groups = vec![GroupSpec {
            label: "restarter".to_string(),
            share: 0.5,
            strategy: Strategy::Restart {
                old_design: false,
                window_s: 3_600.0,
            },
            always_online: false,
        }];
        jobs.push(j);
    }
    jobs
}

/// A6 arrival rates of the M3a.1 wait study, multiples of R.
const A6_ARRIVALS: [f64; 3] = [1.0, 2.0, 3.0];

pub(crate) fn a6_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> SectionOut {
    let mut out = SectionOut::default();
    let mut table = Table::new(
        "A6_growth_and_patience.csv",
        &[
            "patience_median_days",
            "arrivals",
            "first_ids",
            "dropouts",
            "still_waiting",
            "dropout_share",
            "wait_median_days",
            "wait_p90_days",
            "days",
        ],
    );
    let mut summary = Vec::new();
    for (i, patience) in ctx.cfg.sim.growth.patience_median_days.iter().enumerate() {
        let Some(res) = results.get(i) else { continue };
        let resolved = res.first_ids + res.dropouts;
        table.push(vec![
            f(*patience),
            n(res.arrivals),
            n(res.first_ids),
            n(res.dropouts),
            n(res.waiting_newcomers),
            f(div(res.dropouts as f64, resolved as f64)),
            f(quantile(&res.waits_days, 0.5)),
            f(quantile(&res.waits_days, 0.9)),
            f(res.sim_s / 86_400.0),
        ]);
        out.checks.push(count_check(
            A6,
            &format!("A6-conservation-p{patience}"),
            "every newcomer is counted once: arrivals = first IDs + dropouts + still waiting",
            res.arrivals,
            res.first_ids + res.dropouts + res.waiting_newcomers,
        ));
        let expected = ctx.cfg.sim.growth.arrivals_multiple_of_r * res.sim_s
            / ctx.cfg.issuance.pow_id_target_interval_s.value;
        out.checks.push(mc_check(
            A6,
            &format!("A6-arrivals-p{patience}"),
            "newcomer arrivals vs their Poisson mean",
            expected,
            res.arrivals as f64,
            expected.sqrt(),
            res.arrivals,
            ctx.k,
            0.0,
        ));
        summary.push(format!(
            "patience median {patience} days: {} arrivals, {} % gave up before a first ID, median wait to a first ID {} days{}",
            res.arrivals,
            r(100.0 * div(res.dropouts as f64, resolved as f64), 1),
            r(quantile(&res.waits_days, 0.5), 2),
            storm_note(&[res])
        ));
    }
    out.tables.push(table);
    // M3a.1: the wait to a first ID, among newcomers followed for 30 days or more.
    let mut waits = Table::new(
        "A6_first_id_wait.csv",
        &[
            "arrivals_multiple_of_r",
            "arrivals_followed_30_days",
            "share_within_1_day",
            "share_within_7_days",
            "share_within_30_days",
            "median_wait_days",
        ],
    );
    let base = ctx.cfg.sim.growth.patience_median_days.len();
    let mut wait_lines = Vec::new();
    for (k, mult) in A6_ARRIVALS.iter().enumerate() {
        let Some(res) = results.get(base + k) else {
            continue;
        };
        let end = res.sim_s / 86_400.0;
        let followed: Vec<f64> = res
            .newcomers
            .iter()
            .filter(|(joined, _)| *joined <= end - 30.0)
            .map(|(_, w)| w.unwrap_or(f64::INFINITY))
            .collect();
        let share = |d: f64| {
            div(
                followed.iter().filter(|w| **w <= d).count() as f64,
                followed.len() as f64,
            )
        };
        let mut sorted = followed.clone();
        sorted.sort_by(f64::total_cmp);
        let median = sorted.get(sorted.len() / 2).copied().unwrap_or(f64::NAN);
        waits.push(vec![
            f(*mult),
            n(followed.len() as u64),
            f(share(1.0)),
            f(share(7.0)),
            f(share(30.0)),
            f(median),
        ]);
        wait_lines.push(format!(
            "{mult}·R arrivals: of {} newcomers followed 30 days, {} % had a first ID within a day, {} % within 7 days, {} % within 30 days; median wait {} days",
            followed.len(),
            r(100.0 * share(1.0), 1),
            r(100.0 * share(7.0), 1),
            r(100.0 * share(30.0), 1),
            r(median, 2)
        ));
    }
    let mut h4 = Table::new(
        "A6_post_admission_wait.csv",
        &[
            "post_admission_wait_rounds",
            "restarts",
            "restarter_advantage",
            "se",
        ],
    );
    let mut h4_lines = Vec::new();
    for (k, wait) in [0u32, 5].iter().enumerate() {
        let Some(res) = results.get(base + A6_ARRIVALS.len() + k) else {
            continue;
        };
        let (adv, se) = super::v::per_slot_advantage(res, 0.5, ctx.pick(1_000, 100));
        h4.push(vec![n(u64::from(*wait)), n(res.restarts), f(adv), f(se)]);
        h4_lines.push(format!(
            "post-admission wait {wait} rounds: {} restarts, restarter advantage {} (SE {})",
            res.restarts,
            r(adv, 3),
            r(se, 3)
        ));
    }
    out.tables.push(waits);
    out.tables.push(h4);
    out.summary = format!(
        "## A6 — honest growth and patience\n\nNew honest miners arrive at {}·R per year (`A6_growth_and_patience.csv`):\n\n{}\n\nThe wait to a first ID with no patience limit (`A6_first_id_wait.csv`):\n\n{}\n\nH4 — is the post-admission wait still needed now that restarts gain nothing? V2's current-design restarters (half the slots, keep window 1 h) with and without it (`A6_post_admission_wait.csv`):\n\n{}\n",
        ctx.cfg.sim.growth.arrivals_multiple_of_r,
        summary
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n"),
        wait_lines
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n"),
        h4_lines
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    out
}
