# Evidência de implementação

O objetivo continua sendo todo o `plano_fbfast_final.md`, P0 a P6. Este registro
não substitui nem reduz os critérios originais, salvo os ajustes expressamente
pedidos pelo usuário: distribuição principal por uv/pip e remoção do limite
total de bytes, documentados nas decisões 0004 e 0009.

| Fase | Estado | Evidência exigida / restante |
|---|---|---|
| P0 | Verificada no alvo local | Release, runtime, manifesto, hashes, cópia externa e instalação uv por wheel/Git local; repositório público existente, revisão remota atual não validada como entrega final |
| P1 | Parcial | Attach/TPB/prepare/describe/bind/fetch/cleanup e cancel independente Rust+C++ reais, tipos escalares, cache e JSON; faltam sinks analíticos e baselines com outros drivers |
| P2 | Parcial | Pool de workers, fila limitada, APIs sync/stream, explain e política lexical+TPB; faltam matriz de tipos e homologação completa de lifecycle |
| P3 | Parcial | API async e sync, snapshot público, pools nomeadas, HTTP com linhas/JSON nativo, stats básicos, fila/timeout, cancel, sinais, loops e fork; faltam métricas de duração e maior homologação de lifecycle |
| P4 | Parcial | JSON nativo materializado/streaming, BLOBs transacionais e probe interno JSON medido; faltam Arrow/Polars, matriz completa de tipos, ownership analítico e benchmarks com outros drivers |
| P5 | Pendente | Partições restritas, reserva de grupo, snapshot comum e equivalência |
| P6 | Pendente | Artefato final, consumidor real quando disponível e auditoria completa da seção 20 |

Validações executadas no estágio atual:

- 25 testes Rust unitários/propriedades: config, SQL, versão, precisão/limites,
  fila FIFO limitada, remoção por cancelamento, retenção de slot ocupado e
  janelas de concorrência da invalidação de cache, precisão JSON e datas extremas.
- 30 testes de integração Rust/C++ no Firebird 5.0.3 descartável: TPB efetivo,
  bind, status, ownership, reset/cache, precisão, role, limites, cleanup,
  cancel real por geração, deadline autônomo, FIFO/teto, panic do worker,
  handshake efetivo, reposição de min_idle, lifetime e backoff. Incluem streams
  com buffers próprios, demanda/backpressure e base Rust de snapshot consistente
  entre consultas, streams e commit concorrente em outra conexão.
  JSON confere por MON$TRANSACTIONS que `]` só é entregue depois de fechar a
  transação; parada/erro do consumer não fecha artificialmente um array parcial.
- 18 testes Python de empacotamento/config por cópia externa, incluindo resolução
  automática no construtor e remoção de max_result_bytes.
- 146 testes Python reais: escalares/Decimal/INT128/datas, erros, aliases,
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
- Cache coordenado sync/async: dois attachments aquecidos, invalidação em ambos,
  coalescência, timeout e cancelamento sem interromper o cursor de outro chamador.
- Async usa o mesmo scheduler e ponte PyO3 consolidada; testa fila cheia,
  cancel antes/durante/depois, materialização, deadlines, loops sucessivos,
  loop fechado sem callback tardio e um milhão de tuplas com heartbeat.
- `uv add` de wheel e de commit em Git temporário local: ambos com o mesmo
  build_id, carregamento do runtime interno, consulta real e MON$ comprovando
  READ ONLY=1 e READ CONSISTENCY=4. Também validam init/get/close e consultas pelos
  atalhos de pool nomeada, fetch_json, stream_json e BLOBs reais. Projetos/locks são
  independentes do checkout.
- sdist criado com backend/scripts/fontes nativos e sem `.tools`, `.env` ou
  `target`; instalação do sdist ainda não testada separadamente.
- fmt, clippy com warnings negados, ruff, mypy e módulos/dependências passaram.
- Pyright 1.1.414 via LSP: ajuda de assinatura completa para os 35 parâmetros
  do construtor, com função, ambiente e padrão; rejeição estática dos três valores
  inválidos exercitados e de max_result_bytes. Probe e evidência em `.tools/typing/`.
- Probe consumidor mypy: retorno bytes de JSON em pool/snapshot/atalhos e ambos
  os context managers; rejeita as seis opções literais inválidas exercitadas.

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
Ainda faltam mais cenários de
conexão interrompida e métricas de durações. A fixture padrão negociou criptografia,
mas não compressão, mesmo quando solicitada; a matriz de capacidades segue aberta.
Não há DECFLOAT ou tipos SQL com timezone, e o executor atual exige conexão UTF8.
BLOBs conhecidos podem ser transliterados pelo cliente; inline/cache de BLOB
ainda não têm negociação/ganho medidos contra outro driver.
Os critérios restantes de
P3–P6 continuam abertos, incluindo sinks analíticos e benchmarks equivalentes.

Cancelamento e política de sinais estão descritos em
`decisions/0005-cancellation-signals.md`. O probe de dez amostras no build anterior
`cb0e7a9d04874b6d81a48d56320cac3f3f4867dd6ef254f7550db50b173bd647`
registrou mediana de 8,366 ms até o chamador receber Ctrl+C e 12,307 ms até a
liberação observada do slot; máximos de 13,787 e 17,618 ms. São medidas locais
diagnósticas, com polling de 1 ms, sem promessa para partições de rede ou
comparação de desempenho com outros drivers. Amostras em
`benchmarks/results/20261015T121644Z-cancel.json`.

No checkpoint atual, o heartbeat de um milhão de tuplas teve máximo de
10,854 ms em execução isolada (2.804 amostras; 281 durante conversão Python;
maior pausa de GC medida: 9,046 ms). O stream de um milhão de tuplas teve máximo
de 9,096 ms, com 547 amostras durante materialização. JSON materializado teve
máximo de 10,460 ms para um milhão de linhas (17.786.001 bytes). BLOBs grandes
de texto/binário tiveram máximo de 7,506 ms. Esses números verificam
responsividade local, sem equivaler a um benchmark comparativo de throughput.
Uma validação concorrente anterior falhou com pico de 269,345 ms; a causa não
foi isolada. A fase de responsividade sob carga controlada continua aberta,
conforme `decisions/0006-async-delivery.md`, sem aumento do limite do teste.

Checkpoint atual: build `fb2b5129d53ad74dc9039b27c2465f1228d712315ebf9835d27386a2737de647`,
macOS arm64, CPython 3.14, release. Logs locais em `.tools/{unit,integration,
python-integration,packaging,uv-install,verify,clippy}.log`.

Os 146 testes de integração Python, 18 de empacotamento, verificador de inventário
e os consumidores uv por wheel/Git local passaram neste artefato. Os 25 testes
unitários/propriedades e 30 integrações Rust/C++ passaram no código correspondente.
O probe uv confirmou mesmo build_id e BLOB/JSON real em ambos os consumidores.

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
