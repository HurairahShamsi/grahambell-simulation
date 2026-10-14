//! M3 Tier 1 simulator settings (`gb sim`, ARCHITECTURE §3).
//!
//! These are modelling inputs, not protocol parameters: network latency, household downtime,
//! growth and the size of each experiment. Each value's source, or its label "illustrative",
//! is listed in `docs/ASSUMPTIONS.md` (entries SIM-n). Protocol rules come from the protocol
//! sections of [`crate::Config`].

use serde::{Deserialize, Serialize};

/// How the simulator stands in for cryptography (ARCHITECTURE §3; architect, 2026-10-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CryptoMode {
    /// Winning steps drawn from the per-step success probability of the target; block hashes
    /// still real SHA-256 over stand-in entropy. Used only after the mode-agreement gate.
    #[default]
    Fast,
    /// The M2 C6 stand-in: uniform 32-byte entropy, real SHA-256 hash chains.
    StandIn,
    /// Real BLS12-381 entropy and PoWit signatures and real SHA-256 chains (small runs).
    Real,
}

/// Where the inter-region latency comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LatencySource {
    /// `data/latency/region_rtt.csv`, built from RIPE Atlas anchor-mesh pings.
    #[default]
    RipeAtlas,
    /// Fallback: great-circle distance × route factor at fibre speed, plus a per-hop delay.
    Stylised,
}

/// Everything `gb sim` reads besides the protocol parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimConfig {
    /// Crypto mode for the large runs (the gate compares all three).
    pub crypto_mode: CryptoMode,
    /// Worker threads for independent runs (each run is single-threaded and reproducible).
    pub threads: u32,
    /// Fraction of events written to the optional sampled trace (0 = no trace).
    pub trace_sample: f64,
    /// Most nodes' acceptances of unsettled blocks kept in the run's cache at once; past it
    /// the cache is emptied and refills as needed. A cached value is what recomputing it
    /// gives, so this bounds memory (about 80 bytes an entry) and never changes a result.
    pub acceptance_cache_entries: u64,
    /// Population and protocol-glue settings.
    pub world: SimWorld,
    /// Network model.
    pub network: SimNetwork,
    /// Household downtime model.
    pub households: SimHouseholds,
    /// Honest growth and patience (A6).
    pub growth: SimGrowth,
    /// Experiment sizes.
    pub suite: SimSuite,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            crypto_mode: CryptoMode::Fast,
            threads: 4,
            trace_sample: 0.0,
            acceptance_cache_entries: 2_000_000,
            world: SimWorld::default(),
            network: SimNetwork::default(),
            households: SimHouseholds::default(),
            growth: SimGrowth::default(),
            suite: SimSuite::default(),
        }
    }
}

/// Population and protocol-glue settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimWorld {
    /// Registered IDs at the start of a run (genesis of the simulated network); 290,000 is
    /// 1:10 of the protocol's 2.9M genesis.
    pub registered_ids: u64,
    /// A winning /64 re-admits to compete again once its ID is confirmed.
    pub winners_readmit: bool,
    /// Ideal validators (no downtime) whose majority tip defines the network's chain.
    pub panel_nodes: u32,
    /// Offline duration after which a registered ID is deactivated, in seconds; the SPEC
    /// leaves it open (§2 sweep 1 h–7 d), and M3a uses one value of that sweep.
    pub deactivation_threshold_s: f64,
    /// A run stops early, recorded as a fork storm, once more blocks than this are unsettled
    /// at once (ASSUMPTIONS SIM-29). After a stall of hours, start rule (a) without a PoWit
    /// deadline lets every slot with a winning step in the missed time broadcast at once, and
    /// each competing block opens a round with the same backlog. Most such storms resolve;
    /// some keep growing and cannot be simulated to their end. Ordinary forks stay below
    /// about 100 unsettled blocks.
    pub fork_storm_unsettled: u32,
}

