# Inter-region latency (RIPE Atlas)

`region_rtt.csv` is the latency input of the M3 simulator (`gb sim`). It was built from public **RIPE Atlas** measurements: RIPE NCC, RIPE Atlas, <https://atlas.ripe.net>. We thank RIPE NCC and the RIPE Atlas anchor hosts for the data.

## How it was made

```bash
python3 -I tools/ripe_atlas_latency.py --day 2026-10-12 --out data/latency
```

The script uses only the Python standard library.

- **Anchors.** It keeps the RIPE Atlas anchors that are live, have IPv6, and sit in one of the simulator's 8 regions. Regions are by country; the US, Canada and Mexico are split at longitude −100 into east and west.
- **Selection.** Each region contributes up to 8 anchors, chosen round-robin across its countries (countries with more anchors first, anchors in id order), so each region mixes its countries.
- **Targets.** The first 3 picks whose mesh ping got replies from the chosen sources are the targets. A pick that got none, for example one that did not answer IPv6 pings that day, is skipped; the skips are listed when the script runs.
- **Measurements.** For each target it reads the "Anchoring Mesh Measurement: Ping IPv6", which pings every 240 s with 3 packets, from the other chosen anchors.
- **Time windows.** It samples four 30-minute windows of one UTC day: 00:00, 06:00, 12:00 and 18:00.
- **Pooling.** It pools replies by region pair and treats both directions of a pair as one sample.

`meta.json` records:
- the retrieval time and the day sampled;
- every anchor used (id, probe, host name, country, city);
- every measurement ID.

## Columns

| Column | Meaning |
|---|---|
| `region_a`, `region_b` | The region pair; the diagonal is within one region. |
| `results` | Ping results, 3 packets each. |
| `replies` | RTT samples. |
| `rtt_p10_ms` … `rtt_p99_ms` | Round-trip time percentiles in milliseconds, over the replies. |
| `unreachable` | Share of results with no reply at all. These are mostly anchor pairs whose IPv6 path did not answer during the window. This is not random loss, and it is not used by the simulator. |
| `packet_loss` | Share of packets lost in results that got at least one reply. Reported for reference; the simulator sweeps its own loss rate (0, 0.5, 2 and 5%). |

## How the simulator uses it

One-way delay between two regions is half of a round trip, drawn from these percentiles by piecewise-linear interpolation, plus a per-node access delay. See `docs/ASSUMPTIONS.md`, entries SIM-n. Anchors sit in data centres, so the matrix describes backbone latency; the access delay of a household connection is added separately.

**Coverage.** The `as_south` region, which carries the unreliable-grid household profile, includes RIPE Atlas's two live IPv6 anchors in Pakistan (Lahore and Faisalabad) as sources and Lahore as a target, next to anchors in India, Nepal and Sri Lanka.
