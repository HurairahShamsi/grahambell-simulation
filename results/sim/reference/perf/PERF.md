# M3a performance (machine-dependent)

Machine: Intel(R) Core(TM) i5-8257U CPU @ 1.40GHz (macos x86_64), 8 logical cores, rustc 1.97.1 (8bab26f4f 2026-07-14). Threads: 4. Each run is single-threaded; `perf.csv` has every run.

| section | runs | CPU seconds | events | simulated days | events per second |
|---|---|---|---|---|---|
| V0 | 8 | 801.0 | 582805 | 17.08 | 728 |
| V1 | 800 | 1863.9 | 29063064 | 1583.67 | 15592 |
| V2 | 7 | 162.4 | 18459501 | 130.00 | 113667 |
| V3 | 38 | 1995.7 | 70929560 | 1795.20 | 35541 |
| C1 | 9 | 990.8 | 544669426 | 258.00 | 549746 |
| C2 | 27 | 993.1 | 438618987 | 294.19 | 441662 |
| C3 | 18 | 590.3 | 284062829 | 198.00 | 481237 |
| C4 | 20 | 1781.9 | 352538845 | 360.96 | 197845 |
| C5 | 15 | 1798.0 | 1183261200 | 331.38 | 658084 |
| C6 | 61 | 398.7 | 26972099 | 244.00 | 67647 |
| C7 | 6 | 333.1 | 278637530 | 108.00 | 836607 |
| A6 | 2 | 505.6 | 84859947 | 92.00 | 167847 |

Total CPU time 12214 s over 1011 runs.

## Slowest runs

| run | seconds | events | simulated days |
|---|---|---|---|
| V0-real-32 | 444.3 | 3416 | 0.18 |
| C4-B+P2-all_globally_active-laptop-heavy | 334.3 | 27255451 | 9.19 |
| C2-v0.6-literal-partitions | 333.9 | 21044542 | 9.19 |
| C4-B-all_globally_active-laptop-heavy | 308.3 | 26251900 | 9.19 |
| V0-standin-1000 | 303.1 | 3556 | 0.20 |
| A6-patience3 | 255.9 | 47355350 | 46.00 |
| A6-patience14 | 249.7 | 37504597 | 46.00 |
| C5-W1-strict, ladder with yearly reset [P] | 212.9 | 84977341 | 9.19 |
| C5-W1-strict, ladder | 211.2 | 84977341 | 9.19 |
| C5-W2-strict, ladder | 210.7 | 103470983 | 31.00 |
