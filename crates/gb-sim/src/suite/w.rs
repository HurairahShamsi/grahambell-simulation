//! W1, where the withholder sits (architect, 2026-10-23): a supplementary job set, not part of
//! the reference run. It runs only when named (`gb sim --only W1`).
//!
//! E1's two finalists ((r), first seen, literal, with D = 10 s soft and without a deadline) and
//! C3's withholder runs at D = none and D = 10 s soft, at the final defaults, with the 10 %
//! withholder placed three ways: as in the reference (by the slot order that places the
//! regions, mostly as_east; ASSUMPTIONS SIM-51), spread over the regions in proportion to their
//! miners, and all in one small, distant region. The jobs placed as in the reference are those
//! of the reference run under other names, so they reproduce its figures.

use super::c::{c3_job, region_names};
use super::e::{ForkConfig, e1_job, e1_seeds};
use super::{Ctx, SectionOut, Table, amplification, div, f, n, r};
use crate::job::{ForkRule, JobResult, JobSpec};
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
}