impl Default for SimWorld {
    fn default() -> Self {
        SimWorld {
            registered_ids: 290_000,
            winners_readmit: true,
            panel_nodes: 101,
            deactivation_threshold_s: 86_400.0,
            fork_storm_unsettled: 4_000,
        }
    }
}

/// Network model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimNetwork {
    /// Latency source.
    pub latency_source: LatencySource,
    /// Region names, in the order of the other per-region lists; they must match the latency
    /// file.
    pub regions: Vec<String>,
    /// Share of miners and witnesses in each region.
    pub region_shares: Vec<f64>,
    /// Local time offset of each region from UTC, in hours (for night-time device schedules).
    pub utc_offsets_h: Vec<f64>,
    /// Region centroids `[latitude, longitude]` for the stylised fallback.
    pub region_centroids: Vec<[f64; 2]>,
    /// Regions with the unreliable-grid household profile.
    pub unreliable_grid_regions: Vec<String>,
    /// Median one-way access (last-mile) delay of a household connection, in milliseconds.
    pub access_delay_median_ms: f64,
    /// Log-normal sigma of the access delay across households.
    pub access_delay_sigma: f64,
    /// Validation and relay time per gossip hop, in milliseconds.
    pub hop_processing_ms: f64,
    /// Message loss probability per transmission.
    pub loss: f64,
    /// First retransmission timeout after a loss, in seconds (doubling).
    pub rto_initial_s: f64,
    /// Retransmissions before a message is given up and resent after the next timeout cap.
    pub rto_max_retries: u32,
    /// Nodes in the gossip overlay used to calibrate block propagation.
    pub overlay_nodes: u32,
    /// Overlay sources per region in the calibration.
    pub overlay_sources_per_region: u32,
    /// Stylised fallback: route length ÷ great-circle distance.
    pub stylised_route_factor: f64,
    /// Stylised fallback: fixed delay per round trip, in milliseconds.
    pub stylised_per_hop_ms: f64,
    /// Clock skew: standard deviation of the main component, in seconds.
    pub skew_sd_s: f64,
    /// Clock skew: share of nodes in the badly synchronised tail.
    pub skew_tail_fraction: f64,
    /// Clock skew: standard deviation of the tail, in seconds.
    pub skew_tail_sd_s: f64,
    /// A witness online but cut off by a partition counts as unreachable for its KWCs, so
    /// One Chance applies to it (architect, 2026-10-14; ASSUMPTIONS SIM-7). `false` restores
    /// the M3a model, in which entropy across a partition waits until it heals.
    pub partition_cuts_witnesses: bool,
}

impl Default for SimNetwork {
    fn default() -> Self {
        let names = [
            "na_east", "na_west", "sa_east", "eu_west", "af", "as_south", "as_east", "oc",
        ];
        SimNetwork {
            latency_source: LatencySource::RipeAtlas,
            regions: names.iter().map(|n| (*n).to_string()).collect(),
            region_shares: vec![0.065, 0.028, 0.062, 0.117, 0.117, 0.233, 0.372, 0.006],
            utc_offsets_h: vec![-5.0, -8.0, -3.0, 1.0, 2.0, 5.5, 8.0, 10.0],
            region_centroids: vec![
                [39.0, -80.0],
                [37.0, -120.0],
                [-20.0, -50.0],
                [49.0, 8.0],
                [0.0, 25.0],
                [23.0, 78.0],
                [25.0, 115.0],
                [-30.0, 145.0],
            ],
            unreliable_grid_regions: vec!["as_south".to_string(), "af".to_string()],
            access_delay_median_ms: 10.0,
            access_delay_sigma: 0.5,
            hop_processing_ms: 5.0,
            loss: 0.0,
            rto_initial_s: 0.2,
            rto_max_retries: 6,
            overlay_nodes: 2_000,
            overlay_sources_per_region: 20,
            stylised_route_factor: 1.5,
            stylised_per_hop_ms: 5.0,
            skew_sd_s: 0.05,
            skew_tail_fraction: 0.01,
            skew_tail_sd_s: 2.0,
            partition_cuts_witnesses: true,
        }
    }
}

