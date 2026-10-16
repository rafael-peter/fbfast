# Entrega focada na distribuição solicitada pelo autor

Em 15/10/2026, durante a primeira matriz comparativa, o autor esclareceu que a API
já atende seu uso: biblioteca Rust para consultas de leitura, com prioridade de
desempenho. Sua expectativa de pendência era workflow por SO e publicação PyPI.
Isso substitui a exigência anterior de concluir todo P0–P6 antes de entregar.

Escopo ativo:

- Preservar a API e as correções verificadas; sync já existe e significa chamada
  bloqueante normal, sem `await`. Não reimplementar nem remover essa superfície.
- Gerar wheels release com extensão, runtime Firebird e licenças por plataforma.
- Executar verificação de instalação/relocação e consulta real no CI por alvo.
- Preparar publicação via GitHub Actions/PyPI Trusted Publishing por release/tag.
- Documentar configuração que pertence à conta PyPI e distinguir workflow local
  de jobs executados e pacotes efetivamente publicados.

Extração paralela, novas métricas, novas APIs e matriz extensa de benchmarks ficam
como backlog. A rodada comparativa já em execução será preservada com resultados
honestos; não se inicia outra frente de desempenho antes da distribuição.

O alvo já demonstrado é macOS arm64 / CPython 3.14.5. O empacotador existente
rejeita outros SOs: a matriz CI exige adaptar runtime e identificação de alvo,
não apenas criar YAML. Linux/Windows só serão chamados de verificados após
executarem seus próprios builds, import/relocação e smoke de consulta.

Não houve autorização para commit/push deste worktree ou publicação imediata.
O workflow publicado pelo autor poderá emitir os pacotes pela tag/release. Usar
OIDC do [Trusted Publishing](https://docs.pypi.org/trusted-publishers/using-a-publisher/)
evita pedir ou armazenar um token PyPI de longa duração.
