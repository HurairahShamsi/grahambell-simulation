#!/usr/bin/env python3
"""Build the M3 inter-region latency matrix from public RIPE Atlas anchor-mesh pings.

Data: RIPE Atlas (RIPE NCC), https://atlas.ripe.net. Anchors ping each other in the
"Anchoring Mesh Measurement: Ping IPv6" measurements every 240 s (3 packets each).

What this script does (standard library only; run with `python3 -I`):
  1. Lists RIPE Atlas anchors and keeps those that are live, have IPv6, and sit in one of the
     simulator's 8 regions (by country; the US, Canada and Mexico are split at longitude
     -100 into east and west).
  2. In each region, picks up to SOURCES anchors round-robin across its countries (countries
     with more anchors first, anchors in id order), so each region mixes its countries. The
     targets are the first TARGETS picks whose mesh ping got replies from the sources (a pick
     that got none, for example an anchor that does not answer IPv6 pings, is skipped).
  3. For each target, finds its IPv6 mesh ping and downloads four 30-minute windows of one
     UTC day (00:00, 06:00, 12:00, 18:00), only for the source anchors' probes.
  4. Pools the RTT of every reply by (source region, target region), treats the two
     directions of a pair as one sample, and writes percentiles and the loss rate.

Outputs (written to data/latency/ unless --out is given):
  region_rtt.csv  region_a, region_b, results, replies, rtt_p10/p50/p90/p99 (ms),
                  unreachable (share of results with no reply: a pair that never answered,
                  usually a broken IPv6 path rather than random loss) and packet_loss (share
                  of packets lost in results that got at least one reply)
  meta.json       retrieval time, the day and windows, and every anchor and measurement used

Usage:
  python3 -I tools/ripe_atlas_latency.py --day 2026-10-12 [--out data/latency]
"""

import argparse
import csv
import datetime as dt
import json
import pathlib
import sys
import time
import urllib.parse
import urllib.request

API = "https://atlas.ripe.net/api/v2"
SOURCES = 8
TARGETS = 3
WINDOW_HOURS = (0, 6, 12, 18)
WINDOW_S = 1800
PAUSE_S = 0.5

REGIONS = ["na_east", "na_west", "sa_east", "eu_west", "af", "as_south", "as_east", "oc"]
COUNTRIES = {
    "sa_east": "BR AR CL CO PE UY PY EC VE BO".split(),
    "eu_west": (
        "AT BE BG CH CY CZ DE DK EE ES FI FR GB GR HR HU IE IS IT LI LT LU LV MT NL NO PL "
        "PT RO SE SI SK RS BA ME MK AL"
    ).split(),
    "af": "ZA KE NG EG MA TN GH TZ UG RW SN CI ZM ZW MU NA BW AO ET DZ CM MG MZ".split(),
    "as_south": "IN PK BD LK NP".split(),
    "as_east": "JP KR HK TW SG MY TH VN PH ID".split(),
    "oc": "AU NZ".split(),
}


def get_json(url, params=None):
    if params:
        url = url + ("&" if "?" in url else "?") + urllib.parse.urlencode(params)
    request = urllib.request.Request(url, headers={"User-Agent": "gb-sim-latency/1"})
    for attempt in range(5):
        try:
            with urllib.request.urlopen(request, timeout=180) as response:
                data = json.load(response)
            time.sleep(PAUSE_S)
            return data
        except Exception as error:  # noqa: BLE001 - retry any network error
            wait = 5 * (attempt + 1)
            print(f"  retry in {wait} s after {error}", file=sys.stderr)
            time.sleep(wait)
    raise RuntimeError(f"giving up on {url}")


def region_of(anchor):
    country = anchor.get("country") or ""
    if country in ("US", "CA", "MX"):
        lon = anchor["geometry"]["coordinates"][0]
        return "na_east" if lon > -100.0 else "na_west"
    for region, countries in COUNTRIES.items():
        if country in countries:
            return region
    return None


def live_ipv6_anchors():
    anchors = []
    url = f"{API}/anchors/?format=json&page_size=500"
    while url:
        page = get_json(url)
        anchors.extend(page["results"])
        url = page.get("next")
    keep = [
        a
        for a in anchors
        if not a.get("is_disabled")
        and a.get("date_decommissioned") is None
        and a.get("ip_v6")
        and region_of(a)
    ]
    return sorted(keep, key=lambda a: a["id"])


def round_robin(anchors, count):
    by_country = {}
    for a in anchors:
        by_country.setdefault(a["country"], []).append(a)
    order = sorted(by_country, key=lambda c: (-len(by_country[c]), c))
    picked = []
    while len(picked) < count and any(by_country[c] for c in order):
        for c in order:
            if by_country[c] and len(picked) < count:
                picked.append(by_country[c].pop(0))
    return picked


def mesh_ping_v6(fqdn):
    page = get_json(
        f"{API}/measurements/",
        {"format": "json", "target": fqdn, "type": "ping", "af": 6, "status": 2},
    )
    for m in page["results"]:
        if (m.get("description") or "").startswith("Anchoring Mesh Measurement: Ping IPv6"):
            return m["id"]
    return None


