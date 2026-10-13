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

#[cfg(test)]
mod tests {
    use super::*;

    const LADDER: [u64; 2] = [86_400, 2_592_000];
    const YEAR: u64 = 31_536_000;

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
