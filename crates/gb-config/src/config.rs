//! The top-level [`Config`]: loading, merging over defaults, and validation.

use crate::analytic::{AnalyticConfig, ModelConstants, RunConfig};
use crate::crypto_tests::CryptoTestsConfig;
use crate::error::ConfigError;
use crate::protocol::{
    Admission, Cac, Capacity, CountBasedCorrection, Crypto, Entropy, Genesis, Issuance, Network,
    Offline, Pacing, Penalties, Quorum, Timing, Transactions, Witness,
};
use crate::sim::SimConfig;
use serde::{Deserialize, Serialize};
use std::path::Path;
use toml::{Table, Value};

/// Complete configuration: every SPEC §2 parameter, modelling constants, M1 grids and run
/// settings.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Hash pacing.
    pub pacing: Pacing,
    /// PoW-ID issuance.
    pub issuance: Issuance,
    /// Transaction layer.
    pub transactions: Transactions,
    /// Witness Chain structure.
    pub witness: Witness,
    /// PoWit quorums.
    pub quorum: Quorum,
    /// Entropy lock.
    pub entropy: Entropy,
    /// Miner and witnessing capacity.
    pub capacity: Capacity,
    /// Network limits.
    pub network: Network,
    /// Timing.
    pub timing: Timing,
    /// Admission lifecycle.
    pub admission: Admission,
    /// Chain Allocation Committee.
    pub cac: Cac,
    /// Genesis distribution.
    pub genesis: Genesis,
    /// Offline handling.
    pub offline: Offline,
    /// Penalties.
    pub penalties: Penalties,
    /// Cryptographic primitives.
    pub crypto: Crypto,
    /// Modelling constants.
    pub model: ModelConstants,
    /// M1 analysis grids.
    pub analytic: AnalyticConfig,
    /// M2 measurement settings (`gb crypto`).
    pub crypto_tests: CryptoTestsConfig,
    /// M3 simulator settings (`gb sim`).
    pub sim: SimConfig,
    /// Seed and Monte Carlo sample sizes.
    pub run: RunConfig,
}

