# Plano do próximo update — segurança e limites de recursos

Status: implementado localmente; CI multiplataforma e publicação pendentes  
Baseline auditado: `a6051256e4cac13f275cb484c038e66d27363baf`  
Data da decisão: 2026-10-16

## Objetivo

Fechar os riscos encontrados na revisão de segurança sem impedir o uso do
`fbfast` com servidores Firebird 5.0.3. O update deve restaurar uma proteção
configurável contra resultados materializados excessivos, remover advisories das
dependências, esclarecer o limite real da garantia de leitura e reforçar a
fronteira nativa e o tratamento de credenciais.

A implementação integra a release 0.2.1.

## Resultado implementado

- limite padrão de 512 MiB aplicado a todas as materializações, configurável por
  conexão e desativável explicitamente com `None`;
- dependências atualizadas e gate RustSec sem advisory local;
- credencial compartilhada em buffer zeroizável e buffer temporário de attach
  limpo após o uso;
- fixture com usuário realmente restrito e teste explícito do limite de
  segurança de rotinas/generators;
- cliente 5.0.3 mantido após rejeição comprovada do cliente 5.0.4 contra servidor
  5.0.3; cliente empacotado testado contra servidores 5.0.3 e 5.0.4;
- fuzz targets e gate Linux ASan/UBSan adicionados ao workflow;
- release consolidada no comando retomável `./scripts/release.py`.

A evidência local detalhada está em `docs/progress.md`. Sanitizers Linux, matriz
completa de wheels e publicação continuam pendentes do CI disparado pelo comando
de release.

## Decisões já tomadas

1. Servidores Firebird 5.0.3 continuam suportados. Não haverá bloqueio exigindo
   5.0.4 no handshake.
2. O cliente Firebird empacotado permanece em 5.0.3. O candidato 5.0.4 falhou
   contra servidor 5.0.3 na conversão de fusos nomeados; a matriz adotada usa
   cliente 5.0.3 contra servidores 5.0.3 e 5.0.4.
3. `max_result_bytes` voltará a existir em `FirebirdConfig`, com valor padrão
   alto e possibilidade de configuração por argumento, ambiente e `.env`.
4. O valor padrão é 512 MiB (`536_870_912`). A aplicação pode escolher outro
   teto conforme sua carga.
5. `max_result_bytes=None` continuará sendo uma escolha explícita para desligar
   o limite. A documentação deverá explicar o risco dessa escolha.
6. O limite será aplicado a resultados materializados. Streams continuarão
   controlados por `batch_bytes`, `prefetch_batches`, `max_blob_bytes` e
   backpressure, sem limite cumulativo do conteúdo transmitido.
7. A biblioteca continuará sendo um cliente de leitura, sem prometer sandbox de
   SQL arbitrário. A segurança contra rotinas, funções e sequences depende dos
   privilégios do usuário e da role no Firebird.

## Escopo 1 — restaurar `max_result_bytes`

### Contrato público

Adicionar novamente:

```python
FirebirdConfig(
    max_result_bytes=512 * 1024 * 1024,
)
```

Contrato completo:

- tipo: `int | None`;
- variável: `FB_MAX_RESULT_BYTES`;
- padrão: 512 MiB;
- valor inteiro mínimo: 1;
- `None` explícito: sem limite cumulativo de bytes;
- precedência: argumento explícito → ambiente → `.env` → padrão;
- unidade: bytes;
- escopo: cada resultado materializado, não o pool inteiro;
- métodos afetados: rows, dicts, modelos Pydantic, JSON, Arrow e Polars, nas
  variantes síncronas, assíncronas e de snapshot;
- métodos não afetados: streams de rows, JSON e Arrow.

O limite não será anunciado como teto exato de RSS. Ele é um orçamento do
resultado retido pelo core. Conversão posterior para objetos Python ou Pydantic,
overhead do allocator, buffers temporários e consultas concorrentes podem elevar
o consumo real. Com `pool_size=8`, por exemplo, oito resultados simultâneos podem
consumir múltiplos do limite individual.

### Contabilização

Criar um helper central de orçamento para evitar três implementações com regras
e mensagens diferentes. Toda soma deve usar aritmética verificada e retornar
`ResultLimitError` em overflow.

- Rows: somar o tamanho estrutural de cada `Value` e seus dados alocados. Ajustar
  `payload_bytes` ou substituí-lo por um nome que represente memória retida; para
  `String` e `Vec`, considerar a alocação conservadora e não apenas o conteúdo
  visível.
- JSON: contabilizar os bytes serializados mantidos no `Vec<u8>`, incluindo
  nomes de campos, escape, separadores e expansão base64.
- Arrow/Polars: somar `get_array_memory_size()` de cada batch finalizado antes de
  retê-lo no resultado.
- BLOB: manter `max_blob_bytes` como limite por célula. O BLOB também conta no
  orçamento cumulativo da materialização.

