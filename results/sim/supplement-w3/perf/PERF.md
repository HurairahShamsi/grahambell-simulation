# M3a performance (machine-dependent)

Machine: Intel(R) Core(TM) i5-8257U CPU @ 1.40GHz (macos x86_64), 8 logical cores, rustc 1.97.1 (8bab26f4f 2026-07-14). Threads: 6. Each run is single-threaded; `perf.csv` has every run.

| section | runs | CPU seconds | events | simulated days | events per second |
|---|---|---|---|---|---|
| V0 | 10 | 878.7 | 741083 | 24.08 | 843 |
| W3 | 520 | 19479.3 | 5277948284 | 1299.47 | 270952 |

Total CPU time 20358 s over 530 runs.

## Slowest runs

| run | seconds | events | simulated days |
|---|---|---|---|
| V0-standin-1000 | 416.0 | 3556 | 0.20 |
| V0-real-32 | 395.6 | 2488 | 0.18 |
| W3-as_east-s59-reeval-d10 | 99.2 | 9464819 | 2.50 |
| W3-as_east-s51-reeval-d10 | 95.0 | 9228956 | 2.50 |
| W3-as_east-s62-reeval-d10 | 90.7 | 11184464 | 2.50 |
| W3-as_east-s72-reeval-d10 | 89.5 | 10593285 | 2.50 |
| W3-as_east-s47-reeval-d10 | 87.2 | 10613093 | 2.50 |
| W3-as_east-s20-reeval-d10 | 79.1 | 11294349 | 2.50 |
| W3-as_east-s67-reeval-d10 | 75.9 | 10347452 | 2.50 |
| W3-as_east-s54-reeval-d10 | 75.4 | 10340849 | 2.50 |
