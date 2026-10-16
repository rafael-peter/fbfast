# Arrow/Polars materializados e ownership por C Stream

## Implementação

O decoder compartilhado alimenta builders colunares Rust (Arrow 60.0.0), sem
construir uma tabela intermediária de tuplas/dicts Python. O cursor, o bind,
os prazos, o cancelamento e o cleanup continuam no mesmo executor das outras
saídas. Pool, snapshot e atalhos de pools nomeadas oferecem `fetch_arrow`,
`fetch_polars` e suas variantes assíncronas.

Os builders têm capacidade limitada pelo fetch e pela estimativa conservadora
de tamanho da linha. BLOBs são consumidos dentro da transação, uma linha por
lote, conservando `max_blob_bytes`. Buffers variáveis reservam inicialmente no
máximo 64 bytes por linha/coluna, evitando reter `batch_bytes` por célula de
BLOB pequena. Não há contagem de bytes por célula no sink Arrow; stats somam
a memória dos arrays por lote. `max_result_rows` limita a materialização inteira.

O schema vem do describe, inclusive para resultado vazio e primeira linha NULL.
NUMERIC/DECIMAL usam capacidade conservadora do armazenamento (5/10/19/39
dígitos), com a escala original. INT128 usa Decimal256(39,0). DECFLOAT conserva
texto decimal exato, inclusive sinais, coortes e valores especiais. Tipos com
fuso usam structs UTC/zona, independentes das regras de zona do Python. Metadados
por campo conservam tipo SQL, escala, subtipo, charset e largura armazenada.
Aliases duplicados são rejeitados antes do consumo.

## Fronteira com os consumidores

Um proprietário Python privado expõe `__arrow_c_stream__`, com um novo leitor
para cada exportação. O leitor possui referências aos buffers Arrow, sem
ponteiros para o buffer reutilizável de fetch. PyO3 possui a estrutura ABI e o
crate Arrow implementa seus callbacks/drop. Nenhum novo unsafe foi necessário.
Schema solicitado pelo consumidor é rejeitado explicitamente nesta versão.

PyArrow 26.0.0 importa esse stream como Table. Polars 2.0.0 importa diretamente
o protocolo com `rechunk=False`, sem PyArrow; a Series Struct entregue por essa
versão é expandida em DataFrame. Tipos, nomes e ordem das colunas são conferidos
depois da conversão. Decimal256 falha no describe por padrão; apenas a opção
explícita `unsupported="string"` preserva essas colunas como texto exato.

Python importa os extras sob demanda. A importação inicial também consome o
prazo da operação; uma primeira chamada pode exceder um timeout muito curto.
O executor async faz a adaptação final no worker de entrega, e verifica prazo
e cancelamento antes/depois. Uma chamada de consumidor já em execução não é
interrompida à força; sua capacidade de entrega continua contada até terminar.
A exclusividade do snapshot também cobre esse período.

## Verificação e limites

Os testes de integração exercitam schemas vazios/NULL, domínios integrais,
decimais, datas extremas, DST, BLOBs e charsets conhecidos, aliases/limites,
parâmetros, snapshots, pools nomeadas, cancelamento e prazo durante adaptação.
Um consumidor Polars alterado deliberadamente deve falhar se mudar tipos ou
reordenar colunas. O teste de um milhão de linhas observa separadamente o
intervalo da adaptação final e confirma que ele executa fora da thread do loop.

Testes Rust da fronteira C Stream contam exatamente uma liberação do leitor,
com e sem consumo, e retêm um lote depois de destruir o stream. Testes Python
destroem capsules não consumidas, rejeitam reuso de uma capsule consumida,
exportam repetidamente e retêm resultados depois de fechar a pool. Endereços
dos buffers INTEGER são idênticos nos consumidores PyArrow e Polars, inclusive
em múltiplos chunks sem rechunk. Isso não afirma ausência de cópias em todas
as colunas: por exemplo, representações temporais/string dos consumidores
podem exigir adaptação.

O teste escalar revelou códigos SQL_FLOAT/SQL_DOUBLE invertidos no decoder e
no bind anteriores. O header oficial confirma SQL_FLOAT=482 (4 bytes) e
SQL_DOUBLE=480 (8 bytes). Decoder, bind e schema Arrow foram corrigidos.
Regressões usam bytes IEEE independentes em testes de propriedade e valores
SQL/bind reais, incluindo subnormais, zeros assinados e não finitos, nas
saídas linhas, Arrow, Polars e JSON. Os checkpoints anteriores não comprovavam
esse domínio de ponto flutuante.

A coleta de licenças passou a filtrar o grafo Cargo pelo target do wheel, como
previsto por [`cargo metadata --filter-platform`](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html).
O grafo anterior incluía r-efi (somente UEFI), cujo pacote não contém arquivos
de licença, e interrompia o build macOS sem que esse crate fosse ligado nele.
As licenças de todas as dependências efetivas continuam obrigatórias, incluindo
Arrow; os hashes e o inventário do artefato continuam estritos.

ArrowStream/ArrowBatch públicos e a ponte C Array para lotes async foram
implementados depois, conforme a decisão 0017. Ainda faltam medição de RSS e
benchmarks comparativos. Esta decisão não encerra P4–P6 nem
declara zero-copy para o pipeline completo. Contagens finais e build_id ficam
em `docs/progress.md`.