impl Config {
    /// Loads a configuration file and merges it over the defaults.
    pub fn from_file(path: &Path) -> Result<Config, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Config::from_toml_str(&text)
    }

    /// Parses TOML text and merges it over the defaults.
    ///
    /// The text only needs the keys it overrides. Unknown keys and changed status tags are
    /// errors, and the merged result is validated. An optional parameter is unset with
    /// `value = "none"` ([`NO_VALUE`]).
    pub fn from_toml_str(text: &str) -> Result<Config, ConfigError> {
        let overrides: Table =
            toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))?;
        let defaults = Config::default().to_table()?;
        let mut merged = defaults.clone();
        merge_tables(&mut merged, overrides);
        check_statuses_unchanged(&defaults, &merged, "")?;
        let mut unset = Vec::new();
        take_unset_values(&mut merged, "", &mut unset);
        let config: Config = merged
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError::Structure(e.to_string()))?;
        // Only an optional parameter reads back without a value; any other would silently take
        // its type's default.
        let plain = config.to_plain_table()?;
        if let Some(path) = unset.iter().find(|path| has_value(&plain, path)) {
            return Err(ConfigError::Structure(format!(
                "{path}.value = \"{NO_VALUE}\" is only allowed for an optional parameter"
            )));
        }
        config.validate()?;
        Ok(config)
    }

    /// Serializes the configuration to a TOML table.
    ///
    /// An optional parameter without a value is normally left out, but where the default has
    /// a value it is written as `value = "none"`, because a file is merged over the defaults
    /// and leaving it out would bring the default back.
    pub fn to_table(&self) -> Result<Table, ConfigError> {
        let mut table = self.to_plain_table()?;
        mark_unset_values(&mut table, &Config::default().to_plain_table()?);
        Ok(table)
    }

    fn to_plain_table(&self) -> Result<Table, ConfigError> {
        Table::try_from(self).map_err(|e| ConfigError::Serialize(e.to_string()))
    }

    /// Serializes the configuration to TOML text (see [`Config::to_table`] for unset values).
    pub fn to_toml_string(&self) -> Result<String, ConfigError> {
        toml::to_string(&self.to_table()?).map_err(|e| ConfigError::Serialize(e.to_string()))
    }

    /// PoW-ID issuance per year at the target interval, R = seconds_per_year / interval.
    pub fn issuance_per_year(&self) -> f64 {
        self.model.seconds_per_year as f64 / self.issuance.pow_id_target_interval_s.value
    }

    /// Checks that every value is inside its valid range.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut problems = Vec::new();
        self.validate_protocol(&mut problems);
        self.validate_grids(&mut problems);
        problems.extend(self.crypto_tests.problems());
        problems.extend(self.sim.problems());
        if problems.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Invalid(problems))
        }
    }

    fn validate_protocol(&self, problems: &mut Vec<String>) {
        let wc = self.witness.wc_size.value;
        let subs = self.witness.subordinate_wcs_per_kwc.value;
        let q = &self.quorum;
        require(problems, wc > 0, "witness.wc_size must be positive");
        require(
            problems,
            subs > 0,
            "witness.subordinate_wcs_per_kwc must be positive",
        );
        require(
            problems,
            q.registered_leader_min.value <= wc,
            "quorum.registered_leader_min exceeds the WC size",
        );
        require(
            problems,
            q.registered_subordinate_min.value <= wc * subs,
            "quorum.registered_subordinate_min exceeds the subordinate seats",
        );
        require(
            problems,
            q.registered_subordinate_min_30_node.value <= wc * 2,
            "quorum.registered_subordinate_min_30_node exceeds 20 seats",
        );
        require(
            problems,
            q.unregistered_total_min.value <= wc * (1 + subs),
            "quorum.unregistered_total_min exceeds the KWC size",
        );
        require(
            problems,
            q.unregistered_total_min_30_node.value <= wc * 3,
            "quorum.unregistered_total_min_30_node exceeds 30 seats",
        );
        require(
            problems,
            self.issuance.pow_id_target_interval_s.value > 0.0,
            "issuance.pow_id_target_interval_s must be positive",
        );
        let pen = &self.penalties;
        require(
            problems,
            pen.offline_points_after_s.value.is_none_or(|n| n > 0.0),
            "penalties.offline_points_after_s must be positive",
        );
        require(
            problems,
            !pen.offline_points_steps.value.is_empty()
                && pen.offline_points_steps.value.iter().all(|s| *s > 0),
            "penalties.offline_points_steps must be positive and not empty",
        );
        require(
            problems,
            pen.offline_points_cap.value > 0,
            "penalties.offline_points_cap must be positive",
        );
        require(
            problems,
            pen.offline_points_reset_s.value > 0.0
                && pen.offline_points_seat_cooldown_s.value >= 0.0,
            "penalties.offline_points_reset_s must be positive and the seat cooldown not negative",
        );
        require(
            problems,
            self.admission
                .failover_after_rounds
                .value
                .is_none_or(|r| r > 0),
            "admission.failover_after_rounds must be at least 1",
        );
        require(
            problems,
            self.transactions.pow_tx_interval_s.value > 0.0,
            "transactions.pow_tx_interval_s must be positive",
        );
        require(
            problems,
            self.issuance.min_attack_time_floor_years.value > 0.0,
            "issuance.min_attack_time_floor_years must be positive",
        );
        require(
            problems,
            self.cac.approval_threshold.value.is_proper(),
            "cac.approval_threshold must be a fraction in (0, 1]",
        );
        let k = self.issuance.cap_safety_factor.value;
        require(
            problems,
            k.numerator > 0 && k.denominator > 0,
            "issuance.cap_safety_factor must be positive",
        );
        require(
            problems,
            self.issuance
                .cap_checkpoint_interval_fraction_of_t_min
                .value
                > 0.0,
            "issuance.cap_checkpoint_interval_fraction_of_t_min must be positive",
        );
        require(
            problems,
            self.issuance.retarget_window_blocks.value > 1,
            "issuance.retarget_window_blocks must exceed 1",
        );
        require(
            problems,
            self.issuance.retarget_clamp_factor.value > 1.0,
            "issuance.retarget_clamp_factor must exceed 1",
        );
        check_offsets(
            problems,
            "witness.ring_offsets",
            &self.witness.ring_offsets.value,
            subs,
        );
        check_offsets(
            problems,
            "witness.ring_offsets_30_node",
            &self.witness.ring_offsets_30_node.value,
            2,
        );
        require(
            problems,
            self.offline.long_absence_threshold_s.value > 0.0,
            "offline.long_absence_threshold_s must be positive",
        );
        require(
            problems,
            self.cac.join_every_n_tx_blocks.value > 0,
            "cac.join_every_n_tx_blocks must be positive",
        );
        require(
            problems,
            self.capacity.registered_per_leader_wc().is_some(),
            "capacity.unregistered_per_leader_wc exceeds the total capacity",
        );
        require(
            problems,
            self.model.seconds_per_year > 0,
            "model.seconds_per_year must be positive",
        );
        let correction = &self.issuance.count_based_correction;
        let corrections = correction.value.iter().chain(
            correction
                .sweep
                .iter()
                .flat_map(|s| s.values.iter().flatten()),
        );
        for form in corrections {
            if let CountBasedCorrection::RecentIntervalRatio {
                window_blocks,
                bound,
            }
            | CountBasedCorrection::Accumulated {
                window_blocks,
                bound,
            } = *form
            {
                require(
                    problems,
                    window_blocks > 0 && bound > 0.0 && bound < 1.0,
                    "issuance.count_based_correction needs window_blocks > 0 and 0 < bound < 1",
                );
            }
        }
    }

    fn validate_grids(&self, problems: &mut Vec<String>) {
        let a = &self.analytic;
        let tt = &a.time_to_threshold;
        check_fractions(
            problems,
            "attacker_shares",
            &tt.attacker_shares,
            false,
            true,
        );
        check_fractions(problems, "thresholds", &tt.thresholds, false, false);
        check_fractions(
            problems,
            "online_fractions",
            &tt.online_fractions,
            false,
            true,
        );
        check_fractions(
            problems,
            "checkpoint_fractions_of_t_min",
            &tt.checkpoint_fractions_of_t_min,
            false,
            true,
        );
        require(
            problems,
            tt.floor_threshold > 0.0 && tt.floor_threshold < 1.0,
            "floor_threshold must lie strictly between 0 and 1",
        );
        require(
            problems,
            tt.trajectory_step_years > 0.0 && tt.trajectory_horizon_years > 0.0,
            "trajectory horizon and step must be positive",
        );
        require(
            problems,
            tt.honest_growth_multiples_of_r.iter().all(|g| *g >= 0.0),
            "honest_growth_multiples_of_r must be non-negative",
        );
        require(
            problems,
            tt.t_min_years.iter().all(|t| *t > 0.0),
            "t_min_years must be positive",
        );
        check_fractions(
            problems,
            "witness.attacker_fractions",
            &a.witness.attacker_fractions,
            false,
            true,
        );
        check_fractions(
            problems,
            "witness.ban_rates_per_year",
            &a.witness.ban_rates_per_year,
            true,
            true,
        );
        check_fractions(
            problems,
            "cac.attacker_fractions",
            &a.cac.attacker_fractions,
            false,
            false,
        );
        check_fractions(
            problems,
            "cac.honest_mining_fractions",
            &a.cac.honest_mining_fractions,
            false,
            true,
        );
        require(
            problems,
            a.cac
                .committee_sizes
                .iter()
                .all(|n| *n > 0 && *n < a.cac.active_population),
            "CAC committee sizes must be positive and smaller than the active population",
        );
        require(
            problems,
            a.restart.competing_miners.iter().all(|n| *n > 0),
            "restart.competing_miners must be positive",
        );
        require(
            problems,
            !a.hopping.attacker_ratios.is_empty()
                && a.hopping.attacker_ratios.iter().all(|m| *m > 0.0),
            "hopping.attacker_ratios must be positive",
        );
        self.validate_quorum_grid(problems);
        self.validate_absence_grid(problems);
        self.validate_feasibility_grid(problems);
        self.validate_kwc_size_grid(problems);
        require(
            problems,
            a.ties.competing_miners.iter().all(|n| *n > 1),
            "ties.competing_miners must exceed 1",
        );
        require(
            problems,
            self.run.monte_carlo.tolerance_standard_errors > 0.0,
            "run.monte_carlo.tolerance_standard_errors must be positive",
        );
        require(
            problems,
            self.run.monte_carlo.committee_population > 100
                && self.run.monte_carlo.lottery_population > 100,
            "run.monte_carlo committee and lottery populations must exceed 100, the largest simulated committee",
        );
    }

    fn validate_absence_grid(&self, problems: &mut Vec<String>) {
        let g = &self.analytic.absence;
        require(
            problems,
            !g.absences_per_id_per_year.is_empty()
                && g.absences_per_id_per_year
                    .iter()
                    .all(|c| c.is_finite() && *c >= 0.0),
            "absence.absences_per_id_per_year must be non-negative",
        );
        require(
            problems,
            g.duration_scale_days > 0.0
                && g.duration_shape > 0.0
                && g.comparison_exponential_mean_days > 0.0,
            "absence duration scale, shape and comparison mean must be positive",
        );
        check_fractions(
            problems,
            "absence.departures_per_id_per_year",
            &g.departures_per_id_per_year,
            true,
            true,
        );
    }

    fn validate_feasibility_grid(&self, problems: &mut Vec<String>) {
        let g = &self.analytic.quorum_feasibility;
        check_fractions(
            problems,
            "quorum_feasibility.online_fractions",
            &g.online_fractions,
            false,
            true,
        );
        check_fractions(
            problems,
            "quorum_feasibility.attacker_fractions",
            &g.attacker_fractions,
            true,
            false,
        );
        check_fractions(
            problems,
            "quorum_feasibility.failure_targets",
            &g.failure_targets,
            false,
            false,
        );
        require(
            problems,
            g.comparison_kwcs > 0 && self.run.monte_carlo.feasibility_samples > 0,
            "quorum_feasibility.comparison_kwcs and run.monte_carlo.feasibility_samples must be positive",
        );
    }

    fn validate_kwc_size_grid(&self, problems: &mut Vec<String>) {
        let g = &self.analytic.kwc_size;
        require(
            problems,
            !g.wcs_per_kwc.is_empty()
                && g.wcs_per_kwc.len() == g.ring_offsets.len()
                && g.wcs_per_kwc.iter().all(|k| *k >= 2),
            "kwc_size.wcs_per_kwc must list sizes of at least 2 WCs, one ring per size",
        );
        for (k, offsets) in g.wcs_per_kwc.iter().zip(&g.ring_offsets) {
            let as_u32: Vec<u32> = offsets
                .iter()
                .map(|o| u32::try_from(*o).unwrap_or(0))
                .collect();
            let expected = u32::try_from(k.saturating_sub(1)).unwrap_or(0);
            check_offsets(problems, "kwc_size.ring_offsets", &as_u32, expected);
        }
        // The size of the protocol's layout must use the protocol's ring.
        let protocol_size = 1 + u64::from(self.witness.subordinate_wcs_per_kwc.value);
        let protocol_ring: Vec<u64> = self
            .witness
            .ring_offsets
            .value
            .iter()
            .map(|o| u64::from(*o))
            .collect();
        let consistent = g
            .wcs_per_kwc
            .iter()
            .zip(&g.ring_offsets)
            .filter(|(k, _)| **k == protocol_size)
            .all(|(_, offsets)| *offsets == protocol_ring);
        require(
            problems,
            consistent,
            "kwc_size.ring_offsets for the protocol's KWC size must equal witness.ring_offsets",
        );
        let quorums_valid = !g.quorum_fractions.is_empty()
            && g.quorum_fractions
                .iter()
                .all(|q| q.is_proper() && q.exceeds_half());
        require(
            problems,
            quorums_valid,
            "kwc_size.quorum_fractions must lie above 1/2 and at most 1",
        );
        check_fractions(
            problems,
            "kwc_size.attacker_fractions",
            &g.attacker_fractions,
            false,
            false,
        );
        check_fractions(
            problems,
            "kwc_size.liveness_attacker_fractions",
            &g.liveness_attacker_fractions,
            true,
            false,
        );
        require(
            problems,
            g.comparison_kwcs > 0
                && !g.unregistered_per_wc_policy_c.is_empty()
                && g.message_bytes > 0.0
                && !g.registered_cadences_s.is_empty()
                && g.registered_cadences_s.iter().all(|c| *c > 0.0)
                && g.unregistered_cadence_s > 0.0
                && !g.miner_session_changes_per_day.is_empty()
                && g.miner_session_changes_per_day.iter().all(|s| *s >= 0.0),
            "kwc_size load inputs must be positive (sessions non-negative)",
        );
    }

    fn validate_quorum_grid(&self, problems: &mut Vec<String>) {
        let g = &self.analytic.quorum_tradeoff;
        let all_quorums = g
            .quorum_fractions
            .iter()
            .chain(&g.powit_quorum_fractions)
            .chain(&g.cac_quorum_fractions)
            .chain(std::iter::once(&g.decision_quorum));
        let mut valid = true;
        for q in all_quorums {
            valid &= q.is_proper() && q.exceeds_half();
        }
        require(
            problems,
            valid,
            "quorum_tradeoff fractions must lie above 1/2 and at most 1 (a quorum at or below one half lets two disjoint groups approve conflicting decisions)",
        );
        require(
            problems,
            !g.quorum_fractions.is_empty()
                && !g.powit_quorum_fractions.is_empty()
                && !g.cac_quorum_fractions.is_empty(),
            "quorum_tradeoff fraction lists must not be empty",
        );
        check_fractions(
            problems,
            "quorum_tradeoff.attacker_fractions",
            &g.attacker_fractions,
            false,
            false,
        );
        require(
            problems,
            !g.kwc_counts.is_empty() && g.kwc_counts.iter().all(|w| *w > 0),
            "quorum_tradeoff.kwc_counts must be positive",
        );
    }

    /// Notes where parameters that the SPEC defines together are not mutually consistent.
    ///
    /// An empty list means every relation checked here holds. These are reported, not
    /// enforced, because sweeps legitimately vary one side of a relation.
    pub fn consistency_notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        let roles = u64::from(1 + self.witness.subordinate_wcs_per_kwc.value);
        let unregistered = u64::from(self.capacity.unregistered_per_leader_wc.value);
        let registered = self
            .capacity
            .registered_per_leader_wc()
            .map(u64::from)
            .unwrap_or(0);
        let watched_unregistered = u64::from(self.witness.watched_unregistered_per_node.value);
        let watched_registered = u64::from(self.witness.watched_registered_per_node.value);
        if roles * unregistered != watched_unregistered {
            notes.push(format!(
                "unregistered miners watched per node is {watched_unregistered}, but each node \
                 sits in {roles} KWCs with {unregistered} unregistered slots each ({})",
                roles * unregistered
            ));
        }
        if roles * registered != watched_registered {
            notes.push(format!(
                "registered miners watched per node is {watched_registered}, but each node sits \
                 in {roles} KWCs with {registered} registered slots each ({})",
                roles * registered
            ));
        }
        notes
    }
}

