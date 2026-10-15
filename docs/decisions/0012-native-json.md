# JSON nativo e streaming com o scheduler existente

O novo sink consome o mesmo `DecodePlan` por célula e escreve diretamente em
`Vec<u8>` usando `serde_json` para escaping/números e seu formatter para o texto
decimal exato. Não constrói `Rows`, objetos Python por célula nem árvore
`serde_json::Value`. Valores temporários de texto/binário ainda são decodificados
pelo caminho comum; esse custo não é anunciado como zero-copy. `time` converte
datas e `base64` atende apenas a opção explícita. Dependências são fixadas no
Cargo.lock e seus avisos entram no artefato pelo coletor existente.

BIGINT/INT128 seguem a política pela metadata, inclusive para valores pequenos.
Decimais fixos nunca passam por float; number usa coeficiente/expoente exatos.
Datas têm faixa 0001–9999; TIME mantém a precisão de 100 microssegundos do Firebird.
Nomes duplicados em records falham no describe, antes de consumir linhas. Values
preserva colunas repetidas. BLOB/DECFLOAT/timezone continuam dependências abertas
da matriz de tipos, sem fallback textual genérico.

Consultas materializadas e streams usam `ExecutionConnection::operation/pump`:
binding, cache, TPB, deadlines, cancel e cleanup são os mesmos. A enum de trabalho
e a enum de entrega acrescentam JSON; não há outro scheduler, executor ou pool.
Snapshots passam pela mesma sessão reservada. Estatísticas contam os bytes JSON
codificados e as linhas, incluindo linhas divididas entre chunks de array.

O consumer de streaming compartilha fila limitada, créditos de demanda e prazo
ocioso com linhas. NDJSON guarda registros inteiros; um registro grande pode
superar o alvo de lote, sozinho. Array corta em bytes e não promete caracteres
UTF-8 completos por chunk. Há somente um registro codificado temporário, lote
nativo e buffers de saída/prefetch; não se acumula o resultado inteiro no stream.
O chunk final `]` espera sucesso do cleanup. Um abandono ou erro não produz um
documento falsamente completo. A espera desse chunk respeita o deadline mesmo
depois de desarmar o cancelamento do cursor nativo já fechado.

Um teste Rust real consulta MON$TRANSACTIONS por outra conexão do mesmo usuário:
observa a transação ativa durante os chunks de dados e nenhuma transação no
attachment ao receber `]`. Também confere linhas/bytes quando o consumer retorna
false ao aceitar o último chunk. Outro caso interrompe em `[` por parada/erro e
verifica que a conexão volta a servir uma consulta, sem emitir fechamento falso.

Python recebe apenas bytes próprios. A cópia final de resultados grandes ocorre
em blocos de 1 MiB com GIL liberado e checkpoints; alocação/inicialização inicial
continua sendo custo da fronteira Python. A entrega async mantém a mesma admissão
limitada e o guard de exclusividade até finalizar a cópia. Classes de stream
genéricas compartilham lifecycle entre lotes de linhas e bytes JSON, mantendo
tipos públicos distintos e precisos.

Os testes exercitam precisão, aliases, binários, não finitos via binding real,
Unicode/escaping, datas extremas, limites, chunks de um byte, retenção, snapshots,
cancelamento, prazos e um milhão de linhas com heartbeat. A matriz HTTP acrescenta
NDJSON e array aos cenários reais de desconexão, erro, EOF e shutdown do lifespan.
O probe `scripts/benchmark_json.py` compara somente os dois caminhos da própria
fbfast, com SQL/cliente/cache iguais e hashes idênticos; não substitui os baselines
firebird-driver/fdb nem a matriz completa de desempenho do plano.
