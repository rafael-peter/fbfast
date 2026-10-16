# Wheels e publicação no PyPI

A API atual é o escopo desta entrega. O workflow gera wheels com extensão Rust,
cliente Firebird e recursos necessários; o consumidor instala com `uv add fbfast`
ou `pip install fbfast`, sem compilar Rust/C++ ou instalar Firebird localmente.
Instalação por Git/sdist continua exigindo toolchain e SDK durante o build.

Matriz configurada, CPython 3.11, 3.12, 3.13 e 3.14 **com GIL**:

| SO | Arquitetura | Runner/base |
|---|---|---|
| macOS | arm64 | `macos-15` |
| macOS | x86_64 | `macos-15-intel` |
| Linux glibc | x86_64 | manylinux 2.28 em `ubuntu-24.04` |
| Linux glibc | aarch64 | manylinux 2.28 em `ubuntu-24.04-arm` |
| Windows | x86_64 | `windows-2022`, MSVC |

São 20 wheels e um sdist. Linux musl/Alpine, Windows arm64, Python free-threaded
e versões fora da matriz não são anunciados como suportados.

`build-wheels.yml` roda em pull requests, pushes para `main`, manualmente e como
workflow reutilizável de release. Cada job usa SDK oficial 5.0.3 com SHA256 fixado,
compila release, verifica a pasta copiada, instala o wheel exato e consulta um
servidor Firebird descartável criado a partir de sua própria extração do SDK.
O smoke inclui Unicode, NULL, Decimal, float, timezone/ICU, JSON, Arrow, Polars,
streaming, async e transação read-only efetiva. Nenhum banco externo é usado.

Ações e imagens manylinux estão fixadas por commit/digest. O Linux audita o wheel
inteiro após incluir as bibliotecas dinâmicas. O job de publicação exige a matriz
completa, verifica versão/tag e hashes internos e só então entrega os artefatos
ao PyPI. Um build falho impede a publicação.

## Configuração única na conta PyPI

O projeto/repositório conferido é `rafael-peter/fbfast` (o remote antigo
`rafaelpeter03/fbfast` redireciona para ele). Não é necessário compartilhar token
ou senha. Na conta PyPI do proprietário, cadastrar um GitHub Trusted Publisher:

| Campo | Valor |
|---|---|
| PyPI project name | `fbfast` |
| Owner | `rafael-peter` |
| Repository name | `fbfast` |
| Workflow name | `release.yml` |
| Environment name | `pypi` |

Se o projeto ainda não existe, usar um **pending publisher** na página de
[publishing da conta](https://pypi.org/manage/account/publishing/). Se já existe,
adicionar o publisher na configuração de Publishing do projeto. O nome precisa
estar disponível ou pertencer à conta; este documento não comprova esse vínculo.
O environment `pypi` já foi criado no GitHub; regras de proteção/aprovação podem
ser configuradas pelo proprietário conforme seu fluxo de releases.

O job `publish` tem `id-token: write`, usa OIDC e gera atestações. Nenhum secret
`PYPI_API_TOKEN` é necessário. Referências:
[cadastro](https://docs.pypi.org/trusted-publishers/adding-a-publisher/) e
[uso do publisher](https://docs.pypi.org/trusted-publishers/using-a-publisher/).

## Publicar uma versão

1. Atualizar a mesma versão em `pyproject.toml`, `Cargo.toml` e
   `python/fbfast/__init__.py`; atualizar o lock Cargo se necessário.
2. Commitar/revisar as alterações e enviar para `main`. Conferir o workflow de
   build/teste antes de criar a tag.
3. Criar/enviar a tag `v<versão>`, por exemplo `v0.1.0`. `release.yml` recompila,
   testa a matriz inteira e publica os artefatos dessa tag.
4. Conferir o job `publish` e a página do projeto no PyPI. Só a partir daí a versão
   pode ser considerada publicada e instalada de lá.

Não houve commit/push, criação de tag ou upload PyPI pelo agente. Workflows locais
e smoke local não comprovam jobs remotos nem publicação. O estado de validação
por plataforma é registrado em `progress.md`.