fn require(problems: &mut Vec<String>, condition: bool, message: &str) {
    if !condition {
        problems.push(message.to_string());
    }
}

fn check_offsets(problems: &mut Vec<String>, name: &str, offsets: &[u32], expected: u32) {
    let mut sorted = offsets.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let valid = sorted.len() == offsets.len()
        && offsets.len() as u64 == u64::from(expected)
        && offsets.iter().all(|o| *o > 0);
    require(
        problems,
        valid,
        &format!("{name} must list {expected} distinct positive offsets"),
    );
}

fn check_fractions(
    problems: &mut Vec<String>,
    name: &str,
    values: &[f64],
    allow_zero: bool,
    allow_one: bool,
) {
    let valid = |x: f64| {
        let lower = if allow_zero { x >= 0.0 } else { x > 0.0 };
        let upper = if allow_one { x <= 1.0 } else { x < 1.0 };
        lower && upper
    };
    if values.is_empty() {
        problems.push(format!("{name} must not be empty"));
    } else if !values.iter().all(|x| valid(*x)) {
        problems.push(format!("{name} must lie in the valid fraction range"));
    }
}

/// Recursively merges `overlay` into `base`: tables merge key by key, other values replace.
pub(crate) fn merge_tables(base: &mut Table, overlay: Table) {
    for (key, value) in overlay {
        match (base.get_mut(&key), value) {
            (Some(Value::Table(existing)), Value::Table(incoming)) => {
                merge_tables(existing, incoming);
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// A parameter's `value` in a configuration file that leaves an optional parameter unset.
/// TOML has no null, and a file is merged over the defaults, so leaving `value` out would
/// keep the default's value.
pub const NO_VALUE: &str = "none";

/// Removes every parameter `value` equal to [`NO_VALUE`] from `table`, recording its path.
fn take_unset_values(table: &mut Table, prefix: &str, unset: &mut Vec<String>) {
    for (key, value) in table.iter_mut() {
        let Value::Table(child) = value else {
            continue;
        };
        let path = join_path(prefix, key);
        if child.contains_key("status") {
            if child.get("value").and_then(Value::as_str) == Some(NO_VALUE) {
                child.remove("value");
                unset.push(path);
            }
        } else {
            take_unset_values(child, &path, unset);
        }
    }
}

/// Writes [`NO_VALUE`] as the value of each parameter of `table` that has no value where the
/// same parameter in `defaults` has one.
fn mark_unset_values(table: &mut Table, defaults: &Table) {
    for (key, value) in table.iter_mut() {
        let (Value::Table(child), Some(Value::Table(default_child))) = (value, defaults.get(key))
        else {
            continue;
        };
        if child.contains_key("status") {
            if !child.contains_key("value") && default_child.contains_key("value") {
                // First, where a set value would be written.
                let mut marked = Table::new();
                marked.insert("value".to_string(), Value::String(NO_VALUE.to_string()));
                marked.extend(std::mem::take(child));
                *child = marked;
            }
        } else {
            mark_unset_values(child, default_child);
        }
    }
}

/// Whether the parameter at dotted path `path` in `table` has a value.
fn has_value(table: &Table, path: &str) -> bool {
    path.split('.')
        .try_fold(table, |current, part| current.get(part)?.as_table())
        .is_some_and(|param| param.contains_key("value"))
}

/// Fails if any parameter's `status` differs between the defaults and the merged table.
fn check_statuses_unchanged(
    defaults: &Table,
    merged: &Table,
    prefix: &str,
) -> Result<(), ConfigError> {
    for (key, value) in defaults {
        let Value::Table(default_child) = value else {
            continue;
        };
        let path = join_path(prefix, key);
        let Some(Value::Table(merged_child)) = merged.get(key) else {
            continue;
        };
        if let Some(expected) = default_child.get("status") {
            let found = merged_child.get("status");
            if found != Some(expected) {
                return Err(ConfigError::StatusOverride {
                    path,
                    expected: render(Some(expected)),
                    found: render(found),
                });
            }
        }
        check_statuses_unchanged(default_child, merged_child, &path)?;
    }
    Ok(())
}

pub(crate) fn join_path(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}.{key}")
    }
}

fn render(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => "(missing)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::param::Status;

    #[test]
    fn empty_override_file_gives_defaults() {
        assert_eq!(Config::from_toml_str("").unwrap(), Config::default());
    }

    #[test]
    fn partial_override_changes_only_the_named_value() {
        let config = Config::from_toml_str("[genesis]\nids.value = 2000000\n").unwrap();
        assert_eq!(config.genesis.ids.value, 2_000_000);
        assert_eq!(config.issuance.cap_safety_factor.value.numerator, 7);
        assert_eq!(config.genesis.ids.status, Status::D);
        assert_eq!(config.cac.size.value, 600);
    }

    #[test]
    fn unknown_key_is_rejected() {
        let error = Config::from_toml_str("[genesis]\nidz.value = 5\n").unwrap_err();
        assert!(matches!(error, ConfigError::Structure(_)), "{error}");
    }

    #[test]
    fn changing_a_status_tag_is_rejected() {
        let error = Config::from_toml_str("[genesis]\nids.status = \"O\"\n").unwrap_err();
        assert!(
            matches!(error, ConfigError::StatusOverride { .. }),
            "{error}"
        );
    }

    #[test]
    fn restating_the_same_status_is_allowed() {
        assert!(Config::from_toml_str("[genesis]\nids.status = \"D\"\n").is_ok());
    }

    #[test]
    fn invalid_quorum_is_reported() {
        let error =
            Config::from_toml_str("[quorum]\nregistered_leader_min.value = 11\n").unwrap_err();
        assert!(matches!(error, ConfigError::Invalid(_)), "{error}");
    }

    #[test]
    fn quorum_fractions_at_or_below_one_half_are_rejected() {
        let error = Config::from_toml_str(
            "[analytic.quorum_tradeoff]\nquorum_fractions = [{ numerator = 1, denominator = 2 }]\n",
        )
        .unwrap_err();
        assert!(matches!(error, ConfigError::Invalid(_)), "{error}");
        assert!(
            Config::from_toml_str(
                "[analytic.quorum_tradeoff]\nquorum_fractions = [{ numerator = 51, denominator = 100 }]\n"
            )
            .is_ok()
        );
    }

    #[test]
    fn kwc_size_rings_must_match_their_sizes_and_the_protocol() {
        let wrong_length = Config::from_toml_str(
            "[analytic.kwc_size]\nwcs_per_kwc = [3, 4]\nring_offsets = [[1, 3], [1, 4]]\n",
        )
        .unwrap_err();
        assert!(
            matches!(wrong_length, ConfigError::Invalid(_)),
            "{wrong_length}"
        );
        let not_protocol = Config::from_toml_str(
            "[analytic.kwc_size]\nwcs_per_kwc = [4]\nring_offsets = [[1, 3, 7]]\n",
        )
        .unwrap_err();
        assert!(
            matches!(not_protocol, ConfigError::Invalid(_)),
            "{not_protocol}"
        );
    }

    #[test]
    fn absence_and_feasibility_inputs_are_checked() {
        assert!(Config::from_toml_str("[analytic.absence]\nduration_shape = 0.0\n").is_err());
        assert!(
            Config::from_toml_str("[analytic.quorum_feasibility]\nfailure_targets = [1.0]\n")
                .is_err()
        );
        assert!(
            Config::from_toml_str("[offline]\nlong_absence_threshold_s.value = 0.0\n").is_err()
        );
    }

    #[test]
    fn ring_offsets_must_match_the_kwc_layout() {
        let error = Config::from_toml_str("[witness]\nring_offsets.value = [1, 4]\n").unwrap_err();
        assert!(matches!(error, ConfigError::Invalid(_)), "{error}");
        let repeated =
            Config::from_toml_str("[witness]\nring_offsets.value = [1, 4, 4]\n").unwrap_err();
        assert!(matches!(repeated, ConfigError::Invalid(_)), "{repeated}");
    }

    #[test]
    fn variant_b_corrections_are_parsed_and_checked() {
        let p1 = Config::from_toml_str(
            "[issuance]\ncount_based_correction.value = { form = \"recent_interval_ratio\", window_blocks = 144, bound = 0.1 }\n",
        )
        .unwrap();
        assert_eq!(
            p1.issuance.count_based_correction.value,
            Some(CountBasedCorrection::RecentIntervalRatio {
                window_blocks: 144,
                bound: 0.1
            })
        );
        let exact = Config::from_toml_str(
            "[issuance]\ncount_based_correction.value = { form = \"discrete_exact\" }\n",
        )
        .unwrap();
        assert_eq!(
            exact.issuance.count_based_correction.value,
            Some(CountBasedCorrection::DiscreteExact)
        );
        assert!(
            Config::from_toml_str(
                "[issuance]\ncount_based_correction.value = { form = \"accumulated\", window_blocks = 144, bound = 1.5 }\n",
            )
            .is_err()
        );
    }

    #[test]
    fn default_issuance_rate_is_1_051_200_per_year() {
        assert_eq!(Config::default().issuance_per_year(), 1_051_200.0);
    }

    #[test]
    fn default_parameters_are_mutually_consistent() {
        assert!(Config::default().consistency_notes().is_empty());
    }

    #[test]
    fn inconsistent_capacity_split_is_noted() {
        let config =
            Config::from_toml_str("[capacity]\nunregistered_per_leader_wc.value = 150\n").unwrap();
        assert_eq!(config.consistency_notes().len(), 2);
    }

    #[test]
    fn default_config_round_trips_through_toml_text() {
        let text = Config::default().to_toml_string().unwrap();
        assert_eq!(Config::from_toml_str(&text).unwrap(), Config::default());
    }

    #[test]
    fn writing_through_a_table_keeps_the_struct_layout() {
        let config = Config::default();
        assert_eq!(
            config.to_toml_string().unwrap(),
            toml::to_string(&config).unwrap()
        );
    }

    #[test]
    fn an_optional_parameter_with_a_default_can_be_unset_and_read_back() {
        // The offline points ladder defaults to 24 h (architect, 2026-10-21); "none" removes it.
        let text = "[penalties]\noffline_points_after_s.value = \"none\"\n";
        let config = Config::from_toml_str(text).unwrap();
        assert_eq!(config.penalties.offline_points_after_s.value, None);
        let written = config.to_toml_string().unwrap();
        assert!(written.contains("value = \"none\""), "{written}");
        assert_eq!(Config::from_toml_str(&written).unwrap(), config);
        // An optional parameter whose default is already unset is still left out.
        assert!(
            !Config::default()
                .to_toml_string()
                .unwrap()
                .contains(NO_VALUE)
        );
    }

    #[test]
    fn none_is_rejected_for_a_parameter_that_is_not_optional() {
        let error = Config::from_toml_str("[genesis]\nids.value = \"none\"\n").unwrap_err();
        assert!(error.to_string().contains("genesis.ids"), "{error}");
        let error =
            Config::from_toml_str("[offline]\nunreachable_rule.value = \"none\"\n").unwrap_err();
        assert!(
            error.to_string().contains("offline.unreachable_rule"),
            "{error}"
        );
    }

    #[test]
    fn invalid_toml_is_a_parse_error() {
        assert!(matches!(
            Config::from_toml_str("[genesis"),
            Err(ConfigError::Parse(_))
        ));
    }

    #[test]
    fn missing_file_is_a_read_error() {
        let error = Config::from_file(Path::new("/nonexistent/gb.toml")).unwrap_err();
        assert!(matches!(error, ConfigError::Read { .. }));
    }
}