/// One device profile: when a household's node is off (SIM entries in ASSUMPTIONS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceProfile {
    /// Probability that the device is switched off on a given night (announced).
    pub night_off_prob: f64,
    /// Local hour at which it is switched off.
    pub off_start_h: f64,
    /// Local hour at which it is switched on again.
    pub off_end_h: f64,
    /// Standard deviation of both times, in hours.
    pub jitter_h: f64,
    /// Other announced shutdowns (updates, user), per day.
    pub user_off_per_day: f64,
    /// Median length of those shutdowns, in hours.
    pub user_off_median_h: f64,
    /// Log-normal sigma of their length.
    pub user_off_sigma: f64,
    /// Crashes (unannounced), per day.
    pub crashes_per_day: f64,
    /// Median crash downtime, in minutes.
    pub crash_median_min: f64,
    /// Log-normal sigma of crash downtime.
    pub crash_sigma: f64,
    /// Network glitches (unannounced), per day.
    pub glitches_per_day: f64,
    /// Median glitch length, in seconds.
    pub glitch_median_s: f64,
    /// Log-normal sigma of glitch length.
    pub glitch_sigma: f64,
}

impl DeviceProfile {
    /// An always-on device (mini-PC, Raspberry Pi).
    pub fn always_on() -> DeviceProfile {
        DeviceProfile {
            night_off_prob: 0.0,
            off_start_h: 23.0,
            off_end_h: 7.0,
            jitter_h: 1.0,
            user_off_per_day: 1.0 / 30.0,
            user_off_median_h: 0.1,
            user_off_sigma: 0.5,
            crashes_per_day: 1.0 / 60.0,
            crash_median_min: 10.0,
            crash_sigma: 1.0,
            glitches_per_day: 0.13,
            glitch_median_s: 120.0,
            glitch_sigma: 1.0,
        }
    }

    /// A desktop left on.
    pub fn desktop_on() -> DeviceProfile {
        DeviceProfile {
            user_off_per_day: 1.0 / 7.0,
            user_off_median_h: 2.0,
            user_off_sigma: 1.0,
            crashes_per_day: 1.0 / 30.0,
            ..DeviceProfile::always_on()
        }
    }

    /// A laptop or PC switched off at night.
    pub fn laptop_night() -> DeviceProfile {
        DeviceProfile {
            night_off_prob: 0.9,
            user_off_per_day: 0.5,
            user_off_median_h: 1.0,
            user_off_sigma: 1.0,
            crashes_per_day: 1.0 / 30.0,
            glitches_per_day: 0.2,
            ..DeviceProfile::always_on()
        }
    }
}

/// A regional power-grid profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridProfile {
    /// Unplanned interruptions per customer per year (SAIFI).
    pub faults_per_year: f64,
    /// Median interruption length, in minutes.
    pub fault_median_min: f64,
    /// Log-normal sigma of interruption length.
    pub fault_sigma: f64,
    /// Feeder groups per region; scheduled load-shedding and faults are shared within one.
    pub feeder_groups: u32,
    /// Hours of scheduled load-shedding per day a feeder group may have.
    pub loadshed_hours: Vec<f64>,
    /// Share of feeder groups with each entry of `loadshed_hours`.
    pub loadshed_weights: Vec<f64>,
    /// Longest scheduled block, in hours (a day's load-shedding is split into blocks).
    pub loadshed_block_h: f64,
    /// Share of households with a UPS or inverter.
    pub ups_share: f64,
    /// UPS runtime, in hours: shorter cuts do not take the node offline.
    pub ups_runtime_h: f64,
}

