# Reuso de statement após reset: evidência inicial

O protótipo `benchmarks/adapters/native_probe.cpp` usa a API OO dos headers
oficiais e o cliente contido no artefato. O servidor é exclusivamente o fixture
Docker Firebird 5.0.3 arm64, com digest fixado no harness.

Em 15/10/2026, as 60 operações do experimento
`benchmarks/results/20261015T102650Z-native/` terminaram com sucesso. Todas
consultaram `MON$TRANSACTIONS` e exigiram `MON$READ_ONLY=1` e
`MON$ISOLATION_MODE=4`. Cada operação fechou o cursor, fez rollback integral e
executou `ALTER SESSION RESET` sem transação ativa. A conexão exigiu WireCrypt.

Nos primeiros 30 ciclos o statement foi liberado; nos 30 seguintes foi mantido
entre operações. O handle e seus metadados continuaram utilizáveis após reset
nesse caso. Isso justifica implementar o cache por attachment e continuar a
homologação. Não prova validade após DDL, timeouts, BLOBs ou alterações de sessão.

Medianas diagnósticas (ms, consulta de dois inteiros):

| Modo | Transação | Prepare | Execute | Fetch | Cleanup | Total |
|---|---:|---:|---:|---:|---:|---:|
| Sem cache | 0,160 | 0,163 | 0,001 | 0,423 | 0,356 | 1,138 |
| Com cache | 0,175 | <0,001 | 0,001 | 0,451 | 0,384 | 1,038 |

Os grupos foram sequenciais e não há baseline Python. Não se infere speedup,
percentis de cauda ou metas aceitas desses números. Execute pode ser adiado pelo
cliente; suas etapas são tempos de chamadas, não RTT. O total inclui checagem
de resultado e não equivale à soma das medianas.

P1 continua aberta: falta ligar o executor Rust, bind, tipos, cancelamento,
protótipos dos sinks e baselines equivalentes antes de congelar metas.
