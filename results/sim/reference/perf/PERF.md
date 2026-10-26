# M3a performance (machine-dependent)

Machine: Intel(R) Core(TM) i5-8257U CPU @ 1.40GHz (macos x86_64), 8 logical cores, rustc 1.97.1 (8bab26f4f 2026-07-14). Threads: 4. Each run is single-threaded; `perf.csv` has every run.

| section | runs | CPU seconds | events | simulated days | events per second |
|---|---|---|---|---|---|
| V0 | 10 | 632.1 | 741083 | 24.08 | 1172 |
| V1 | 1600 | 4852.9 | 57934559 | 3155.49 | 11938 |
| V2 | 7 | 207.8 | 18014554 | 130.00 | 86694 |
| V3 | 38 | 2623.5 | 70929560 | 1795.20 | 27036 |
| C1 | 9 | 3878.2 | 361956179 | 258.00 | 93331 |
| C2 | 28 | 26774.8 | 49264038 | 229.96 | 1840 |
| C3 | 18 | 18127.5 | 87238857 | 198.00 | 4813 |
| C4 | 20 | 1020.9 | 297701833 | 420.00 | 291604 |
| C5 | 15 | 2794.7 | 365954136 | 375.00 | 130945 |
| C6 | 61 | 536.2 | 35405277 | 244.00 | 66035 |
| C7 | 6 | 402.3 | 143700259 | 108.00 | 357196 |
| A6 | 7 | 16259.0 | 337555105 | 335.00 | 20761 |
| E2 | 229 | 16029.1 | 1995627983 | 6264.00 | 124500 |
| E1 | 660 | 317127.6 | 2543113119 | 6853.05 | 8019 |
| E3 | 264 | 6547.0 | 1293130742 | 1215.00 | 197516 |
| E4 | 6 | 13201.6 | 5054909534 | 2196.00 | 382900 |

Total CPU time 431015 s over 2978 runs.

## Slowest runs

| run | seconds | events | simulated days |
|---|---|---|---|
| E1-(a) v0.6 [P]same-parent no-deadline-honest-s0 | 35822.0 | 613640 | 0.33 |
| E1-(a) v0.6 [P]same-parent no-deadline-honest-s3 | 20427.0 | 1075193 | 2.08 |
| E1-(a) v0.6 [P]same-parent no-deadline-honest-s1 | 11219.4 | 168739 | 0.36 |
| E1-(a) v0.6 [P]same-parent no-deadline-honest-s2 | 11000.7 | 1098852 | 2.86 |
| E1-(r) v0.6 [P]same-parent no-deadline-withholder-s0 | 9915.0 | 46726 | 0.01 |
| E1-(a) v0.6 [P]same-parent no-deadline-withholder-s2 | 9056.8 | 43015 | 0.01 |
| E1-(r) v0.6 [P]same-parent D10-soft-honest-s3 | 7506.7 | 60429 | 0.15 |
| E1-(r) v0.6 [P]same-parent no-deadline-honest-s0 | 7270.8 | 1742003 | 5.22 |
| E1-(r) v0.6 [P]same-parent D10-soft-withholder-s1 | 7205.6 | 107047 | 0.45 |
| E1-(r) v0.6 [P]same-parent no-deadline-honest-s3 | 7094.4 | 60439 | 0.15 |
