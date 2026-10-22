//! Validation ladder V0–V4 (ARCHITECTURE §4): the mode-agreement gate, and reproductions of
//! M1 sections A, D, E and F and of the M2 test vectors.

use super::{
    Chart, ChartPanel, ChartSeries, Ctx, SectionOut, Table, count_check, div, f, mc_check, mean_se,
    n, p_check, r,
};
use crate::job::{ForkRule, GroupSpec, JobResult, JobSpec, Sampler, Strategy};
use crate::rng::{Keyed, Purpose};
use crate::samplers::any_success;
use crate::stats::{binned, chi_square_p, chi_square_two_sample, geometric_pit, ks_uniform};
use gb_analytic::validation::Check;
use gb_config::protocol::{
    CountBasedCorrection, DeadlineScope, DifficultyCountRule, DifficultyVariant, EntropySigners,
    StartRule,
};
use gb_config::sim::CryptoMode;
use gb_protocol::issuance::variant_b_target;

const V0: &str = "V0";
const V1: &str = "V1";
const V2: &str = "V2";
const V3: &str = "V3";
const V4: &str = "V4";

/// Minimum p-value of the agreement tests (M2 K-11).
fn min_p(ctx: &Ctx<'_>) -> f64 {
    ctx.cfg.crypto_tests.equivalence_min_p_value
}

// ---------------------------------------------------------------------- V0

/// A job in the configuration that M1's and M2's references model, whatever the defaults:
/// start rule (a), the v0.6 tie-break, the count after the post-admission wait and no
/// correction (architect, 2026-10-21). The PoWit deadline, the entropy rule and the offline
/// points ladder, which those references do not model, stay at the values V0–V3 ran with
/// before SPEC v0.7 changed them (10 s with the soft scope, every online member, no ladder),
/// so V0–V3's tables stay byte-identical; V2's restart runs depend slightly on them.
pub(crate) fn m1_job(ctx: &Ctx<'_>, id: &str, section: &'static str, rep: u64) -> JobSpec {
    let mut j = ctx.job(id, section, rep);
    j.start_rule = StartRule::A;
    j.fork_rule = ForkRule::V06;
    j.count_rule = DifficultyCountRule::AfterPostAdmissionWait;
    j.correction = None;
    j.deadline = Some(10.0);
    j.deadline_scope = DeadlineScope::NotExtendedOnly;
    j.entropy_signers = EntropySigners::AllOnlineMembers;
    j.offline_points = None;
    j
}

pub(crate) fn v0_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = Vec::new();
    // A fixed slot count (instant re-admission, no outages) so every round has the same law.
    let small = |mut j: JobSpec, m: u32| {
        j.miners = m;
        j.registered_ids = 130;
        j.outages = false;
        j.instant_admission = true;
        j.record_rounds = true;
        j
    };
    let mut real = small(m1_job(ctx, "V0-real-32", V0, 0), ctx.pick(32, 8));
    real.crypto = CryptoMode::Real;
    // Quick runs still pass 12 blocks, where placements (confirmation + beacon delay) begin.
    real.days = ctx.days(500.0 * 30.5 / 86_400.0, 40.0 * 30.5 / 86_400.0);
    jobs.push(real);
    let mut standin = small(m1_job(ctx, "V0-standin-32", V0, 0), ctx.pick(32, 8));
    standin.crypto = CryptoMode::StandIn;
    standin.days = ctx.days(2_000.0 * 30.5 / 86_400.0, 40.0 * 30.5 / 86_400.0);
    jobs.push(standin);
    let ideal = |id: &str, m: u32, registered: u64, days: f64| {
        let mut j = m1_job(ctx, id, V0, 0).idealised();
        j.miners = m;
        j.registered_ids = registered;
        j.days = days;
        j.record_rounds = true;
        j
    };
    let m = ctx.pick(1_000, 100);
    let mut si = ideal(
        "V0-standin-1000",
        m,
        ctx.pick(2_900, 1_300),
        ctx.days(0.2, 0.02),
    );
    si.crypto = CryptoMode::StandIn;
    jobs.push(si);
    let days = ctx.days(3.5, 0.2);
    jobs.push(ideal("V0-gap-1000", m, ctx.pick(2_900, 1_300), days));
    let mut pm = ideal("V0-permin-1000", m, ctx.pick(2_900, 1_300), days);
    pm.sampler = Sampler::PerMiner;
    jobs.push(pm);
    jobs.push(ideal("V0-gap-10", 10, 130, days));
    let mut pm10 = ideal("V0-permin-10", 10, 130, days);
    pm10.sampler = Sampler::PerMiner;
    jobs.push(pm10);
    // Horizon audit under stress: loss, partitions, skew, a short deadline, One Chance of one round.
    let mut audit = m1_job(ctx, "V0-audit", V0, 0);
    audit.miners = ctx.pick(10_000, 300);
    audit.registered_ids = ctx.pick(29_000, 2_900);
    audit.days = ctx.days(2.0, 0.4);
    audit.warmup_days = ctx.pick(0.5, 0.1);
    audit.loss = 0.05;
    audit.partition_rate_per_region_per_day = 1.0;
    audit.partition_length_s = 300.0;
    audit.skew_sd = 1.0;
    audit.deadline = Some(5.0);
    audit.one_chance_wait_rounds = 1;
    audit.audit = true;
    jobs.push(audit);
    // M3a.1: start rule (r) with real latency, where chain starts differ between miners,
    // under both samplers.
    for (id, sampler) in [
        ("V0-gap-r-1000", Sampler::Gap),
        ("V0-permin-r-1000", Sampler::PerMiner),
    ] {
        let mut j = m1_job(ctx, id, V0, 0);
        j.miners = m;
        j.registered_ids = ctx.pick(2_900, 1_300);
        j.outages = false;
        j.instant_admission = true;
        j.days = days;
        j.start_rule = StartRule::R;
        j.sampler = sampler;
        jobs.push(j);
    }
    jobs
}

fn find<'a>(results: &'a [JobResult], id: &str) -> Option<&'a JobResult> {
    results.iter().find(|r| r.id == id)
}

/// Relative winning-step bins (K-11 edges) on steps already scaled by their round's per-step
/// success probability.
fn step_bins(scaled: &[f64]) -> Vec<u64> {
    binned(scaled, &[0.125, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0])
}

