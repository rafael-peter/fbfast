# Comparação local reproduzível

Build: `c1555f90c7d1b5dcb34e19e83ccb174ceb487bf6da3aec2dab2fc4635104a1b1`.

Medianas descritivas; esta amostragem não estima p95/p99. Falhas permanecem no relatório.
RSS é o pico do processo na janela da consulta, incluindo runtime e memória retida pelo warmup.
O baseline `firebird-driver-reset` inclui o mesmo rollback + reset de sessão do fbfast;
`firebird-driver` e `fdb` só fecham cursor e fazem rollback. Nenhum reconecta por consulta.

| Caso | Saída | Driver | Fetch | n/erros | Tempo s | Linhas/s | RSS MiB |
|---|---|---|---:|---:|---:|---:|---:|
| narrow100 | arrow | fbfast | 5000 | 1/0 | 0.001440 | 69426 | 46.8 |
| narrow100 | arrow | firebird-driver | 5000 | 1/0 | 0.001860 | 53777 | 55.8 |
| narrow100 | arrow | firebird-driver-reset | 5000 | 1/0 | 0.001849 | 54098 | 56.0 |
| narrow100 | dicts | fbfast | 5000 | 1/0 | 0.001868 | 53545 | 30.2 |
| narrow100 | dicts | firebird-driver | 5000 | 1/0 | 0.001890 | 52923 | 39.1 |
| narrow100 | dicts | firebird-driver-reset | 5000 | 1/0 | 0.001775 | 56347 | 39.2 |
| narrow100 | json | fbfast | 5000 | 1/0 | 0.001586 | 63065 | 30.1 |
| narrow100 | json | firebird-driver | 5000 | 1/0 | 0.001704 | 58680 | 39.4 |
| narrow100 | json | firebird-driver-reset | 5000 | 1/0 | 0.002109 | 47421 | 39.4 |
| narrow100 | polars | fbfast | 5000 | 1/0 | 0.001726 | 57942 | 60.5 |
| narrow100 | polars | firebird-driver | 5000 | 1/0 | 0.001971 | 50729 | 68.3 |
| narrow100 | polars | firebird-driver-reset | 5000 | 1/0 | 0.002087 | 47917 | 68.5 |
| narrow100 | tuples | fbfast | 5000 | 1/0 | 0.002205 | 45355 | 31.3 |
| narrow100 | tuples | firebird-driver | 5000 | 1/0 | 0.001787 | 55948 | 39.2 |
| narrow100 | tuples | firebird-driver-reset | 5000 | 1/0 | 0.002173 | 46027 | 38.9 |

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
- BLOB/modern types, slow streams, controlled RTT and parallel extraction require separate runs.
