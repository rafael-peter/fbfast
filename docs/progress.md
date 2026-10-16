# Evidência de implementação

## Security and release update 0.2.1 — 2026-10-16 (local, unpublished)

- Restored `max_result_bytes` with a 512 MiB default, explicit `None`, checked
  accounting for rows, JSON, Arrow/Polars and Pydantic inputs, and no cumulative
  quota on streams. Cleanup and pool reuse after limit failures passed against
  the disposable Firebird fixture.
- Updated PyO3, `pyo3-async-runtimes` and `time`; shared credentials through a
  zeroizing Rust buffer and zeroized the temporary native attach buffer. The
  pinned `cargo-audit 0.22.2` scanned 123 dependencies with no advisory.
- Local macOS arm64 / CPython 3.14 validation passed: **264 Python integration
  tests**, **31 live Rust/C++ tests**, **27 packaging tests**, and **25 model
  tests** with 2 SDK-only skips. Workspace tests, Clippy with warnings denied,
  Rustfmt, Ruff, Mypy, module boundaries, actionlint, fuzz-target compilation,
  vendor inventory, release guards, sdist build, and `twine check` also passed.
- Rebuilt wheel `fbfast-0.2.1-cp314-cp314-macosx_11_0_arm64.whl`; verified build
  ID `8dce4228de94097a191a84ed490544cb5044114b3486f69baa8a0d90f0c9c093`.
  The installed wheel passed the same smoke, including a named time zone and DST
  fold, against disposable Firebird 5.0.3 and 5.0.4 servers.
- A wheel with the Firebird 5.0.4 client failed the named-time-zone contract
  against a 5.0.3 server. The release therefore keeps the 5.0.3 client and tests
  it against both server versions. This is local compatibility evidence, not
  homologation against an operational database.
- Added RustSec, Linux ASan/UBSan, and deterministic fuzz gates to CI. The
  corrected sanitizer runner linked both runtimes and loaded the official 5.0.3
  client through the C++ bridge inside a Linux container with no finding.
- The resumable `./scripts/release.py` flow created and pushed release commit
  `787a3da`. Its first CI run exposed missing sanitizer runtime linkage and the
  absent Windows `tzdata` dependency before any tag, PyPI upload, or GitHub
  Release was created; this follow-up corrects both failures.

## Typed results and documentation — 2026-10-16 (local, unpublished)

- Added optional Pydantic v2 `model=` for materialized row queries, sync/async,
  snapshots and named-pool helpers. Static overloads infer the concrete model;
  async validation runs outside the event loop, after native fetching.
- Local release wheel rebuilt and verified on macOS arm64 / CPython 3.14.
  Build ID: `a7334008fa6093ae26e65322ad2a7b985ec596698d0a48c9a916ee5fd70f4b10`.
  Model and packaging checks: **47 passed, 2 SDK checks skipped**. Mypy (library
  and consumer assertions), Ruff, module boundaries, actionlint, and diff checks
  passed. These are local checks, not a new published release or remote CI run.
- Real-server attempts were blocked before connecting: Docker socket access was
  denied and the private SDK fixture could not bind a loopback port in the
  sandbox. The new database integration checks have not passed locally. Wheel
  CI now includes typed-row tests and real Pydantic smoke queries for its matrix.
- Conversion-only benchmark (`benchmarks/run_models.py`, nine samples per case,
  prepared dictionaries, CPython 3.14.5 / Pydantic 2.14.1): 100,000 rows had median
  102.24 ms for manual construction, 111.22 ms for manual `model_validate`, and
  109.74 ms for `model=`. At 1,000 rows: 0.389 / 0.450 / 0.439 ms respectively.
  This excludes SQL, network, native decoding and dictionary allocation; it is
  not an end-to-end performance claim. Raw samples: `.tools/models-benchmark.json`.
- Rewrote the README in English, moved surviving plan requirements into
  `docs/roadmap.md`, and removed the superseded root plan (available in Git
  history). Build storage policy is in `docs/development.md`. `target/` (3.3 GB)
  and `dist/` (1.9 GB) remain local and ignored; no build artifacts are tracked
  and none were deleted in this change.