pub(crate) fn v0_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> SectionOut {
    let mut out = SectionOut::default();
    let minimum = min_p(ctx);
    let mut table = Table::new(
        "V0_mode_agreement.csv",
        &["test", "data", "samples", "statistic", "p_value", "passes"],
    );
    let mut add = |out: &mut SectionOut,
                   id: &str,
                   desc: &str,
                   data: &str,
                   samples: u64,
                   stat: f64,
                   p: f64| {
        table.push(vec![
            id.to_string(),
            data.to_string(),
            n(samples),
            f(stat),
            f(p),
            (p >= minimum).to_string(),
        ]);
        out.checks.push(p_check(V0, id, desc, p, samples, minimum));
    };
    // M2 C6 rerun.
    let rows = gb_crypto_tests::equivalence::equivalence(ctx.seed, &ctx.cfg.crypto_tests);
    for row in &rows {
        add(
            &mut out,
            &format!("V0-c6-{}", row.test),
            &format!(
                "M2 C6 rerun: {} (real BLS entropy vs the 32-byte stand-in)",
                row.test
            ),
            "M2 equivalence",
            row.trials,
            row.statistic,
            row.p_value,
        );
    }
    // Real vs stand-in: each slot's first winning step (real SHA-256 chains, real or stand-in E).
    if let (Some(real), Some(standin)) =
        (find(results, "V0-real-32"), find(results, "V0-standin-32"))
    {
        let (stat, _, p) = chi_square_two_sample(
            &step_bins(&real.slot_steps),
            &step_bins(&standin.slot_steps),
        );
        add(
            &mut out,
            "V0-real-vs-standin-steps",
            "each slot's first winning step: real BLS entropy vs stand-in entropy (two-sample χ², K-11 bins)",
            "V0-real-32, V0-standin-32",
            (real.slot_steps.len() + standin.slot_steps.len()) as u64,
            stat,
            p,
        );
        out.checks.push(count_check(
            V0,
            "V0-validate-equivalence",
            "real blocks: validate() and validate_facts give the same verdict (two receptions each)",
            0,
            real.validate_mismatches,
        ));
        out.checks.push(Check::at_least(
            V0,
            "V0-validate-compared",
            "real blocks were compared by validate() and validate_facts",
            1.0,
            real.validated as f64,
            0.0,
            real.validated,
        ));
    }
    // Start rule (r) with real latency: the samplers agree on what the chain does.
    if let (Some(g), Some(p)) = (
        find(results, "V0-gap-r-1000"),
        find(results, "V0-permin-r-1000"),
    ) {
        let interval = |r: &JobResult| {
            let k = r.interval_count.max(1) as f64;
            let mean = r.interval_sum_s / k;
            let var = (r.interval_sq_sum_s / k - mean * mean).max(0.0);
            (mean, (var / k).sqrt())
        };
        let ((gm, gs), (pm, ps)) = (interval(g), interval(p));
        out.checks.push(mc_check(
            V0,
            "V0-r-sampler-interval",
            "start rule (r), real latency: mean interval, gap sampler vs per-miner sampler",
            pm,
            gm,
            (gs * gs + ps * ps).sqrt(),
            g.interval_count,
            ctx.k,
            0.0,
        ));
        let orphan = |r: &JobResult| {
            let k = r.canonical.max(1) as f64;
            let rate = r.orphans as f64 / k;
            (rate, (rate * (1.0 - rate) / k).sqrt())
        };
        let ((go, gse), (po, pse)) = (orphan(g), orphan(p));
        out.checks.push(mc_check(
            V0,
            "V0-r-sampler-orphans",
            "start rule (r), real latency: orphan rate, gap sampler vs per-miner sampler",
            po,
            go,
            (gse * gse + pse * pse).sqrt(),
            g.canonical,
            ctx.k,
            0.0,
        ));
        out.checks.push(Check::at_least(
            V0,
            "V0-r-late-starts",
            "start rule (r), real latency: some chains start after the round's rule (a) start",
            1.0,
            g.late_starts.min(p.late_starts) as f64,
            0.0,
            g.late_starts.min(p.late_starts),
        ));
    }
    // Earliest winning step of each round against its exact law, per mode.
    let mut pit_keyed = Keyed::new(ctx.seed, "v0-pit");
    for id in [
        "V0-standin-1000",
        "V0-gap-1000",
        "V0-permin-1000",
        "V0-gap-10",
        "V0-permin-10",
    ] {
        let Some(res) = find(results, id) else {
            continue;
        };
        let pits: Vec<f64> = res
            .nmin
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let q = any_success(res.round_p[i], res.round_r[i]);
                let u = pit_keyed.uniforms(Purpose::Sampling, 42, i as u64)[0];
                geometric_pit(*step, q, u)
            })
            .collect();
        let (d, p) = ks_uniform(&pits);
        add(
            &mut out,
            &format!("{id}-nmin-law"),
            "earliest winning step of each round vs Geometric(1 − (1 − p)^r) (randomised PIT, KS)",
            id,
            pits.len() as u64,
            d,
            p,
        );
        let (_, p_hash) = ks_uniform(&res.hash_uniforms);
        add(
            &mut out,
            &format!("{id}-hash-uniform"),
            "block hashes uniform (KS on the top 53 bits)",
            id,
            res.hash_uniforms.len() as u64,
            0.0,
            p_hash,
        );
        // Winners uniform over slots.
        let slots: Vec<u64> = res.wins_by_slot.clone();
        if slots.len() >= 2 {
            let total: u64 = slots.iter().sum();
            let e = total as f64 / slots.len() as f64;
            let stat: f64 = slots.iter().map(|o| (*o as f64 - e).powi(2) / e).sum();
            let p = chi_square_p(stat, (slots.len() - 1) as f64);
            add(
                &mut out,
                &format!("{id}-winners-uniform"),
                "blocks per slot uniform (χ² goodness of fit)",
                id,
                total,
                stat,
                p,
            );
        }
        // Tie rate vs M1 F1 at the simulated size.
        let m = res.round_r.first().copied().unwrap_or(2).max(2);
        if let Ok(tie) = gb_analytic::ties::tie_probability(m, 30.0) {
            let rounds = res.rounds as f64;
            let rate = div(res.tie_rounds as f64, rounds);
            out.checks.push(mc_check(
                V0,
                &format!("{id}-tie-rate"),
                &format!("same-step tie rate vs M1 tie_probability({m}, 30 s)"),
                tie,
                rate,
                (tie * (1.0 - tie) / rounds).sqrt(),
                res.rounds,
                ctx.k,
                0.0,
            ));
        }
    }
    // Stand-in vs fast: earliest steps; gap vs per-miner: earliest steps and tie sizes.
    let nmin_bins = |res: &JobResult| -> Vec<u64> {
        let v: Vec<f64> = res.nmin.iter().map(|x| *x as f64).collect();
        binned(&v, &[3.0, 7.0, 12.0, 18.0, 25.0, 35.0, 50.0, 70.0, 100.0])
    };
    let tie_bins = |res: &JobResult| -> Vec<u64> {
        let v: Vec<f64> = res.tie_size.iter().map(|x| *x as f64).collect();
        binned(&v, &[1.5, 2.5])
    };
    for (a, b, label) in [
        ("V0-standin-1000", "V0-gap-1000", "stand-in vs fast"),
        (
            "V0-gap-1000",
            "V0-permin-1000",
            "gap vs per-miner sampler (1,000 slots)",
        ),
        (
            "V0-gap-10",
            "V0-permin-10",
            "gap vs per-miner sampler (10 slots)",
        ),
    ] {
        if let (Some(x), Some(y)) = (find(results, a), find(results, b)) {
            let (stat, _, p) = chi_square_two_sample(&nmin_bins(x), &nmin_bins(y));
            add(
                &mut out,
                &format!("V0-{a}-vs-{b}-nmin"),
                &format!("earliest winning step, {label} (two-sample χ²)"),
                &format!("{a}, {b}"),
                (x.nmin.len() + y.nmin.len()) as u64,
                stat,
                p,
            );
            if !label.starts_with("stand-in") {
                let (stat, _, p) = chi_square_two_sample(&tie_bins(x), &tie_bins(y));
                add(
                    &mut out,
                    &format!("V0-{a}-vs-{b}-ties"),
                    &format!("winners at the earliest step, {label} (two-sample χ²)"),
                    &format!("{a}, {b}"),
                    (x.tie_size.len() + y.tie_size.len()) as u64,
                    stat,
                    p,
                );
            }
        }
    }
    let self_invalid: u64 = results.iter().map(|r| r.self_invalid).sum();
    out.checks.push(count_check(
        V0,
        "V0-self-validation",
        "every broadcast block passes validate_facts at its own miner (all V0 runs)",
        0,
        self_invalid,
    ));
    let invariants: u64 = results.iter().map(|r| r.invariant_failures).sum();
    out.checks.push(count_check(
        V0,
        "V0-invariants",
        "allocation and counter invariants (all V0 runs)",
        0,
        invariants,
    ));
    if let Some(audit) = find(results, "V0-audit") {
        out.checks.push(count_check(
            V0,
            "V0-horizon-audit",
            "candidates past the normal horizon that were broadcast, under loss, partitions, skew and D = 5 s (must be 0)",
            0,
            audit.horizon_hits,
        ));
    }
    let passed = out.checks.iter().filter(|c| c.passed).count();
    out.summary = format!(
        "## V0 — mode agreement (the gate)\n\nThe three crypto modes agree, and the fast sampler is exact, when every test below has p ≥ {minimum}. **{passed} of {} gate checks pass.** Details in `V0_mode_agreement.csv` and `validation.csv`.\n",
        out.checks.len()
    );
    out.tables.push(table);
    out
}

