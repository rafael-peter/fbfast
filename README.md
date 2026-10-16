# fbfast

Biblioteca de leitura para Firebird 5, com core Rust e ponte para o cliente oficial.
Oferece pool, consultas sync/async, snapshots, streaming e saídas em linhas,
JSON, Arrow e Polars. A entrega atual está focada na distribuição dessa API;
extração paralela e outras evoluções do plano ficam para versões posteriores.

Matriz de wheels: **macOS arm64/x86_64, Linux aarch64/x86_64 e Windows x86_64**,
com CPython 3.11–3.14 com GIL. Cada alvo tem build e consulta real no CI. Consulte
[evidências e limites de validação](docs/progress.md).

## Instalar com uv

A distribuição principal passa a ser um pacote versionado, com fontes e tags
no Git. Um wheel pronto dispensa Rust, C++ e instalação local do Firebird no
consumidor. Ainda requer um servidor Firebird acessível para executar consultas.

Para versões publicadas no PyPI:

```sh
uv add fbfast
# Ou, em um ambiente gerenciado com pip:
pip install fbfast
```

Também é possível instalar um wheel baixado dos artefatos do GitHub Actions:

```sh
uv add /caminho/para/fbfast-0.1.0-cp314-cp314-macosx_11_0_arm64.whl
```

