# Streaming Arrow: demanda, capsules e encerramento

## Contrato

`stream_arrow` e `stream_arrow_async` usam o scheduler e o controle de demanda
dos streams de linhas/JSON. O trabalho agora seleciona um formato explícito
(linhas, JSON ou Arrow), sem combinações ambíguas de flags no core. O polling
foi separado do estado em `pool/stream/{mod,wait}.rs`, mantendo a exclusividade
até a entrega e os guards de PID antes de locks herdados.

O sink Arrow usa o mesmo decoder/builders da materialização. A abertura publica
o schema depois de validar parâmetros e abrir o cursor, antes de autorizar o
primeiro fetch. Consulta vazia tem schema completo e não fabrica um lote NULL.
O número de linhas por lote respeita batch_rows e a estimativa conservadora de
batch_bytes; BLOBs ocupam uma linha por lote, com leitura na transação. Uma linha
indivisível pode ultrapassar batch_bytes. Não se aplica max_result_rows ao stream.

Além dos lotes completos previstos pelo prefetch, o produtor mantém seu buffer
de fetch e um builder reservado. Buffers de lotes entregues pertencem aos seus
proprietários Arrow e não apontam para memória de fetch reutilizável. Guardar
lotes no consumidor aumenta a memória retida pelo consumidor.

## Interfaces Python e ownership

ArrowBatch é imutável, expõe contagens/schema e `__arrow_c_array__`. ArrowSchema
expõe nomes e `__arrow_c_schema__`. Ambos funcionam sem extras Python. Cada
exportação de lote/schema cria novas estruturas ABI proprietárias; o par C Array
segue a ordem schema/array e conserva metadados SQL. PyO3 possui as estruturas,
e os callbacks/drop do crate Arrow liberam os buffers, inclusive em capsules
não consumidas. Uma capsule consumida não pode ser consumida novamente.

ArrowStream síncrono também exporta um C Stream. Uma exportação assume o consumo
exclusivo, rejeitando segunda exportação e iteração Python concorrente. O leitor
usa o mesmo StreamWait, orçamento, fila e canal limitado, sem outra pool nem
thread produtora. C Stream é pull bloqueante; AsyncArrowStream não anuncia esse
protocolo e entrega ArrowBatch por async for, usando o executor existente.

Os callbacks do leitor C verificam os handlers Python existentes entre polls,
liberando o GIL durante a espera. try_attach permite falhar quando o intérprete
não está disponível, sem anexar uma thread durante sua finalização. Erros do
core viram erros do consumidor externo com tipo, fase, SQLSTATE, códigos nativos
e diagnósticos de cleanup. A iteração fbfast continua usando as exceções fbfast.

EOF reconhece o cleanup antes de concluir o leitor. Release antecipado apenas
sinaliza parada, sem bloquear um callback estrangeiro; sair do contexto espera
o cleanup até close_timeout. Abandono final cancela pelo mesmo Drop do Stream.
Fechar explicitamente ArrowStream enquanto outro thread lê pelo C Stream cancela
a leitura, espera o waiter terminar e reconhece sua limpeza. O leitor recebe
cancelamento; close não reapresenta o cancelamento que ele próprio solicitou.
Outros erros e timeout de cleanup continuam visíveis. Um snapshot cancelado
continua abortado, seguindo seu contrato preexistente.

Fechar a pool mantém a política geral: bloquear novos pedidos e esperar pelos
slots até close_timeout, sem prometer cancelar automaticamente uma consulta
ativa sem deadline. O slot permanece contado até término/cancelamento real.

## Evidência e limites

Os testes exercitam igualdade com materialização para tipos comuns, BLOBs,
DECFLOAT e fusos; primeiro NULL, consulta vazia, schema/metadados, capsules
repetidas/consumidas, buffers retidos depois de fechar pool e limite de demanda.
PyArrow 26.0.0 e Polars 2.0.0 consomem o C Stream. O teste Rust adicional observa
as referências do buffer exportado por C Array e sua liberação ao abandonar a
estrutura ABI. Nenhum novo unsafe foi adicionado.

Lifecycle cobre cancel async em snapshot, consumidor simultâneo, idle timeout,
fechamento durante leitura externa, fork e dois subprocessos de sinais reais
(KeyboardInterrupt e handler personalizado). Um milhão de linhas em async
streaming verifica heartbeat sem reter todos os lotes. As instalações isoladas
também verificam lotes sync/async e capsules antes de instalar quaisquer extras.
Contagens finais, tempos e fingerprint estão em `docs/progress.md`.

A primeira rodada do teste de shutdown supôs incorretamente cancelamento
imediato por Pool.close; a política existente somente espera trabalho ativo.
Esse processo de teste foi encerrado pelo timeout do harness (90 s), e o banco
descartável foi recriado antes da repetição. O teste passou a verificar prazo
de shutdown, slot ainda contado e conclusão após o deadline da consulta. Outro
caso verifica fechamento explícito do stream e cancelamento da leitura externa.

Pico de RSS, comparação com outros drivers, matriz restante de capacidades,
métricas de duração e extração paralela continuam pendentes. Os testes de buffers
comprovam propriedades da fronteira Arrow; não declaram o pipeline todo zero-copy.
