# Ponte async com executor compartilhado

`fetch_all_async`, `fetch_one_async`, `fetch_val_async`, `explain_async`,
`warmup`, `close` e `async with Pool` usam os mesmos jobs, regras, deadlines,
cache e cleanup da API sync. Parâmetros são convertidos antes do despacho.

O canal de resposta é `flume` 0.11.1 bounded(1), que fornece espera síncrona e
future sem thread adicional por chamada. A ponte consolidada é
`pyo3-async-runtimes` 0.27.0, compatível com PyO3 0.27.2. Tokio 1.48.0 usa duas
threads de coordenação e no máximo duas de trabalho bloqueante para acesso ao
Python. Chamadas Firebird continuam exclusivamente nos workers nativos da pool.
Importar o pacote e construir config/pool não inicia o runtime.

A admissão de resultados async é limitada por pool a `pool_size + max_waiters`,
sem espera adicional: saturação gera `PoolBusyError`. O permit continua retido
se o Future Python for cancelado enquanto a materialização ainda está rodando.
Uma tarefa Tokio não pode matar uma função `spawn_blocking` em execução;
cancelamento cooperativo confere o token durante a conversão.

Objetos Python são criados fora da thread do event loop. Checkpoints ocorrem
por 512 células ou após 2 ms de trabalho entre células, liberando o GIL por um
intervalo pedido de 100 microssegundos. São parâmetros de escalonamento, não
garantias físicas: uma conversão individual, o SO e o GIL podem demorar mais.
Buffers nativos grandes são desalocados fora do GIL. O teste de um milhão de
tuplas registra heartbeat também depois do cleanup nativo, durante a criação
dos objetos; isso evita medir apenas a espera de rede.

Cada chamada captura o loop/contexto atual. Uma proxy local intercepta somente
`call_soon_threadsafe` para impedir entrega em loop encerrado; não substitui o
loop no aplicativo. O future também verifica fechamento enquanto aguarda e
antes/durante a conversão. A biblioteca upstream cuida do cancelamento normal
de Future, preservando `asyncio.CancelledError`; a proxy resolve a tentativa
de agendar conclusão quando o loop original já foi fechado. A inicialização
do runtime verifica PID antes de acessar seu singleton após fork.

Cobertura real: resultados sync/async, parâmetros e explain, loops sucessivos,
fila cheia, cancel antes do despacho, na fila, ativo e durante conversão,
cancel depois de concluir, timeout nativo e durante conversão, `timeout=None`,
context manager, loop fechado sem callback tardio e heartbeat de um milhão de
tuplas. Métricas do heartbeat ficam em `.tools/python-integration.xml`, com
build_id. Limite verificado pelo teste: nenhum intervalo acima de 150 ms no
cenário local; não representa uma meta universal ou benchmark comparativo.

Houve uma falha de 269,345 ms enquanto outros testes nativos e validações
executavam em paralelo. Não foi atribuída automaticamente ao GC: duas execuções
isoladas posteriores, sem alterar o limite nem desabilitar GC, tiveram máximos
de 25,322 e 8,230 ms. Na última, o GC medido teve máximo de 5,945 ms, com 3.010
heartbeats, 287 deles durante materialização. A causa do pico sob concorrência
não foi isolada; testes sob carga controlada continuam pendentes. Não se declara
garantia de 150 ms em qualquer carga do sistema.

No checkpoint de manutenção `15b7346105d586188cf4c6bffe43f7535f5455c15fc03c7d86b825911db75140`,
a execução isolada registrou máximo de 20,882 ms, 2.850 heartbeats (285 durante
materialização) e maior pausa de GC de 8,499 ms. Os números anteriores são
históricos; a variação não é tratada como ganho ou regressão comprovada.

Continuam pendentes streaming/snapshot, HTTP lifecycle, registry, métricas
completas e comparação equivalente contra outros drivers. O teste local não
homologa outros loops/interpretadores nem todos os cenários de shutdown/falha.

Referências: [PyO3 async runtimes](https://github.com/PyO3/pyo3-async-runtimes),
[versão fixada](https://crates.io/crates/pyo3-async-runtimes/0.27.0),
[canal flume](https://docs.rs/flume/0.11.1/flume/).
