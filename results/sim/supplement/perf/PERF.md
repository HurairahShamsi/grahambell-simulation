# M3a performance (machine-dependent)

Machine: Intel(R) Core(TM) i5-8257U CPU @ 1.40GHz (macos x86_64), 8 logical cores, rustc 1.97.1 (8bab26f4f 2026-07-14). Threads: 4. Each run is single-threaded; `perf.csv` has every run.

| section | runs | CPU seconds | events | simulated days | events per second |
|---|---|---|---|---|---|
| V0 | 10 | 698.7 | 741083 | 24.08 | 1061 |
| W1 | 36 | 4306.6 | 147796857 | 396.00 | 34319 |
| W2 | 41 | 710.6 | 86040156 | 109.50 | 121080 |

Total CPU time 5716 s over 87 runs.

## Slowest runs

| run | seconds | events | simulated days |
|---|---|---|---|
| V0-standin-1000 | 327.9 | 3556 | 0.20 |
| V0-real-32 | 311.8 | 2488 | 0.18 |
| C3-withholder-D10-not_extended_only-W1-oc | 125.7 | 4104972 | 11.00 |
| C3-withholder-D10-not_extended_only-W1-spread | 124.6 | 4113637 | 11.00 |
| E1-(r) first_seen literal D10-soft-withholder-s3-W1-oc | 123.4 | 4382260 | 11.00 |
| E1-(r) first_seen literal D10-soft-withholder-s1-W1-oc | 123.0 | 4256610 | 11.00 |
| C3-withholder-D10-not_extended_only-W1-as-reference | 122.8 | 4102717 | 11.00 |
| E1-(r) first_seen literal D10-soft-withholder-s3-W1-spread | 122.8 | 4393084 | 11.00 |
| E1-(r) first_seen literal D10-soft-withholder-s0-W1-oc | 122.7 | 3995778 | 11.00 |
| E1-(r) first_seen literal D10-soft-withholder-s2-W1-oc | 122.5 | 3851610 | 11.00 |
