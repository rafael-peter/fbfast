# Comparação local reproduzível

Build: `c1555f90c7d1b5dcb34e19e83ccb174ceb487bf6da3aec2dab2fc4635104a1b1`.

Medianas descritivas; esta amostragem não estima p95/p99. Falhas permanecem no relatório.
RSS é o pico do processo na janela da consulta, incluindo runtime e memória retida pelo warmup.
O baseline `firebird-driver-reset` inclui o mesmo rollback + reset de sessão do fbfast;
`firebird-driver` e `fdb` só fecham cursor e fazem rollback. Nenhum reconecta por consulta.

| Caso | Saída | Driver | Fetch | n/erros | Tempo s | Linhas/s | RSS MiB |
|---|---|---|---:|---:|---:|---:|---:|
| narrow100k | arrow | fbfast | 5000 | 3/0 | 0.529005 | 189034 | 47.8 |
| narrow100k | arrow | fdb-arm64 | 5000 | 3/0 | 0.533879 | 187308 | 70.2 |
| narrow100k | arrow | firebird-driver | 5000 | 3/0 | 0.534441 | 187112 | 77.0 |
| narrow100k | arrow | firebird-driver-reset | 5000 | 3/0 | 0.545248 | 183403 | 77.1 |
| narrow100k | dicts | fbfast | 5000 | 3/0 | 0.542892 | 184199 | 73.3 |
| narrow100k | dicts | fdb-arm64 | 5000 | 3/0 | 0.545402 | 183351 | 65.7 |
| narrow100k | dicts | firebird-driver | 5000 | 3/0 | 0.542426 | 184357 | 71.8 |
| narrow100k | dicts | firebird-driver-reset | 5000 | 3/0 | 0.544607 | 183619 | 70.9 |
| narrow100k | json | fbfast | 5000 | 3/0 | 0.529683 | 188792 | 36.5 |
| narrow100k | json | fdb-arm64 | 5000 | 3/0 | 0.548413 | 182344 | 68.5 |
| narrow100k | json | firebird-driver | 5000 | 3/0 | 0.548164 | 182427 | 75.6 |
| narrow100k | json | firebird-driver-reset | 5000 | 3/0 | 0.555116 | 180143 | 74.7 |
| narrow100k | polars | fbfast | 5000 | 3/0 | 0.536699 | 186324 | 61.6 |
| narrow100k | polars | fdb-arm64 | 5000 | 3/0 | 0.552195 | 181095 | 87.8 |
| narrow100k | polars | firebird-driver | 5000 | 3/0 | 0.539359 | 185405 | 93.5 |
| narrow100k | polars | firebird-driver-reset | 5000 | 3/0 | 0.573136 | 174479 | 92.8 |
| narrow100k | tuples | fbfast | 5000 | 3/0 | 0.540276 | 185090 | 56.5 |
| narrow100k | tuples | fdb-arm64 | 5000 | 3/0 | 0.519641 | 192440 | 46.0 |
| narrow100k | tuples | firebird-driver | 5000 | 3/0 | 0.515698 | 193912 | 52.2 |
| narrow100k | tuples | firebird-driver-reset | 5000 | 3/0 | 0.528130 | 189347 | 51.7 |
| wide16 | arrow | fbfast | 5000 | 3/0 | 0.133079 | 75143 | 55.8 |
| wide16 | arrow | fdb-arm64 | 5000 | 3/0 | 0.145947 | 68518 | 70.2 |
| wide16 | arrow | firebird-driver | 5000 | 3/0 | 0.137364 | 72799 | 76.7 |
| wide16 | arrow | firebird-driver-reset | 5000 | 3/0 | 0.144379 | 69262 | 74.2 |
| wide16 | dicts | fbfast | 5000 | 3/0 | 0.144896 | 69015 | 72.3 |
| wide16 | dicts | fdb-arm64 | 5000 | 3/0 | 0.143234 | 69816 | 50.7 |
| wide16 | dicts | firebird-driver | 5000 | 3/0 | 0.137946 | 72492 | 56.9 |
| wide16 | dicts | firebird-driver-reset | 5000 | 3/0 | 0.145830 | 68573 | 56.9 |
| wide16 | json | fbfast | 5000 | 3/0 | 0.131600 | 75988 | 39.5 |
| wide16 | json | fdb-arm64 | 5000 | 3/0 | 0.145295 | 68825 | 54.0 |
| wide16 | json | firebird-driver | 5000 | 3/0 | 0.145184 | 68878 | 61.0 |
| wide16 | json | firebird-driver-reset | 5000 | 3/0 | 0.142157 | 70345 | 62.7 |
| wide16 | polars | fbfast | 5000 | 3/0 | 0.136980 | 73004 | 73.0 |
| wide16 | polars | fdb-arm64 | 5000 | 3/0 | 0.149176 | 67035 | 92.0 |
| wide16 | polars | firebird-driver | 5000 | 3/0 | 0.145368 | 68791 | 96.5 |
| wide16 | polars | firebird-driver-reset | 5000 | 3/0 | 0.144125 | 69384 | 98.6 |
| wide16 | tuples | fbfast | 5000 | 3/0 | 0.140457 | 71196 | 62.7 |
| wide16 | tuples | fdb-arm64 | 5000 | 3/0 | 0.137961 | 72484 | 46.3 |
| wide16 | tuples | firebird-driver | 5000 | 3/0 | 0.134525 | 74336 | 52.3 |
| wide16 | tuples | firebird-driver-reset | 5000 | 3/0 | 0.136334 | 73349 | 52.3 |
| wide64 | arrow | fbfast | 5000 | 3/0 | 0.424123 | 23578 | 99.5 |
| wide64 | arrow | fdb-arm64 | 5000 | 3/0 | 0.527088 | 18972 | 138.2 |
| wide64 | arrow | firebird-driver | 5000 | 3/0 | 0.455640 | 21947 | 143.6 |
| wide64 | arrow | firebird-driver-reset | 5000 | 3/0 | 0.459689 | 21754 | 144.6 |
| wide64 | dicts | fbfast | 5000 | 3/0 | 0.471906 | 21191 | 185.5 |
| wide64 | dicts | fdb-arm64 | 5000 | 3/0 | 0.498188 | 20073 | 105.1 |
| wide64 | dicts | firebird-driver | 5000 | 3/0 | 0.446654 | 22389 | 110.9 |
| wide64 | dicts | firebird-driver-reset | 5000 | 3/0 | 0.456908 | 21886 | 110.5 |
| wide64 | json | fbfast | 5000 | 3/0 | 0.419567 | 23834 | 81.7 |
| wide64 | json | fdb-arm64 | 5000 | 3/0 | 0.525064 | 19045 | 152.5 |
| wide64 | json | firebird-driver | 5000 | 3/0 | 0.459304 | 21772 | 155.7 |
| wide64 | json | firebird-driver-reset | 5000 | 3/0 | 0.459697 | 21753 | 159.0 |
| wide64 | polars | fbfast | 5000 | 3/0 | 0.424666 | 23548 | 124.1 |
| wide64 | polars | fdb-arm64 | 5000 | 3/0 | 0.529714 | 18878 | 188.0 |
| wide64 | polars | firebird-driver | 5000 | 3/0 | 0.464904 | 21510 | 192.0 |
| wide64 | polars | firebird-driver-reset | 5000 | 3/0 | 0.457865 | 21840 | 191.8 |
| wide64 | tuples | fbfast | 5000 | 3/0 | 0.457225 | 21871 | 146.3 |
| wide64 | tuples | fdb-arm64 | 5000 | 3/0 | 0.490199 | 20400 | 88.9 |
| wide64 | tuples | firebird-driver | 5000 | 3/0 | 0.429359 | 23291 | 94.3 |
| wide64 | tuples | firebird-driver-reset | 5000 | 3/0 | 0.413620 | 24177 | 94.0 |

Limites e procedimento:

- TCP localhost into Docker/OrbStack VM; no injected latency or LAN/WAN claim.
- One attachment per worker; sequential samples; no concurrency/queue/tail acceptance yet.
- One full query warmup, then one sample per fresh process. Server data cache remains warm.
- Adapter order reverses each round. Failed runs stay in raw samples; no best-run filtering.
- Full equality/types/NULL/multiplicity and checksums validated AFTER timer and RSS sampling.
- Client CPU covers the query; cgroup CPU spans the measured barrier plus docker counter overhead.
- Server memory current/peak are whole-container counters; peak is lifetime, not query-only.
- RSS samples every 5 ms can miss short peaks; ru_maxrss also records imports and full warmup.
- Range queries use ORDER BY PK; scans use ROWS and validate a sorted multiset afterward. SQL server work is included.
- Arrow values/types match; SQL metadata is additionally preserved only by fbfast.
- FDB uses the bundled client's default crypt policy; fixture requires encryption for all adapters.
- fdb-arm64 includes an explicit variadic ctypes ABI workaround; unmodified fdb is separate.
- BLOB/modern types, slow streams, controlled RTT and parallel extraction require separate runs.
