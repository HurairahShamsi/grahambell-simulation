//! Parameter registry: descriptions, SPEC references and the SPEC §2 row mapping.
//!
//! The registry is the human-readable index of every [`crate::Param`] in [`Config`]. Tests
//! check that it lists every parameter exactly once and that every row of the SPEC §2 table
//! maps to configuration keys whose status tags match the SPEC.

use crate::config::{Config, join_path};
use crate::error::ConfigError;
use crate::param::Status;
use std::collections::BTreeSet;
use toml::{Table, Value};

/// Documentation for one configuration key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamDoc {
    /// Dotted configuration key, for example `genesis.ids`.
    pub key: &'static str,
    /// SPEC sections that define the parameter.
    pub spec_ref: &'static str,
    /// Unit of the value, or an empty string.
    pub unit: &'static str,
    /// One-sentence description.
    pub description: &'static str,
}

const fn doc(
    key: &'static str,
    spec_ref: &'static str,
    unit: &'static str,
    description: &'static str,
) -> ParamDoc {
    ParamDoc {
        key,
        spec_ref,
        unit,
        description,
    }
}

/// Every protocol parameter, in display order.
pub const PARAM_DOCS: &[ParamDoc] = &[
    doc(
        "pacing.hashes_per_second",
        "§2",
        "hash/s",
        "Hash attempts per second per admitted /64 (unregistered) or per ID (registered).",
    ),
    doc(
        "issuance.pow_id_target_interval_s",
        "§2, §3.9",
        "s",
        "Target interval between PoW-ID blocks.",
    ),
    doc(
        "issuance.rate_mode",
        "§2, §3.9",
        "",
        "Fixed issuance rate, or the adaptive variant capped by registered IDs.",
    ),
    doc(
        "issuance.difficulty_variant",
        "§2, §3.8",
        "",
        "Difficulty rule: Variant B (count-based, default) or Variant A (Bitcoin-style, comparison).",
    ),
    doc(
        "issuance.retarget_window_blocks",
        "§2, §3.8",
        "PoW-ID blocks",
        "Variant A comparison: retarget window K.",
    ),
    doc(
        "issuance.retarget_clamp_factor",
        "§2, §3.8",
        "factor",
        "Variant A comparison: largest difficulty change per retarget.",
    ),
    doc(
        "issuance.count_based_correction",
        "§2, §3.8",
        "",
        "Variant B's correction from recent block times: P2 with bound ×1.1 by default since SPEC v0.7; none, P1, P2 at ×1.25 and ×1.5 (which fails H5) and B-exact are comparisons.",
    ),
    doc(
        "issuance.difficulty_count_rule",
        "§3.8",
        "",
        "Which admitted miners Variant B counts: those whose KWC issued entropy for the round (the default since SPEC v0.7), those past the post-admission wait, all Globally Active, or the issued count as committed at the block a confirmation depth before the parent (lagged).",
    ),
    doc(
        "issuance.min_attack_time_floor_years",
        "§2, §3.9",
        "years",
        "Minimum attack time floor T_min for adaptive issuance.",
    ),
    doc(
        "issuance.cap_checkpoint_interval_fraction_of_t_min",
        "§2, §3.9",
        "× T_min",
        "Interval between recalculations of the adaptive cap.",
    ),
    doc(
        "issuance.cap_safety_factor",
        "§2, §3.9",
        "fraction",
        "Safety factor k in the adaptive cap registered_IDs / (k × T_min).",
    ),
    doc(
        "issuance.confirmation_depth_blocks",
        "§2, §3.7",
        "PoW-ID blocks",
        "Confirmations before a new ID becomes active.",
    ),
    doc(
        "issuance.start_rule",
        "§3.5",
        "",
        "Where the PoW-ID hash chain starts: rule (r), the KWC-attested entropy issue time (the default since SPEC v0.7); rule (a), the previous timestamp + 1 s, is the comparison and rule (b) is kept for S13.",
    ),
    doc(
        "issuance.same_height_tie_break",
        "§3.7",
        "",
        "Which of two valid PoW-ID blocks at the same height is kept: the first seen (the default since SPEC v0.7), or the earlier timestamp and then the lower hash (v0.6, the comparison).",
    ),
    doc(
        "transactions.pow_tx_interval_s",
        "§2, §5",
        "s",
        "Interval between PoW-Tx blocks.",
    ),
    doc(
        "witness.wc_size",
        "§2",
        "nodes",
        "Witness nodes per Witness Chain.",
    ),
    doc(
        "witness.subordinate_wcs_per_kwc",
        "§2",
        "WCs",
        "Subordinate WCs per KWC (3 gives 40 nodes; 2 is the 30-node comparison).",
    ),
    doc(
        "witness.kwc_count_rule",
        "§2",
        "",
        "Each WC leads one KWC and is a subordinate in as many as a KWC has subordinates.",
    ),
    doc(
        "witness.ring_offsets",
        "§2, §4.2",
        "WCs",
        "Golomb-ring offsets: KWC w has subordinate WCs (w + offset) mod W.",
    ),
    doc(
        "witness.ring_offsets_30_node",
        "§2, §4.2",
        "WCs",
        "Golomb-ring offsets in the 30-node comparison layout.",
    ),
    doc(
        "witness.allocation_beacon_delay_blocks",
        "§2, §4.2",
        "PoW-ID blocks",
        "Blocks between an ID's confirmation and the block whose hash is its allocation beacon.",
    ),
    doc(
        "witness.watched_registered_per_node",
        "§2",
        "miners",
        "Registered miners watched per witness node.",
    ),
    doc(
        "witness.watched_unregistered_per_node",
        "§2",
        "miners",
        "Unregistered miners watched per witness node.",
    ),
    doc(
        "quorum.registered_leader_min",
        "§2",
        "signatures",
        "Registered-miner PoWit quorum: minimum from the leader WC.",
    ),
    doc(
        "quorum.registered_subordinate_min",
        "§2",
        "signatures",
        "Registered-miner PoWit quorum: minimum from the subordinate WCs (40-node KWC).",
    ),
    doc(
        "quorum.registered_subordinate_min_30_node",
        "§2",
        "signatures",
        "Registered-miner PoWit quorum from subordinates in the 30-node comparison.",
    ),
    doc(
        "quorum.unregistered_total_min",
        "§2",
        "signatures",
        "Unregistered-miner PoWit quorum: any members of the 40-node KWC.",
    ),
    doc(
        "quorum.unregistered_total_min_30_node",
        "§2",
        "signatures",
        "Unregistered-miner PoWit quorum in the 30-node comparison.",
    ),
    doc(
        "entropy.signers",
        "§2, §3.4",
        "",
        "KWC members whose signatures make the entropy: a threshold of the seated members (⌈2n/3⌉ by default since SPEC v0.7, [P]; or 21 of 40), or every online member.",
    ),
    doc(
        "entropy.equivocation_scope",
        "§3.4, Appendix A",
        "",
        "What counts as miner equivocation: two headers at one height, or [P] two headers on one parent.",
    ),
    doc(
        "capacity.miners_per_leader_wc",
        "§2",
        "miners",
        "Total miner capacity per leader WC (registered + unregistered).",
    ),
    doc(
        "capacity.unregistered_per_leader_wc",
        "§2",
        "miners",
        "Unregistered share of the capacity; the registered share is the remainder.",
    ),
    doc(
        "capacity.spare_target_fraction",
        "§2",
        "fraction",
        "Spare witnessing capacity target over demand.",
    ),
    doc(
        "network.node_request_rate_limit_per_s",
        "§2",
        "req/s",
        "Per-node request rate limit.",
    ),
    doc(
        "timing.grace_epoch_rounds",
        "§2",
        "rounds",
        "Grace epoch in rounds of the miner's own block type.",
    ),
    doc(
        "timing.clock_tolerance_s",
        "§2, §3.6, §3.7",
        "s",
        "Clock tolerance δ for signing and validation.",
    ),
    doc(
        "timing.one_chance_wait_rounds",
        "§2, §4.5",
        "rounds",
        "Wait before One Chance is judged to have failed (one grace epoch by default).",
    ),
    doc(
        "timing.powit_deadline_s",
        "§2, §3.7",
        "s",
        "PoWit arrival deadline D after the block's attested timestamp, on each node's own clock; none by default since 2026-10-24 (10 s with the soft scope is [P]).",
    ),
    doc(
        "timing.powit_deadline_scope",
        "§3.7",
        "",
        "How a node treats a block whose PoWit reached it after D: invalid for good, or only not extended.",
    ),
    doc(
        "timing.early_block",
        "§3.7",
        "",
        "How a node treats a block received before its timestamp minus δ: declined until its own clock reaches the timestamp and then reconsidered (the default since 2026-10-24), or rejected for good.",
    ),
    doc(
        "timing.witness_signing",
        "§3.6",
        "",
        "When an honest witness signs the PoWit: at its own clock t0 + N, or at t0 + N − δ.",
    ),
    doc(
        "admission.post_admission_wait_rounds",
        "§2, §3.1",
        "rounds",
        "Wait after admission before mining.",
    ),
    doc(
        "admission.convergence_interval_epochs",
        "§2, §3.1",
        "epochs",
        "Convergence interval before a claim becomes active.",
    ),
    doc(
        "admission.peer_bootstrap_count",
        "§2, §3.1",
        "peers",
        "Registered peers a newcomer bootstraps from.",
    ),
    doc(
        "admission.failover_after_rounds",
        "§3.1, §3.4",
        "rounds",
        "Failover (M3a.1 experiment): rounds a miner's KWC may stay below quorum before the miner moves to a KWC that can issue entropy; unset is off.",
    ),
    doc(
        "cac.size",
        "§2, §4.3",
        "members",
        "Chain Allocation Committee size.",
    ),
    doc(
        "cac.membership_policy",
        "§2, §4.3",
        "",
        "Committee membership policy.",
    ),
    doc(
        "cac.join_every_n_tx_blocks",
        "§2, §4.3",
        "PoW-Tx blocks",
        "One new committee member joins for every n-th PoW-Tx block.",
    ),
    doc(
        "cac.seat_selection",
        "§2, §4.3",
        "",
        "How the new member is chosen: lottery over the canonical active list, or the v0.2 rule.",
    ),
    doc(
        "cac.lottery_beacon_offset_blocks",
        "§2, §4.3",
        "PoW-Tx blocks",
        "Lottery beacon offset k: the draw for block B uses the hash of block B + k.",
    ),
    doc(
        "cac.approval_threshold",
        "§2, §4.3",
        "fraction",
        "Approval threshold as a fraction of members, rounded up.",
    ),
    doc("genesis.ids", "§2", "IDs", "Number of genesis IDs G."),
    doc(
        "genesis.distribution",
        "§2",
        "",
        "Genesis distribution method.",
    ),
    doc(
        "genesis.secretly_controlled_fraction",
        "§2",
        "fraction",
        "Fraction of genesis IDs secretly controlled by one party.",
    ),
    doc(
        "offline.deactivation_threshold_s",
        "§2, §4.7",
        "s",
        "Absence after which an ID is deactivated.",
    ),
    doc(
        "offline.reactivation_wait_s",
        "§2, §4.7",
        "s",
        "Wait before a returning ID is re-activated.",
    ),
    doc(
        "offline.long_absence_threshold_s",
        "§2, §4.2, §4.7",
        "s",
        "Long-absence threshold L: a seat is vacated only after a longer continuous absence, or a ban.",
    ),
    doc(
        "offline.unreachable_rule",
        "§4.6, §12",
        "",
        "Failing One Chance: refusal and penalised (strict), or [P] recorded offline when unreachable.",
    ),
    doc(
        "penalties.suspension_ladder_s",
        "§2, §4.8",
        "s",
        "Successive suspensions for lesser offences.",
    ),
    doc(
        "penalties.final_step",
        "§2, §4.8",
        "",
        "Final step of the penalty ladder.",
    ),
    doc(
        "penalties.strict_offence",
        "§4.6, §4.8",
        "",
        "Penalty for a failed One Chance under the strict rule: the ladder, the ladder with a yearly reset [P], or a ban.",
    ),
    doc(
        "penalties.offline_points_after_s",
        "§4.7, §4.8",
        "s",
        "Offline points: an offline period at least this long earns points, whatever its cause (24 h by default since SPEC v0.7; unset is off).",
    ),
    doc(
        "penalties.offline_points_steps",
        "§4.7, §4.8",
        "points",
        "Points added by successive awards since the points last reset; the last step repeats.",
    ),
    doc(
        "penalties.offline_points_cap",
        "§4.7, §4.8",
        "points",
        "Points at which the outcome applies.",
    ),
    doc(
        "penalties.offline_points_reset_s",
        "§4.7, §4.8",
        "s",
        "Block time after the most recent award at which the points reset to zero.",
    ),
    doc(
        "penalties.offline_points_outcome",
        "§4.7, §4.8",
        "",
        "Outcome at the cap: the witness seat is lost (the ID can still mine), or the ID is blacklisted.",
    ),
    doc(
        "penalties.offline_points_seat_cooldown_s",
        "§4.7, §4.8",
        "s",
        "After a seat loss, how long before the member may take a seat again.",
    ),
    doc("crypto.signature_scheme", "§2", "", "Signature scheme."),
    doc("crypto.hash_function", "§2", "", "Hash function."),
];