`uv pip install arquivo.whl` e `pip install arquivo.whl` também funcionam.
`uv add` registra a dependência e atualiza o lock do projeto. Instalar de um
checkout (`uv add /caminho/para/firebird_rust`) ou do Git compila no computador
consumidor e exige Rust 1.91.1, C++ e SDK Firebird. Os fontes ficam no
[repositório público fbfast](https://github.com/rafael-peter/fbfast).

Para instalar uma revisão publicada, substitua `COMMIT` pelo hash desejado:

```sh
uv add "git+https://github.com/rafael-peter/fbfast.git" --rev COMMIT
# Alternativa sem registrar dependência no pyproject.toml:
pip install "fbfast @ git+https://github.com/rafael-peter/fbfast.git@COMMIT"
```

Fixar um commit ou uma tag evita atualizar a biblioteca implicitamente a cada
instalação. A publicação PyPI é automatizada por tag `v<versão>` após os testes
dos wheels e a configuração do Trusted Publisher. Os resultados do CI e o
estado de publicação estão registrados em [docs/progress.md](docs/progress.md).
Veja o [guia de release e configuração PyPI](docs/releasing.md).

O backend PEP 517 inclui extensão, runtime, manifesto e licenças no mesmo wheel;
não há cópia manual posterior. Git não torna o binário universal. Outros
sistemas/arquiteturas e `abi3` ainda precisam de validação própria. Veja
`docs/decisions/0004-distribution-uv.md`.

## Compilar

No ambiente de desenvolvimento, instale Rust 1.91.1, CPython 3.14 e as dependências
de `requirements-dev.txt`. O SDK/distribuição Firebird deve ser 5.0.3 ou superior
na família 5.0.x. O build não baixa o cliente.

```sh
python3.14 scripts/build_vendor.py --release --out dist/vendor \
  --client-root /Library/Frameworks/Firebird.framework
python3.14 scripts/verify_vendor.py --package dist/vendor/fbfast
```

O build gera o wheel em `dist/wheels/` e extrai o mesmo artefato em
`dist/vendor/fbfast/`. Como alternativa ao uv/pip, copie essa pasta inteira
para a raiz importável do projeto. Ao migrar para uv, remova a cópia antiga
para não sombrear a instalação.

```python
import fbfast
print(fbfast.build_info())  # Sem conexão e sem carregar cliente
print(fbfast.client_info())  # Carrega cliente, sem conexão ao banco
```

## Configuração

`FirebirdConfig()` já lê automaticamente `FB_*` do ambiente e o `.env` do diretório
atual. Para cada campo: **argumento explícito → ambiente → .env → padrão**.
Por exemplo, `host` omitido usa `FB_HOST` e, se ausente nas duas fontes, `localhost`.
`database`, `user` e `password` não têm padrão e precisam existir em alguma fonte.

```python
from fbfast import FirebirdConfig, Pool

config = FirebirdConfig()  # Usa FB_DATABASE, FB_USER, FB_PASSWORD etc.
config = FirebirdConfig(host="outro-servidor", query_timeout=None)
# host explícito prevalece; None desativa o prazo mesmo se FB_QUERY_TIMEOUT existir.
```

O construtor inclui tipos e documentação por parâmetro para o IntelliSense:
função, variável de ambiente, valores aceitos e padrão. `...` na assinatura
tipada representa um argumento omitido; não é um valor para passar ao construtor.
`env_file=False` desativa apenas o arquivo, mantendo o ambiente. Um caminho
explícito deve existir. `from_env(prefix="FB_BI_")` fica disponível para prefixos
personalizados; não é necessário no uso normal.

A resolução e validação ocorrem em Rust, sem modificar o ambiente global.
A configuração é imutável e oculta senha em `repr`.

O resultado completo é limitado apenas por `max_result_rows` (padrão `None`, sem
limite). `max_result_bytes` foi removido; remova esse argumento de chamadas antigas.
`batch_bytes` continua sendo o orçamento de cada lote e os bytes produzidos
continuam disponíveis nas estatísticas. A remoção da checagem não constitui uma
medição de ganho de desempenho.

`wire_crypt` aceita `"required"` (padrão, exige criptografia), `"enabled"` (negocia
e permite conexão sem criptografia) e `"disabled"` (desativa). `True` equivale a
`"required"`; `False`, a `"disabled"`. Se o servidor estiver com `WireCrypt=Disabled`,
o modo `"required"` falha com `Incompatible wire encryption levels requested on
client and server`. Nesse ambiente, `FirebirdConfig(wire_crypt="enabled")` permite
a conexão e também permite criptografia quando ela estiver disponível no servidor.
O modo `"required"` não faz fallback automático para uma conexão sem criptografia.
Veja as [regras de negociação do Firebird](https://www.firebirdsql.org/docs/html/en/refdocs/fbconf/firebird-configuration-reference.html).

## Pools nomeadas

`init_pool_sync(config, name="default")` aquece e registra uma pool pronta;
`await init_pool(...)` faz o equivalente assíncrono. `get_pool(name)` devolve a
mesma instância. Os atalhos do módulo usam essa pool, seus limites e seu cache:

```python
await fbfast.init_pool(fbfast.FirebirdConfig(), name="bi")
try:
    rows = await fbfast.fetch_all_async(
        "SELECT ID FROM MINHA_TABELA WHERE ID >= ?", [10], pool="bi"
    )
    async with fbfast.stream_async("SELECT ID FROM MINHA_TABELA", pool="bi") as batches:
        async for batch in batches:
            await processar_async(batch)
finally:
    await fbfast.close_pool("bi")
```

`fetch_all`, `fetch_one`, `fetch_val`, `explain` e suas versões async, além de
`stream`/`stream_async`, aceitam `pool="nome"`; omitir seleciona `"default"`.
Nenhuma consulta inicializa uma pool global automaticamente.
`PoolNotInitializedError` indica nome ausente, em abertura ou em fechamento.
Um nome já reservado gera `PoolAlreadyInitializedError`, inclusive em corridas.

`close_pool_sync(name)` ou `await close_pool(name)` bloqueia novas buscas, drena
e remove a instância; fechar um nome ausente é inofensivo. Em timeout/erro ou
cancelamento durante cleanup, o nome continua reservado: repita o fechamento
para concluir antes de inicializar outra pool com esse nome. Fechar durante a
abertura impede a publicação posterior. Nomes pertencem ao processo; inicialize
no lifespan após fork, ou use spawn. Pools herdadas de outro PID são rejeitadas
antes de acessar locks/handles. O teto agregado soma todas as pools e workers.

Em FastAPI, inicialize no `lifespan` e feche em `finally`:

```python
from contextlib import asynccontextmanager
from fastapi import FastAPI

@asynccontextmanager
async def lifespan(app: FastAPI):
    await fbfast.init_pool(fbfast.FirebirdConfig())
    try:
        yield
    finally:
        await fbfast.close_pool()

app = FastAPI(lifespan=lifespan)
```

Mantenha o `async with stream_async(...)` dentro do gerador que fornece o corpo
HTTP. Os testes usam TCP real com FastAPI 0.143.2, Starlette 1.8.0 e Uvicorn 0.54.0
(ASGI HTTP 2.3): desconexão enquanto espera fetch ou consome um lote, erro após
headers, término normal e shutdown. Comprovam saída do contexto e liberação da
conexão, mantendo referências ao gerador/stream para não depender de GC. Outras
versões/servidores precisam repetir essa verificação. Os testes também transportam
JSON nativo em NDJSON e array. Depois de enviar headers,
um erro interrompe o corpo e não transforma a resposta parcial em sucesso.

## Consultas

Já estão implementados
`Pool`, `fetch_all`, `fetch_one`, `fetch_val`, `fetch_json`, `explain` (com params), `stats`,
`warmup_sync`, `close_sync` e `with Pool(config)`. Consultas usam binding `?`, TPB READ ONLY e
rollback/reset antes da reutilização. `fetch_all`/`fetch_one` aceitam `as_dict`.
DECIMAL/INT128 e datas/horas sem timezone preservam a precisão suportada.

As consultas e `explain` têm variantes `*_async`. Use `await pool.warmup()`,
`await pool.close()` ou `async with Pool(config)`. A API async usa o mesmo
executor, com entrega vinculada ao loop da chamada; não exige loop global.
Materialização Python ocorre em workers limitados com liberação periódica do
GIL, incluindo cancelamento e deadline durante a conversão.

As APIs de linhas `stream` e `stream_async` entregam lotes com a mesma conversão
de tipos. A criação do context manager é lazy; o trabalho começa ao entrar nele.

```python
with pool.stream("SELECT ID, LABEL FROM MINHA_TABELA", batch_rows=1000) as batches:
    for batch in batches:
        processar(batch)

async with pool.stream_async("SELECT ID, LABEL FROM MINHA_TABELA") as batches:
    async for batch in batches:
        await processar_async(batch)
```

`batch_rows=None` usa `fetch_size`. `batch_bytes` também limita o tamanho do lote
considerando estruturas decodificadas; uma linha maior pode formar um lote único.
`prefetch_batches` limita lotes antecipados além do corrente; zero desativa essa
antecipação. O primeiro fetch espera demanda. Limites totais de materialização
não são aplicados ao stream; guardar todos os lotes continua consumindo memória.

Use `with`/`async with`, ou `Stream.close()`/`AsyncStream.aclose()`, para liberar
o cursor em uma parada antecipada. Há um consumidor exclusivo por stream,
incluindo a conversão Python. `query_timeout` cobre consumo e conversão;
`stream_idle_timeout` limita a espera do produtor parado sem nova demanda.
`timeout=None` remove o prazo da consulta, mantendo o de ociosidade configurado.
Erros finais de cleanup são propagados antes de indicar fim do stream.

A pool repõe `min_idle` após reciclagem ou descarte, dentro de `pool_size`,
com espera crescente após falha de conexão. O handshake verifica role, charset,
timezone e isolamento efetivos antes de disponibilizar o attachment. Compressão
é negociada com o servidor; pedir compressão não comprova que ela esteja ativa.

`snapshot()` e `snapshot_async()` reservam uma conexão para uma transação
READ ONLY SNAPSHOT. A sessão oferece as mesmas consultas e streams já disponíveis:

```python
with pool.snapshot(timeout=120.0) as session:
    total = session.fetch_val("SELECT COUNT(*) FROM MINHA_TABELA")
    with session.stream("SELECT ID FROM MINHA_TABELA") as batches:
        for batch in batches:
            processar(batch)

async with pool.snapshot_async(timeout=120.0) as session:
    rows = await session.fetch_all_async("SELECT ID FROM MINHA_TABELA")
```

O prazo da sessão inclui intervalos entre consultas e conversão Python; omitido,
usa `query_timeout`, e `None` remove o prazo da sessão. Cada consulta também tem
seu próprio prazo: `timeout=None` nela não remove o prazo da sessão. Uma sessão
aceita apenas uma operação por vez, incluindo conversão e um stream aberto.
Chamadas diretamente na pool usam outras transações. No snapshot, as tabelas
`MON$` também representam a fotografia de monitoramento dessa transação.

Use o context manager ou `Snapshot.close_sync()`/`await Snapshot.close()` para
encerrar. Erro de execução ou cancelamento encerra a sessão; uma consulta posterior
não abre outra transação implicitamente. O slot só volta à pool após cleanup.

`pool.clear_statement_cache_sync()` e `await pool.clear_statement_cache()`
invalidam os statements de todos os attachments sem interromper cursores em uso.
O prazo padrão é `query_timeout`; `timeout=None` remove esse prazo. Cancelar ou
esgotar a espera não desfaz a invalidação solicitada: o worker a conclui quando
o cursor termina. Antes de migrar schema, pare novas consultas, aguarde as atuais
e limpe os caches, ou feche e recrie a pool; statements preparados podem reter
dependências de metadata. A biblioteca não executa migrações.

Cancelamento ativo usa uma thread de controle limitada por worker, com geração
por operação. Ctrl+C e timeout cancelam a consulta sem encerrar o cliente inteiro;
o attachment cancelado é descartado após cleanup. O prazo inclui a conversão
para Python. Sinais e recuperação após cancel foram testados no alvo local.

Arrow/Polars materializados e streaming Arrow estão disponíveis; extração
paralela ainda falta, além da homologação completa. O estado verificado fica
em `docs/progress.md`.
Em attach sem resposta ou partição
de rede, o chamador pode retornar antes da chamada nativa; o slot permanece
contado até o cleanup terminar. Fechar a pool também respeita um prazo para o
chamador, sem prometer encerrar threads nativas presas.

O consumidor precisa da mesma arquitetura, SO e ABI de Python. Um artefato de
CPython 3.14 não serve para 3.15. Extras Arrow/Polars precisam estar disponíveis
no consumidor quando essas saídas forem utilizadas.

Para atualizar, pare os processos consumidores, atualize a dependência e reinicie.
No modo copiável, substitua a pasta inteira. Não misture arquivos de builds
diferentes nem substitua uma dylib carregada.
Nenhum script publica ou faz upload de pacotes.

## Arrow e Polars materializados

Instale `fbfast[arrow]` para `fetch_arrow` (PyArrow >= 26), ou `fbfast[polars]`
para `fetch_polars` (Polars >= 2). Polars recebe os buffers nativos diretamente
pelo protocolo Arrow e não exige instalar PyArrow. O pacote básico continua
importável sem nenhum dos dois extras.

```python
table = pool.fetch_arrow("SELECT ID, VALOR FROM MINHA_TABELA")
frame = await pool.fetch_polars_async("SELECT ID, VALOR FROM MINHA_TABELA")
```

As variantes sync/async também existem em snapshots e nos atalhos de pools
nomeadas. O Rust constrói as colunas sem uma lista intermediária de objetos
Python. A conversão final assíncrona executa fora do event loop. O resultado
completo permanece em memória e respeita `max_result_rows`.

Arrow conserva tipos descritos, mesmo com resultado vazio ou primeiro valor
NULL. Campos incluem metadados SQL (`fbfast.sql_type`, `scale`, `subtype`,
`charset`, `storage_bytes`). A precisão decimal representa a capacidade do
armazenamento descrito pelo cliente: INT64 usa até 19 dígitos e INT128, 39.
INT128 e NUMERIC/DECIMAL armazenados em INT128 usam Decimal256, sem passar por
float. DECFLOAT usa texto exato, inclusive sinal/coorte e valores especiais.
TIME/TIMESTAMP WITH TIME ZONE usam structs com campos UTC e nome da zona.

Polars usa `rechunk=False` e o schema recebido é conferido. Decimal256 falha
antes do fetch por padrão; `unsupported="string"` permite explicitamente texto
decimal exato nessas colunas. Outros tipos não homologados continuam gerando
erro. Aliases duplicados exigem aliases SQL distintos. O resultado mantém os
buffers vivos depois do fechamento da pool.

## Streaming Arrow

`stream_arrow` e `stream_arrow_async` entregam `ArrowBatch` sem exigir PyArrow
ou Polars. Cada lote possui `num_rows`, `num_columns`, `nbytes` (memória dos
arrays), `schema` e exportação `__arrow_c_array__`. `ArrowSchema` oferece `names`
e `__arrow_c_schema__`; o schema está disponível logo após entrar no contexto,
inclusive quando a consulta não retorna linhas.

```python
async with pool.stream_arrow_async("SELECT ID, VALOR FROM MINHA_TABELA", batch_rows=2000) as stream:
    async for batch in stream:
        processar_lote_arrow(batch)

# Com o extra arrow instalado:
import pyarrow as pa
with pool.stream_arrow("SELECT ID, VALOR FROM MINHA_TABELA") as stream:
    reader = pa.RecordBatchReader.from_stream(stream)
    for batch in reader:
        processar_lote_pyarrow(batch)
```

O C Stream é bloqueante e tem um único consumidor: depois de exportá-lo, não
se pode exportar novamente nem iterar pelo ArrowStream Python. A versão async
usa o scheduler para entregar lotes C Array e não exporta um C Stream bloqueante.
`pa.record_batch(batch)` importa um ArrowBatch; cada exportação do lote cria
novas capsules, cada uma consumível uma única vez.

`batch_rows=None` usa `fetch_size`; `batch_bytes` limita o lote nativo, permitindo
uma linha isolada maior que esse orçamento. BLOBs mantêm seu limite por célula.
`max_result_rows` não se aplica ao streaming. O produtor só avança por demanda
e pelo prefetch configurado. Além dos lotes produzidos, há o buffer de fetch e
um builder reservado. Lotes retidos pelo consumidor permanecem válidos depois
do fechamento do stream/pool e contam como memória do consumidor.

Sair do contexto espera o cleanup até `close_timeout`. Fechar explicitamente
ArrowStream durante uma leitura C Stream em outro thread cancela essa leitura
e espera a limpeza. O consumidor estrangeiro recebe a exceção do seu próprio
pacote, com o diagnóstico fbfast (tipo, fase, SQLSTATE e códigos nativos).
Os handlers Python de sinais são respeitados durante a espera bloqueante.
Fechar a pool segue a política geral de shutdown limitado por prazo: trabalho
ativo continua contado até terminar ou ser cancelado pelo seu proprietário.

## JSON nativo

`fetch_json`/`fetch_json_async` devolvem `bytes` contendo um array JSON UTF-8
compacto; resultado vazio é `b"[]"`. O Rust serializa diretamente do decoder,
sem criar uma tabela de objetos Python nem uma árvore JSON intermediária.
Pool, snapshot e atalhos do módulo oferecem a mesma API e os mesmos limites.

```python
data = await pool.fetch_json_async("SELECT ID, VALOR FROM MINHA_TABELA")

async with pool.stream_json_async(
    "SELECT ID, VALOR FROM MINHA_TABELA", format="ndjson", timeout=None
) as chunks:
    async for chunk in chunks:
        await enviar(chunk)
```

| Opção | Padrão | Alternativa |
|---|---|---|
| `orient` | `"records"`: objetos com aliases únicos | `"values"`: arrays, permite nomes repetidos |
| `json_integers` | `"safe"`: BIGINT/INT128 como strings por tipo da coluna | `"number"`: número JSON exato, sujeito à precisão do consumidor |
| `json_decimals` | `"string"`: decimal exato | `"number"`: texto numérico exato, sem float intermediário |
| `json_binary` | `"error"`: rejeita valores binários | `"base64"`: string base64 explícita |
| `json_non_finite` | `"error"`: rejeita NaN e infinitos | `"null"`: converte esses valores em null |

SMALLINT/INTEGER são números; NULL/bool têm seus equivalentes JSON. Texto mantém
Unicode e escaping JSON. Datas e horários usam ISO 8601, sem offset inventado.
BLOBs textuais seguem a política de charset; binários exigem `json_binary="base64"`.
Tipos SQL com timezone preservam os campos UTC e zona em objetos JSON,
conforme a seção de tipos temporais abaixo.

`stream_json`/`stream_json_async` entregam chunks próprios, válidos após fechar o
contexto. `format="ndjson"` preserva registros completos com newline; um registro
maior que `batch_bytes` ocupa sozinho um chunk. `format="array"` limita cada
chunk a `batch_bytes` e pode dividir o documento inclusive no meio de um caractere
UTF-8: concatene bytes ou use decoder incremental, sem decodificar cada chunk
isoladamente. A concatenação final é um array JSON completo. `batch_rows` controla
o fetch nativo, não promete um número exato de registros por chunk codificado.

O fechamento `]` só é enviado após cleanup bem-sucedido; erro/cancelamento não
fecha artificialmente um array parcial. O mesmo controle de demanda/prefetch
limita a produção. A memória temporária inclui o lote nativo e um registro
codificado, além dos chunks entregues/prefetch. Materialização completa também
faz uma cópia final para `bytes` Python; essa cópia verifica prazo/cancelamento e
libera o GIL em blocos. `max_result_rows` aplica-se a fetch_json, não aos streams.

## Ambiente de validação

```sh
.venv/bin/python scripts/firebird_fixture.py start
.venv/bin/python scripts/test_native.py
.venv/bin/python scripts/test_python.py
.venv/bin/python scripts/verify_install.py
.venv/bin/python scripts/benchmark_json.py --samples 3
.venv/bin/python benchmarks/run_native_probe.py
.venv/bin/python benchmarks/run_cancel_probe.py
.venv/bin/python scripts/firebird_fixture.py stop
```

O harness usa exclusivamente um container descartável identificado por label,
imagem Firebird 5.0.3 fixada por digest e porta TCP em localhost. As credenciais
geradas ficam em `.tools/` com permissão 0600. Não aceita um banco operacional
como alvo de DDL ou benchmark. O protótipo C++ é uma referência nativa separada;
seus números não representam o desempenho da API Python.

`verify_install.py` cria dois consumidores uv e um Git temporário local, sem
remoto, para provar instalação por wheel e por commit dos fontes. Esses comandos
locais não publicam. A publicação é feita pelo workflow `release.yml` quando o
autor envia uma tag de versão e todos os builds/testes passam.


## BLOBs

Linhas, streams, snapshots e JSON leem BLOBs inteiros dentro da transação.
BLOB binário vira `bytes`; BLOB TEXT com charset conhecido é transliterado para
UTF8 pelo cliente oficial e vira `str`. `NONE` permanece `bytes`, salvo
`fallback_charset="UTF8"` explícito; `OCTETS` permanece `bytes`. Conteúdo inválido
não recebe replacement, e `trim_char` não altera BLOBs. Subtipos diferentes de
BINARY (0) e TEXT (1) geram `UnsupportedTypeError`, inclusive em resultados vazios.

`max_blob_bytes` limita cada BLOB (padrão 64 MiB), contando bytes efetivamente
lidos, após eventual transliteração. Exceder o limite gera `ResultLimitError`,
sem conteúdo truncado. Esse limite é independente do limite total de linhas.
Streams com BLOBs entregam uma linha por lote nesta implementação; uma linha
pode exceder `batch_bytes`, mas cada BLOB obedece ao seu limite. Os resultados
retidos continuam válidos após fechar a pool. Nenhum handle preguiçoso é exposto.
Parâmetros BLOB não são criados implicitamente para fazer bind.


## DECFLOAT

`DECFLOAT(16/34)` retorna `decimal.Decimal`, sem passar por float nem arredondar
pelo contexto Decimal do Python. A conversão usa os utilitários oficiais do
cliente. Mantém zeros com sinal, subnormais, infinitos, NaN e sNaN com payload.

No bind, envie `Decimal` ou `int` para um parâmetro DECFLOAT. A biblioteca confere
a conversão de volta e rejeita perda de precisão, overflow, underflow e perda de
payload com `DataConversionError`, identificando o parâmetro sem imprimir seu
valor. Zeros finais insignificantes podem mudar de expoente para caber na precisão,
mas o valor não é arredondado. NUMERIC/DECIMAL fixos continuam rejeitando especiais
e valores que não cabem exatamente na escala preparada.

JSON usa strings para DECFLOAT finito por padrão; `json_decimals="number"`
escreve números decimais exatos, inclusive expoentes grandes, cuja interpretação
fica a cargo do consumidor. NaN e infinitos exigem `json_non_finite="null"`, mesmo
quando `json_decimals="string"`. Streams e snapshots usam as mesmas regras.

## Tipos temporais com fuso

`TIMESTAMP WITH TIME ZONE` retorna `datetime` aware: offsets fixos usam
`datetime.timezone`; regiões usam `zoneinfo.ZoneInfo`. A conversão conserva
o instante UTC, o horário local e `fold` nas repetições de horário de verão.
Se a zona estiver ausente no Python ou suas regras divergirem das do cliente
Firebird, a leitura em linhas gera `DataConversionError`.

`TIME WITH TIME ZONE` retorna `fbfast.TimeWithZone(utc_time, zone)`, imutável e
hashable. `utc_time` é um `datetime.time` sem tzinfo que representa o horário UTC;
`zone` é o nome da região ou offset. O objeto não inventa uma data para deduzir
horário local. A própria semântica Firebird usa regras de 2020-01-01 para converter
horários locais desse tipo no servidor.

```python
from datetime import datetime, time
from zoneinfo import ZoneInfo
from fbfast import TimeWithZone

# fold=1 seleciona a segunda ocorrência desse horário em Nova York.
stamp = datetime(2024, 11, 3, 1, 30, tzinfo=ZoneInfo("America/New_York"), fold=1)
clock = TimeWithZone(time(17), "America/New_York")
row = pool.fetch_one(
    "SELECT CAST(? AS TIMESTAMP WITH TIME ZONE), CAST(? AS TIME WITH TIME ZONE) "
    "FROM RDB$DATABASE", [stamp, clock],
)
```

No bind, datetimes aware aceitam `ZoneInfo` com nome ou `datetime.timezone`.
Horários locais inexistentes, UTC fora do domínio, offsets fixos fora de ±14h
ou com segundos e perda de precisão temporal geram erro. A precisão é de
100 microssegundos. `TimeWithZone.utc_time` exige time sem tzinfo e fold=0;
um time aware comum não é adaptado implicitamente. Tipos SQL sem fuso continuam
aceitando apenas valores sem fuso, sem aplicar `session_time_zone` a eles.

JSON conserva `{"utc":"2024-11-03T06:30:00Z","zone":"America/New_York"}`
para timestamp e `{"utc_time":"17:00:00","zone":"America/New_York"}` para
horário. Esses caminhos não constroem objetos temporais Python nem dependem
do banco de zonas do Python. Streams e snapshots têm o mesmo contrato.