Uma linha ou um batch indivisível pode ultrapassar o saldo antes da detecção. O
excesso transitório permanece limitado por `max_blob_bytes` e `batch_bytes`; a
consulta deve falhar sem devolver resultado parcial e executar todo o cleanup da
transação e do cursor.

As consultas internas de handshake não devem consumir o orçamento configurado
pelo usuário. Elas já possuem formato e cardinalidade fixos e precisam continuar
funcionando mesmo quando um teste usa um limite muito pequeno.

### Erro e observabilidade

Separar as mensagens dos limites:

- `Materialized result exceeds max_result_rows`;
- `Materialized result exceeds max_result_bytes`.

A mensagem de bytes deve informar limite e quantidade observada quando isso não
exigir custo adicional. Não truncar o resultado silenciosamente.

### Arquivos principais

- `crates/fbfast-core/src/config/fields.rs`
- `crates/fbfast-core/src/config/resolve.rs`
- `crates/fbfast-core/src/decode/value.rs`
- `crates/fbfast-core/src/sinks/rows.rs`
- `crates/fbfast-core/src/execution/json.rs`
- `crates/fbfast-core/src/execution/arrow.rs`
- `crates/fbfast-python/src/config.rs`
- `python/fbfast/_native.pyi`
- `README.md`

### Testes de aceite

- validar padrão, argumento explícito, ambiente, `.env`, `None`, zero, negativos,
  overflow e opção desconhecida;
- aceitar resultado exatamente no limite e rejeitar o primeiro valor acima;
- testar rows, dict, Pydantic, JSON, Arrow e Polars;
- provar contabilização de texto, bytes, BLOB, JSON escapado e base64;
- provar o mesmo comportamento em sync, async e snapshot;
- verificar que falha não devolve resultado parcial e que o pool permanece
  reutilizável;
- verificar que handshake, `fetch_one` pequeno e streams não recebem o limite
  cumulativo indevidamente;
- preservar os testes de `max_result_rows` e `max_blob_bytes`.

### Validação futura do padrão

Medir 256 MiB, 512 MiB e 1 GiB em rows, JSON e Arrow, registrando:

- bytes lógicos contabilizados;
- pico de RSS;
- tempo de cleanup após atingir o limite;
- efeito de duas materializações concorrentes;
- diferença entre texto, BLOB e colunas numéricas.

O padrão desta release foi fixado em 512 MiB conforme a decisão do produto. A
medição poderá orientar outra escolha em uma release futura, sem bloquear a
proteção atual. O artefato de benchmark deve registrar máquina, Python, Firebird
e commit.

## Escopo 2 — dependências e gate automático

Atualizar como um conjunto compatível:

- PyO3 para uma versão `>=0.29.0` sem os advisories
  `RUSTSEC-2026-0176` e `RUSTSEC-2026-0177`;
- `pyo3-async-runtimes` para a linha compatível com o PyO3 escolhido;
- `time` para `>=0.3.47`, corrigindo `RUSTSEC-2026-0009`;
- `Cargo.lock` e versões exatas do workspace.

Executar novamente a matriz Python 3.11–3.14, incluindo fork guards, asyncio,
Arrow C Data Interface, modelos Pydantic e lifecycle. Não suprimir advisory para
fazer o pipeline passar.

Adicionar ao CI um job de segurança com:

```sh
cargo audit --deny warnings
```

Fixar a versão da ferramenta ou a action por SHA, seguindo o padrão atual dos
workflows. A release fica bloqueada enquanto houver vulnerabilidade RustSec sem
uma decisão explícita documentada.

## Escopo 3 — compatibilidade Firebird 5.0.3 e 5.0.4

Não alterar o contrato para exigir servidor 5.0.4. A tentativa de atualizar
somente o cliente empacotado para 5.0.4 foi rejeitada pela matriz local: o cliente
5.0.4 não representou corretamente fusos nomeados recebidos do servidor 5.0.3.
Em vez disso:

1. manter `scripts/release_assets.json` fixado no cliente 5.0.3 oficial;
2. manter um manifesto 5.0.4 separado para iniciar o servidor da matriz;
3. provar em CI que o cliente 5.0.3 empacotado opera contra servidores 5.0.3 e
   5.0.4;
4. manter suporte a `client_library` compatível para instalações controladas;
5. documentar que advisories do servidor não são corrigidos pelo wheel cliente;
6. recomendar upgrade do servidor quando o responsável pela infraestrutura puder
   fazê-lo, sem impedir a conexão com 5.0.3.

Para ambientes que precisam permanecer em 5.0.3, documentar controles
compensatórios: acesso de rede restrito, usuário sem DDL, sem criação de rotinas
externas, role mínima e acompanhamento das correções do servidor.

A fixture e o workflow precisam aceitar uma matriz explícita de versões. Um teste
local isolado não substitui a matriz dos wheels de Linux, macOS e Windows.