/// Mapping from SPEC §2 row names to the configuration keys that implement them.
pub const SPEC_SECTION_2_ROWS: &[(&str, &[&str])] = &[
    ("Hash pacing", &["pacing.hashes_per_second"]),
    (
        "PoW-ID target interval",
        &["issuance.pow_id_target_interval_s", "issuance.rate_mode"],
    ),
    ("Difficulty rule (§3.8)", &["issuance.difficulty_variant"]),
    (
        "Variant A comparison settings (§3.8)",
        &[
            "issuance.retarget_window_blocks",
            "issuance.retarget_clamp_factor",
        ],
    ),
    (
        "Variant B correction from recent block times (§3.8)",
        &["issuance.count_based_correction"],
    ),
    ("PoW-Tx interval", &["transactions.pow_tx_interval_s"]),
    ("WC size", &["witness.wc_size"]),
    ("KWC composition", &["witness.subordinate_wcs_per_kwc"]),
    ("Number of KWCs", &["witness.kwc_count_rule"]),
    (
        "KWC ring offsets (§4.2)",
        &["witness.ring_offsets", "witness.ring_offsets_30_node"],
    ),
    (
        "Allocation beacon delay (§4.2)",
        &["witness.allocation_beacon_delay_blocks"],
    ),
    (
        "Registered-miner PoWit quorum",
        &[
            "quorum.registered_leader_min",
            "quorum.registered_subordinate_min",
            "quorum.registered_subordinate_min_30_node",
        ],
    ),
    (
        "Unregistered-miner PoWit quorum",
        &[
            "quorum.unregistered_total_min",
            "quorum.unregistered_total_min_30_node",
        ],
    ),
    ("Entropy signatures", &["entropy.signers"]),
    (
        "Miner capacity per leader WC",
        &[
            "capacity.miners_per_leader_wc",
            "capacity.unregistered_per_leader_wc",
        ],
    ),
    (
        "Miners watched per witness node",
        &[
            "witness.watched_registered_per_node",
            "witness.watched_unregistered_per_node",
        ],
    ),
    (
        "Spare witnessing capacity target",
        &["capacity.spare_target_fraction"],
    ),
    (
        "Per-node request rate limit",
        &["network.node_request_rate_limit_per_s"],
    ),
    ("Grace epoch", &["timing.grace_epoch_rounds"]),
    (
        "Post-admission wait before mining",
        &["admission.post_admission_wait_rounds"],
    ),
    (
        "Convergence interval (admission)",
        &["admission.convergence_interval_epochs"],
    ),
    (
        "Peer bootstrap count (newcomers)",
        &["admission.peer_bootstrap_count"],
    ),
    ("CAC size", &["cac.size", "cac.membership_policy"]),
    (
        "CAC seat selection (§4.3)",
        &["cac.seat_selection", "cac.join_every_n_tx_blocks"],
    ),
    (
        "CAC lottery beacon offset k (§4.3)",
        &["cac.lottery_beacon_offset_blocks"],
    ),
    ("CAC approval threshold", &["cac.approval_threshold"]),
    ("Genesis IDs (G)", &["genesis.ids"]),
    (
        "Genesis distribution",
        &[
            "genesis.distribution",
            "genesis.secretly_controlled_fraction",
        ],
    ),
    (
        "Confirmation depth for new IDs",
        &["issuance.confirmation_depth_blocks"],
    ),
    ("Clock tolerance δ", &["timing.clock_tolerance_s"]),
    ("One Chance wait (§4.5)", &["timing.one_chance_wait_rounds"]),
    (
        "PoWit arrival deadline D (§3.7)",
        &["timing.powit_deadline_s"],
    ),
    (
        "Offline deactivation threshold",
        &["offline.deactivation_threshold_s"],
    ),
    (
        "Re-activation wait after return",
        &["offline.reactivation_wait_s"],
    ),
    (
        "Long-absence threshold L (§4.7)",
        &["offline.long_absence_threshold_s"],
    ),
    (
        "Penalty ladder (lesser offences)",
        &["penalties.suspension_ladder_s", "penalties.final_step"],
    ),
    (
        "Minimum attack time floor (adaptive issuance)",
        &["issuance.min_attack_time_floor_years"],
    ),
    (
        "Adaptive-cap checkpoint interval (§3.9)",
        &["issuance.cap_checkpoint_interval_fraction_of_t_min"],
    ),
    (
        "Adaptive-cap safety factor k (§3.9)",
        &["issuance.cap_safety_factor"],
    ),
    ("Signature scheme", &["crypto.signature_scheme"]),
    ("Hash function", &["crypto.hash_function"]),
];

