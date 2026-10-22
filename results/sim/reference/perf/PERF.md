# M3a performance (machine-dependent)

Machine: Intel(R) Core(TM) i5-8257U CPU @ 1.40GHz (macos x86_64), 8 logical cores, rustc 1.97.1 (8bab26f4f 2026-07-14). Threads: 4. Each run is single-threaded; `perf.csv` has every run.

| section | runs | CPU seconds | events | simulated days | events per second |
|---|---|---|---|---|---|
| V0 | 10 | 627.8 | 741083 | 24.08 | 1181 |
| V1 | 1600 | 5102.4 | 57922857 | 3151.96 | 11352 |
| V2 | 7 | 210.6 | 18014554 | 130.00 | 85527 |
| V3 | 38 | 2535.6 | 70929560 | 1795.20 | 27974 |
| C1 | 9 | 3859.2 | 361693855 | 258.00 | 93723 |
| C2 | 27 | 25990.5 | 49449476 | 219.49 | 1903 |
| C3 | 18 | 70745.7 | 104713029 | 198.00 | 1480 |
| C4 | 20 | 1034.1 | 297665103 | 420.00 | 287859 |
| C5 | 15 | 3010.9 | 366119974 | 375.00 | 121600 |
| C6 | 61 | 692.6 | 35682661 | 244.00 | 51522 |
| C7 | 6 | 432.2 | 143836548 | 108.00 | 332808 |
| A6 | 7 | 15366.8 | 337180512 | 335.00 | 21942 |
| E2 | 229 | 16237.1 | 1995139789 | 6264.00 | 122875 |
| E1 | 560 | 244567.3 | 2132173333 | 5739.91 | 8718 |
| E3 | 264 | 7434.6 | 1291684181 | 1215.00 | 173740 |
| E4 | 6 | 13575.7 | 5054026223 | 2196.00 | 372284 |

Total CPU time 411423 s over 2877 runs.

## Slowest runs

| run | seconds | events | simulated days |
|---|---|---|---|
| E1-(a) v0.6 [P]same-parent no-deadline-honest-s3 | 23963.5 | 255366 | 0.48 |
| C3-withholder-D60-permanent | 23387.0 | 11474243 | 11.00 |
| C3-withholder-D5-permanent | 15562.3 | 10732125 | 11.00 |
| C3-withholder-D30-permanent | 12564.9 | 9875149 | 11.00 |
| E1-(a) v0.6 [P]same-parent no-deadline-honest-s0 | 11818.7 | 266622 | 0.50 |
| C3-honest-D5-permanent | 10005.0 | 8970111 | 11.00 |
| E1-(r) v0.6 [P]same-parent D10-soft-withholder-s2 | 9540.6 | 89364 | 0.20 |
| E1-(a) v0.6 [P]same-parent no-deadline-withholder-s2 | 8408.2 | 46147 | 0.01 |
| E1-(r) v0.6 [P]same-parent no-deadline-withholder-s1 | 8143.5 | 59693 | 0.11 |
| E1-(a) v0.6 [P]same-parent D10-soft-withholder-s1 | 7958.7 | 44958 | 0.03 |
