# Latency: fbfast 0.2.1 vs 0.3.0

Base `20261106T162013Z-warm-baseline-0.2.1` (build `0b80bee36fb3`), new `20261106T162154Z-warm-0.3.0` (build `5e85e5a77af8`). Median of 5 samples per query, milliseconds. Round trips are the steady-state count from `pool.diagnostics()`. RTT is added by toxiproxy.

| RTT | case | 0.2.1 ms | RT | 0.3.0 ms | RT | speedup | firebird-driver ms |
|---:|---|---:|---:|---:|---:|---:|---:|
| 0 | pk | 0.7 | 4 | 0.3 | 1 | 2.1x | 0.8 |
| 0 | narrow1000 | 6.5 | 5 | 6.9 | 2 | 0.9x | 7.1 |
| 0 | cast_tight | 47.2 | 5 | 20.5 | 2 | 2.3x | 27.1 |
| 0 | list_with | 49.1 | 105 | 9.1 | 1 | 5.4x | 8.9 |
| 5 | pk | 28.3 | 4 | 6.1 | 1 | 4.6x | 19.1 |
| 5 | narrow1000 | 95.7 | 5 | 17.4 | 2 | 5.5x | 32.0 |
| 5 | cast_tight | 50.8 | 5 | 40.4 | 2 | 1.3x | 60.1 |
| 5 | list_with | 750.6 | 105 | 23.8 | 1 | 31.5x | 36.7 |
| 20 | pk | 91.1 | 4 | 22.0 | 1 | 4.1x | 67.3 |
| 20 | narrow1000 | 134.2 | 5 | 48.8 | 2 | 2.7x | 111.4 |
| 20 | cast_tight | 154.3 | 5 | 73.1 | 2 | 2.1x | 120.2 |
| 20 | list_with | 2,627.4 | 105 | 39.1 | 1 | 67.2x | 93.6 |
| 50 | pk | 228.6 | 4 | 54.5 | 1 | 4.2x | 164.8 |
| 50 | narrow1000 | 287.1 | 5 | 124.4 | 2 | 2.3x | 229.7 |
| 50 | cast_tight | 305.7 | 5 | 144.2 | 2 | 2.1x | 253.1 |
| 50 | list_with | 5,815.7 | 105 | 78.3 | 1 | 74.3x | 182.8 |
