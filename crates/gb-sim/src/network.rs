//! The network model (ARCHITECTURE §3): regional latency, loss, partitions and gossip.
//!
//! - **Latency.** One-way delay between two regions is half a round trip, drawn by
//!   piecewise-linear interpolation through the RIPE Atlas percentiles of
//!   `data/latency/region_rtt.csv` (or the stylised fallback), plus each end's household
//!   access delay and a processing delay.
//! - **Loss.** Each transmission is lost with probability `loss`; a loss costs a
//!   retransmission timeout that doubles, up to a retry cap.
//! - **Partitions.** A region cut off from the others delivers nothing across the cut until
//!   the partition heals; a message in flight is delivered one fresh delay after healing.
//! - **Gossip.** Blocks travel over a peer overlay. Arrival times are drawn from region-to-
//!   region quantile tables calibrated once per configuration by shortest paths over a random
//!   overlay (degree = the SPEC peer bootstrap count), with loss on every edge.

use crate::engine::{Micros, micros};
use crate::rng::{Keyed, Purpose, unit, unit_positive};
use crate::samplers::{lognormal, normal, quantile};
use gb_config::sim::{LatencySource, SimNetwork};
use std::collections::BinaryHeap;

/// The RIPE Atlas latency matrix shipped with the repository.
pub const RIPE_MATRIX: &str = include_str!("../../../data/latency/region_rtt.csv");

/// Errors while building the network model.
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    /// The latency file is malformed or misses a region pair.
    #[error("latency matrix: {0}")]
    Matrix(String),
}

/// One-way backbone latency between regions.
#[derive(Debug, Clone)]
pub struct Latency {
    regions: usize,
    knots: Vec<Vec<(f64, f64)>>,
    /// Where the matrix came from.
    pub source: LatencySource,
}

impl Latency {
    /// The matrix from RIPE Atlas percentiles (milliseconds of round-trip time).
    pub fn from_ripe(csv: &str, names: &[String]) -> Result<Latency, NetworkError> {
        let r = names.len();
        let mut knots = vec![Vec::new(); r * r];
        let index = |name: &str| names.iter().position(|n| n == name);
        for (line_no, line) in csv.lines().enumerate().skip(1) {
            let cells: Vec<&str> = line.split(',').collect();
            if cells.len() < 8 {
                return Err(NetworkError::Matrix(format!(
                    "line {} is short",
                    line_no + 1
                )));
            }
            let (Some(a), Some(b)) = (index(cells[0]), index(cells[1])) else {
                continue;
            };
            let ms = |i: usize| -> Result<f64, NetworkError> {
                cells[i]
                    .trim()
                    .parse::<f64>()
                    .map_err(|e| NetworkError::Matrix(format!("line {}: {e}", line_no + 1)))
            };
            let (p10, p50, p90, p99) = (ms(4)?, ms(5)?, ms(6)?, ms(7)?);
            if !(p10 <= p50 && p50 <= p90 && p90 <= p99) || p10.is_nan() {
                return Err(NetworkError::Matrix(format!(
                    "line {}: percentiles out of order",
                    line_no + 1
                )));
            }
            // One-way seconds: half the round trip.
            let k = |v: f64| v / 2.0 / 1_000.0;
            let row = vec![
                (0.0, k(0.8 * p10)),
                (0.1, k(p10)),
                (0.5, k(p50)),
                (0.9, k(p90)),
                (0.99, k(p99)),
                (1.0, k(1.1 * p99)),
            ];
            knots[a * r + b] = row.clone();
            knots[b * r + a] = row;
        }
        if let Some(missing) = knots.iter().position(Vec::is_empty) {
            return Err(NetworkError::Matrix(format!(
                "no row for {} – {}",
                names[missing / r],
                names[missing % r]
            )));
        }
        Ok(Latency {
            regions: r,
            knots,
            source: LatencySource::RipeAtlas,
        })
    }