// ---------------------------------------------------------------------- V1

/// V1 cases: (attacker share s, honest online fraction f).
const V1_CASES: [(f64, f64); 4] = [(0.6, 1.0), (0.75, 1.0), (1.0, 1.0), (0.6, 0.7)];
const THRESHOLD: f64 = 0.51;

fn v1_genesis(ctx: &Ctx<'_>) -> u64 {
    ctx.pick(2_000, 300)
}

/// The closed-form blocks to threshold (M1 A1/A2, realistic online race) and the Wald
/// overshoot band, per block.
fn v1_reference(s: f64, f: f64, genesis: u64) -> (f64, f64) {
    let t = THRESHOLD;
    let drift = s - t * (s + f * (1.0 - s));
    let blocks = t * f * genesis as f64 / drift;
    let overshoot = (1.0 - t) / drift;
    (blocks, overshoot)
}

pub(crate) fn v1_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let mut jobs = v1_set(ctx, true);
    jobs.extend(v1_set(ctx, false));
    jobs
}

/// V1's jobs: pinned to M1's model (the checks), or at the configured defaults (`pinned`
/// false: a reported comparison, SPEC v0.7; architect, 2026-10-21), with the same seeds.
fn v1_set(ctx: &Ctx<'_>, pinned: bool) -> Vec<JobSpec> {
    let genesis = v1_genesis(ctx);
    let reps = ctx.reps(200, 3);
    let mut jobs = Vec::new();
    for (ci, (s, f)) in V1_CASES.iter().enumerate() {
        let (blocks, _) = v1_reference(*s, *f, genesis);
        for rep in 0..reps {
            let id = if pinned {
                format!("V1-s{s}-f{f}-r{rep}")
            } else {
                format!("V1-defaults-s{s}-f{f}-r{rep}")
            };
            let base = if pinned {
                m1_job(ctx, &id, V1, 1_000 * ci as u64 + rep)
            } else {
                ctx.job(&id, V1, 1_000 * ci as u64 + rep)
            };
            let mut j = base.idealised();
            j.miners = ctx.pick(1_000, 100);
            j.registered_ids = genesis.max(130);
            j.days = 3.0 * blocks * 30.6 / 86_400.0 + 0.1;
            j.threshold = Some(THRESHOLD);
            j.threshold_genesis = genesis;
            j.honest_id_online = (*f < 1.0).then_some(*f);
            j.groups = vec![GroupSpec {
                label: "attacker".to_string(),
                share: *s,
                strategy: Strategy::Honest,
                always_online: true,
            }];
            jobs.push(j);
        }
    }
    jobs
}

