# Snapshot público no mesmo executor

`Pool.snapshot()` e `snapshot_async()` criam context managers lazy. Ao entrar,
o scheduler reserva um slot pela mesma fila e orçamento de aquisição da pool.
O próprio worker mantém a transação READ ONLY SNAPSHOT e recebe as operações
da sessão por um canal de capacidade um. Não há pool, attachment coordenador,
executor de SQL ou thread extra por sessão.

As consultas e streams usam os mesmos pumps, conversores e limites de entrega
das consultas normais. O binding seleciona pool ou sessão; a superfície Python
compartilha a delegação de consultas em `queries.py`. O permit exclusivo da
sessão vive até o fim da conversão Python e, para streams, até o fechamento.
Erro de execução ou cancelamento aborta a sessão; não se inicia uma transação implícita
substituta. Consultas diretamente na pool permanecem independentes.

O prazo do snapshot começa depois da aquisição e inclui os intervalos entre
operações. Uma consulta usa o menor prazo entre seu próprio timeout e o da
sessão. A thread de controle do slot cancela a geração ativa; durante ociosidade,
o worker verifica o prazo e fecha a transação sem esperar nova chamada Python.
Timeouts não são contados novamente no envelope da sessão. O último cleanup e
a invalidação de cache pendente terminam antes da liberação do slot.

Drop apenas sinaliza responsabilidade para o worker. O wrapper Python também
solicita fechamento sem I/O ao ser descartado, porque um stream filho retido
pode manter o executor nativo vivo. Guardas de PID precedem mutexes/handles;
nenhuma limpeza nativa é feita no filho após fork.

Verificação real no Firebird descartável:

- Identidade de attachment e transação permanece constante, com READ ONLY=1
  e SNAPSHOT=1; a mesma pool fora da sessão usa READ CONSISTENCY=4.
- Outra conexão atualiza e confirma 10→20. Consultas e streams da sessão seguem
  vendo 10; consultas independentes e posteriores ao encerramento veem 20.
- Consulta e stream concorrentes são rejeitados, inclusive durante criação
  lenta de objetos Python. Fila normal e invalidação de cache aguardam o lease.
- Deadline ativo/ocioso/conversão, erro SQL, falha de abertura, cancelamento
  em fila, abandono e fork não liberam o slot antes do cleanup.
- Descartar a sessão com stream filho ainda referenciado também libera o slot.

Contagens, fingerprint e resultados da suíte estão em `../progress.md`.
Compartilhamento de snapshot entre attachments para extração paralela ainda é
trabalho de P5; esta API pública reserva exatamente uma conexão.

Durante esta validação, os testes Rust de falha de rede produziram um firebird.log
no runtime entregue e o verificador estrito rejeitou a pasta. O harness passou
a usar uma cópia temporária do runtime, como já faz o consumidor Python. O
verificador mantém a comparação exata de inventário e hashes; nenhum arquivo
extra foi permitido para esconder esse efeito colateral dos testes.

Na validação dos tipos com fuso, o teste de cancelamento durante conversão Python
revelou uma corrida: request_close publicava closing antes de cancelar a entrega
ativa, e o worker podia retornar sucesso sem consultar esse cancelamento. Agora
o sinal de cancelamento precede a publicação de closing, e ambos os caminhos de
saída conferem a operação ativa e, depois, o cancelamento do escopo. Essa ordem
também cobre Permit::drop, que aborta o escopo antes de retirar a operação ativa.
O fechamento também verifica prazo expirado, mesmo antes do próximo polling do
worker, sem tratar o próprio estado closing como um erro de uma sessão ociosa.
Um teste Rust mantém o permit de entrega retido e verifica cancelamento/timeout
no fechamento; fechamento ocioso continua normal. A asserção do teste Python
original foi preservada.