/// Parameters defined outside the SPEC §2 table, with the section that defines them.
pub const OUTSIDE_SECTION_2: &[(&str, &str)] = &[
    ("issuance.start_rule", "§3.5"),
    ("issuance.same_height_tie_break", "§3.7"),
    ("issuance.difficulty_count_rule", "§3.8"),
    ("entropy.equivocation_scope", "§3.4, Appendix A"),
    ("timing.powit_deadline_scope", "§3.7"),
    ("timing.early_block", "§3.7"),
    ("timing.witness_signing", "§3.6"),
    ("offline.unreachable_rule", "§4.6"),
    ("penalties.strict_offence", "§4.6, §4.8"),
    ("admission.failover_after_rounds", "§3.1, §3.4"),
    ("penalties.offline_points_after_s", "§4.7, §4.8"),
    ("penalties.offline_points_steps", "§4.7, §4.8"),
    ("penalties.offline_points_cap", "§4.7, §4.8"),
    ("penalties.offline_points_reset_s", "§4.7, §4.8"),
    ("penalties.offline_points_outcome", "§4.7, §4.8"),
    ("penalties.offline_points_seat_cooldown_s", "§4.7, §4.8"),
];

/// One row of the SPEC §2 parameter table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecRow {
    /// Parameter name as written in the SPEC.
    pub name: String,
    /// Default as written in the SPEC.
    pub default: String,
    /// Sweep or variants as written in the SPEC.
    pub sweep: String,
    /// Status tags found in the status cell.
    pub statuses: BTreeSet<Status>,
}