pub(crate) fn v1_analyse(ctx: &Ctx<'_>, all: &[JobResult]) -> SectionOut {
    let mut out = SectionOut::default();
    let genesis = v1_genesis(ctx);
    // The pinned jobs come first; the jobs at the configured defaults follow.
    let (results, defaults) = all.split_at(all.len() / 2);
    let mut table = Table::new(
        "V1_time_to_threshold.csv",
        &[
            "attacker_share",
            "honest_online_fraction",
            "genesis_ids",
            "threshold",
            "replicates",
            "reached",
            "mean_blocks",
            "se_blocks",
            "closed_form_blocks",
            "wald_overshoot_blocks",
            "mean_days",
            "closed_form_years_in_g_over_r",
        ],
    );
    let reps = results.len() / V1_CASES.len().max(1);
    let mut series = Vec::new();
    for (ci, (s, fr)) in V1_CASES.iter().enumerate() {
        let runs = &results[ci * reps..(ci + 1) * reps];
        let blocks: Vec<f64> = runs
            .iter()
            .filter_map(|r| r.threshold_blocks)
            .map(|b| b as f64)
            .collect();
        let (mean, se) = mean_se(&blocks);
        let (reference, overshoot) = v1_reference(*s, *fr, genesis);
        table.push(vec![
            f(*s),
            f(*fr),
            n(genesis),
            f(THRESHOLD),
            n(runs.len() as u64),
            n(blocks.len() as u64),
            f(mean),
            f(se),
            f(reference),
            f(overshoot),
            f(mean * 30.5 / 86_400.0),
            f(reference / genesis as f64),
        ]);
        series.push((*s, *fr, mean, reference));
        if *s >= 1.0 {
            // Every block is the attacker's: the threshold is reached at exactly ⌈T·G/(1 − T)⌉.
            let exact = (THRESHOLD * genesis as f64 / (1.0 - THRESHOLD) - 1e-9).ceil() as u64;
            let all_exact = runs.iter().all(|r| r.threshold_blocks == Some(exact));
            out.checks.push(Check::exact(
                V1,
                "V1-s100-exact",
                &format!("s = 1, f = 1: every replicate reaches 51 % at exactly ⌈0.51·G/0.49⌉ = {exact} blocks (G = {genesis})"),
                exact as f64,
                mean,
                all_exact,
            ));
        } else {
            out.checks.push(mc_check(
                V1,
                &format!("V1-s{s}-f{fr}"),
                &format!("blocks to 51 % of active IDs (G = {genesis}, s = {s}, f = {fr}) vs the M1 closed form t* + ½ Wald overshoot"),
                reference + 0.5 * overshoot,
                mean,
                se,
                blocks.len() as u64,
                ctx.k,
                0.5 * overshoot,
            ));
        }
    }
    // A7: long-run share of active new IDs at (0.6, 0.7).
    let ci = V1_CASES.iter().position(|c| *c == (0.6, 0.7)).unwrap_or(3);
    let runs = &results[ci * reps..(ci + 1) * reps];
    let (att, hon): (u64, u64) = runs.iter().fold((0, 0), |a, r| {
        (a.0 + r.v1_attacker_blocks, a.1 + r.v1_honest_active)
    });
    let share = div(att as f64, (att + hon) as f64);
    let reference = 0.6 / (0.6 + 0.7 * 0.4);
    let total = (att + hon) as f64;
    out.checks.push(mc_check(
        V1,
        "V1-a7-long-run-share",
        "attacker share of active new IDs at s = 0.6, f = 0.7 vs M1 A7 s/(s + f(1 − s))",
        reference,
        share,
        (reference * (1.0 - reference) / total).sqrt(),
        att + hon,
        ctx.k,
        0.0,
    ));
    // F1 ties and the mean interval, pooled over all V1 runs.
    let m = u64::from(ctx.pick(1_000u32, 100));
    let rounds: u64 = results.iter().map(|r| r.rounds).sum();
    let ties: u64 = results.iter().map(|r| r.tie_rounds).sum();
    let tie = gb_analytic::ties::tie_probability(m, 30.0).unwrap_or(f64::NAN);
    let rate = div(ties as f64, rounds as f64);
    out.checks.push(mc_check(
        V1,
        "V1-f1-tie-rate",
        &format!("same-step tie rate vs M1 F1 tie_probability({m}, 30 s)"),
        tie,
        rate,
        (tie * (1.0 - tie) / rounds as f64).sqrt(),
        rounds,
        ctx.k,
        0.0,
    ));
    let (sum, sq, count) = results.iter().fold((0.0, 0.0, 0u64), |a, r| {
        (
            a.0 + r.interval_sum_s,
            a.1 + r.interval_sq_sum_s,
            a.2 + r.interval_count,
        )
    });
    let mean = div(sum, count as f64);
    let var = div(sq, count as f64) - mean * mean;
    let expected = 1.0 / any_success(1.0 / (m as f64 * 30.0), m);
    out.checks.push(mc_check(
        V1,
        "V1-mean-interval",
        "mean interval with an exact count vs the whole-second law 1/(1 − (1 − p)^r) (not 30 s: see SUMMARY)",
        expected,
        mean,
        (var / count as f64).sqrt(),
        count,
        ctx.k,
        0.0,
    ));
    let mut t2 = Table::new(
        "V1_ties_and_interval.csv",
        &[
            "rounds",
            "tie_rounds",
            "tie_rate",
            "m1_tie_probability",
            "intervals",
            "mean_interval_s",
            "whole_second_law_s",
            "target_s",
        ],
    );
    t2.push(vec![
        n(rounds),
        n(ties),
        f(rate),
        f(tie),
        n(count),
        f(mean),
        f(expected),
        "30".to_string(),
    ]);
    out.tables.push(table);
    out.tables.push(t2);
    // A reported comparison, not a check: the same runs at the configured defaults (SPEC v0.7:
    // start rule (r), first seen, the issued-entropy count, P2 ×1.1; architect, 2026-10-22).
    let interval_of = |runs: &[JobResult]| {
        let (sum, count) = runs.iter().fold((0.0, 0u64), |a, r| {
            (a.0 + r.interval_sum_s, a.1 + r.interval_count)
        });
        div(sum, count as f64)
    };
    let blocks_of = |runs: &[JobResult]| {
        let b: Vec<f64> = runs
            .iter()
            .filter_map(|r| r.threshold_blocks)
            .map(|b| b as f64)
            .collect();
        mean_se(&b)
    };
    let share_of = |runs: &[JobResult]| {
        let (a, h) = runs.iter().fold((0u64, 0u64), |x, r| {
            (x.0 + r.v1_attacker_blocks, x.1 + r.v1_honest_active)
        });
        div(a as f64, (a + h) as f64)
    };
    let (pinned_interval, defaults_interval) = (interval_of(results), interval_of(defaults));
    let dreps = defaults.len() / V1_CASES.len().max(1);
    let mut comparison = Table::new(
        "V1_defaults_comparison.csv",
        &[
            "attacker_share",
            "honest_online_fraction",
            "closed_form_blocks",
            "pinned_mean_blocks",
            "pinned_se_blocks",
            "defaults_mean_blocks",
            "defaults_se_blocks",
            "pinned_mean_interval_s",
            "defaults_mean_interval_s",
            "pinned_mean_days",
            "defaults_mean_days",
        ],
    );
    let mut comparison_rows = Vec::new();
    for (ci, (s, fr)) in V1_CASES.iter().enumerate() {
        let (pm, ps) = blocks_of(
            &results[(ci * reps).min(results.len())..((ci + 1) * reps).min(results.len())],
        );
        let (dm, ds) = blocks_of(
            &defaults[(ci * dreps).min(defaults.len())..((ci + 1) * dreps).min(defaults.len())],
        );
        let (reference, _) = v1_reference(*s, *fr, genesis);
        comparison.push(vec![
            f(*s),
            f(*fr),
            f(reference),
            f(pm),
            f(ps),
            f(dm),
            f(ds),
            f(pinned_interval),
            f(defaults_interval),
            f(pm * pinned_interval / 86_400.0),
            f(dm * defaults_interval / 86_400.0),
        ]);
        comparison_rows.push(format!(
            "s = {s}, f = {fr}: {} blocks pinned, {} at the defaults (closed form {})",
            r(pm, 1),
            r(dm, 1),
            r(reference, 1)
        ));
    }
    let a7 = V1_CASES.iter().position(|c| *c == (0.6, 0.7)).unwrap_or(3);
    let mut long_run = Table::new(
        "V1_defaults_long_run_share.csv",
        &[
            "attacker_share",
            "honest_online_fraction",
            "m1_a7",
            "pinned",
            "defaults",
        ],
    );
    let (pinned_share, defaults_share) = (
        share_of(&results[(a7 * reps).min(results.len())..((a7 + 1) * reps).min(results.len())]),
        share_of(
            &defaults[(a7 * dreps).min(defaults.len())..((a7 + 1) * dreps).min(defaults.len())],
        ),
    );
    long_run.push(vec![
        f(0.6),
        f(0.7),
        f(0.6 / (0.6 + 0.7 * 0.4)),
        f(pinned_share),
        f(defaults_share),
    ]);
    out.tables.push(comparison);
    out.tables.push(long_run);
    // x is the attacker's share of slots; the f = 0.7 case (the realistic online race, M1 A-3)
    // is a single point per series.
    let pick = |online: f64, simulated: bool| -> Vec<(f64, f64)> {
        series
            .iter()
            .filter(|c| c.1 == online)
            .map(|c| (c.0, if simulated { c.2 } else { c.3 }))
            .collect()
    };
    out.charts.push(Chart {
        file: "V1_time_to_threshold.png".to_string(),
        title: "V1 — blocks to 51 % of active IDs, simulator vs M1".to_string(),
        subtitle: format!(
            "idealised, G = {genesis} genesis IDs; f is the share of honest IDs online (M1 A-3)"
        ),
        panels: vec![ChartPanel {
            title: "mean blocks to threshold".to_string(),
            x_label: "attacker share of mining slots (s)".to_string(),
            y_label: "blocks".to_string(),
            log_x: false,
            series: vec![
                ChartSeries {
                    label: "simulator, f = 1".to_string(),
                    points: pick(1.0, true),
                },
                ChartSeries {
                    label: "M1 closed form, f = 1".to_string(),
                    points: pick(1.0, false),
                },
                ChartSeries {
                    label: "simulator, f = 0.7".to_string(),
                    points: pick(0.7, true),
                },
                ChartSeries {
                    label: "M1 closed form, f = 0.7".to_string(),
                    points: pick(0.7, false),
                },
            ],
            references: Vec::new(),
        }],
    });
    out.summary = format!(
        "## V1 — M1 A1/A7/F1 in the idealised simulator\n\nBlocks to 51 % of active IDs, the long-run share and the tie rate are compared with M1's closed forms at G = {genesis} (`V1_time_to_threshold.csv`). The mean interval with an exact count is **{} s**, not 30 s: blocks start one second after their parent (start rule (a)) and each step is a whole second, so the mean interval is 1/(1 − (1 − p)^r) = {} s for p = 1/(30r). This is the discrete-step bias that B-exact removes (C4).\n\nA reported comparison, not a check: the same runs at the configured defaults (`V1_defaults_comparison.csv`, `V1_defaults_long_run_share.csv`) have a mean interval of {} s (pinned {} s); blocks to 51 %: {}; long-run share at s = 0.6, f = 0.7: {} at the defaults, {} pinned, M1 A7 {}.\n",
        r(mean, 3),
        r(expected, 3),
        r(defaults_interval, 3),
        r(pinned_interval, 3),
        comparison_rows.join("; "),
        r(defaults_share, 4),
        r(pinned_share, 4),
        r(0.6 / (0.6 + 0.7 * 0.4), 4)
    );
    out
}