    /// The stylised fallback: great-circle distance between region centroids × route factor at
    /// 200 km per millisecond in fibre, plus a fixed delay; 500 km within a region. The spread
    /// around the median (×0.9 at p10, ×1.3 at p90, ×1.8 at p99) is illustrative.
    pub fn stylised(net: &SimNetwork) -> Latency {
        let r = net.regions.len();
        let mut knots = vec![Vec::new(); r * r];
        for a in 0..r {
            for b in 0..r {
                let km = if a == b {
                    500.0
                } else {
                    great_circle_km(net.region_centroids[a], net.region_centroids[b])
                };
                let median =
                    (net.stylised_route_factor * km / 200.0 + net.stylised_per_hop_ms) / 1_000.0;
                knots[a * r + b] = vec![
                    (0.0, 0.8 * median),
                    (0.1, 0.9 * median),
                    (0.5, median),
                    (0.9, 1.3 * median),
                    (0.99, 1.8 * median),
                    (1.0, 2.0 * median),
                ];
            }
        }
        Latency {
            regions: r,
            knots,
            source: LatencySource::Stylised,
        }
    }

    /// One-way backbone delay from region `a` to `b` in seconds, at quantile `u`.
    pub fn one_way(&self, a: usize, b: usize, u: f64) -> f64 {
        quantile(&self.knots[a * self.regions + b], u)
    }

    /// The largest one-way backbone delay.
    pub fn max_one_way(&self) -> f64 {
        self.knots
            .iter()
            .map(|k| k.last().map_or(0.0, |x| x.1))
            .fold(0.0, f64::max)
    }

    /// Median one-way delay from `a` to `b`, in seconds.
    pub fn median(&self, a: usize, b: usize) -> f64 {
        self.one_way(a, b, 0.5)
    }

    /// Number of regions.
    pub fn regions(&self) -> usize {
        self.regions
    }
}

fn great_circle_km(a: [f64; 2], b: [f64; 2]) -> f64 {
    let rad = core::f64::consts::PI / 180.0;
    let (la1, lo1, la2, lo2) = (a[0] * rad, a[1] * rad, b[0] * rad, b[1] * rad);
    let h = libm::pow(libm::sin((la2 - la1) / 2.0), 2.0)
        + libm::cos(la1) * libm::cos(la2) * libm::pow(libm::sin((lo2 - lo1) / 2.0), 2.0);
    2.0 * 6_371.0 * libm::asin(libm::sqrt(h.min(1.0)))
}

/// A region cut off from every other region during `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Partition {
    /// The isolated region.
    pub region: usize,
    /// Start of the partition.
    pub start: Micros,
    /// End of the partition.
    pub end: Micros,
}

/// Loss and retransmission settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LossModel {
    /// Loss probability per transmission.
    pub loss: f64,
    /// First retransmission timeout, seconds.
    pub rto_initial: f64,
    /// Retransmissions before giving up the timeout doubling.
    pub max_retries: u32,
}

impl LossModel {
    /// Extra delay from losses, in seconds, from a uniform in `(0, 1]`: `k` losses cost
    /// `rto (1 + 2 + … + 2^(k−1))`, with `k` geometric and capped at `max_retries`.
    pub fn extra(&self, u_positive: f64) -> f64 {
        if self.loss <= 0.0 {
            return 0.0;
        }
        // P(k losses) = loss^k (1 − loss): k = ⌊ln u / ln loss⌋.
        let k = (libm::log(u_positive) / libm::log(self.loss)).floor();
        let k = if k.is_finite() { k as u32 } else { 0 }.min(self.max_retries);
        self.rto_initial * (libm::pow(2.0, f64::from(k)) - 1.0)
    }

    /// The largest extra delay.
    pub fn max_extra(&self) -> f64 {
        if self.loss <= 0.0 {
            0.0
        } else {
            self.rto_initial * (libm::pow(2.0, f64::from(self.max_retries)) - 1.0)
        }
    }
}

/// Gossip arrival quantiles between regions.
#[derive(Debug, Clone)]
pub struct GossipTable {
    regions: usize,
    /// For each (source region, destination region), 101 quantiles (0 %, 1 %, …, 100 %) of
    /// the delay in seconds from a source node until the block reaches the destination's
    /// network edge (the destination's own access delay is added per node).
    quantiles: Vec<Vec<f64>>,
}

impl GossipTable {
    /// A table of zero delays (idealised runs).
    pub fn zero(regions: usize) -> GossipTable {
        GossipTable {
            regions,
            quantiles: vec![vec![0.0; 101]; regions * regions],
        }
    }

    /// Delay at quantile `u` from region `a` to region `b`, in seconds.
    pub fn delay(&self, a: usize, b: usize, u: f64) -> f64 {
        let q = &self.quantiles[a * self.regions + b];
        let x = u.clamp(0.0, 1.0) * 100.0;
        let i = (x.floor() as usize).min(99);
        let f = x - i as f64;
        q[i] + (q[i + 1] - q[i]) * f
    }

