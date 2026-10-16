# Limite de materialização e fronteira de segurança

Decisão de 2026-10-16.

`max_result_bytes` volta ao contrato público com padrão de 512 MiB por resultado
materializado e `None` como desligamento explícito. A contagem inclui buffers
retidos de rows/valores, o JSON serializado e a memória dos batches Arrow. Ela
usa soma verificada e falha com `ResultLimitError`, sem devolver resultado
parcial. Streams continuam limitados por batch, prefetch, BLOB, backpressure e
deadline, pois um limite cumulativo impediria exportações grandes feitas de
forma incremental.

O orçamento não é um limite exato de RSS. Objetos Python, validação Pydantic,
buffers temporários, allocator e materializações concorrentes podem consumir
mais memória. O padrão protege contra resultados acidentais muito grandes sem
ser baixo para o uso comum; aplicações com envelopes diferentes devem escolher
um valor próprio.

Esta decisão substitui somente o trecho da decisão
`0009-config-ergonomics.md` que removeu `max_result_bytes`. O restante do
histórico e do contrato de configuração permanece válido.

Os três controles de leitura — validação lexical, tipo do statement preparado e
TPB `READ ONLY` — continuam obrigatórios. Eles não formam um sandbox de SQL:
rotinas, funções, UDRs, generators e tabelas temporárias podem ter efeitos fora
do que um rollback desfaz. A fronteira para SQL não confiável é uma allowlist na
aplicação combinada com usuário/role mínimos, sem `EXECUTE` ou `USAGE`
desnecessários. O Firebird 5.0.3 continua aceito como servidor. O cliente
empacotado permanece em 5.0.3: o teste de um cliente 5.0.4 contra servidor 5.0.3
revelou incompatibilidade com identificadores de fuso nomeados. A combinação
inversa, cliente 5.0.3 contra servidores 5.0.3 e 5.0.4, preservou esse contrato e
é a matriz adotada para a release.