// ---------------------------------------------------------------------- V2

const V2_OLD_WINDOWS: [f64; 4] = [1_299.0, 3_600.0, 5_196.0, 20_785.0];
const V2_CURRENT_WINDOWS: [f64; 3] = [3_600.0, 5_196.0, 1e12];

pub(crate) fn v2_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let m = ctx.pick(1_000, 100);
    let target = variant_b_target(u64::from(m), 30);
    let mut jobs = Vec::new();
    let base = |id: &str, rep: u64| {
        let mut j = m1_job(ctx, id, V2, rep);
        j.miners = m;
        j.registered_ids = ctx.pick(2_900, 1_300);
        j.outages = false;
        j.frozen_target = Some(target);
        j
    };
    for (i, w) in V2_OLD_WINDOWS.iter().enumerate() {
        let mut j = base(&format!("V2-old-w{w}"), i as u64);
        j.days = ctx.days(10.0, 0.5);
        j.groups = vec![GroupSpec {
            label: "restarter".to_string(),
            share: 0.05,
            strategy: Strategy::Restart {
                old_design: true,
                window_s: *w,
            },
            always_online: false,
        }];
        jobs.push(j);
    }
    for (i, w) in V2_CURRENT_WINDOWS.iter().enumerate() {
        let mut j = base(&format!("V2-current-w{w}"), 10 + i as u64);
        j.days = ctx.days(30.0, 0.5);
        j.groups = vec![GroupSpec {
            label: "restarter".to_string(),
            share: 0.5,
            strategy: Strategy::Restart {
                old_design: false,
                window_s: *w,
            },
            always_online: false,
        }];
        jobs.push(j);
    }
    jobs
}