    /// The largest delay in the table.
    pub fn max(&self) -> f64 {
        self.quantiles
            .iter()
            .map(|q| q.last().copied().unwrap_or(0.0))
            .fold(0.0, f64::max)
    }

    /// Median delay from `a` to `b`.
    pub fn median(&self, a: usize, b: usize) -> f64 {
        self.delay(a, b, 0.5)
    }
}

/// Settings the overlay calibration needs.
#[derive(Debug, Clone, Copy)]
pub struct OverlaySettings {
    /// Overlay nodes.
    pub nodes: usize,
    /// Peers each node connects to.
    pub degree: usize,
    /// Sources per region.
    pub sources_per_region: usize,
    /// Validation and relay time per hop, seconds.
    pub hop_processing: f64,
    /// Household access delay: median (seconds) and log-normal sigma.
    pub access_median: f64,
    /// Log-normal sigma of the access delay.
    pub access_sigma: f64,
}

/// Calibrates gossip delays by shortest paths over a random overlay. Overlay nodes are spread
/// over regions by `shares` (largest remainders); each connects to `degree` random peers; an
/// edge costs both ends' access delays, a backbone delay, the hop processing time and the loss
/// penalty, all drawn afresh for every source.
pub fn calibrate_gossip(
    keyed: &mut Keyed,
    latency: &Latency,
    loss: &LossModel,
    shares: &[f64],
    s: &OverlaySettings,
) -> GossipTable {
    let r = latency.regions();
    let regions = spread_regions(s.nodes, shares, keyed);
    let access: Vec<f64> = (0..s.nodes)
        .map(|i| {
            let w = keyed.uniforms(Purpose::Overlay, i as u64, 0);
            lognormal(s.access_median, s.access_sigma, normal(1.0 - w[0], w[1]))
        })
        .collect();
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); s.nodes];
    for i in 0..s.nodes {
        let w = keyed.words(Purpose::Overlay, i as u64, 1);
        let mut added = 0;
        let mut k = 0;
        while added < s.degree && k < 64 {
            let word = if k < 8 {
                w[k]
            } else {
                keyed.words(Purpose::Overlay, i as u64, 2 + (k / 8) as u64)[k % 8]
            };
            k += 1;
            let j = (unit(word) * s.nodes as f64) as usize % s.nodes;
            if j != i && !adjacency[i].contains(&j) {
                adjacency[i].push(j);
                adjacency[j].push(i);
                added += 1;
            }
        }
    }
    let mut samples: Vec<Vec<f64>> = vec![Vec::new(); r * r];
    for region in 0..r {
        let members: Vec<usize> = (0..s.nodes).filter(|i| regions[*i] == region).collect();
        if members.is_empty() {
            continue;
        }
        for n in 0..s.sources_per_region {
            let source = members[n % members.len()];
            let tag = (region * 1_000 + n) as u64;
            let dist = dijkstra(&adjacency, source, |a, b| {
                let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                let edge = (lo * s.nodes + hi) as u64;
                let w = keyed.words(Purpose::Overlay, 1_000_000 + tag, edge);
                access[a]
                    + latency.one_way(regions[a], regions[b], unit(w[0]))
                    + access[b]
                    + s.hop_processing
                    + loss.extra(unit_positive(w[1]))
            });
            for (x, d) in dist.iter().enumerate() {
                if x != source && d.is_finite() {
                    samples[region * r + regions[x]].push(d - access[x]);
                }
            }
        }
    }
    let quantiles = samples
        .into_iter()
        .enumerate()
        .map(|(i, mut v)| {
            if v.is_empty() {
                // No overlay node in one of the regions: fall back to the direct median.
                let (a, b) = (i / r, i % r);
                let d = latency.median(a, b) + s.hop_processing;
                return vec![d; 101];
            }
            v.sort_by(f64::total_cmp);
            (0..=100)
                .map(|q| v[((q as f64 / 100.0) * (v.len() - 1) as f64).round() as usize])
                .collect()
        })
        .collect();
    GossipTable {
        regions: r,
        quantiles,
    }
}

/// Assigns `n` items to regions in proportion to `shares` (largest remainders), shuffled.
pub fn spread_regions(n: usize, shares: &[f64], keyed: &mut Keyed) -> Vec<usize> {
    spread_keyed(n, shares, keyed, 7_000_000 + n as u64)
}