def quantile(sorted_values, q):
    if not sorted_values:
        return float("nan")
    index = min(len(sorted_values) - 1, max(0, int(round(q * (len(sorted_values) - 1)))))
    return sorted_values[index]


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--day", required=True, help="UTC day YYYY-MM-DD (a complete past day)")
    parser.add_argument("--out", default="data/latency")
    args = parser.parse_args()
    day = dt.datetime.strptime(args.day, "%Y-%m-%d").replace(tzinfo=dt.timezone.utc)
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    anchors = live_ipv6_anchors()
    chosen = {}
    for region in REGIONS:
        in_region = [a for a in anchors if region_of(a) == region]
        chosen[region] = round_robin(in_region, SOURCES)
        print(f"{region}: {len(in_region)} live IPv6 anchors, using {len(chosen[region])}")
    probe_region = {a["probe"]: r for r, members in chosen.items() for a in members}
    probe_ids = ",".join(str(p) for p in sorted(probe_region))

    windows = [
        (int((day + dt.timedelta(hours=h)).timestamp()),
         int((day + dt.timedelta(hours=h)).timestamp()) + WINDOW_S)
        for h in WINDOW_HOURS
    ]
    rtts = {}
    sent = {}
    lost = {}
    results = {}
    silent = {}
    targets_meta = []
    for region in REGIONS:
        used = 0
        for anchor in chosen[region]:
            if used == TARGETS:
                break
            msm = mesh_ping_v6(anchor["fqdn"])
            if msm is None:
                print(f"  no IPv6 mesh ping for {anchor['fqdn']}", file=sys.stderr)
                continue
            fetched = []
            for start, stop in windows:
                fetched.extend(get_json(
                    f"{API}/measurements/{msm}/results/",
                    {"format": "json", "start": start, "stop": stop, "probe_ids": probe_ids},
                ))
            answered = [
                r for r in fetched
                if r.get("prb_id") in probe_region and r.get("prb_id") != anchor["probe"]
                and any("rtt" in x for x in r.get("result", []))
            ]
            if not answered:
                print(f"  {region} skipped {anchor['fqdn']} msm {msm}: no replies from the sources")
                continue
            used += 1
            count = 0
            for r in fetched:
                source = probe_region.get(r.get("prb_id"))
                if source is None or r.get("prb_id") == anchor["probe"]:
                    continue
                key = tuple(sorted((source, region), key=REGIONS.index))
                results[key] = results.get(key, 0) + 1
                replies = [x["rtt"] for x in r.get("result", []) if "rtt" in x]
                if not replies:
                    silent[key] = silent.get(key, 0) + 1
                    continue
                sent[key] = sent.get(key, 0) + int(r.get("sent", 0))
                # Duplicate replies can outnumber packets sent; count each packet once.
                lost[key] = lost.get(key, 0) + max(0, int(r.get("sent", 0)) - len(replies))
                rtts.setdefault(key, []).extend(replies)
                count += 1
            targets_meta.append(
                {"region": region, "anchor": anchor["id"], "fqdn": anchor["fqdn"],
                 "measurement": msm, "results": count}
            )
            print(f"  {region} target {anchor['fqdn']} msm {msm}: {count} results")

    with open(out / "region_rtt.csv", "w", newline="") as handle:
        writer = csv.writer(handle, lineterminator="\n")
        writer.writerow(["region_a", "region_b", "results", "replies", "rtt_p10_ms",
                         "rtt_p50_ms", "rtt_p90_ms", "rtt_p99_ms", "unreachable",
                         "packet_loss"])
        for i, a in enumerate(REGIONS):
            for b in REGIONS[i:]:
                values = sorted(rtts.get((a, b), []))
                total = sent.get((a, b), 0)
                count = results.get((a, b), 0)
                writer.writerow([
                    a, b, results.get((a, b), 0), len(values),
                    f"{quantile(values, 0.10):.3f}", f"{quantile(values, 0.50):.3f}",
                    f"{quantile(values, 0.90):.3f}", f"{quantile(values, 0.99):.3f}",
                    f"{(silent.get((a, b), 0) / count) if count else float('nan'):.5f}",
                    f"{(lost.get((a, b), 0) / total) if total else float('nan'):.5f}",
                ])

    meta = {
        "source": "RIPE Atlas (RIPE NCC), https://atlas.ripe.net — anchoring mesh pings, IPv6",
        "retrieved_utc": dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "day_utc": args.day,
        "windows_utc": [[dt.datetime.fromtimestamp(s, dt.timezone.utc).strftime("%H:%M"),
                         dt.datetime.fromtimestamp(e, dt.timezone.utc).strftime("%H:%M")]
                        for s, e in windows],
        "sources_per_region": SOURCES,
        "targets_per_region": TARGETS,
        "anchors": {
            region: [{"anchor": a["id"], "probe": a["probe"], "fqdn": a["fqdn"],
                      "country": a["country"], "city": a["city"]} for a in members]
            for region, members in chosen.items()
        },
        "targets": targets_meta,
    }
    with open(out / "meta.json", "w") as handle:
        json.dump(meta, handle, indent=1, ensure_ascii=False)
        handle.write("\n")
    print(f"wrote {out / 'region_rtt.csv'} and {out / 'meta.json'}")


if __name__ == "__main__":
    main()
