# Latency benchmark 20261106T162154Z

fbfast 0.3.0 build `5e85e5a77af8c23e`, 5 samples, median per query.

| RTT ms | case | fbfast ms | fbfast round trips | firebird-driver ms |
|---:|---|---:|---:|---:|
| 0 | pk | 0.35 | 1 | 0.82 |
| 0 | narrow1000 | 6.93 | 2 | 7.05 |
| 0 | cast_tight | 20.48 | 2 | 27.08 |
| 0 | list_with | 9.07 | 1 | 8.87 |
| 5 | pk | 6.13 | 1 | 19.09 |
| 5 | narrow1000 | 17.39 | 2 | 31.99 |
| 5 | cast_tight | 40.35 | 2 | 60.13 |
| 5 | list_with | 23.81 | 1 | 36.72 |
| 20 | pk | 22.02 | 1 | 67.25 |
| 20 | narrow1000 | 48.84 | 2 | 111.42 |
| 20 | cast_tight | 73.08 | 2 | 120.18 |
| 20 | list_with | 39.10 | 1 | 93.64 |
| 50 | pk | 54.55 | 1 | 164.83 |
| 50 | narrow1000 | 124.45 | 2 | 229.65 |
| 50 | cast_tight | 144.22 | 2 | 253.12 |
| 50 | list_with | 78.29 | 1 | 182.84 |

## Limited bandwidth

| case | compression | negotiated | ms | received bytes |
|---|---|---|---:|---:|