O autor revisou o escopo em 15/10/2026: a API atual atende seu uso e a entrega
ativa é distribuição por SO e integração PyPI. P5 e a matriz extensa de métricas/
benchmarks ficam como backlog, conforme `docs/decisions/0018-release-scope.md`.
As fases P0–P6 abaixo registram o estado histórico da especificação. Este registro
preserva o histórico e incorpora os ajustes expressamente pedidos pelo usuário:
distribuição uv/pip, remoção do limite total de bytes e foco atual em release,
documentados nas decisões 0004, 0009 e 0018.

Entrega de distribuição concluída em 16/10/2026:

- A tag `v0.1.0` aponta para `06da0cd9de0125a087b99c04731417088fab399b`.
  O workflow de release
  [38915993858](https://github.com/rafael-peter/fbfast/actions/runs/38915993858)
  terminou com **22 jobs aprovados, zero falhas**: 20 wheels, sdist e publicação.
- [PyPI 0.1.0](https://pypi.org/project/fbfast/0.1.0/): API pública confirmou
  os 20 wheels e um sdist, sem arquivos yanked. O upload via Trusted Publishing
  terminou em 16/10/2026 às 14:39 UTC, após `check_release.py` e `twine check`.
- [GitHub Release v0.1.0](https://github.com/rafael-peter/fbfast/releases/tag/v0.1.0)
  publicada, com notas, instalação e link para os wheels no PyPI.
- Consumidor isolado neste macOS arm64/CPython 3.14.5 instalou `fbfast==0.1.0`
  diretamente de `https://pypi.org/simple`, usando `uv`, sem cache e exigindo
  wheel. O smoke real passou com cliente embarcado, Unicode, Decimal, NULL,
  timezone/ICU, JSON, Arrow/Polars, streams sync/async e transação read-only.
  Build ID: `749146ffd627d173ed1f5fd86850303acc8b6e89e136230adbce76ae321d75ef`.
  Evidências locais: `.tools/pypi-v0.1.0.json`, `.tools/publish-v0.1.0.log`
  e `.tools/pypi-consumer-smoke.log`.

Preparação e validação anteriores à publicação:

- `.github/workflows/build-wheels.yml`: 20 wheels (macOS arm64/x86_64, Linux
  aarch64/x86_64, Windows x86_64; CPython 3.11–3.14 com GIL), mais sdist.
- `.github/workflows/release.yml`: tag deve coincidir com versão; todos os builds
  e smokes devem passar; gate confere matriz completa e hashes antes de PyPI OIDC.
- `actionlint 1.7.12` passou. Em 16/10/2026 o autor commitou e enviou `c10f0f9`,
  iniciando o CI. A execução revelou conflitos de ferramentas Rust, linker do
  Git Bash e duas diferenças Windows (include C++ e limpeza de DLL carregada).
  As correções foram enviadas em `4ca8855`, `4c0c9c9` e `80094bf`; a rodada
  [38912311995](https://github.com/rafael-peter/fbfast/actions/runs/38912311995)
  concluiu com **21 jobs aprovados, zero falhas** em `80094bf`: os 20 wheels e
  o sdist. Cada wheel passou por inventário/relocação, instalação e consultas
  reais no servidor descartável, incluindo Windows e variantes Intel. Os jobs
  CPython 3.14 também executaram os testes Rust unitários/propriedades. A etapa
  Linux aarch64/3.11 demorou na instalação de ferramentas, mas terminou com
  sucesso na mesma tentativa. Nessa rodada, ainda não havia tag ou publicação PyPI.
- Os 21 artefatos desse run foram baixados em `dist/ci-38912311995/` e reunidos
  em `dist/release-38912311995/`. `check_release.py --artifacts` passou com
  `GITHUB_REF_TYPE=tag` e `GITHUB_REF_NAME=v0.1.0`: matriz completa, versão e
  hashes conferidos. `twine check` passou nos 20 wheels e no sdist. A revisão
  posterior só atualiza README/registro de evidências; seu sdist também passou
  no `twine check`. O workflow de release recompila e confere a tag antes do upload.
- O autor confirmou por imagem o pending Trusted Publisher com projeto `fbfast`,
  repositório `rafael-peter/fbfast`, workflow `release.yml` e environment `pypi`.
  O cadastro coincide com o workflow; ativação depende do primeiro upload.
- O environment `pypi` foi criado e consultado pela API GitHub em
  `rafael-peter/fbfast`, sem regras opcionais de proteção. Isso prepara o destino
  do job de publicação; não configura o Trusted Publisher na conta PyPI.

Evidências locais anteriores ao CI completo acima:

- Wheel final desta revisão em macOS arm64/CPython 3.14, build
  `2a3b6ed4de16f11c877af34ec279d832596b03db461bf8456f541e8989c3e2f6`,
  passou por inventário/relocação, 22 testes de empacotamento/config/guards e smoke
  CI real em servidor descartável do SDK extraído. Instalações uv de wheel e Git
  temporário produziram o mesmo build ID e passaram com/sem extras Arrow/Polars.
  Clippy, fmt, ruff, diff-check e actionlint passaram. Logs `.tools/release-*`.
- A mesma revisão passou também em macOS arm64/CPython 3.11.17, 3.12.11 e
  3.13.16: build release, inventário/relocação em processo isolado, instalação
  uv do wheel e smoke CI real com runtime embarcado. Build IDs, respectivamente:
  `5715b68bcc5088660b597549af9857e9f07cd1346e5c3adf66885b2afea59d55`,
  `bdbfdb8047ca74cc85a989e9f34b2313aa4193008b8e3d3ba6f837f026ecd686` e
  `df1850ce4b0d1a9c133d39d385bebf32d07821e12f166ed8a37c3b6abb44acc4`.
  Logs `.tools/macos-cp311.log`, `.tools/macos-cp312.log` e
  `.tools/macos-cp313.log`. Isso completa a matriz local macOS arm64 3.11–3.14;
  naquela etapa, a execução remota dos workflows ainda estava pendente.
- Wheel final desta revisão em Linux aarch64/CPython 3.14, build
  `1c25d36de7d3cd3cfea17c192be771277030f7120d0c03b1db9388e8b51729f1`,
  compilou e passou por auditwheel manylinux 2.28, inventário/relocação, instalação
  e smoke real (escalares, timezone/ICU, JSON, Arrow/Polars, streams sync/async e
  TPB efetivo). Log `.tools/linux-release-final.log`. Nessa etapa local, Windows e
  variantes x86_64 ainda aguardavam a execução de seus próprios jobs.
- Linux aarch64/CPython 3.11, 3.12 e 3.13 também passaram pelo build release,
  auditwheel manylinux 2.28, inventário/relocação, instalação pip e smoke CI real.
  Build IDs, respectivamente:
  `9f36abada5e076fe4b1a7ba81be46cf26580a31e1796d3dc590c3c1eadb3400f`,
  `b5456b34b4332bafbf8e01510bb92eced4c0228605bc108a4c85f6a5d57ba490` e
  `1d705dc1ffeb768d6b1223c268c321f91a6df2790113f86587915eeab269aa7a`.
  Logs `.tools/linux-cp311.log`, `.tools/linux-cp312.log` e
  `.tools/linux-cp313.log`. A matriz ARM64 local está verificada em ambos os SOs
  para os quatro Pythons; ainda não representa uma execução do GitHub Actions.
- O comando CI `cargo test -p fbfast-core -p fbfast-native --lib --tests` passou
  no mesmo container Linux aarch64: 34 testes executados, zero falhas e 31 testes
  de integração opt-in ignorados, conforme esperado por esse comando. Log
  `.tools/linux-unit-ci.log`. Os smokes reais dos wheels são evidência separada.
  `twine check` passou também nos três wheels Linux adicionais.
- `uv build --sdist` passou; o arquivo contém helpers de todas as plataformas,
  registry dos SDKs e stubs. O grafo de licenças Rust do target Windows também foi
  coletado com sucesso, sem alegar que isso equivale a executar o binário Windows.
  `twine check` passou nos dois wheels e no sdist. Consulta ao PyPI retornou 404
  para `fbfast`; não havia projeto publicado na data da verificação. Cadastro
  de pending Trusted Publisher ainda aguardava confirmação naquela etapa.

A rodada comparativa já iniciada terminou com **180 amostras válidas/zero erros**:
`benchmarks/results/20261015T175028Z-comparison/`. Há fontes congelados, manifesto,
hashes, planos, JSON/CSV e relatório; 9 testes do oráculo passaram também com `-O`.
Para 100 mil linhas/2 inteiros, medianas fbfast vs firebird-driver ajustado:
tuplas 0,5403/0,5157 s; JSON 0,5297/0,5482 s; Arrow 0,5290/0,5344 s;
Polars 0,5367/0,5394 s. RSS JSON 36,5/75,6 MiB, Arrow 47,8/77,0 MiB,
Polars 61,6/93,5 MiB. Não houve ganho 2–5x de tempo nesses workloads; o probe C++
equivalente de leitura bruta levou 0,5402 s/100 mil e 5,3172 s/1 milhão, apontando
domínio do custo servidor/fetch. Ele não produz os mesmos objetos finais.
FDB 2.0.4 original falhou na chamada variádica em Apple arm64; `fdb-arm64` identifica
explicitamente o adaptador com correção de ABI, sem alterar o pacote instalado.
O preflight `20261015T174237Z` foi marcado como excluído de aceitação porque o
harness mudou durante a execução; a rodada final congela fontes em `harness.zip`.
Novos benchmarks e extração paralela não bloqueiam a distribuição solicitada.

| Fase | Estado | Evidência exigida / restante |
|---|---|---|
| P0 | Verificada no alvo local | Release, runtime, manifesto, hashes, cópia externa e instalação uv por wheel/Git local; repositório público existente, revisão remota atual não validada como entrega final |
| P1 | Parcial | Attach/TPB/prepare/describe/bind/fetch/cleanup e cancel independente Rust+C++ reais, tipos escalares, cache, JSON e Arrow materializados/streaming, Polars materializado; faltam baselines com outros drivers |
| P2 | Parcial | Pool de workers, fila limitada, APIs sync/stream, explain e política lexical+TPB; faltam matriz de tipos e homologação completa de lifecycle |
| P3 | Parcial | API async e sync, snapshot público, pools nomeadas, HTTP com linhas/JSON nativo, stats básicos, fila/timeout, cancel, sinais, loops e fork; faltam métricas de duração e maior homologação de lifecycle |
| P4 | Parcial | JSON e Arrow nativos materializados/streaming, Polars materializado, BLOBs transacionais, DECFLOAT exato, tipos temporais com fuso e probe interno JSON medido; faltam matriz completa de charsets/capacidades, RSS e benchmarks com outros drivers |
| P5 | Pendente | Partições restritas, reserva de grupo, snapshot comum e equivalência |
| P6 | Pendente | Artefato final, consumidor real quando disponível e auditoria completa da seção 20 |

Validações executadas no estágio atual:

- 34 testes Rust unitários/propriedades: config, SQL, versão, precisão/limites,
  fila FIFO limitada, remoção por cancelamento, retenção de slot ocupado e
  janelas de concorrência da invalidação de cache, precisão JSON, datas extremas
  e parsing/reescala decimal exata com coeficientes INT128 e zeros redundantes.
  Incluem fechamento de snapshot com permit de entrega retido e cancelamento/
  timeout pendente, além de prazo expirado antes do polling do worker.
  Incluem bytes IEEE independentes de FLOAT/DOUBLE e ownership do C Stream
  Arrow: release único com/sem consumo e lote válido depois de destruir o leitor.
  O C Array também conserva a referência ao buffer e a libera quando abandonado.
- 31 testes de integração Rust/C++ no Firebird 5.0.3 descartável: TPB efetivo,
  bind, status, ownership, reset/cache, precisão, role, limites, cleanup,
  cancel real por geração, deadline autônomo, FIFO/teto, panic do worker,
  handshake efetivo, reposição de min_idle, lifetime e backoff. Incluem streams
  com buffers próprios, demanda/backpressure e base Rust de snapshot consistente
  entre consultas, streams e commit concorrente em outra conexão.
  JSON confere por MON$TRANSACTIONS que `]` só é entregue depois de fechar a
  transação; parada/erro do consumer não fecha artificialmente um array parcial.
  O codec temporal testa os 1.681 offsets de minuto entre -14h e +14h, nos
  limites do relógio, além de largura de buffer, padding e zona inválida.
- 19 testes Python de empacotamento/config por cópia externa, incluindo resolução
  automática no construtor, remoção de max_result_bytes e licenças Arrow do target.
- 252 testes Python reais: escalares/Decimal/INT128/datas, erros, aliases,
  explain parametrizado, cleanup, role, destrutor após fork e 96 consultas
  concorrentes com teto de 3 attachments. Incluem Ctrl+C, prazo durante conversão
  Python, timeout nativo, close durante attach pendente e seis subprocessos
  cobrindo SIGINT/SIGTERM padrão, ignorado e personalizado. Streams incluem
  cancelamento em fila, abandono, conversão, loop fechado e fork. Quinze casos
  verificam as políticas WireCrypt dos dois lados e os aliases booleanos,
  observando MON$WIRE_ENCRYPTED em três bancos descartáveis independentes.
- 21 desses testes exercitam snapshot público: mesma transação/attachment,
  commit concorrente e leitura estável, queries independentes na pool, streams,
  exclusividade durante conversão, prazos ativos/ociosos, cancelamento/abertura,
  abandono com stream retido, fork e liberação para fila e invalidação de cache.
- 14 exercitam pools nomeadas e atalhos: publicação após warmup, nomes isolados,
  oito inicializações concorrentes, falha/cancelamento durante abertura, segundo
  cancelamento durante cleanup, timeout com attach ainda ocupado, fechamento
  concorrente e preservação de instância substituta. Fork rejeita uso antes do
  mutex herdado; outro subprocesso comprova import/config antes de fork e primeira
  inicialização no filho. Atalhos compartilham cache, limites e executor.
- 15 exercitam HTTP por sockets TCP reais em FastAPI 0.143.2, Starlette 1.8.0,
  Uvicorn 0.54.0 e AnyIO 4.15.1 (ASGI HTTP 2.3): desconexão durante fetch ou consumo,
  término normal, erro após headers e shutdown com corpo ativo. Gerador/stream
  ficam referenciados para provar cleanup sem GC; consulta subsequente funciona
  e o lifespan fecha/remove a pool. Cada caso cobre linhas, NDJSON e array JSON
  nativo. Outros servidores e o caminho ASGI 2.4+ seguem sem homologação.
- 24 novos casos Python específicos de JSON cobrem tipos/opções, INT128 integral,
  decimais sem float, datas extremas, Unicode/escaping, binários e não finitos via
  binding real. Exercitam aliases antes do consumo, limites, chunks de um byte,
  retenção após close, pool nomeada/snapshot, cancel/deadline, exclusividade e
  um milhão de linhas sem objetos Python por célula.
- 13 casos Python de BLOB: NULL/vazio, Unicode, WIN1252, NONE com/sem fallback,
  OCTETS/binário, subtipos desconhecidos, tamanhos até 1.048.577 caracteres,
  limite por célula após transliteração, erro sem truncar, bind sem criação,
  consultas/streams/JSON/snapshots sync/async, retenção após close, cancelamento,
  deadline e heartbeat. O Rust também interrompe entre leituras reais de BLOB
  para provar cleanup e reutilização sem depender de cancelamento do servidor.
- 29 casos Python de DECFLOAT(16/34): precisão e extremos, subnormais, NULL,
  zero negativo, NaN/sNaN com payload e sinal, bind exato e rejeição de perda,
  regressão para NUMERIC fixo, JSON sem Decimal intermediário, streams e snapshots
  sync/async. O contexto decimal Python reduzido não arredonda os valores lidos.
- 23 casos Python de TIME/TIMESTAMP WITH TIME ZONE: UTC, offset/região, fold e
  gaps de DST, mudanças de meia hora, datas extremas, NULL/vazio, bind, JSON,
  streams/snapshots sync/async e retenção após fechar pool. TimeWithZone é imutável.
  Regras Python ausentes/divergentes falham em linhas e não impedem JSON UTC/zona.
  Cinquenta mil pares de valores temporais exercitam responsividade na conversão.
- 30 casos Arrow/Polars cobrem schemas vazios/NULL, precisão e tipos, fusos,
  BLOBs/charsets, aliases/limites, capsules, buffers retidos depois de fechar pool,
  snapshots/pools nomeadas, adaptação off-loop, cancelamento e deadlines.
  PyArrow 26.0.0 e Polars 2.0.0 recebem buffers INTEGER com endereços idênticos
  aos exportados, em múltiplos chunks. Schema alterado/reordenado é rejeitado.
  Dois casos adicionais verificam FLOAT/DOUBLE reais em linhas, JSON, Arrow e
  Polars, incluindo bind, extremos, subnormais, zeros assinados e não finitos.
- 22 casos de streaming Arrow verificam C Array/C Stream, schema vazio/NULL,
  igualdade de tipos/metadados com a materialização, demanda/prefetch limitado,
  consumo exclusivo, capsules descartadas/consumidas, retenção após close,
  snapshots/atalhos, cancel async, idle timeout, shutdown limitado, fechamento
  explícito durante leitura externa, fork e dois subprocessos de interrupção.
  Um milhão de linhas em async streaming verifica responsividade sem reter todos
  os lotes. O close explícito interrompe o leitor externo e espera seu cleanup.
- Cache coordenado sync/async: dois attachments aquecidos, invalidação em ambos,
  coalescência, timeout e cancelamento sem interromper o cursor de outro chamador.
- Async usa o mesmo scheduler e ponte PyO3 consolidada; testa fila cheia,
  cancel antes/durante/depois, materialização, deadlines, loops sucessivos,
  loop fechado sem callback tardio e um milhão de tuplas com heartbeat.
- `uv add` de wheel e de commit em Git temporário local: ambos com o mesmo
  build_id, carregamento do runtime interno, consulta real e MON$ comprovando
  READ ONLY=1 e READ CONSISTENCY=4. Também validam init/get/close e consultas pelos
  atalhos de pool nomeada, fetch_json, stream_json, BLOBs, DECFLOAT e tipos com fuso reais. Projetos/locks são
  independentes do checkout.
  Em ambos os consumidores, Polars foi instalado/testado antes de PyArrow,
  com ausência de PyArrow conferida no processo. Depois, PyArrow foi instalado
  separadamente e validou os mesmos dados e o domínio INT128 integral.
  Antes de instalar os dois extras, ambos os consumidores validaram ArrowBatch,
  ArrowSchema, capsules e streams sync/async. Depois validaram C Stream diretamente
  em Polars (ainda sem PyArrow) e em PyArrow, mantendo o mesmo build_id.
- sdist criado com backend/scripts/fontes nativos e sem `.tools`, `.env` ou
  `target`; instalação do sdist ainda não testada separadamente.
- fmt, clippy com warnings negados, ruff, mypy e módulos/dependências passaram.
- Pyright 1.1.414 via LSP: ajuda de assinatura completa para os 35 parâmetros
  do construtor, com função, ambiente e padrão; rejeição estática dos três valores
  inválidos exercitados e de max_result_bytes. Probe e evidência em `.tools/typing/`.
- Probe consumidor mypy: retorno bytes de JSON em pool/snapshot/atalhos e ambos
  os context managers; rejeita as seis opções literais inválidas exercitadas.
  O probe analítico confere DataFrame em seis combinações sync/async de pool,
  snapshot e atalhos; rejeita unsupported='ignore' em pool e atalho.
  O probe de streaming confere ArrowStream/AsyncArrowStream, ArrowBatch e
  ArrowSchema; rejeita await no context manager e as_dict em stream_arrow.

O header oficial C++ emite warning de parâmetro não usado. O teste intencional
de uso indevido após fork gera DeprecationWarning do CPython; apenas verifica
rejeição antes de mutex/handle, sem declarar fork com threads um modo suportado.

O runtime carregado identificou seu root dentro da cópia temporária, com
site-packages desabilitados e sem caminhos de loader auxiliares. O protótipo
nativo comprovou o lifecycle descrito em `decisions/0002-reset-cache-prototype.md`.
Agora há consultas via API Python comprovadas, mas nenhuma meta de desempenho
comparativo foi atingida/declarada.

Handshake, reposição de min_idle e invalidação coordenada estão descritos em
`decisions/0007-pool-maintenance.md`. Attach/handshake agora respeitam o orçamento
restante de aquisição, sem herdar o prazo de query. Streams e a base Rust de
snapshot estão em `decisions/0008-row-streams.md`; configuração e mudanças de
limites em `decisions/0009-config-ergonomics.md`. O snapshot público está integrado
ao mesmo worker e aos mesmos conversores, conforme `decisions/0010-public-snapshots.md`.
Pools nomeadas e a evidência HTTP inicial estão em `decisions/0011-named-pools.md`.
JSON e a extensão da matriz HTTP estão em `decisions/0012-native-json.md`.
BLOBs e os limites do cancelamento nativo estão em `decisions/0013-transactional-blobs.md`.
DECFLOAT e a validação exata no bind estão em `decisions/0014-exact-decfloat.md`.
Tipos temporais com fuso e a representação UTC/zona estão em `decisions/0015-time-zones.md`.
Arrow/Polars materializados, ownership e a correção de FLOAT/DOUBLE estão em
`decisions/0016-arrow-materialization.md`.
Streaming Arrow e os contratos de encerramento/exportação estão em
`decisions/0017-arrow-streams.md`.
Ainda faltam mais cenários de
conexão interrompida e métricas de durações. A fixture padrão negociou criptografia,
mas não compressão, mesmo quando solicitada; a matriz de capacidades segue aberta.
O executor atual exige conexão UTF8; a matriz de charsets ainda está incompleta.
BLOBs conhecidos podem ser transliterados pelo cliente; inline/cache de BLOB
ainda não têm negociação/ganho medidos contra outro driver.
Os critérios restantes de
P3–P6 continuam abertos, incluindo métricas, RSS, extração paralela e benchmarks equivalentes.

Cancelamento e política de sinais estão descritos em
`decisions/0005-cancellation-signals.md`. O probe de dez amostras no build anterior
`cb0e7a9d04874b6d81a48d56320cac3f3f4867dd6ef254f7550db50b173bd647`
registrou mediana de 8,366 ms até o chamador receber Ctrl+C e 12,307 ms até a
liberação observada do slot; máximos de 13,787 e 17,618 ms. São medidas locais
diagnósticas, com polling de 1 ms, sem promessa para partições de rede ou
comparação de desempenho com outros drivers. Amostras em
`benchmarks/results/20261015T121644Z-cancel.json`.

No checkpoint atual, o heartbeat de um milhão de tuplas teve máximo de
3,627 ms em execução isolada (2.823 amostras; 279 durante conversão Python;
maior pausa de GC medida: 0,289 ms). O stream de um milhão de tuplas teve máximo
de 34,833 ms, com 544 amostras durante materialização. JSON materializado teve
máximo de 7,294 ms para um milhão de linhas (17.786.001 bytes). BLOBs grandes
de texto/binário tiveram máximo de 7,189 ms. Cinquenta mil pares temporais com
fuso tiveram máximo de 34,838 ms, com 41 amostras durante materialização.
Um milhão de linhas em Arrow teve máximo de 5,336 ms e adaptação final de
2,102 ms (2 ticks durante adaptação); Polars teve máximo de 5,625 ms e adaptação
de 2,402 ms (2 ticks durante adaptação). Ambos conservaram 2.000 lotes nativos;
a thread de adaptação foi conferida como distinta da thread do loop. Arrow em
async streaming teve máximo de 2,107 ms para um milhão de linhas, conservando
somente o primeiro lote no consumidor. Esses números verificam
responsividade local, sem equivaler a um benchmark comparativo de throughput.
Uma validação concorrente anterior falhou com pico de 269,345 ms; a causa não
foi isolada. A fase de responsividade sob carga controlada continua aberta,
conforme `decisions/0006-async-delivery.md`, sem aumento do limite do teste.

Checkpoint atual: build `c1555f90c7d1b5dcb34e19e83ccb174ceb487bf6da3aec2dab2fc4635104a1b1`,
macOS arm64, CPython 3.14, release. Logs locais em `.tools/{unit,integration,
python-integration,packaging,uv-install,verify,clippy}.log`.

Os 252 testes de integração Python (95,83 s), 19 de empacotamento, verificador de inventário
e os consumidores uv por wheel/Git local passaram neste artefato. Os 34 testes
unitários/propriedades e 31 integrações Rust/C++ passaram no código correspondente.
O probe uv confirmou mesmo build_id e BLOB/JSON/DECFLOAT/fusos/Arrow/Polars reais
em ambos os consumidores, incluindo streaming sem extras e C Stream externo. A fixture descartável foi usada; nenhum banco
operacional foi alterado.

O checkpoint analítico materializado anterior foi
`480c7046ffb41bb7c36858aef4f0b7b1a2664eb20116202ae52e98933ee74391`.
Na primeira rodada de streaming, um teste supôs cancelamento imediato por
Pool.close, e o harness encerrou seu processo após 90 s. A fixture foi recriada.
A repetição verificou a política existente de shutdown com prazo e a nova
interrupção de leitura externa por ArrowStream.close, conforme decisão 0017.

Na primeira rodada analítica, os valores SQL FLOAT/DOUBLE incorretos revelaram
a inversão dos códigos 482/480 no decoder/bind anteriores. Os tipos e larguras
foram corrigidos com referência ao header oficial e regressões independentes.
O teste de timeout Polars também passou a explicitar o fallback para o SUM
INT128, pois o default corretamente rejeita Decimal256 antes do fetch. O teste
de recuperação permite 2 s para incluir o primeiro import dos extras; o teste
de expiração continua com 50 ms, sem relaxar o prazo da operação testada.

O checkpoint de fusos anterior foi
`b697fb4b9bae3cd92aa1237cb07a2b8470450373ae867f9e307c02b1588e33f1`.

O checkpoint DECFLOAT anterior foi
`9d12a03edb83963942e0900144403461638f6778124411454c64d611fd8df1b5`.
Na etapa de fusos, a primeira suíte completa passou 197 testes e revelou uma
corrida no fechamento de snapshot durante conversão Python. O worker podia
observar closing antes de observar cancelamento. A ordem de publicação e a
verificação de erros no fechamento foram corrigidas, com regressão determinística
Rust e preservação do teste Python original. A execução final passou 198 casos
em 71,62 s. Detalhes da correção na decisão 0010; log inicial preservado em
`.tools/python-integration-timezone-initial.log`.

O OrbStack parou durante a etapa, antes de uma execução nativa. Foi reativado e
a fixture descartável foi recriada pelo harness; as validações finais acima
foram realizadas depois dessa recuperação. Nenhum banco operacional foi alterado.

O checkpoint BLOB anterior foi
`fb2b5129d53ad74dc9039b27c2465f1228d712315ebf9835d27386a2737de647`.
A rodada DECFLOAT passou os 175 casos em 67,99 s. Uma tentativa anterior parou
na checagem do harness porque a fixture estava desligada; não chegou a executar
testes. Após recriar o banco descartável, os 29 casos específicos e a suíte
completa passaram, sem alteração do código para acomodar essa interrupção.

Histórico: checkpoint funcional/benchmark JSON anterior foi
`e55e27ca09843506dcd96cc01ee1fe115d1f9e845f4c440a4ed755aff8d7e06c`.

Na etapa anterior, o artefato local foi regenerado como
`76c9dfa04388f534a2b588b9936780f4ebabfe70d74919621b901bc2386bb685`
após corrigir somente a URL/instrução Git no README. Código Rust/Python é o
mesmo do checkpoint funcional acima; o novo fingerprint altera a extensão.
Inventário comparado com a cópia usada nos testes difere apenas no `.so`.
Neste artefato foram repetidos os 18 testes de empacotamento, verificação de
hashes/cópia externa e instalação uv por wheel/Git local, incluindo JSON real.

A medição `benchmarks/results/20261015T152200Z-json.json` compara os dois caminhos
da própria fbfast, em processos separados, com cache aquecido e três amostras:
JSON nativo teve mediana de 5,917 s e pico de RSS de 74,5 MiB; dicts + json.dumps,
6,289 s e 355,0 MiB. Saídas têm tamanho/hash idênticos. RSS inclui runtime,
allocator e warmup; isso não substitui os baselines firebird-driver/fdb nem
estabelece uma meta geral de desempenho da biblioteca.

A primeira suíte completa da etapa JSON teve uma falha no teste antigo que
esperava idle=1 com pool_size=2/min_idle=1. A manutenção pode abrir o segundo
slot enquanto o primeiro está leased. O teste de cleanup passou a configurar
pool_size=1, mantendo a asserção exata, sem alterar o comportamento da pool.
A execução final passou os 133 testes.

Na etapa BLOB, a primeira suíte completa passou 145 casos e falhou no teste
HTTP rows-fetch: o teste fechava TCP ao observar leased=1, estado que também
inclui preparação e cancelamento cooperativo antes do fetch. A asserção de
native_cancels foi preservada; agora uma conexão observadora própria confirma
a consulta longa ativa em MON$STATEMENTS antes de desconectar. Os 15 casos HTTP
e, depois, os 146 casos completos passaram. Nenhum comportamento da pool mudou
para acomodar essa corrida do teste.

A distribuição principal mudou para uv/pip por orientação do usuário, conforme
`decisions/0004-distribution-uv.md`. O checkout já possui origin para o repositório
público `rafael-peter/fbfast` (a URL configurada com rafaelpeter03 redireciona).
O HEAD remoto observado foi `6c761106595bc67cb050ed20cad3a45377e03b2b`;
nenhum commit/push remoto foi feito nesta etapa. As novas alterações continuam
locais; a instalação validada usa um commit temporário com os fontes atuais.
O banco descartável desta etapa foi recriado pelo harness,
incluindo os três bancos auxiliares de políticas de criptografia. Testes nativos
também usam agora uma cópia temporária do runtime: os cenários de rede podem gerar
firebird.log e não devem modificar a pasta entregue. O verificador continuou
exigindo inventário e hashes exatos e passou após os testes nativos.
