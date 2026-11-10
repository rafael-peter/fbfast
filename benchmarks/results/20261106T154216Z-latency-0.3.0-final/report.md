# Latency benchmark 20261106T154216Z

fbfast 0.3.0 build `a003b71a08cf40af`, 3 samples, median per query.

| RTT ms | case | fbfast ms | fbfast round trips | firebird-driver ms |
|---:|---|---:|---:|---:|
| 0 | pk | 2.32 | 3 | 0.75 |
| 0 | narrow1000 | 6.54 | 2 | 6.19 |
| 0 | narrow100k | 597.07 | 101 | 583.50 |
| 0 | wide64 | 1107.91 | 117 | 1015.32 |
| 0 | blob_small | 41.90 | 3 | 48.31 |
| 0 | blob_medium | 188.48 | 10 | 167.47 |
| 0 | blob_large | 2275.54 | 141 | 2384.18 |
| 0 | blob_binary | 84.69 | 5 | 84.23 |
| 0 | blob_latin1 | 69.55 | 4 | 72.48 |
| 0 | list_with | 7.67 | 1 | 10.51 |
| 0 | cast_tight | 26.43 | 2 | 31.60 |
| 0 | cast_8191 | 18.76 | 33 | 20.68 |
| 0 | concurrent8 (threads) | 2.96 | - | 3.36 |
| 0 | concurrent8 (gather) | 4.00 | - | - |
| 0 | concurrent8 (run_all) | 1.33 | - | - |
| 0 | concurrent8 (run_all_async) | 4.20 | - | - |
| 0 | concurrent32 (threads) | 4.44 | - | 12.31 |
| 0 | concurrent32 (gather) | 8.65 | - | - |
| 0 | concurrent32 (run_all) | 4.18 | - | - |
| 0 | concurrent32 (run_all_async) | 11.94 | - | - |
| 0 | concurrent128 (threads) | 18.45 | - | 36.45 |
| 0 | concurrent128 (gather) | 32.15 | - | - |
| 0 | concurrent128 (run_all) | 18.20 | - | - |
| 0 | concurrent128 (run_all_async) | 35.77 | - | - |
| 5 | pk | 22.03 | 3 | 21.05 |
| 5 | narrow1000 | 17.07 | 2 | 45.62 |
| 5 | narrow100k | 2021.82 | 101 | 2180.22 |
| 5 | wide64 | 2243.88 | 117 | 2998.61 |
| 5 | blob_small | 94.17 | 3 | 87.88 |
| 5 | blob_medium | 391.12 | 10 | 267.81 |
| 5 | blob_large | 3455.33 | 142 | 3448.66 |
| 5 | blob_binary | 127.47 | 5 | 124.21 |
| 5 | blob_latin1 | 160.53 | 4 | 117.99 |
| 5 | list_with | 33.24 | 1 | 40.84 |
| 5 | cast_tight | 47.00 | 2 | 56.09 |
| 5 | cast_8191 | 356.55 | 33 | 205.18 |
| 5 | concurrent8 (threads) | 13.44 | - | 20.82 |
| 5 | concurrent8 (gather) | 15.67 | - | - |
| 5 | concurrent8 (run_all) | 11.99 | - | - |
| 5 | concurrent8 (run_all_async) | 11.90 | - | - |
| 5 | concurrent32 (threads) | 51.32 | - | 78.57 |
| 5 | concurrent32 (gather) | 49.09 | - | - |
| 5 | concurrent32 (run_all) | 82.52 | - | - |
| 5 | concurrent32 (run_all_async) | 47.70 | - | - |
| 5 | concurrent128 (threads) | 214.28 | - | 305.53 |
| 5 | concurrent128 (gather) | 199.79 | - | - |
| 5 | concurrent128 (run_all) | 229.65 | - | - |
| 5 | concurrent128 (run_all_async) | 199.00 | - | - |
| 20 | pk | 69.92 | 3 | 94.49 |
| 20 | narrow1000 | 49.32 | 2 | 121.54 |
| 20 | narrow100k | 4232.35 | 102 | 4392.23 |
| 20 | wide64 | 4882.13 | 117 | 5285.04 |
| 20 | blob_small | 185.44 | 3 | 215.19 |
| 20 | blob_medium | 448.35 | 10 | 510.99 |
| 20 | blob_large | 5772.01 | 142 | 5922.37 |
| 20 | blob_binary | 220.90 | 6 | 250.32 |
| 20 | blob_latin1 | 195.75 | 4 | 230.51 |
| 20 | list_with | 49.33 | 1 | 99.19 |
| 20 | cast_tight | 100.37 | 2 | 139.04 |
| 20 | cast_8191 | 842.04 | 33 | 1312.54 |
| 20 | concurrent8 (threads) | 100.90 | - | 74.73 |
| 20 | concurrent8 (gather) | 52.79 | - | - |
| 20 | concurrent8 (run_all) | 53.67 | - | - |
| 20 | concurrent8 (run_all_async) | 46.99 | - | - |
| 20 | concurrent32 (threads) | 190.63 | - | 426.85 |
| 20 | concurrent32 (gather) | 205.71 | - | - |
| 20 | concurrent32 (run_all) | 186.50 | - | - |
| 20 | concurrent32 (run_all_async) | 195.36 | - | - |
| 20 | concurrent128 (threads) | 822.20 | - | 1465.90 |
| 20 | concurrent128 (gather) | 891.19 | - | - |
| 20 | concurrent128 (run_all) | 844.03 | - | - |
| 20 | concurrent128 (run_all_async) | 889.84 | - | - |
| 50 | pk | 242.58 | 3 | 216.61 |
| 50 | narrow1000 | 134.29 | 2 | 307.87 |
| 50 | narrow100k | 8270.01 | 101 | 8505.92 |
| 50 | wide64 | 9192.21 | 117 | 10325.83 |
| 50 | blob_small | 221.55 | 3 | 441.05 |
| 50 | blob_medium | 1014.02 | 10 | 1102.98 |
| 50 | blob_large | 9557.04 | 142 | 11224.24 |
| 50 | blob_binary | 394.61 | 5 | 534.04 |
| 50 | blob_latin1 | 335.40 | 4 | 444.50 |
| 50 | list_with | 72.98 | 1 | 225.85 |
| 50 | cast_tight | 156.68 | 2 | 348.56 |
| 50 | cast_8191 | 1752.15 | 33 | 2521.98 |
| 50 | concurrent8 (threads) | 125.77 | - | 198.19 |
| 50 | concurrent8 (gather) | 112.59 | - | - |
| 50 | concurrent8 (run_all) | 101.39 | - | - |
| 50 | concurrent8 (run_all_async) | 105.97 | - | - |
| 50 | concurrent32 (threads) | 429.99 | - | 920.46 |
| 50 | concurrent32 (gather) | 439.46 | - | - |
| 50 | concurrent32 (run_all) | 442.88 | - | - |
| 50 | concurrent32 (run_all_async) | 445.10 | - | - |
| 50 | concurrent128 (threads) | 2009.00 | - | 3219.51 |
| 50 | concurrent128 (gather) | 2176.20 | - | - |
| 50 | concurrent128 (run_all) | 2714.44 | - | - |
| 50 | concurrent128 (run_all_async) | 2175.62 | - | - |

## Limited bandwidth

| case | compression | negotiated | ms | received bytes |
|---|---|---|---:|---:|
| narrow100k | False | False | 5267.67 | 9609612 |
| blob_medium | False | False | 2559.15 | 6216224 |
| narrow100k | True | True | 3168.65 | 2070579 |
| blob_medium | True | True | 85.43 | 53139 |
