# BLOBs materializados na transação

O decoder compartilhado por linhas e JSON aceita BLOB BINARY (0) e TEXT (1).
Cada célula é consumida por um leitor do worker, sem tabela universal de valores
e sem handles escapando para Python. A ponte oficial usa openBlob/getSegment/close
e mantém no máximo um IBlob por attachment; cursor/rollback recusam um BLOB aberto.
O cleanup desarma cancelamento, fecha o BLOB, fecha o cursor e então faz rollback
e reset. Falha no cleanup descarta o attachment e preserva o erro original.

Tipos conhecidos de texto pedem transliteração oficial para UTF8 com BPB.
NONE só vira texto com fallback UTF8 explícito; OCTETS e subtipo binário mantêm
bytes. Não há replacement nem trim de BLOBs. O limite max_blob_bytes é por célula,
aplicado aos bytes lidos após transliteração, antes de anexá-los ao resultado.
Uma leitura adicional de um byte distingue EOF exato de excesso sem truncar.
Não se consulta o tamanho inteiro para alocar cegamente, nem se cria BLOB no bind.

O pump avança uma linha por vez quando o schema tem BLOB, materializando antes
do próximo fetch. Streams de linhas produzem um lote de uma linha nesse caso;
uma linha pode exceder batch_bytes e continua limitada por max_blob_bytes por
célula. O buffering de rede do cliente permanece ativo. JSON recebe uma célula
por vez; array limita chunks por bytes e NDJSON conserva registros inteiros.

Cada chamada à ponte agrega até 256 segmentos, limitada por buffer de 32 KiB,
para evitar uma travessia C ABI por segmento diminuto. A fixture com RPAD produz
segmentos de um byte. A contagem também limita chamadas com segmentos vazios.
Cancelamento e prazo são consultados antes/depois de cada bloco, fora do GIL.
CancelOperation isoladamente não interrompeu getSegment no teste local; o core
não depende disso para interromper entre leituras. Uma chamada nativa presa em
rede continua sujeita às limitações de timeout/cleanup já documentadas.

O teste Rust interrompe deterministicamente depois de ler um trecho de um BLOB
real, com handle/transação abertos: cancelamento e deadline passam pelo cleanup
comum e a próxima consulta encontra apenas sua própria transação. O teste nativo
exercita segmento parcial, fechamento antecipado e cancel pendente durante
cleanup. Python verifica NULL/vazio, Unicode, WIN1252, NONE válido/inválido,
OCTETS/binário, subtipos desconhecidos, limites exatos/excedidos, conteúdo retido,
streams/JSON/snapshots sync/async e recuperação após erro/cancelamento/prazo.

Referência primária do lifecycle:
[exemplo oficial Firebird 5.0.3](https://github.com/FirebirdSQL/firebird/blob/v5.0.3/examples/interfaces/04.print_table.cpp).
A conversão usa os parâmetros BPB do SDK oficial instalado. Inline BLOBs e cache
do cliente ainda precisam de medição com baseline equivalente; tamanhos próximos
de 64 KiB nos testes não demonstram por si só negociação ou ganho de inline.