/// Restarter IDs per slot over honest IDs per slot; the standard error treats both counts as
/// Poisson: relative SE `sqrt(1/n_r + 1/n_h)`.
pub(crate) fn per_slot_advantage(res: &JobResult, restarter_share: f64, slots: u32) -> (f64, f64) {
    let r_slots = (restarter_share * f64::from(slots)).round().max(1.0);
    let h_slots = f64::from(slots) - r_slots;
    let (n_r, n_h) = (
        res.ids_by_group.get(1).copied().unwrap_or(0) as f64,
        res.ids_by_group[0] as f64,
    );
    let adv = div(n_r / r_slots, n_h / h_slots);
    let se = adv * (1.0 / n_r.max(1.0) + 1.0 / n_h.max(1.0)).sqrt();
    (adv, se)
}

pub(crate) fn v2_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> SectionOut {
    let mut out = SectionOut::default();
    let m = ctx.pick(1_000u32, 100);
    let q = 1.0 / (f64::from(m) * 30.0);
    let mut table = Table::new(
        "V2_restart_advantage.csv",
        &[
            "design",
            "keep_window_s",
            "restarter_share",
            "restarts",
            "realised_cost_median_s",
            "advantage",
            "se",
            "m1_advantage_old_design",
            "days",
        ],
    );
    let mut costs = Vec::new();
    for (i, w) in V2_OLD_WINDOWS.iter().enumerate() {
        let res = &results[i];
        let (adv, se) = per_slot_advantage(res, 0.05, m);
        // The median: a restart that coincides with the slot's own confirmed win also waits
        // for the winner's re-admission, a rare outlier the mean would carry.
        let cost = super::quantile(&res.restart_s, 0.5);
        costs.extend(res.restart_s.iter().copied());
        // No restart in a short run: use the configured restart cost (M1 D-5).
        let cost = if cost.is_nan() {
            gb_analytic::restart::realistic_restart_cost_s(ctx.cfg)
        } else {
            cost
        };
        let reference = gb_analytic::restart::advantage_old_design(q, cost, *w as u64);
        table.push(vec![
            "old (fixed entropy)".to_string(),
            f(*w),
            "0.05".to_string(),
            n(res.restarts),
            f(cost),
            f(adv),
            f(se),
            f(reference),
            f(res.sim_s / 86_400.0),
        ]);
        out.checks.push(mc_check(
            V2,
            &format!("V2-old-w{w}"),
            &format!("old design: restart advantage at keep window {w} s vs M1 advantage_old_design(q, C_realised, W), allowance 1 % for tie losses"),
            reference,
            adv,
            se,
            res.ids_by_group.get(1).copied().unwrap_or(0),
            ctx.k,
            0.01 * reference,
        ));
    }
    let cost = super::quantile(&costs, 0.5);
    out.checks.push(Check::exact(
        V2,
        "V2-restart-cost",
        "realised restart cost (decision to mining again, median) within [450, 455] s",
        450.0,
        cost,
        (450.0..=455.0).contains(&cost),
    ));
    let mut h4 = Vec::new();
    for (i, w) in V2_CURRENT_WINDOWS.iter().enumerate() {
        let res = &results[V2_OLD_WINDOWS.len() + i];
        let (adv, se) = per_slot_advantage(res, 0.5, m);
        h4.push((*w, adv, se));
        table.push(vec![
            "current (per-round entropy)".to_string(),
            f(*w),
            "0.5".to_string(),
            n(res.restarts),
            String::new(),
            f(adv),
            f(se),
            "1".to_string(),
            f(res.sim_s / 86_400.0),
        ]);
        if *w >= 1e11 {
            out.checks.push(mc_check(
                V2,
                "V2-current-control",
                "current design, restarters that never restart: advantage vs 1",
                1.0,
                adv,
                se,
                res.ids_by_group.get(1).copied().unwrap_or(0),
                ctx.k,
                0.0,
            ));
        }
    }
    out.tables.push(table);
    let worst = h4
        .iter()
        .filter(|x| x.0 < 1e11)
        .map(|x| x.1)
        .fold(f64::NAN, f64::max);
    out.summary = format!(
        "## V2 — S5 restart attack\n\nUnder the old fixed-entropy design the restart advantage matches M1 section D at the simulator's size (q = 1/{}, C ≈ {} s realised through the admission path). Under per-round entropy the restart strategy gains nothing: its largest advantage is **{}** (H4 asks ≤ 1.02; reported, not a check). `V2_restart_advantage.csv`.\n",
        m * 30,
        r(cost, 1),
        r(worst, 3)
    );
    out
}

// ---------------------------------------------------------------------- V3

const V3_RATIOS: [f64; 4] = [0.1, 0.5, 1.0, 4.0];

fn v3_variants(ctx: &Ctx<'_>) -> Vec<(&'static str, u32, Option<f64>)> {
    if ctx.quick {
        vec![("A", 144, Some(4.0))]
    } else {
        vec![
            ("A", 144, Some(4.0)),
            ("A", 144, None),
            ("A", 2_016, Some(4.0)),
            ("A", 2_016, None),
        ]
    }
}

fn v3_corrections() -> Vec<(&'static str, Option<CountBasedCorrection>)> {
    vec![
        ("B", None),
        (
            "B+P1",
            Some(CountBasedCorrection::RecentIntervalRatio {
                window_blocks: 144,
                bound: 0.1,
            }),
        ),
        (
            "B+P2",
            Some(CountBasedCorrection::Accumulated {
                window_blocks: 144,
                bound: 0.1,
            }),
        ),
        ("B-exact", Some(CountBasedCorrection::DiscreteExact)),
    ]
}

