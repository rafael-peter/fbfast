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
As variantes Python 3.14 também repetem o smoke contra servidor 5.0.4, mantendo
o cliente 5.0.3 empacotado em todas as plataformas.

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

Depois de atualizar a mesma versão em `pyproject.toml`, `Cargo.toml` e
`python/fbfast/__init__.py`, preparar `docs/releases/<versão>.md` e executar:

```sh
./scripts/release.py
```

Esse comando mostra todos os arquivos que entrarão no commit e pede uma única
confirmação. Em seguida ele valida a versão, cria o commit, envia `main`, aguarda
o workflow de build/teste, cria e envia a tag, aguarda a publicação no PyPI e
cria a GitHub Release com as notas da versão. `--yes` elimina a confirmação e
`--dry-run` valida o estado sem modificar Git ou serviços externos.

O processo pode ser executado novamente depois de uma interrupção: commit, push,
tag e GitHub Release já existentes são reconhecidos. Tags publicadas são
imutáveis; se uma execução do workflow de release falhar, corrija ou reexecute o
workflow no GitHub e rode o comando novamente para concluir a página da release.
O computador precisa ter `git`, `gh` autenticado e acesso ao remote `origin`.

O autor confirmou o cadastro do pending publisher em 16/10/2026. Esse estado
é esperado antes da primeira publicação: o primeiro upload cria o projeto e
converte o cadastro em publisher ativo. Não é uma aprovação manual pendente.
O estado de validação por plataforma e publicação é registrado em `progress.md`.

A primeira publicação foi concluída em 16/10/2026:
[PyPI 0.1.0](https://pypi.org/project/fbfast/0.1.0/) e
[GitHub Release v0.1.0](https://github.com/rafael-peter/fbfast/releases/tag/v0.1.0).
