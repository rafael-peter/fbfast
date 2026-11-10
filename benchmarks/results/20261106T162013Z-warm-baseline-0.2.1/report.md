# Latency benchmark 20261106T162013Z

fbfast 0.2.1 build `0b80bee36fb37ecd`, 5 samples, median per query.

| RTT ms | case | fbfast ms | fbfast round trips | firebird-driver ms |
|---:|---|---:|---:|---:|
| 0 | pk | 0.74 | 4 | 0.78 |
| 0 | narrow1000 | 6.53 | 5 | 7.07 |
| 0 | cast_tight | 47.22 | 5 | 41.77 |
| 0 | list_with | 49.14 | 105 | 10.74 |
| 5 | pk | 28.28 | 4 | 19.88 |
| 5 | narrow1000 | 95.66 | 5 | 30.57 |
| 5 | cast_tight | 50.84 | 5 | 47.43 |
| 5 | list_with | 750.65 | 105 | 41.09 |
| 20 | pk | 91.09 | 4 | 68.70 |
| 20 | narrow1000 | 134.24 | 5 | 113.17 |
| 20 | cast_tight | 154.27 | 5 | 133.00 |
| 20 | list_with | 2627.35 | 105 | 88.19 |
| 50 | pk | 228.60 | 4 | 164.45 |
| 50 | narrow1000 | 287.15 | 5 | 234.12 |
| 50 | cast_tight | 305.66 | 5 | 245.35 |
| 50 | list_with | 5815.67 | 105 | 181.12 |

## Limited bandwidth

| case | compression | negotiated | ms | received bytes |
|---|---|---|---:|---:|
