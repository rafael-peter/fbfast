# Comparação local reproduzível

Build: `c1555f90c7d1b5dcb34e19e83ccb174ceb487bf6da3aec2dab2fc4635104a1b1`.

Medianas descritivas; esta amostragem não estima p95/p99. Falhas permanecem no relatório.
RSS é o pico do processo na janela da consulta, incluindo runtime e memória retida pelo warmup.
O baseline `firebird-driver-reset` inclui o mesmo rollback + reset de sessão do fbfast;
`firebird-driver` e `fdb` só fecham cursor e fazem rollback. Nenhum reconecta por consulta.

| Caso | Saída | Driver | Fetch | n/erros | Tempo s | Linhas/s | RSS MiB |
|---|---|---|---:|---:|---:|---:|---:|
| narrow100 | arrow | fdb-arm64 | 5000 | 1/0 | 0.002207 | 45312 | 49.7 |
| narrow100 | dicts | fdb-arm64 | 5000 | 1/0 | 0.003833 | 26090 | 32.9 |
| narrow100 | json | fdb-arm64 | 5000 | 1/0 | 0.001594 | 62747 | 32.8 |
| narrow100 | polars | fdb-arm64 | 5000 | 1/0 | 0.001621 | 61681 | 62.9 |
| narrow100 | tuples | fdb-arm64 | 5000 | 1/0 | 0.002528 | 39563 | 33.0 |

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
