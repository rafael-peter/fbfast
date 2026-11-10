# Latency benchmark 20261106T152247Z

fbfast 0.3.0 build `3a7cb6aca17e7833`, 3 samples, median per query.

| RTT ms | case | fbfast ms | fbfast round trips | firebird-driver ms |
|---:|---|---:|---:|---:|
| 0 | pk | 2.40 | 3 | 0.75 |
| 0 | narrow1000 | 6.82 | 3 | 7.32 |
| 0 | narrow100k | 603.84 | 102 | 569.54 |
| 0 | wide64 | 1056.12 | 118 | 1154.99 |
| 0 | blob_small | 31.60 | 4 | 50.23 |
| 0 | blob_medium | 198.81 | 11 | 193.69 |
| 0 | blob_large | 2075.46 | 142 | 2134.70 |
| 0 | blob_binary | 82.71 | 6 | 79.09 |
| 0 | blob_latin1 | 66.12 | 5 | 66.19 |
| 0 | list_with | 8.08 | 2 | 10.44 |
| 0 | cast_tight | 20.26 | 3 | 31.51 |
| 0 | cast_8191 | 16.45 | 34 | 18.48 |
| 0 | concurrent8 (threads) | 2.37 | - | 3.41 |
| 0 | concurrent8 (gather) | 5.09 | - | - |
| 0 | concurrent8 (run_all) | 2.02 | - | - |
| 0 | concurrent8 (run_all_async) | 3.75 | - | - |
| 0 | concurrent32 (threads) | 8.80 | - | 9.67 |
| 0 | concurrent32 (gather) | 9.67 | - | - |
| 0 | concurrent32 (run_all) | 10.30 | - | - |
| 0 | concurrent32 (run_all_async) | 11.92 | - | - |
| 0 | concurrent128 (threads) | 36.00 | - | 38.71 |
| 0 | concurrent128 (gather) | 37.92 | - | - |
| 0 | concurrent128 (run_all) | 35.72 | - | - |
| 0 | concurrent128 (run_all_async) | 37.83 | - | - |
| 5 | pk | 24.42 | 3 | 20.36 |
| 5 | narrow1000 | 24.08 | 3 | 31.90 |
| 5 | narrow100k | 2241.44 | 102 | 2370.10 |
| 5 | wide64 | 2164.99 | 118 | 2718.02 |
| 5 | blob_small | 90.32 | 4 | 90.29 |
| 5 | blob_medium | 250.84 | 11 | 258.08 |
| 5 | blob_large | 3152.50 | 142 | 3105.28 |
| 5 | blob_binary | 143.92 | 6 | 133.29 |
| 5 | blob_latin1 | 118.38 | 5 | 118.32 |
| 5 | list_with | 48.71 | 2 | 38.78 |
| 5 | cast_tight | 40.71 | 3 | 63.05 |
| 5 | cast_8191 | 274.64 | 34 | 245.93 |
| 5 | concurrent8 (threads) | 23.67 | - | 21.35 |
| 5 | concurrent8 (gather) | 22.35 | - | - |
| 5 | concurrent8 (run_all) | 24.76 | - | - |
| 5 | concurrent8 (run_all_async) | 23.51 | - | - |
| 5 | concurrent32 (threads) | 100.63 | - | 81.48 |
| 5 | concurrent32 (gather) | 111.68 | - | - |
| 5 | concurrent32 (run_all) | 104.45 | - | - |
| 5 | concurrent32 (run_all_async) | 105.27 | - | - |
| 5 | concurrent128 (threads) | 413.47 | - | 311.26 |
| 5 | concurrent128 (gather) | 420.46 | - | - |
| 5 | concurrent128 (run_all) | 434.41 | - | - |
| 5 | concurrent128 (run_all_async) | 431.69 | - | - |
| 20 | pk | 71.99 | 3 | 70.86 |
| 20 | narrow1000 | 89.55 | 3 | 187.52 |
| 20 | narrow100k | 4795.86 | 102 | 4875.11 |
| 20 | wide64 | 5547.15 | 118 | 5414.12 |
| 20 | blob_small | 181.26 | 4 | 174.10 |
| 20 | blob_medium | 509.79 | 11 | 466.63 |
| 20 | blob_large | 7172.13 | 142 | 5444.29 |
| 20 | blob_binary | 401.14 | 6 | 245.25 |
| 20 | blob_latin1 | 233.52 | 5 | 205.75 |
| 20 | list_with | 91.54 | 3 | 91.35 |
| 20 | cast_tight | 206.74 | 4 | 130.70 |
| 20 | cast_8191 | 1449.84 | 34 | 874.00 |
| 20 | concurrent8 (threads) | 99.72 | - | 78.37 |
| 20 | concurrent8 (gather) | 94.06 | - | - |
| 20 | concurrent8 (run_all) | 131.26 | - | - |
| 20 | concurrent8 (run_all_async) | 127.36 | - | - |
| 20 | concurrent32 (threads) | 702.22 | - | 287.70 |
| 20 | concurrent32 (gather) | 539.91 | - | - |
| 20 | concurrent32 (run_all) | 578.55 | - | - |
| 20 | concurrent32 (run_all_async) | 437.77 | - | - |
| 20 | concurrent128 (threads) | 2380.57 | - | 1146.84 |
| 20 | concurrent128 (gather) | 2224.92 | - | - |
| 20 | concurrent128 (run_all) | 1830.17 | - | - |
| 20 | concurrent128 (run_all_async) | 2017.92 | - | - |
| 50 | pk | 173.20 | 3 | 168.88 |
| 50 | narrow1000 | 189.34 | 3 | 258.47 |
| 50 | narrow100k | 7496.57 | 102 | 7728.96 |
| 50 | wide64 | 8684.47 | 118 | 8827.65 |
| 50 | blob_small | 269.14 | 4 | 337.86 |
| 50 | blob_medium | 810.82 | 11 | 858.67 |
| 50 | blob_large | 10036.88 | 142 | 9994.89 |
| 50 | blob_binary | 440.97 | 6 | 477.96 |
| 50 | blob_latin1 | 361.25 | 5 | 408.60 |
| 50 | list_with | 141.74 | 2 | 190.68 |
| 50 | cast_tight | 199.53 | 3 | 256.65 |
| 50 | cast_8191 | 1972.17 | 35 | 2036.63 |
| 50 | concurrent8 (threads) | 239.98 | - | 176.02 |
| 50 | concurrent8 (gather) | 229.09 | - | - |
| 50 | concurrent8 (run_all) | 224.78 | - | - |
| 50 | concurrent8 (run_all_async) | 226.67 | - | - |
| 50 | concurrent32 (threads) | 922.15 | - | 686.48 |
| 50 | concurrent32 (gather) | 923.59 | - | - |
| 50 | concurrent32 (run_all) | 924.87 | - | - |
| 50 | concurrent32 (run_all_async) | 934.72 | - | - |
| 50 | concurrent128 (threads) | 3638.18 | - | 2805.26 |
| 50 | concurrent128 (gather) | 3696.23 | - | - |
| 50 | concurrent128 (run_all) | 3676.27 | - | - |
| 50 | concurrent128 (run_all_async) | 3751.09 | - | - |

## Limited bandwidth

| case | compression | negotiated | ms | received bytes |
|---|---|---|---:|---:|
| narrow100k | False | False | 6061.19 | 7208052 |
| blob_medium | False | False | 2444.17 | 8617628 |
| narrow100k | True | True | 3108.07 | 1553137 |
| blob_medium | True | True | 77.27 | 557490 |