## Escopo 4 — contrato de leitura e privilégios

Promover a limitação já registrada na decisão `0003-readonly-gates.md` para uma
seção visível do README e para a documentação de segurança:

- filtro lexical + tipo preparado + TPB `READ ONLY` bloqueiam DML comum;
- selectable procedures, funções, UDRs, generators e GTTs impedem que isso seja
  tratado como sandbox;
- SQL de usuário, endpoint público ou agente precisa de uma camada de consultas
  permitidas acima do driver;
- a conta do banco deve possuir somente os `SELECT` necessários e não deve ter
  `EXECUTE`/`USAGE` desnecessários.

Não ampliar indefinidamente uma blacklist de nomes SQL. Corpos de rotinas e views
podem ocultar efeitos, então privilégios e allowlist de consultas são as
fronteiras corretas.

Adicionar testes de segurança na fixture descartável:

- `GEN_ID` direto continua rejeitado pelo filtro;
- uma rotina privilegiada demonstra que o driver não é sandbox e que generator
  não volta no rollback;
- a role restrita usada normalmente não consegue executar essa rotina;
- DML, locking SELECT e múltiplos statements continuam rejeitados;
- a sessão observada continua `READ ONLY`, com role, charset, timezone e
  WireCrypt efetivos verificados.

Criar uma nova decisão de arquitetura para o limite de bytes e para o contrato de
ameaça. Ela deve declarar que substitui apenas o trecho de
`0009-config-ergonomics.md` que removeu `max_result_bytes`, preservando o histórico
da decisão anterior.

## Escopo 5 — credenciais em memória

Preservar o comportamento público e reduzir cópias internas:

- avaliar `secrecy` + `zeroize`, ou wrapper equivalente, para o segredo do core;
- compartilhar o segredo entre workers em vez de clonar a `String` por conexão;
- limpar o buffer nativo temporário da senha imediatamente após o attach;
- manter senha redigida em `Debug`, `repr`, serialização e erros;
- preservar `config.password` por compatibilidade, documentando que solicitar a
  propriedade cria uma `str` Python que não pode receber garantia de zeroização.

Os testes devem usar uma senha sentinela e procurar essa sentinela em repr,
erros, estatísticas e serialização. Não prometer eliminação de todas as cópias
feitas pelo interpretador, sistema operacional ou `fbclient`.

## Escopo 6 — fronteira FFI

Adicionar uma execução Linux com ASan/UBSan para a ponte C++/Rust e manter os
testes reais de lifecycle. Criar fuzz targets pequenos para:

- tokenizer e validação SQL;
- decode de metadados e buffers;
- parsing de status retornado pela ABI.

O CI comum pode executar corpus curto e determinístico. Campanhas longas ficam
fora do gate de cada commit, mas seus crashes devem virar regressões versionadas.

## Documentação e release

Atualizar no mesmo change set:

- README, tabela de configuração e exemplos;
- stub `_native.pyi` e ajuda dos parâmetros;
- nova decisão de arquitetura;
- documento de segurança e ameaça;
- release notes;
- roadmap e progress somente com evidência executada.

Não declarar homologação em banco operacional. Distinguir testes locais com
fixture, CI multiplataforma, compatibilidade com Firebird 5.0.3 e validação em um
projeto consumidor.

## Ordem de implementação

1. Atualizar PyO3, runtime assíncrono e `time`; deixar `cargo audit` limpo.
2. Restaurar configuração e helper central de `max_result_bytes`.
3. Aplicar o orçamento em rows, JSON e Arrow/Polars.
4. Completar testes de limite, cleanup e concorrência; registrar a medição de
   RSS como validação futura do padrão.
5. Testar o cliente candidato e fixar a combinação que preserve servidores
   5.0.3/5.0.4.
6. Consolidar documentação do contrato de leitura e testes de privilégios.
7. Reduzir exposição de credenciais em memória.
8. Adicionar sanitizer/fuzz smoke e fechar documentação/release notes.

## Gate de conclusão

O update só está pronto quando:

- `cargo audit --deny warnings` passa sem suppressions improvisadas;
- testes Rust, Clippy, packaging, typing e integração Python passam;
- rows, JSON, Arrow, Polars e Pydantic respeitam o orçamento configurado;
- cleanup após `ResultLimitError` deixa conexão e pool reutilizáveis;
- a matriz prova servidor Firebird 5.0.3 e 5.0.4 com o cliente empacotado;
- wheels e sdist passam a verificação completa em todas as plataformas;
- README não descreve `read_only` como sandbox;
- o default de bytes está documentado como orçamento aproximado, com medição de
  RSS mantida como validação futura;
- nenhum segredo aparece nos canais de diagnóstico testados;
- a release informa claramente o que foi validado localmente, no CI e em projeto
  consumidor.

