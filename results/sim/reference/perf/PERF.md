# M3a performance (machine-dependent)

Machine: Intel(R) Core(TM) i5-8257U CPU @ 1.40GHz (macos x86_64), 8 logical cores, rustc 1.97.1 (8bab26f4f 2026-07-14). Threads: 4. Each run is single-threaded; `perf.csv` has every run.

| section | runs | CPU seconds | events | simulated days | events per second |
|---|---|---|---|---|---|
| V0 | 10 | 632.1 | 741083 | 24.08 | 1172 |
| V1 | 1600 | 5077.2 | 57934559 | 3155.49 | 11411 |
| V2 | 7 | 207.8 | 18014554 | 130.00 | 86694 |
| V3 | 38 | 2623.5 | 70929560 | 1795.20 | 27036 |
| C1 | 9 | 3339.6 | 361654010 | 258.00 | 108294 |
| C2 | 28 | 27105.0 | 50575814 | 234.04 | 1866 |
| C3 | 18 | 33887.5 | 96464637 | 198.00 | 2847 |
| C4 | 20 | 1079.5 | 298013643 | 420.00 | 276078 |
| C5 | 15 | 2512.7 | 366341049 | 375.00 | 145793 |
| C6 | 61 | 590.1 | 35414952 | 244.00 | 60014 |
| C7 | 6 | 381.1 | 143938918 | 108.00 | 377668 |
| A6 | 7 | 15842.1 | 337610890 | 335.00 | 21311 |
| E2 | 229 | 17397.8 | 1994615146 | 6264.00 | 114648 |
| E1 | 660 | 231270.2 | 2546576096 | 6869.20 | 11011 |
| E3 | 264 | 6957.4 | 1291796277 | 1215.00 | 185673 |
| E4 | 6 | 13485.9 | 5056168178 | 2196.00 | 374923 |

Total CPU time 362389 s over 2978 runs.

## Slowest runs

| run | seconds | events | simulated days |
|---|---|---|---|
| E1-(a) v0.6 [P]same-parent no-deadline-honest-s2 | 9718.9 | 479893 | 1.44 |
| E1-(r) v0.6 [P]same-parent no-deadline-withholder-s1 | 8715.3 | 59693 | 0.11 |
| C3-withholder-D10-permanent | 8646.9 | 8824986 | 11.00 |
| E1-(a) v0.6 [P]same-parent no-deadline-withholder-s2 | 8025.6 | 46147 | 0.01 |
| C3-withholder-D5-permanent | 7934.5 | 8268580 | 11.00 |
| E1-(a) v0.6 [P]same-parent D10-soft-withholder-s1 | 7637.1 | 44958 | 0.03 |
| E1-(a) v0.6 [P]same-parent D10-soft-withholder-s0 | 7498.2 | 245280 | 0.56 |
| E1-(r) v0.6 [P]same-parent D10-soft-withholder-s3 | 6675.4 | 133659 | 0.41 |
| E1-(r) v0.6 [P]same-parent no-deadline-withholder-s2 | 6505.0 | 112472 | 0.16 |
| C2-v0.6-same-parent-loss0.02 | 6240.9 | 114621 | 0.54 |