/// Extracts the rows of the SPEC §2 parameter table from the SPEC markdown.
pub fn parse_spec_section_2(markdown: &str) -> Vec<SpecRow> {
    let mut rows = Vec::new();
    let mut in_section = false;
    let mut header_seen = false;
    for line in markdown.lines() {
        if line.starts_with("## ") {
            in_section = line.starts_with("## 2.");
            continue;
        }
        if !in_section || !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if !header_seen {
            header_seen = cells.first() == Some(&"Parameter");
            continue;
        }
        if cells.len() < 4 || cells[0].starts_with("---") {
            continue;
        }
        rows.push(SpecRow {
            name: cells[0].to_string(),
            default: cells[1].to_string(),
            sweep: cells[2].to_string(),
            statuses: parse_status_cell(cells[3]),
        });
    }
    rows
}

/// Reads the status tags (`D`, `P`, `O`) from a SPEC status cell such as "D default, O split".
pub fn parse_status_cell(cell: &str) -> BTreeSet<Status> {
    cell.split(|c: char| !c.is_ascii_alphabetic())
        .filter_map(|token| match token {
            "D" => Some(Status::D),
            "P" => Some(Status::P),
            "O" => Some(Status::O),
            _ => None,
        })
        .collect()
}

/// One parameter as shown by `gb params` and written to `parameters.csv`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ParamListing {
    /// Dotted configuration key.
    pub key: String,
    /// SPEC status tag.
    pub status: String,
    /// Value in TOML notation, or "—" when unset.
    pub value: String,
    /// Sweep or comparison values in TOML notation, or an empty string.
    pub sweep: String,
    /// Unit of the value.
    pub unit: String,
    /// SPEC sections that define the parameter.
    pub spec_ref: String,
    /// One-sentence description.
    pub description: String,
}

