# Latency benchmark 20261106T145109Z

fbfast 0.2.1 build `0b80bee36fb37ecd`, 3 samples, median per query.

| RTT ms | case | fbfast ms | fbfast round trips | firebird-driver ms |
|---:|---|---:|---:|---:|
| 0 | pk | 1.60 | 4 | 0.95 |
| 0 | narrow1000 | 7.36 | 5 | 7.49 |
| 0 | narrow100k | 611.68 | 104 | 583.15 |
| 0 | wide64 | 998.61 | 120 | 1206.24 |
| 0 | blob_small | 276.24 | 1004 | 49.58 |
| 0 | blob_medium | 1812.06 | 204 | 195.30 |
| 0 | blob_large | 2435.80 | 144 | 2559.68 |
| 0 | blob_binary | 87.07 | 8 | 96.68 |
| 0 | blob_latin1 | 296.89 | 1004 | 68.35 |
| 0 | list_with | 30.98 | 105 | 17.49 |
| 0 | cast_tight | 25.18 | 5 | 26.62 |
| 0 | cast_8191 | 20.50 | 36 | 21.28 |
| 0 | concurrent8 (threads) | 12.34 | - | 3.13 |
| 0 | concurrent8 (gather) | 5.00 | - | - |
| 0 | concurrent32 (threads) | 10.90 | - | 10.95 |
| 0 | concurrent32 (gather) | 27.08 | - | - |
| 0 | concurrent128 (threads) | 39.62 | - | 44.15 |
| 0 | concurrent128 (gather) | 49.10 | - | - |
| 5 | pk | 33.33 | 4 | 21.06 |
| 5 | narrow1000 | 38.61 | 5 | 35.37 |
| 5 | narrow100k | 2256.86 | 104 | 2288.25 |
| 5 | wide64 | 2384.91 | 120 | 2858.89 |
| 5 | blob_small | 9815.83 | 1004 | 99.78 |
| 5 | blob_medium | 3994.60 | 204 | 266.10 |
| 5 | blob_large | 3588.91 | 144 | 3802.12 |
| 5 | blob_binary | 192.80 | 8 | 156.61 |
| 5 | blob_latin1 | 9740.68 | 1004 | 122.13 |
| 5 | list_with | 929.21 | 105 | 40.91 |
| 5 | cast_tight | 67.01 | 5 | 71.02 |
| 5 | cast_8191 | 587.25 | 36 | 415.85 |
| 5 | concurrent8 (threads) | 31.00 | - | 23.19 |
| 5 | concurrent8 (gather) | 31.27 | - | - |
| 5 | concurrent32 (threads) | 122.60 | - | 82.74 |
| 5 | concurrent32 (gather) | 107.40 | - | - |
| 5 | concurrent128 (threads) | 601.62 | - | 310.20 |
| 5 | concurrent128 (gather) | 413.50 | - | - |
| 20 | pk | 96.24 | 4 | 133.15 |
| 20 | narrow1000 | 147.11 | 5 | 118.00 |
| 20 | narrow100k | 4376.45 | 104 | 4284.50 |
| 20 | wide64 | 5416.82 | 120 | 5318.07 |
| 20 | blob_small | 28314.08 | 1004 | 264.45 |
| 20 | blob_medium | 8057.10 | 204 | 567.30 |
| 20 | blob_large | 6507.95 | 144 | 6426.04 |
| 20 | blob_binary | 311.40 | 8 | 365.35 |
| 20 | blob_latin1 | 28833.73 | 1004 | 234.87 |
| 20 | list_with | 3216.87 | 105 | 146.50 |
| 20 | cast_tight | 182.06 | 5 | 128.76 |
| 20 | cast_8191 | 1051.76 | 36 | 1356.86 |
| 20 | concurrent8 (threads) | 114.58 | - | 73.18 |
| 20 | concurrent8 (gather) | 107.53 | - | - |
| 20 | concurrent32 (threads) | 467.58 | - | 373.87 |
| 20 | concurrent32 (gather) | 512.12 | - | - |
| 20 | concurrent128 (threads) | 1842.57 | - | 1386.88 |
| 20 | concurrent128 (gather) | 1955.95 | - | - |
| 50 | pk | 233.60 | 4 | 182.45 |
| 50 | narrow1000 | 375.01 | 5 | 257.03 |
| 50 | narrow100k | 7697.99 | 104 | 8009.50 |
| 50 | wide64 | 8723.14 | 120 | 9344.38 |
| 50 | blob_small | 75900.62 | 1004 | 334.73 |
| 50 | blob_medium | 12489.54 | 204 | 929.91 |
| 50 | blob_large | 11237.57 | 144 | 8741.49 |
| 50 | blob_binary | 565.70 | 8 | 439.87 |
| 50 | blob_latin1 | 66214.49 | 1004 | 388.46 |
| 50 | list_with | 6010.72 | 105 | 176.46 |
| 50 | cast_tight | 311.66 | 5 | 248.01 |
| 50 | cast_8191 | 2594.49 | 36 | 1891.80 |
| 50 | concurrent8 (threads) | 233.00 | - | 163.21 |
| 50 | concurrent8 (gather) | 254.14 | - | - |
| 50 | concurrent32 (threads) | 1099.13 | - | 659.80 |
| 50 | concurrent32 (gather) | 1004.88 | - | - |
| 50 | concurrent128 (threads) | 3974.86 | - | 2645.69 |
| 50 | concurrent128 (gather) | 4124.75 | - | - |

## Limited bandwidth

| case | compression | negotiated | ms | received bytes |
|---|---|---|---:|---:|
| narrow100k | False | False | 4362.33 | 9607456 |
| blob_medium | False | False | 10139.79 | 16617428 |
| narrow100k | True | False | 4564.27 | 9607456 |
| blob_medium | True | False | 10778.93 | 16617428 |
