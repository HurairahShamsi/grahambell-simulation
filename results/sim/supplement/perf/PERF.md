# M3a performance (machine-dependent)

Machine: Intel(R) Core(TM) i5-8257U CPU @ 1.40GHz (macos x86_64), 8 logical cores, rustc 1.97.1 (8bab26f4f 2026-07-14). Threads: 4. Each run is single-threaded; `perf.csv` has every run.

| section | runs | CPU seconds | events | simulated days | events per second |
|---|---|---|---|---|---|
| V0 | 10 | 714.6 | 741083 | 24.08 | 1037 |
| W1 | 36 | 7028.5 | 147883645 | 396.00 | 21041 |
| W2 | 41 | 1135.4 | 85422133 | 109.50 | 75238 |

Total CPU time 8878 s over 87 runs.

## Slowest runs

| run | seconds | events | simulated days |
|---|---|---|---|
| V0-standin-1000 | 341.4 | 3556 | 0.20 |
| V0-real-32 | 324.1 | 2488 | 0.18 |
| E1-(r) first_seen literal D10-soft-withholder-s4-W1-as-reference | 236.6 | 4002728 | 11.00 |
| E1-(r) first_seen literal D10-soft-withholder-s3-W1-as-reference | 232.2 | 4256556 | 11.00 |
| E1-(r) first_seen literal D10-soft-withholder-s2-W1-as-reference | 229.1 | 3837323 | 11.00 |
| E1-(r) first_seen literal no-deadline-withholder-s0-W1-as-reference | 228.5 | 3968992 | 11.00 |
| E1-(r) first_seen literal no-deadline-withholder-s2-W1-as-reference | 226.6 | 3836886 | 11.00 |
| E1-(r) first_seen literal D10-soft-withholder-s1-W1-as-reference | 225.5 | 4236453 | 11.00 |
| E1-(r) first_seen literal D10-soft-withholder-s0-W1-as-reference | 225.3 | 3960246 | 11.00 |
| E1-(r) first_seen literal D10-soft-withholder-s4-W1-spread | 224.8 | 4096657 | 11.00 |
