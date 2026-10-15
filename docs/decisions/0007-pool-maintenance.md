# Handshake e manutenção da pool

O attachment só é disponibilizado depois de verificar no servidor a versão 5.0.x,
role ativa, charset, timezone, transporte TCP e TPB READ ONLY/READ CONSISTENCY.
A identidade observada fica em `ExecutionConnection::session_info()` no core.
A consulta interna usa limites próprios de uma linha e 64 KiB; uma cota menor
definida pelo usuário continua valendo para suas consultas e não impede attach.

Timezone é comparado sem diferença de caixa e com normalização de offsets
numéricos; aliases não são presumidos equivalentes. Na fixture 5.0.3 foram
observados UTC, GMT, America/Sao_Paulo e offsets, incluindo `+5:30` normalizado
para `+05:30`. Isso não implementa tipos SQL TIME/TIMESTAMP WITH TIME ZONE.
O executor escalar ainda exige UTF8.

Criptografia e compressão registram os valores efetivos de MON$ATTACHMENTS.
`required` exige criptografia e `disabled` a proíbe. Na fixture padrão,
`required`/`enabled` negociaram criptografia; `disabled` foi recusado pelo
servidor. Pedir compressão retornou `MON$WIRE_COMPRESSED=false`: não declaramos
compressão ativa nem a matriz de negociações homologada. Campos oficiais:
[MON$ATTACHMENTS/TRANSACTIONS, Firebird 5](https://www.firebirdsql.org/file/documentation/chunk/en/refdocs/fblangref50/fblangref50-appx05-montables.html).

Depois da primeira demanda, o scheduler repõe o piso `min_idle` usando somente
slots livres e os próprios workers. A fila de consultas recebe preferência.
Slots em abertura contam para a reserva; nenhuma thread adicional de manutenção
ou fila ilimitada é criada. Falhas repetidas usam espera crescente limitada
por slot. `max_lifetime` varia entre 90% e 99,9% do configurado para distribuir
reciclagens e só é aplicado quando o attachment não tem consulta ativa.
`min_idle=0` não reabre conexões aposentadas por inatividade.

A limpeza coordenada usa uma geração global e uma confirmação por slot.
Attachments ociosos recebem um comando; os ocupados concluem o cursor e seu
cleanup antes de limpar. Pedidos simultâneos podem compartilhar a mesma limpeza.
Publicar um slot disponível e conferir sua geração acontece sob o mesmo mutex:
um pedido que chegue entre a limpeza e a publicação recebe novo comando, mesmo
que nenhuma outra consulta seja submetida. O encerramento definitivo confirma
cache vazio também sob o mutex, depois de fechar o handle nativo.

Esperas por invalidação dividem o limite `max_waiters` com consultas. Timeout ou
cancelamento remove só a espera; a geração solicitada continua em vigor e não
cancela a consulta de outro chamador. Falhas de limpeza são propagadas aos
interessados, e o attachment é descartado. `clear_statement_cache*` usa o prazo
de consulta por padrão e aceita `timeout=None`. Fechar a pool acorda os waiters
com `PoolClosedError`. Construtor e invalidação de uma pool nunca aberta não
criam threads ou attachments.

Os testes Rust modelam de forma determinística as janelas entre confirmação e
publicação, fechamento com waiters e propagação de falha injetada. Testes reais
verificam reposição sem nova demanda, teto, consulta ativa além de lifetime,
piso zero e falhas de autenticação com backoff. Os testes Python exercitam dois
attachments com cache aquecido, invalidação sync/async, espera expirada,
coalescência e cancelamento sem interromper o cursor ou trocar seu attachment.

Ainda pendem: limitar attach e handshake ao orçamento restante de aquisição,
mais falhas nativas e partições de rede, métricas de duração e a matriz completa
de capacidades. Estas mudanças não concluem P2/P3 nem substituem os testes
de streaming/snapshot das próximas entregas.
