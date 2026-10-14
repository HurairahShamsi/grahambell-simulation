# M3a performance (machine-dependent)

Machine: Intel(R) Core(TM) i5-8257U CPU @ 1.40GHz (macos x86_64), 8 logical cores, rustc 1.97.1 (8bab26f4f 2026-07-14). Threads: 4. Each run is single-threaded; `perf.csv` has every run.

| section | runs | CPU seconds | events | simulated days | events per second |
|---|---|---|---|---|---|
| V0 | 8 | 608.5 | 570855 | 17.08 | 938 |
| V1 | 800 | 1944.0 | 29063064 | 1583.67 | 14950 |
| V2 | 7 | 153.7 | 15995294 | 130.00 | 104096 |
| V3 | 38 | 2048.1 | 70929560 | 1795.20 | 34632 |
| C1 | 9 | 923.1 | 540681252 | 258.00 | 585742 |
| C2 | 27 | 647.3 | 400413491 | 296.00 | 618626 |
| C3 | 18 | 776.3 | 241426083 | 198.00 | 311002 |
| C4 | 20 | 831.0 | 308212320 | 420.00 | 370914 |
| C5 | 15 | 1296.7 | 1197627916 | 375.00 | 923606 |
| C6 | 61 | 399.2 | 26094443 | 244.00 | 65370 |
| C7 | 6 | 327.0 | 281279407 | 108.00 | 860285 |
| A6 | 2 | 444.8 | 81192017 | 92.00 | 182533 |

Total CPU time 10399 s over 1011 runs.

## Slowest runs

| run | seconds | events | simulated days |
|---|---|---|---|
| C3-withholder-D60-permanent | 391.0 | 12037226 | 11.00 |
| V0-standin-1000 | 296.9 | 3556 | 0.20 |
| V0-real-32 | 277.8 | 2488 | 0.18 |
| A6-patience14 | 225.0 | 36945870 | 46.00 |
| A6-patience3 | 219.8 | 44246147 | 46.00 |
| C1-m100000-literal-s0 | 184.0 | 119003042 | 31.00 |
| C1-m100000-literal-s1 | 153.9 | 35619000 | 31.00 |
| C5-W1-strict, ladder with yearly reset [P] | 131.9 | 99713420 | 31.00 |
| C5-W1-strict, ladder | 131.4 | 99713420 | 31.00 |
| V3-A-K2016-clamp4-m0.1 | 129.4 | 2388489 | 107.10 |
