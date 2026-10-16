> PRE-FLIGHT: harness changed during execution; excluded from acceptance. See manifest.

# Comparação local reproduzível

Build: `c1555f90c7d1b5dcb34e19e83ccb174ceb487bf6da3aec2dab2fc4635104a1b1`.

Medianas descritivas; esta amostragem não estima p95/p99. Falhas permanecem no relatório.
RSS é o pico do processo na janela da consulta, incluindo runtime e memória retida pelo warmup.
O baseline `firebird-driver-reset` inclui o mesmo rollback + reset de sessão do fbfast;
`firebird-driver` e `fdb` só fecham cursor e fazem rollback. Nenhum reconecta por consulta.

| Caso | Saída | Driver | Fetch | n/erros | Tempo s | Linhas/s | RSS MiB |
|---|---|---|---:|---:|---:|---:|---:|
| narrow100k | arrow | fbfast | 5000 | 3/0 | 0.536548 | 186377 | 48.2 |
| narrow100k | arrow | fdb-arm64 | 5000 | 3/0 | 0.545305 | 183384 | 70.3 |
| narrow100k | arrow | firebird-driver | 5000 | 3/0 | 0.543799 | 183892 | 76.0 |
| narrow100k | arrow | firebird-driver-reset | 5000 | 3/0 | 0.543842 | 183877 | 76.6 |
| narrow100k | dicts | fbfast | 5000 | 3/0 | 0.551928 | 181183 | 74.0 |
| narrow100k | dicts | fdb-arm64 | 5000 | 3/0 | 0.555970 | 179866 | 65.5 |
| narrow100k | dicts | firebird-driver | 5000 | 3/0 | 0.553468 | 180679 | 71.2 |
| narrow100k | dicts | firebird-driver-reset | 5000 | 3/0 | 0.561327 | 178149 | 71.2 |
| narrow100k | json | fbfast | 5000 | 3/0 | 0.541375 | 184715 | 36.5 |
| narrow100k | json | fdb-arm64 | 5000 | 3/0 | 0.577807 | 173068 | 68.0 |
| narrow100k | json | firebird-driver | 5000 | 3/0 | 0.563879 | 177343 | 74.9 |
| narrow100k | json | firebird-driver-reset | 5000 | 3/0 | 0.561340 | 178145 | 74.0 |
| narrow100k | polars | fbfast | 5000 | 3/0 | 0.537310 | 186112 | 61.4 |
| narrow100k | polars | fdb-arm64 | 5000 | 3/0 | 0.547299 | 182716 | 87.9 |
| narrow100k | polars | firebird-driver | 5000 | 3/0 | 0.544356 | 183703 | 94.2 |
| narrow100k | polars | firebird-driver-reset | 5000 | 3/0 | 0.545904 | 183182 | 93.5 |
| narrow100k | tuples | fbfast | 5000 | 3/0 | 0.546451 | 182999 | 57.0 |
| narrow100k | tuples | fdb-arm64 | 5000 | 3/0 | 0.523874 | 190886 | 46.1 |
| narrow100k | tuples | firebird-driver | 5000 | 3/0 | 0.526937 | 189776 | 51.9 |
| narrow100k | tuples | firebird-driver-reset | 5000 | 3/0 | 0.536436 | 186415 | 52.6 |
| wide16 | arrow | fbfast | 5000 | 3/0 | 0.134423 | 74392 | 56.5 |
| wide16 | arrow | fdb-arm64 | 5000 | 3/0 | 0.147188 | 67940 | 70.2 |
| wide16 | arrow | firebird-driver | 5000 | 3/0 | 0.144466 | 69221 | 77.1 |
| wide16 | arrow | firebird-driver-reset | 5000 | 3/0 | 0.146854 | 68095 | 74.8 |
| wide16 | dicts | fbfast | 5000 | 3/0 | 0.145530 | 68714 | 74.3 |
| wide16 | dicts | fdb-arm64 | 5000 | 3/0 | 0.139387 | 71743 | 50.6 |
| wide16 | dicts | firebird-driver | 5000 | 3/0 | 0.138162 | 72379 | 56.9 |
| wide16 | dicts | firebird-driver-reset | 5000 | 3/0 | 0.140521 | 71164 | 57.1 |
| wide16 | json | fbfast | 5000 | 3/0 | 0.137296 | 72835 | 41.4 |
| wide16 | json | fdb-arm64 | 5000 | 3/0 | 0.148460 | 67358 | 56.0 |
| wide16 | json | firebird-driver | 5000 | 3/0 | 0.146041 | 68474 | 63.2 |
| wide16 | json | firebird-driver-reset | 5000 | 3/0 | 0.151978 | 65799 | 62.2 |
| wide16 | polars | fbfast | 5000 | 3/0 | 0.135547 | 73775 | 73.0 |
| wide16 | polars | fdb-arm64 | 5000 | 3/0 | 0.151096 | 66183 | 90.7 |
| wide16 | polars | firebird-driver | 5000 | 3/0 | 0.144020 | 69435 | 96.9 |
| wide16 | polars | firebird-driver-reset | 5000 | 3/0 | 0.143723 | 69578 | 96.8 |
| wide16 | tuples | fbfast | 5000 | 3/0 | 0.147062 | 67998 | 62.7 |
| wide16 | tuples | fdb-arm64 | 5000 | 3/0 | 0.138343 | 72284 | 46.3 |
| wide16 | tuples | firebird-driver | 5000 | 3/0 | 0.138709 | 72094 | 52.5 |
| wide16 | tuples | firebird-driver-reset | 5000 | 3/0 | 0.136283 | 73376 | 52.3 |
| wide64 | arrow | fbfast | 5000 | 3/0 | 0.425458 | 23504 | 99.7 |
| wide64 | arrow | fdb-arm64 | 5000 | 3/0 | 0.544770 | 18356 | 139.2 |
| wide64 | arrow | firebird-driver | 5000 | 3/0 | 0.465316 | 21491 | 144.7 |
| wide64 | arrow | firebird-driver-reset | 5000 | 3/0 | 0.462060 | 21642 | 143.9 |
| wide64 | dicts | fbfast | 5000 | 3/0 | 0.477974 | 20922 | 185.7 |
| wide64 | dicts | fdb-arm64 | 5000 | 3/0 | 0.520983 | 19194 | 105.2 |
| wide64 | dicts | firebird-driver | 5000 | 3/0 | 0.455738 | 21942 | 110.5 |
| wide64 | dicts | firebird-driver-reset | 5000 | 3/0 | 0.465319 | 21491 | 110.6 |
| wide64 | json | fbfast | 5000 | 3/0 | 0.424477 | 23558 | 81.8 |
| wide64 | json | fdb-arm64 | 5000 | 3/0 | 0.525708 | 19022 | 151.6 |
| wide64 | json | firebird-driver | 5000 | 3/0 | 0.458901 | 21791 | 153.7 |
| wide64 | json | firebird-driver-reset | 5000 | 3/0 | 0.452182 | 22115 | 158.8 |
| wide64 | polars | fbfast | 5000 | 3/0 | 0.428647 | 23329 | 124.4 |
| wide64 | polars | fdb-arm64 | 5000 | 3/0 | 0.530407 | 18853 | 189.1 |
| wide64 | polars | firebird-driver | 5000 | 3/0 | 0.460175 | 21731 | 193.6 |
| wide64 | polars | firebird-driver-reset | 5000 | 3/0 | 0.467713 | 21381 | 191.1 |
| wide64 | tuples | fbfast | 5000 | 3/0 | 0.460950 | 21694 | 146.3 |
| wide64 | tuples | fdb-arm64 | 5000 | 3/0 | 0.510724 | 19580 | 88.9 |
| wide64 | tuples | firebird-driver | 5000 | 3/0 | 0.520690 | 19205 | 94.2 |
| wide64 | tuples | firebird-driver-reset | 5000 | 3/0 | 0.433613 | 23062 | 94.1 |

Limites e procedimento:

- TCP localhost into Docker/OrbStack VM; no injected latency or LAN/WAN claim.
- One attachment per worker; sequential samples; no concurrency/queue/tail acceptance yet.
- One full query warmup, then one sample per fresh process. Server data cache remains warm.
- Adapter order reverses each round. Failed runs stay in raw samples; no best-run filtering.
- Full equality/types/NULL/multiplicity and checksums validated AFTER timer and RSS sampling.
- Client CPU covers the query; cgroup CPU spans the measured barrier plus docker counter overhead.
- Server memory current/peak are whole-container counters; peak is lifetime, not query-only.
- RSS samples every 5 ms can miss short peaks; ru_maxrss also records imports and full warmup.
- Rows are ordered by PK; SQL server work is included, not assumed to be pure conversion.
- Arrow values/types match; SQL metadata is additionally preserved only by fbfast.
- FDB uses the bundled client's default crypt policy; fixture requires encryption for all adapters.
- fdb-arm64 includes an explicit variadic ctypes ABI workaround; unmodified fdb is separate.
- BLOB/modern types, slow streams, controlled RTT and parallel extraction require separate runs.
