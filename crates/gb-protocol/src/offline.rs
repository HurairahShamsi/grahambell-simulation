//! One Chance outcomes and penalties (SPEC §4.5–§4.8).
//!
//! When an online member's signature is still missing after One Chance (§4.5), the strict
//! rule treats it as refusal and penalises it; the \[P\] rule "unreachable is not refusal"
//! (§4.6) records an unreachable member offline instead. Under the strict rule the penalty
//! follows the §4.8 ladder by default; a yearly reset \[P\] and an immediate ban are the
//! comparisons (architect, 2026-10-12).

use gb_config::protocol::{StrictOffencePenalty, UnreachableRule};

/// What happens to a member whose signature is still missing when One Chance is judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OneChanceOutcome {
    /// The member is recorded offline, so its signature is no longer required in entropy
    /// aggregates until it returns (§4.7).
    pub recorded_offline: bool,
    /// The member is penalised for refusal (§4.8).
    pub penalised: bool,
}

/// The outcome of One Chance for one member. `signed` says whether it signed within the
/// wait, and `reachable` whether it was demonstrably online when the relays tried it.
///
/// An unreachable member is recorded offline under both rules, so the miners' entropy no
/// longer waits for it; the strict rule also penalises it. A reachable member that does not
/// sign is penalised under both rules and stays online.
pub fn one_chance_outcome(
    rule: UnreachableRule,
    signed: bool,
    reachable: bool,
) -> OneChanceOutcome {
    if signed {
        return OneChanceOutcome {
            recorded_offline: false,
            penalised: false,
        };
    }
    if reachable {
        return OneChanceOutcome {
            recorded_offline: false,
            penalised: true,
        };
    }
    OneChanceOutcome {
        recorded_offline: true,
        penalised: rule == UnreachableRule::Strict,
    }
}

/// A penalty under SPEC §4.8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Penalty {
    /// A suspension: the ID keeps its witness duty but earns nothing.
    Suspension {
        /// Length of the suspension, in seconds.
        seconds: u64,
    },
    /// A permanent ban: the ID loses its seat (§4.2).
    Permanent,
}

/// The penalty for an offence at time `now`, given the times of the member's earlier
/// offences in order, the ladder of suspensions (`[24 h, 30 days]`) and, for the yearly
/// reset, the offence-free period after which the count starts again.
///
/// - Ladder: the k-th offence gets the k-th suspension, and every offence after the ladder
///   is permanent.
/// - Ladder with a yearly reset \[P\]: the same, counting only offences since the last gap of
///   at least `reset_after_s` without an offence.
/// - Immediate ban: permanent at the first offence.
pub fn penalty_for(
    rule: StrictOffencePenalty,
    ladder_s: &[u64],
    earlier_offences: &[u64],
    now: u64,
    reset_after_s: u64,
) -> Penalty {
    let index = match rule {
        StrictOffencePenalty::ImmediateBan => return Penalty::Permanent,
        StrictOffencePenalty::Ladder => earlier_offences.len(),
        StrictOffencePenalty::LadderYearlyReset => {
            let mut count = 0usize;
            let mut last: Option<u64> = None;
            for &t in earlier_offences {
                if last.is_some_and(|l| t.saturating_sub(l) >= reset_after_s) {
                    count = 0;
                }
                count += 1;
                last = Some(t);
            }
            if last.is_some_and(|l| now.saturating_sub(l) >= reset_after_s) {
                count = 0;
            }
            count
        }
    };
    ladder_s
        .get(index)
        .map_or(Penalty::Permanent, |seconds| Penalty::Suspension {
            seconds: *seconds,
        })
}

/// A witness member's offline points (the M3a.1 E4 ladder; architect, 2026-10-14). The member
/// earns points for offline periods of at least `n`, whatever the cause: the k-th award since
/// its points last reset adds `steps[k]` (the last step repeats), and the total stops at the
/// cap, where the outcome applies (seat loss or blacklist). Points reset to zero once
/// `reset_s` of block time pass after the most recent award.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OfflinePoints {
    /// Points held after the last award.
    pub total: u32,
    /// Awards since the points last reset.
    pub awards: u32,
    /// Block time of the most recent award, in seconds (`None` before the first).
    pub last_award: Option<u64>,
}

impl OfflinePoints {
    /// Points held at block time `now`: zero once `reset_s` have passed since the last award.
    pub fn at(&self, now: u64, reset_s: u64) -> u32 {
        match self.last_award {
            Some(last) if now.saturating_sub(last) < reset_s => self.total,
            _ => 0,
        }
    }