/// Like [`spread_regions`], with its own shuffle for each `salt`, so that two spreads of the
/// same number of items are independent of each other (SIM-51).
pub fn spread_salted(n: usize, shares: &[f64], keyed: &mut Keyed, salt: u64) -> Vec<usize> {
    spread_keyed(n, shares, keyed, (1 << 40) | (salt << 24) | n as u64)
}

fn spread_keyed(n: usize, shares: &[f64], keyed: &mut Keyed, key: u64) -> Vec<usize> {
    let mut counts: Vec<usize> = shares
        .iter()
        .map(|s| (s * n as f64).floor() as usize)
        .collect();
    let mut remainders: Vec<(f64, usize)> = shares
        .iter()
        .enumerate()
        .map(|(i, s)| (s * n as f64 - counts[i] as f64, i))
        .collect();
    remainders.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let assigned: usize = counts.iter().sum();
    for (_, i) in remainders.iter().cycle().take(n.saturating_sub(assigned)) {
        counts[*i] += 1;
    }
    let mut out: Vec<usize> = counts
        .iter()
        .enumerate()
        .flat_map(|(region, c)| std::iter::repeat_n(region, *c))
        .collect();
    // Fisher–Yates with keyed draws.
    for i in (1..out.len()).rev() {
        let w = keyed.words(Purpose::Setup, key, i as u64)[0];
        let j = (unit(w) * (i + 1) as f64) as usize % (i + 1);
        out.swap(i, j);
    }
    out
}

#[derive(PartialEq)]
struct Item(f64, usize);

impl Eq for Item {}

impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Item {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}

fn dijkstra(
    adjacency: &[Vec<usize>],
    source: usize,
    mut weight: impl FnMut(usize, usize) -> f64,
) -> Vec<f64> {
    let mut dist = vec![f64::INFINITY; adjacency.len()];
    dist[source] = 0.0;
    let mut heap = BinaryHeap::new();
    heap.push(Item(0.0, source));
    while let Some(Item(d, a)) = heap.pop() {
        if d > dist[a] {
            continue;
        }
        for &b in &adjacency[a] {
            let nd = d + weight(a, b);
            if nd < dist[b] {
                dist[b] = nd;
                heap.push(Item(nd, b));
            }
        }
    }
    dist
}

/// The network as one run sees it.
#[derive(Debug, Clone)]
pub struct Network {
    /// Backbone latency.
    pub latency: Latency,
    /// Loss model for direct messages.
    pub loss: LossModel,
    /// Calibrated gossip delays.
    pub gossip: GossipTable,
    /// Scheduled partitions.
    pub partitions: Vec<Partition>,
    /// Processing time per message hop, seconds.
    pub processing: f64,
    /// Zero latency, no loss (idealised mode).
    pub idealised: bool,
}

impl Network {
    /// Delivery time of a message sent at `sent` from region `a` to region `b` that takes
    /// `delay` seconds, given the partitions.
    pub fn deliver(&self, sent: Micros, a: usize, b: usize, delay: f64) -> Micros {
        let arrive = sent + micros(delay);
        if a == b || self.idealised {
            return arrive;
        }
        let mut t = arrive;
        for p in &self.partitions {
            let cut = p.region == a || p.region == b;
            if cut && sent < p.end && t > p.start {
                t = t.max(p.end + micros(delay));
            }
        }
        t
    }

    /// One-way delay of a direct message in seconds (zero when idealised): both ends' access
    /// delays, the backbone delay at quantile `u`, processing, and the loss penalty.
    pub fn direct_delay(
        &self,
        a: usize,
        access_a: f64,
        b: usize,
        access_b: f64,
        u: f64,
        u_loss: f64,
    ) -> f64 {
        if self.idealised {
            return 0.0;
        }
        access_a
            + self.latency.one_way(a, b, u)
            + access_b
            + self.processing
            + self.loss.extra(u_loss)
    }

    /// Gossip delay of a block from a node in region `a` to a node in region `b` with access
    /// delay `access_b`, in seconds (zero when idealised).
    pub fn gossip_delay(&self, a: usize, b: usize, access_b: f64, u: f64) -> f64 {
        if self.idealised {
            return 0.0;
        }
        self.gossip.delay(a, b, u) + access_b
    }