impl GridProfile {
    /// Reliable grid: US EIA 2023 national values including major events (SAIFI 1.3,
    /// SAIDI 366 minutes, so a mean interruption of about 282 minutes; log-normal median 171
    /// minutes with sigma 1).
    pub fn reliable() -> GridProfile {
        GridProfile {
            faults_per_year: 1.3,
            fault_median_min: 171.0,
            fault_sigma: 1.0,
            feeder_groups: 16,
            loadshed_hours: vec![0.0],
            loadshed_weights: vec![1.0],
            loadshed_block_h: 2.0,
            ups_share: 0.1,
            ups_runtime_h: 1.0,
        }
    }

    /// Unreliable grid with scheduled power cuts: NEPRA's orders on loss-based load-shedding
    /// describe feeders with low losses as exempt and higher-loss feeders at 2–12 hours a day;
    /// the mix of feeder groups, the fault rate and the UPS values here are illustrative.
    pub fn unreliable() -> GridProfile {
        GridProfile {
            faults_per_year: 24.0,
            fault_median_min: 60.0,
            fault_sigma: 1.0,
            feeder_groups: 16,
            loadshed_hours: vec![0.0, 2.0, 6.0, 12.0],
            loadshed_weights: vec![0.3, 0.3, 0.25, 0.15],
            loadshed_block_h: 2.0,
            ups_share: 0.5,
            ups_runtime_h: 3.0,
        }
    }
}

/// Household downtime model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimHouseholds {
    /// Share of households with each device profile: always on, desktop left on, laptop off
    /// at night.
    pub device_mix: Vec<f64>,
    /// Always-on device profile.
    pub always_on: DeviceProfile,
    /// Desktop-left-on profile.
    pub desktop_on: DeviceProfile,
    /// Laptop-off-at-night profile.
    pub laptop_night: DeviceProfile,
    /// Reliable-grid profile.
    pub reliable_grid: GridProfile,
    /// Unreliable-grid profile.
    pub unreliable_grid: GridProfile,
    /// Internet providers per region (outages are shared by all their customers).
    pub isps_per_region: u32,
    /// Outages per provider per year.
    pub isp_outages_per_year: f64,
    /// Median provider outage, in minutes.
    pub isp_outage_median_min: f64,
    /// Log-normal sigma of provider outages.
    pub isp_outage_sigma: f64,
    /// Region-wide blackouts per year.
    pub blackouts_per_year: f64,
    /// Median blackout, in hours.
    pub blackout_median_h: f64,
    /// Log-normal sigma of blackouts.
    pub blackout_sigma: f64,
    /// Long absences (long enough to deactivate an ID) per household per year.
    pub long_absences_per_year: f64,
    /// Lomax scale of long absences, in days (the M1 fallback, ASSUMPTIONS B-11).
    pub long_absence_scale_days: f64,
    /// Lomax shape of long absences.
    pub long_absence_shape: f64,
    /// Share of long absences announced with an offline request.
    pub long_absence_announced: f64,
    /// Permanent departures per household per year.
    pub departures_per_year: f64,
}

impl Default for SimHouseholds {
    fn default() -> Self {
        SimHouseholds {
            device_mix: vec![0.3, 0.3, 0.4],
            always_on: DeviceProfile::always_on(),
            desktop_on: DeviceProfile::desktop_on(),
            laptop_night: DeviceProfile::laptop_night(),
            reliable_grid: GridProfile::reliable(),
            unreliable_grid: GridProfile::unreliable(),
            isps_per_region: 4,
            isp_outages_per_year: 2.0,
            isp_outage_median_min: 60.0,
            isp_outage_sigma: 1.0,
            blackouts_per_year: 0.2,
            blackout_median_h: 4.0,
            blackout_sigma: 0.7,
            long_absences_per_year: 4.0,
            long_absence_scale_days: 2.0,
            long_absence_shape: 1.5,
            long_absence_announced: 0.8,
            departures_per_year: 0.0,
        }
    }
}

