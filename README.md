# fbfast

Biblioteca de leitura para Firebird 5, com core Rust e ponte para o cliente oficial.
Implementação em andamento conforme `plano_fbfast_final.md`. Ainda não oferece
a API completa de consultas; acompanhe a evidência por fase em `docs/progress.md`.

Primeiro alvo: **macOS arm64, CPython 3.14 com GIL**. Outros alvos ainda não homologados.

## Instalar com uv

A distribuição principal passa a ser um pacote versionado, com fontes e tags
no Git. Um wheel pronto dispensa Rust, C++ e instalação local do Firebird no
consumidor. Ainda requer um servidor Firebird acessível para executar consultas.

```sh
uv add /caminho/para/fbfast-0.1.0-cp314-cp314-macosx_11_0_arm64.whl
```

`uv pip install arquivo.whl` e `pip install arquivo.whl` também funcionam.
`uv add` registra a dependência e atualiza o lock do projeto. Instalar de um
checkout (`uv add /caminho/para/firebird_rust`) ou do Git compila no computador
consumidor e exige Rust 1.91.1, C++ e SDK Firebird. O endereço e a primeira tag
do repositório público ainda não foram definidos.

Quando o repositório estiver disponível, substitua os marcadores abaixo:

```sh
uv add "git+https://github.com/rafael-peter/fbfast.git" --rev COMMIT
# Alternativa sem registrar dependência no pyproject.toml:
pip install "fbfast @ git+https://github.com/rafael-peter/fbfast.git@COMMIT"
```

Fixar um commit ou uma tag evita atualizar a biblioteca implicitamente a cada
instalação. Releases com wheels permitirão instalar sem toolchain no consumidor.

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

## Consultas

Já estão implementados
`Pool`, `fetch_all`, `fetch_one`, `fetch_val`, `explain` (com params), `stats`,
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

Ainda faltam a API pública de snapshot, pools nomeadas, Arrow/Polars, JSON,
DECFLOAT/timezone/BLOB e exportação paralela. Em attach sem resposta ou partição
de rede, o chamador pode retornar antes da chamada nativa; o slot permanece
contado até o cleanup terminar. Fechar a pool também respeita um prazo para o
chamador, sem prometer encerrar threads nativas presas.

O consumidor precisa da mesma arquitetura, SO e ABI de Python. Um artefato de
CPython 3.14 não serve para 3.15. Extras Arrow/Polars precisarão estar disponíveis
no consumidor quando suas APIs forem implementadas.

Para atualizar, pare os processos consumidores, atualize a dependência e reinicie.
No modo copiável, substitua a pasta inteira. Não misture arquivos de builds
diferentes nem substitua uma dylib carregada.
Nenhum script publica ou faz upload de pacotes.

## Ambiente de validação

```sh
.venv/bin/python scripts/firebird_fixture.py start
.venv/bin/python scripts/test_native.py
.venv/bin/python scripts/test_python.py
.venv/bin/python scripts/verify_install.py
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
remoto, para provar instalação por wheel e por commit dos fontes. Nenhum script
publica pacotes ou cria releases remotas.