    /// A hard bound on any gossip or direct delivery, ignoring partitions, for nodes whose
    /// access delay is at most `max_access`.
    pub fn max_delay(&self, max_access: f64) -> f64 {
        if self.idealised {
            return 0.0;
        }
        let direct =
            2.0 * max_access + self.latency.max_one_way() + self.processing + self.loss.max_extra();
        direct.max(self.gossip.max() + max_access)
    }

    /// The end of the last partition that affects messages from `a` or to `b` and is still
    /// open at `t`, if any.
    pub fn partition_open(&self, t: Micros) -> Option<Micros> {
        self.partitions
            .iter()
            .filter(|p| p.start <= t && t < p.end)
            .map(|p| p.end)
            .max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        SimNetwork::default().regions
    }

    #[test]
    fn the_ripe_matrix_covers_every_region_pair_symmetrically() {
        let l = Latency::from_ripe(RIPE_MATRIX, &names()).unwrap();
        assert_eq!(l.regions(), 8);
        for a in 0..8 {
            for b in 0..8 {
                assert_eq!(l.one_way(a, b, 0.5), l.one_way(b, a, 0.5));
                assert!(l.one_way(a, b, 0.0) <= l.one_way(a, b, 1.0));
            }
        }
        // Europe to Oceania is far slower than within Europe.
        assert!(l.median(3, 7) > 5.0 * l.median(3, 3));
    }

    #[test]
    fn a_missing_pair_is_an_error() {
        let short: String = RIPE_MATRIX.lines().take(5).collect::<Vec<_>>().join("\n");
        assert!(Latency::from_ripe(&short, &names()).is_err());
    }

    #[test]
    fn stylised_latency_grows_with_distance() {
        let l = Latency::stylised(&SimNetwork::default());
        assert!(l.median(3, 7) > l.median(3, 0));
        assert!(l.median(0, 0) < l.median(0, 1));
    }

    #[test]
    fn losses_cost_doubling_timeouts() {
        let m = LossModel {
            loss: 0.5,
            rto_initial: 0.2,
            max_retries: 6,
        };
        assert_eq!(m.extra(0.9), 0.0);
        assert!((m.extra(0.4) - 0.2).abs() < 1e-12);
        assert!((m.extra(0.2) - 0.6).abs() < 1e-12);
        assert!((m.max_extra() - 12.6).abs() < 1e-12);
        let none = LossModel { loss: 0.0, ..m };
        assert_eq!(none.extra(1e-9), 0.0);
    }

    #[test]
    fn partitions_hold_messages_until_they_heal() {
        let net = Network {
            latency: Latency::stylised(&SimNetwork::default()),
            loss: LossModel {
                loss: 0.0,
                rto_initial: 0.2,
                max_retries: 6,
            },
            gossip: GossipTable {
                regions: 8,
                quantiles: vec![vec![0.1; 101]; 64],
            },
            partitions: vec![Partition {
                region: 2,
                start: 10_000_000,
                end: 20_000_000,
            }],
            processing: 0.0,
            idealised: false,
        };
        assert_eq!(net.deliver(11_000_000, 2, 3, 0.5), 20_500_000);
        assert_eq!(net.deliver(11_000_000, 4, 3, 0.5), 11_500_000);
        assert_eq!(net.deliver(1_000_000, 2, 3, 0.5), 1_500_000);
        assert_eq!(net.partition_open(15_000_000), Some(20_000_000));
    }

    #[test]
    fn calibrated_gossip_is_slower_between_far_regions() {
        let net = SimNetwork::default();
        let latency = Latency::from_ripe(RIPE_MATRIX, &net.regions).unwrap();
        let mut keyed = Keyed::new(1, "overlay-test");
        let settings = OverlaySettings {
            nodes: 400,
            degree: 8,
            sources_per_region: 3,
            hop_processing: 0.005,
            access_median: 0.01,
            access_sigma: 0.5,
        };
        let loss = LossModel {
            loss: 0.0,
            rto_initial: 0.2,
            max_retries: 6,
        };
        let table = calibrate_gossip(&mut keyed, &latency, &loss, &net.region_shares, &settings);
        assert!(table.median(3, 7) > table.median(3, 3));
        assert!(table.max() >= table.median(3, 7));
        let regions = spread_regions(1_000, &net.region_shares, &mut keyed);
        let as_east = regions.iter().filter(|r| **r == 6).count();
        assert_eq!(as_east, 372);
    }
}