/// Honest growth and patience (A6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimGrowth {
    /// New honest miners per year, as a multiple of the issuance rate R (the architect's
    /// papers use about 3).
    pub arrivals_multiple_of_r: f64,
    /// Median patience before a newcomer without an ID gives up, in days.
    pub patience_median_days: Vec<f64>,
    /// Log-normal sigma of patience.
    pub patience_sigma: f64,
}

impl Default for SimGrowth {
    fn default() -> Self {
        SimGrowth {
            arrivals_multiple_of_r: 3.0,
            patience_median_days: vec![3.0, 14.0],
            patience_sigma: 1.0,
        }
    }
}

/// Experiment sizes: every job's simulated days and replicates are multiplied by these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimSuite {
    /// Multiplier on simulated days.
    pub days_scale: f64,
    /// Multiplier on replicates and seeds.
    pub replicate_scale: f64,
}

impl Default for SimSuite {
    fn default() -> Self {
        SimSuite {
            days_scale: 1.0,
            replicate_scale: 1.0,
        }
    }
}

impl SimConfig {
    /// Problems with the simulator settings, for [`crate::Config::validate`].
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let n = &self.network;
        let regions = n.regions.len();
        let mut need = |ok: bool, message: &str| {
            if !ok {
                problems.push(message.to_string());
            }
        };
        need(regions >= 6, "sim.network.regions needs at least 6 regions");
        need(
            n.region_shares.len() == regions
                && n.utc_offsets_h.len() == regions
                && n.region_centroids.len() == regions,
            "sim.network per-region lists must match sim.network.regions",
        );
        let share_sum: f64 = n.region_shares.iter().sum();
        need(
            (share_sum - 1.0).abs() < 1e-9 && n.region_shares.iter().all(|s| *s >= 0.0),
            "sim.network.region_shares must be non-negative and sum to 1",
        );
        need(
            n.unreliable_grid_regions
                .iter()
                .all(|r| n.regions.contains(r)),
            "sim.network.unreliable_grid_regions must name configured regions",
        );
        need(
            (0.0..1.0).contains(&n.loss),
            "sim.network.loss must be in [0, 1)",
        );
        need(
            n.overlay_nodes >= 100 && n.overlay_sources_per_region >= 1,
            "sim.network overlay needs at least 100 nodes and one source per region",
        );
        let h = &self.households;
        let mix_sum: f64 = h.device_mix.iter().sum();
        need(
            h.device_mix.len() == 3 && (mix_sum - 1.0).abs() < 1e-9,
            "sim.households.device_mix needs three shares summing to 1",
        );
        for grid in [&h.reliable_grid, &h.unreliable_grid] {
            let weights: f64 = grid.loadshed_weights.iter().sum();
            need(
                grid.loadshed_hours.len() == grid.loadshed_weights.len()
                    && (weights - 1.0).abs() < 1e-9
                    && grid.loadshed_hours.iter().all(|x| (0.0..24.0).contains(x))
                    && grid.feeder_groups > 0
                    && grid.loadshed_block_h > 0.0,
                "sim.households grid load-shedding lists must match and weights sum to 1",
            );
        }
        need(
            h.long_absence_shape > 0.0 && h.long_absence_scale_days > 0.0,
            "sim.households long-absence Lomax parameters must be positive",
        );
        need(
            self.world.registered_ids >= 130,
            "sim.world.registered_ids must fill at least 13 WCs",
        );
        need(
            self.world.panel_nodes >= 1 && self.world.panel_nodes % 2 == 1,
            "sim.world.panel_nodes must be odd and positive",
        );
        need(
            self.world.fork_storm_unsettled >= 100,
            "sim.world.fork_storm_unsettled must be at least 100",
        );
        need(
            self.suite.days_scale > 0.0 && self.suite.replicate_scale > 0.0,
            "sim.suite scales must be positive",
        );
        need(self.threads >= 1, "sim.threads must be at least 1");
        need(
            self.acceptance_cache_entries >= 1,
            "sim.acceptance_cache_entries must be at least 1",
        );
        problems
    }
}
