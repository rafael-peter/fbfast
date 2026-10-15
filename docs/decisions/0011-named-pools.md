# Pools nomeadas e delegação pública

O registro Python sincroniza apenas nomes e estados (`opening`, `ready`,
`closing`). Cada entrada contém a própria `Pool`; consultas e streams continuam
usando o scheduler, fila, conversores, caches e limites existentes em Rust.
Não há executor nem event loop adicional para o registro.

O nome é reservado atomicamente antes do warmup. Somente o sucesso do warmup
permite publicação. Duplicatas são rejeitadas também durante abertura e drenagem.
`get_pool` disponibiliza apenas entradas prontas. Config inválida/role inválida,
cancelamento ou fechamento concorrente não deixam uma pool parcialmente pronta
visível. `min_idle=0` ainda valida uma conexão, conforme o warmup existente.

Fechar marca a entrada indisponível e solicita cancelamento nativo antes de
esperar. O mutex do registro nunca é mantido durante rede ou await. Vários
fechamentos podem aguardar a mesma pool; a remoção confere identidade para que
uma chamada atrasada não remova uma nova instância de mesmo nome. Fechar durante
abertura impede a publicação dessa entrada, mesmo que o warmup retorne depois.

Se cleanup falhar, expirar ou receber outro cancelamento, o nome permanece
reservado até um novo `close_pool*` terminar. A exceção original da abertura é
preservada; falhas adicionais têm nota e, para `FbFastError`, causas estruturadas.
Não liberar a reserva evita substituir recursos ainda em drenagem. Não há task
oculta nem limpeza dependente de um loop global que pode já ter sido fechado.

O PID é verificado antes do mutex Python. Import/config antes de fork e registro
vazio permitem a primeira inicialização no filho, usando mutex novo. Se houver
entradas herdadas, toda operação de registro falha com `ForkSafetyError` sem
tocar esses locks/handles. Isso não promete recuperação de fork com runtime
nativo ativo. As instâncias locais continuam com seus guards nativos existentes.

As funções do módulo têm assinaturas tipadas explícitas. Timeout omitido é
encaminhado como omitido, preservando o `query_timeout` da pool; `None` continua
desativando esse prazo. A fábrica de stream resolve o nome imediatamente e
guarda a instância correspondente; trocar o nome depois não redireciona um stream.

A validação usa o artefato instalado/copiado e o Firebird descartável. Casos
incluem concorrência sync/async, cancelamento durante attach real, timeout com
slot ainda ocupado, abertura/fechamento concorrentes, duas event loops
sucessivas, nomes isolados, limites/cache compartilhados e guards de fork.
O teste HTTP complementar abre sockets reais via FastAPI 0.143.2, Starlette 1.8.0,
Uvicorn 0.54.0 e AnyIO 4.15.1, no caminho ASGI HTTP 2.3. Cinco casos verificam
desconexão durante fetch ativo e espera do consumidor, erro após headers sem
chunk terminal falso, consumo completo e shutdown com corpo ativo. A pool fica
no lifespan; o gerador contém o contexto de stream. Referências ao gerador e ao
stream ficam retidas para evitar atribuir ao GC o cleanup observado. A conexão
volta a atender consultas e o shutdown fecha todos os slots/remove o registro.

Essa evidência inicial era de lifecycle e transporte de linhas. O sink JSON
nativo e a extensão dos testes para NDJSON/array estão em `0012-native-json.md`.
Outras versões, servidores e o
caminho ASGI 2.4+ precisam de verificação própria, incluindo finalização explícita
do gerador caso a implementação de StreamingResponse não a garanta.