/// Lists every protocol parameter of `config`, in registry order.
pub fn parameter_listing(config: &Config) -> Result<Vec<ParamListing>, ConfigError> {
    let table = config.to_table()?;
    PARAM_DOCS
        .iter()
        .map(|doc| {
            let param = lookup_table(&table, doc.key).ok_or_else(|| {
                ConfigError::Structure(format!("registry key `{}` is not a parameter", doc.key))
            })?;
            Ok(ParamListing {
                key: doc.key.to_string(),
                status: param.get("status").map(render_value).unwrap_or_default(),
                value: param
                    .get("value")
                    .map(render_value)
                    .unwrap_or_else(|| "—".to_string()),
                sweep: param.get("sweep").map(render_value).unwrap_or_default(),
                unit: doc.unit.to_string(),
                spec_ref: doc.spec_ref.to_string(),
                description: doc.description.to_string(),
            })
        })
        .collect()
}

/// Dotted keys of every parameter in `config` (every table carrying a `status` key).
pub fn parameter_keys(config: &Config) -> Result<BTreeSet<String>, ConfigError> {
    let mut keys = BTreeSet::new();
    collect_param_keys(&config.to_table()?, "", &mut keys);
    Ok(keys)
}

/// Status tag of the parameter at `key`, if it exists.
pub fn status_of(config: &Config, key: &str) -> Result<Option<Status>, ConfigError> {
    let table = config.to_table()?;
    Ok(lookup_table(&table, key)
        .and_then(|param| param.get("status"))
        .and_then(Value::as_str)
        .and_then(|tag| parse_status_cell(tag).into_iter().next()))
}