pub(crate) fn v3_jobs(ctx: &Ctx<'_>) -> Vec<JobSpec> {
    let honest = ctx.pick(1_000u32, 100);
    let mut jobs = Vec::new();
    let hop = |id: &str, rep: u64, ratio: f64, k: u32, idle: u32, cycles: f64| {
        let mut j = m1_job(ctx, id, V3, rep).idealised();
        let total = (f64::from(honest) * (1.0 + ratio)).round() as u32;
        j.miners = total;
        j.registered_ids = ctx.pick(2_900, 1_300).max(u64::from(total) / 30 * 10 + 130);
        j.window_blocks = k;
        if ratio > 0.0 {
            j.groups = vec![GroupSpec {
                label: "hopper".to_string(),
                share: ratio / (1.0 + ratio),
                strategy: Strategy::Hop {
                    window_blocks: k,
                    idle_windows: idle,
                    rest_windows: 2,
                },
                always_online: true,
            }];
        }
        j.days = cycles * f64::from(3 + idle) * f64::from(k) * 30.6 / 86_400.0;
        j
    };
    let mut rep = 0;
    for (v, k, clamp) in v3_variants(ctx) {
        let cycles = if k > 144 {
            ctx.pick(50.0, 2.0)
        } else {
            ctx.pick(200.0, 12.0)
        };
        for ratio in V3_RATIOS {
            let mut j = hop(
                &format!(
                    "V3-{v}-K{k}-clamp{}-m{ratio}",
                    clamp.map_or("none".to_string(), |c| c.to_string())
                ),
                rep,
                ratio,
                k,
                0,
                cycles,
            );
            j.difficulty = DifficultyVariant::BitcoinRetarget;
            j.clamp = clamp;
            jobs.push(j);
            rep += 1;
        }
    }
    let cycles = ctx.pick(200.0, 12.0);
    for (label, correction) in v3_corrections() {
        for ratio in std::iter::once(0.0).chain(V3_RATIOS) {
            let mut j = hop(&format!("V3-{label}-m{ratio}"), rep, ratio, 144, 0, cycles);
            j.difficulty = DifficultyVariant::CountBased;
            j.correction = correction;
            jobs.push(j);
            rep += 1;
        }
    }
    for (label, correction) in v3_corrections().into_iter().skip(1).take(2) {
        let mut j = hop(
            &format!("V3-{label}-idle-then-active-m1"),
            rep,
            1.0,
            144,
            1,
            cycles,
        );
        j.difficulty = DifficultyVariant::CountBased;
        j.correction = correction;
        jobs.push(j);
        rep += 1;
    }
    jobs
}

/// Hop gain: the hopper's IDs per mining slot-second in its active windows over the fair
/// per-slot rate of an exact count with the same slots, `q_B / slots`, with
/// `q_B = 1 − (1 − 1/(30 n))^n`.
pub(crate) fn hop_gain(res: &JobResult, slots: f64) -> (f64, f64) {
    let rate = div(res.hop_ids as f64, res.hop_slot_s);
    let fair = any_success(1.0 / (slots * 30.0), slots as u64) / slots;
    let gain = rate / fair;
    let se = gain / (res.hop_ids.max(1) as f64).sqrt();
    (gain, se)
}

