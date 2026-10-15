# Streams de linhas e orçamento de conexão

`Pool.stream()` e `Pool.stream_async()` são context managers lazy. Cada stream
mantém um worker/conexão até o cursor e a transação terminarem. A fila de lotes
é limitada, o primeiro fetch exige demanda e `prefetch_batches` controla apenas
a antecipação além do lote corrente. `batch_bytes` inclui uma estimativa das
estruturas decodificadas; uma linha maior pode formar um lote único.

O core compartilha o pump de cursor entre materialização e stream, sem converter
o caminho materializado numa concatenação de lotes. Batches possuem seus buffers
e sobrevivem ao fechamento do cursor. Um stream tem consumidor exclusivo,
inclusive durante conversão Python. A entrega assíncrona usa o mesmo limite de
materialização das consultas; close pode executar sem esperar esse limite.

Erro, abandono, deadline, cancelamento e fechamento da pool pedem cleanup ao
worker. Drop não faz I/O. Cancelamento nativo continua independente do worker e
vinculado à geração da operação. O slot só fica disponível após cleanup. EOF
também espera o resultado final de cleanup. Pausa do produtor sem demanda tem
`stream_idle_timeout`; o prazo da consulta inclui consumo e conversão.

Attach e handshake agora recebem o menor orçamento entre connect_timeout e
aquisição. O limite é reavaliado antes/depois de carregar e conectar. O handshake
não herda query_timeout. Timeout fino do chamador não promete interromper uma
chamada nativa presa; ela continua contada. Tentativas internas após backoff
iniciam um novo orçamento quando a espera termina.

Há base de snapshot somente no executor Rust: queries/streams na mesma transação,
prazo compartilhado, aborto em erro e encerramento explícito. O teste real altera
uma única linha do banco descartável de 10 para 20 por outra conexão: o snapshot
continua vendo 10 e, após encerrar, uma consulta normal vê 20. A API pública de
snapshot/session e sua integração ao scheduler ficaram para a etapa posterior,
agora documentada em `0010-public-snapshots.md`.

Os testes cobrem demanda, prefetch, limites por lote, interrupção antecipada,
retenção de buffers, deadlines, cancelamento em fila e conversão, cache em uso,
loop fechado, guard de fork e heartbeat com um milhão de tuplas. Contagens,
build e medições do checkpoint estão em `../progress.md`.