    /// Records an award at block time `now` and returns the new total (at most `cap`).
    pub fn award(&mut self, now: u64, steps: &[u32], cap: u32, reset_s: u64) -> u32 {
        if self.at(now, reset_s) == 0 {
            self.total = 0;
            self.awards = 0;
        }
        let step = steps
            .get(self.awards as usize)
            .or(steps.last())
            .copied()
            .unwrap_or(1);
        self.total = self.total.saturating_add(step).min(cap);
        self.awards += 1;
        self.last_award = Some(now);
        self.total
    }

    /// Clears the points, after the outcome at the cap has been applied.
    pub fn clear(&mut self) {
        *self = OfflinePoints::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LADDER: [u64; 2] = [86_400, 2_592_000];
    const YEAR: u64 = 31_536_000;

    #[test]
    fn offline_points_escalate_stop_at_the_cap_and_reset_after_a_clean_year() {
        let steps = [1, 1, 2, 3, 4, 5];
        let mut p = OfflinePoints::default();
        let totals: Vec<u32> = (0..8u64)
            .map(|k| p.award(k * 86_400, &steps, 20, YEAR))
            .collect();
        // 1, 1, 2, 3, 4, 5, then the last step repeats; the cap holds at 20.
        assert_eq!(totals, vec![1, 2, 4, 7, 11, 16, 20, 20]);
        assert_eq!(p.at(7 * 86_400 + YEAR - 1, YEAR), 20);
        // A year after the most recent award the points are gone, and the steps start again.
        assert_eq!(p.at(7 * 86_400 + YEAR, YEAR), 0);
        assert_eq!(p.award(7 * 86_400 + YEAR, &steps, 20, YEAR), 1);
        assert_eq!(p.awards, 1);
        // A point within the year keeps the count going.
        let mut q = OfflinePoints::default();
        q.award(0, &steps, 20, YEAR);
        q.award(YEAR - 1, &steps, 20, YEAR);
        assert_eq!(q.award(2 * YEAR - 2, &steps, 20, YEAR), 4);
        q.clear();
        assert_eq!(q, OfflinePoints::default());
    }

    #[test]
    fn an_unreachable_member_is_recorded_offline_and_penalised_only_under_the_strict_rule() {
        let strict = one_chance_outcome(UnreachableRule::Strict, false, false);
        assert!(strict.recorded_offline && strict.penalised);
        let p = one_chance_outcome(UnreachableRule::UnreachableIsNotRefusal, false, false);
        assert!(p.recorded_offline && !p.penalised);
        let refused = one_chance_outcome(UnreachableRule::UnreachableIsNotRefusal, false, true);
        assert!(!refused.recorded_offline && refused.penalised);
        let signed = one_chance_outcome(UnreachableRule::Strict, true, false);
        assert!(!signed.recorded_offline && !signed.penalised);
    }

    #[test]
    fn the_ladder_goes_24_hours_30_days_then_permanent_and_never_resets() {
        use StrictOffencePenalty::Ladder;
        assert_eq!(
            penalty_for(Ladder, &LADDER, &[], 10, YEAR),
            Penalty::Suspension { seconds: 86_400 }
        );
        assert_eq!(
            penalty_for(Ladder, &LADDER, &[10], 5 * YEAR, YEAR),
            Penalty::Suspension { seconds: 2_592_000 }
        );
        assert_eq!(
            penalty_for(Ladder, &LADDER, &[10, 20], 9 * YEAR, YEAR),
            Penalty::Permanent
        );
    }

    #[test]
    fn the_yearly_reset_counts_only_offences_since_a_clean_year() {
        use StrictOffencePenalty::LadderYearlyReset;
        // Two offences within a year, then a third within the year: permanent.
        assert_eq!(
            penalty_for(LadderYearlyReset, &LADDER, &[0, 100], 200, YEAR),
            Penalty::Permanent
        );
        // A clean year before now: back to the first step.
        assert_eq!(
            penalty_for(LadderYearlyReset, &LADDER, &[0, 100], 100 + YEAR, YEAR),
            Penalty::Suspension { seconds: 86_400 }
        );
        // A clean year between earlier offences: only the later one counts.
        assert_eq!(
            penalty_for(
                LadderYearlyReset,
                &LADDER,
                &[0, 2 * YEAR],
                2 * YEAR + 5,
                YEAR
            ),
            Penalty::Suspension { seconds: 2_592_000 }
        );
        assert_eq!(
            penalty_for(StrictOffencePenalty::ImmediateBan, &LADDER, &[], 0, YEAR),
            Penalty::Permanent
        );
    }
}