pub(crate) fn v3_analyse(ctx: &Ctx<'_>, results: &[JobResult]) -> SectionOut {
    let mut out = SectionOut::default();
    let honest = f64::from(ctx.pick(1_000u32, 100));
    let mut table = Table::new(
        "V3_difficulty_hopping.csv",
        &[
            "variant",
            "window_blocks",
            "clamp",
            "ratio_m",
            "idle_windows",
            "hop_ids",
            "hop_slot_days",
            "gain",
            "se",
            "discrete_reference_gain",
            "m1_first_order_gain",
            "mean_interval_s",
            "recovery_window_h",
            "discrete_recovery_window_h",
        ],
    );
    let mut i = 0;
    let mut variant_a_points: Vec<(f64, f64)> = Vec::new();
    for (v, k, clamp) in v3_variants(ctx) {
        for ratio in V3_RATIOS {
            let res = &results[i];
            i += 1;
            let slots = honest * (1.0 + ratio);
            let (gain, se) = hop_gain(res, slots);
            // Variant A equilibrium with H honest slots: 1 − (1 − p_A)^H = 1/30.
            let q_a = 1.0 - libm::pow(29.0 / 30.0, 1.0 + ratio);
            let q_b = any_success(1.0 / (slots * 30.0), slots as u64);
            let reference = q_a / q_b;
            let m1 = gb_analytic::hopping::first_order(u64::from(k), ratio, 30.0, clamp)
                .map_or(f64::NAN, |x| x.gain_factor);
            // Recovery: the window after the hop, at the retargeted difficulty with H slots.
            let ratio_window = (1.0 / q_a) / 30.0;
            let factor = clamp.map_or(ratio_window, |c| ratio_window.max(1.0 / c));
            let p_a = 1.0 - libm::pow(29.0 / 30.0, 1.0 / honest);
            let q_rec = any_success(p_a * factor, honest as u64);
            let recovery_h = f64::from(k) / q_rec / 3_600.0;
            // Window w spans heights wK to (w + 1)K; `window_s[e]` is window e + 1. Cycles are
            // rest, rest, active, so recovery windows are w ≡ 0 (mod 3), w ≥ 3: e ≡ 2.
            let windows: Vec<f64> = res.window_s.iter().skip(2).step_by(3).copied().collect();
            let (measured_rec, se_rec) = mean_se(&windows);
            table.push(vec![
                v.to_string(),
                n(u64::from(k)),
                clamp.map_or("none".to_string(), f),
                f(ratio),
                "0".to_string(),
                n(res.hop_ids),
                f(res.hop_slot_s / 86_400.0),
                f(gain),
                f(se),
                f(reference),
                f(m1),
                f(div(res.interval_sum_s, res.interval_count as f64)),
                f(measured_rec / 3_600.0),
                f(recovery_h),
            ]);
            out.checks.push(mc_check(
                V3,
                &format!("V3-A-K{k}-clamp{}-m{ratio}", clamp.map_or("none".to_string(), |c| c.to_string())),
                &format!("Variant A hopping gain (K = {k}) vs the discrete equivalent of M1 E1 (1 − (29/30)^(1+m)) / q_B; M1's continuous value {m1:.3}"),
                reference,
                gain,
                se,
                res.hop_ids,
                ctx.k,
                0.0,
            ));
            if windows.len() >= 2 {
                let jensen = if clamp.is_none() {
                    f64::from(k) / f64::from(k - 1) - 1.0
                } else {
                    0.0
                };
                out.checks.push(mc_check(
                    V3,
                    &format!("V3-A-K{k}-clamp{}-m{ratio}-recovery", clamp.map_or("none".to_string(), |c| c.to_string())),
                    "Variant A recovery window (hours) vs the discrete first-order value, allowance K/(K − 1) − 1 when unclamped",
                    recovery_h,
                    measured_rec / 3_600.0,
                    se_rec / 3_600.0,
                    windows.len() as u64,
                    ctx.k,
                    (jensen + 0.02) * recovery_h,
                ));
            }
            if k == 144 && clamp.is_some() {
                variant_a_points.push((ratio, gain));
            }
        }
    }
    let mut b_points: Vec<(String, Vec<(f64, f64)>)> = Vec::new();
    let mut summary_rows = Vec::new();
    for (label, _) in v3_corrections() {
        let mut pts = Vec::new();
        for ratio in std::iter::once(0.0).chain(V3_RATIOS) {
            let res = &results[i];
            i += 1;
            let slots = honest * (1.0 + ratio);
            let (gain, se) = if ratio > 0.0 {
                hop_gain(res, slots)
            } else {
                (f64::NAN, f64::NAN)
            };
            let mean = div(res.interval_sum_s, res.interval_count as f64);
            table.push(vec![
                label.to_string(),
                "144".to_string(),
                String::new(),
                f(ratio),
                "0".to_string(),
                n(res.hop_ids),
                f(res.hop_slot_s / 86_400.0),
                f(gain),
                f(se),
                "1".to_string(),
                "1".to_string(),
                f(mean),
                String::new(),
                String::new(),
            ]);
            if ratio > 0.0 {
                pts.push((ratio, gain));
                summary_rows.push(format!(
                    "{label} m={ratio}: gain {} ± {}",
                    r(gain, 4),
                    r(1.96 * se, 4)
                ));
                if label == "B" {
                    out.checks.push(mc_check(
                        V3,
                        &format!("V3-B-m{ratio}"),
                        "Variant B (exact count, no correction) hopping gain vs 1",
                        1.0,
                        gain,
                        se,
                        res.hop_ids,
                        ctx.k,
                        0.0,
                    ));
                }
            }
        }
        b_points.push((label.to_string(), pts));
    }
    for (label, _) in v3_corrections().into_iter().skip(1).take(2) {
        let res = &results[i];
        i += 1;
        let slots = honest * 2.0;
        let (gain, se) = hop_gain(res, slots);
        table.push(vec![
            label.to_string(),
            "144".to_string(),
            String::new(),
            "1".to_string(),
            "1".to_string(),
            n(res.hop_ids),
            f(res.hop_slot_s / 86_400.0),
            f(gain),
            f(se),
            String::new(),
            String::new(),
            f(div(res.interval_sum_s, res.interval_count as f64)),
            String::new(),
            String::new(),
        ]);
        summary_rows.push(format!(
            "{label}, one idle-but-counted window then one active window: gain {} ± {}",
            r(gain, 4),
            r(1.96 * se, 4)
        ));
    }
    out.tables.push(table);
    let references = vec![
        ("no gain".to_string(), 1.0),
        ("H5 bound (1.02)".to_string(), 1.02),
    ];
    // Two panels: Variant A's gains are much larger than Variant B's and would flatten them.
    out.charts.push(Chart {
        file: "V3_hopping_gain.png".to_string(),
        title: "V3 — difficulty-hopping gain".to_string(),
        subtitle: "IDs per hopping slot-second relative to the fair exact-count rate".to_string(),
        panels: vec![
            ChartPanel {
                title: "Variant A (K = 144, 4× clamp, comparison)".to_string(),
                x_label: "hopping slots ÷ honest slots (m)".to_string(),
                y_label: "gain".to_string(),
                log_x: true,
                series: vec![ChartSeries {
                    label: "Variant A".to_string(),
                    points: variant_a_points,
                }],
                // At this scale 1 and 1.02 coincide; the bound is the line that matters.
                references: vec![references[1].clone()],
            },
            ChartPanel {
                title: "Variant B (default) and its [P] corrections".to_string(),
                x_label: "hopping slots ÷ honest slots (m)".to_string(),
                y_label: "gain".to_string(),
                log_x: true,
                series: b_points
                    .into_iter()
                    .map(|(label, points)| ChartSeries { label, points })
                    .collect(),
                references,
            },
        ],
    });
    out.summary = format!(
        "## V3 — S6 difficulty hopping\n\nVariant A reproduces M1 E1 once both use whole-second steps (`V3_difficulty_hopping.csv`). Variant B and its correction candidates (H5 input; reported, not a check):\n\n{}\n",
        summary_rows
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    out
}

// ---------------------------------------------------------------------- V4

pub(crate) fn v4_analyse(_ctx: &Ctx<'_>) -> SectionOut {
    let mut out = SectionOut::default();
    let mut table = Table::new(
        "V4_test_vectors.csv",
        &["field", "expected", "rebuilt", "matches"],
    );
    match crate::real::vectors_check() {
        Ok(fields) => {
            for field in &fields {
                table.push(vec![
                    field.field.clone(),
                    field.expected.clone(),
                    field.rebuilt.clone(),
                    field.matches.to_string(),
                ]);
                out.checks.push(Check::exact(
                    V4,
                    &format!("V4-{}", field.field),
                    "the simulator's real-mode path reproduces this field of protocol_v1.json",
                    1.0,
                    f64::from(u8::from(field.matches)),
                    field.matches,
                ));
            }
            let failing = fields.iter().filter(|x| !x.matches).count();
            out.summary = format!(
                "## V4 — M2 test vectors\n\nThe simulator's real-mode functions rebuild every derived field of `protocol_v1.json` (keys, encodings, digests, signatures, chain steps, verdicts, positions, targets): **{} of {} fields match** (`V4_test_vectors.csv`).\n",
                fields.len() - failing,
                fields.len()
            );
        }
        Err(e) => {
            out.checks.push(Check::exact(
                V4,
                "V4-vectors",
                &format!("rebuilding the vectors failed: {e}"),
                1.0,
                0.0,
                false,
            ));
        }
    }
    out.tables.push(table);
    out
}
