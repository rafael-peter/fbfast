# Evidência de implementação

O objetivo continua sendo todo o `plano_fbfast_final.md`, P0 a P6. Este registro
não substitui nem reduz os critérios originais.

| Fase | Estado | Evidência exigida / restante |
|---|---|---|
| P0 | Verificada no alvo local | Release, runtime, manifesto, hashes, cópia externa e instalação uv por wheel/Git local; URL pública ainda pendente |
| P1 | Parcial | Attach/TPB/prepare/describe/bind/fetch/cleanup e cancel independente Rust+C++ reais, tipos escalares e cache; faltam sinks e baselines comparativos |
| P2 | Parcial | Pool de workers, fila limitada, API sync, explain e política lexical+TPB; faltam streaming, matriz de tipos e homologação completa de lifecycle |
| P3 | Parcial | API async e sync, stats básicos, fila/timeout, cancel por geração, sinais, loops sucessivos/fechados e guard de fork; faltam registry, snapshot e HTTP lifecycle |
| P4 | Pendente | Arrow/Polars, JSON, BLOBs, matriz de tipos, ownership e benchmark |
| P5 | Pendente | Partições restritas, reserva de grupo, snapshot comum e equivalência |
| P6 | Pendente | Artefato final, consumidor real quando disponível e auditoria completa da seção 20 |

Validações executadas no estágio atual:

- 19 testes Rust unitários/propriedades: config, SQL, versão, precisão/limites,
  fila FIFO limitada, remoção por cancelamento e retenção de slot ocupado.
- 11 testes de integração Rust/C++ no Firebird 5.0.3 descartável: TPB efetivo,
  bind, status, ownership, reset/cache, precisão, role, limites, cleanup,
  cancel real por geração, deadline autônomo, FIFO/teto e panic do worker.
- 14 testes Python de empacotamento/config por cópia externa.
- 22 testes Python reais: escalares/Decimal/INT128/datas, erros, aliases,
  explain parametrizado, cleanup, role, destrutor após fork e 96 consultas
  concorrentes com teto de 3 attachments. Incluem Ctrl+C, prazo durante conversão
  Python, timeout nativo, close durante attach pendente e seis subprocessos
  cobrindo SIGINT/SIGTERM padrão, ignorado e personalizado.
- Async usa o mesmo scheduler e ponte PyO3 consolidada; testa fila cheia,
  cancel antes/durante/depois, materialização, deadlines, loops sucessivos,
  loop fechado sem callback tardio e um milhão de tuplas com heartbeat.
- `uv add` de wheel e de commit em Git temporário local: ambos com o mesmo
  build_id, carregamento do runtime interno, consulta real e MON$ comprovando
  READ ONLY=1 e READ CONSISTENCY=4. Projetos/locks são independentes do checkout.
- sdist criado com backend/scripts/fontes nativos e sem `.tools`, `.env` ou
  `target`; instalação do sdist ainda não testada separadamente.
- fmt, clippy com warnings negados, ruff, mypy e módulos/dependências passaram.

O header oficial C++ emite warning de parâmetro não usado. O teste intencional
de uso indevido após fork gera DeprecationWarning do CPython; apenas verifica
rejeição antes de mutex/handle, sem declarar fork com threads um modo suportado.

O runtime carregado identificou seu root dentro da cópia temporária, com
site-packages desabilitados e sem caminhos de loader auxiliares. O protótipo
nativo comprovou o lifecycle descrito em `decisions/0002-reset-cache-prototype.md`.
Agora há consultas via API Python comprovadas, mas nenhuma meta de desempenho
comparativo foi atingida/declarada.

Pendências conhecidas desta base antes de homologar pool/lifecycle: manutenção
do piso min_idle após reciclagem; mais cenários de conexão interrompida;
invalidação coordenada de cache; métricas de durações; handshake de charset e
timezone efetivos. Não há BLOB,
DECFLOAT ou timezone, e o executor atual exige UTF8. Os critérios restantes de
P3–P6 continuam abertos, incluindo sinks nativos e benchmarks equivalentes.

Cancelamento e política de sinais estão descritos em
`decisions/0005-cancellation-signals.md`. O probe de dez amostras no build atual
registrou mediana de 8,366 ms até o chamador receber Ctrl+C e 12,307 ms até a
liberação observada do slot; máximos de 13,787 e 17,618 ms. São medidas locais
diagnósticas, com polling de 1 ms, sem promessa para partições de rede ou
comparação de desempenho com outros drivers. Amostras em
`benchmarks/results/20261015T121644Z-cancel.json`.

No checkpoint async atual, o heartbeat de um milhão de tuplas teve máximo de
8,230 ms em execução isolada (3.010 amostras; 287 durante conversão Python).
Uma validação concorrente anterior falhou com pico de 269,345 ms; a causa não
foi isolada. A fase de responsividade sob carga controlada continua aberta,
conforme `decisions/0006-async-delivery.md`, sem aumento do limite do teste.

Checkpoint atual: build `cb0e7a9d04874b6d81a48d56320cac3f3f4867dd6ef254f7550db50b173bd647`,
macOS arm64, CPython 3.14, release. Logs locais em `.tools/{unit,integration,
python-integration,packaging,uv-install,verify,clippy}.log`.

A distribuição principal mudou para uv/pip por orientação do usuário, conforme
`decisions/0004-distribution-uv.md`. Nenhum repositório remoto foi criado,
conectado ou publicado. O banco descartável pode ser recriado pelo harness.