fn collect_param_keys(table: &Table, prefix: &str, keys: &mut BTreeSet<String>) {
    for (key, value) in table {
        if let Value::Table(child) = value {
            let path = join_path(prefix, key);
            if child.contains_key("status") {
                keys.insert(path);
            } else {
                collect_param_keys(child, &path, keys);
            }
        }
    }
}

fn lookup_table<'a>(table: &'a Table, dotted: &str) -> Option<&'a Table> {
    dotted
        .split('.')
        .try_fold(table, |current, part| current.get(part)?.as_table())
}

fn render_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_cell_with_two_tags_is_parsed() {
        let tags = parse_status_cell("D default, O split");
        assert_eq!(tags, BTreeSet::from([Status::D, Status::O]));
        assert_eq!(
            parse_status_cell("D / P"),
            BTreeSet::from([Status::D, Status::P])
        );
    }

    #[test]
    fn registry_lists_every_parameter_exactly_once() {
        let config = Config::default();
        let documented: BTreeSet<String> = PARAM_DOCS.iter().map(|d| d.key.to_string()).collect();
        assert_eq!(documented.len(), PARAM_DOCS.len(), "duplicate registry key");
        assert_eq!(documented, parameter_keys(&config).unwrap());
    }

    #[test]
    fn every_parameter_is_mapped_to_a_spec_row_or_section() {
        let mut mapped: BTreeSet<&str> = SPEC_SECTION_2_ROWS
            .iter()
            .flat_map(|(_, keys)| keys.iter().copied())
            .collect();
        mapped.extend(OUTSIDE_SECTION_2.iter().map(|(key, _)| *key));
        let documented: BTreeSet<&str> = PARAM_DOCS.iter().map(|d| d.key).collect();
        assert_eq!(mapped, documented);
    }

    #[test]
    fn listing_shows_unset_values_as_a_dash() {
        let listing = parameter_listing(&Config::default()).unwrap();
        let row = listing
            .iter()
            .find(|row| row.key == "offline.deactivation_threshold_s")
            .unwrap();
        assert_eq!(row.value, "—");
        assert_eq!(row.status, "O");
        assert!(row.sweep.contains("3600"));
    }

    #[test]
    fn status_lookup_finds_the_spec_tag() {
        let config = Config::default();
        assert_eq!(
            status_of(&config, "issuance.count_based_correction").unwrap(),
            Some(Status::D)
        );
        assert_eq!(
            status_of(&config, "issuance.min_attack_time_floor_years").unwrap(),
            Some(Status::P)
        );
        assert_eq!(
            status_of(&config, "cac.lottery_beacon_offset_blocks").unwrap(),
            Some(Status::O)
        );
        assert_eq!(status_of(&config, "no.such.key").unwrap(), None);
    }

    #[test]
    fn spec_table_parser_reads_rows_and_statuses() {
        let markdown = "## 2. Parameters\n\n| Parameter | Default | Sweep / variants | Status |\n|---|---|---|---|\n| WC size | 10 | 10 | D |\n| Hash function | SHA-256 | — | P |\n\n---\n\n## 3. Next\n| x | y | z | D |\n";
        let rows = parse_spec_section_2(markdown);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "WC size");
        assert_eq!(rows[1].statuses, BTreeSet::from([Status::P]));
    }
}
