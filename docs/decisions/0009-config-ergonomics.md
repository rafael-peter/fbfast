# Configuração automática, limite por linhas e negociação de criptografia

Por orientação do usuário em 2026-10-15, o uso normal é `FirebirdConfig(...)`,
com resolução por campo: argumento explícito > FB_* do ambiente > .env > padrão.
Essa precedência já existia no core; os testes agora também exercitam o construtor
sem argumentos, campos obrigatórios e ausência de fallback para valores inválidos.
None explícito continua desativando limites opcionais mesmo se houver variável.
`from_env` permanece apenas como atalho compatível para um prefixo personalizado.

O stub distribuído contém tipos específicos e documentação por parâmetro:
função, variável de ambiente, limites e padrão. Omissão usa `...` na assinatura,
sem fingir que um valor padrão literal foi explicitamente passado ao Rust.
read_only aceita somente True na API tipada; env_file aceita str ou False;
wire_crypt oferece três literais e os atalhos booleanos. A documentação e os
tipos estão no mesmo pacote do binário, com py.typed. Cada descrição de parâmetro
fica numa única linha lógica: o probe do Pyright mostrou que quebras no formato
anterior faziam a ajuda específica exibir apenas a primeira linha. As 35 descrições
foram consultadas individualmente via LSP, incluindo variável de ambiente e padrão.

Também por instrução explícita do usuário, `max_result_bytes` foi removido do
core, bindings, stub e caminho de materialização. Essa decisão substitui o
orçamento cumulativo de bytes da seção 6.3 do plano original. O resultado completo
usa somente `max_result_rows`; ultrapassá-lo gera erro sem truncar silenciosamente.
Streams não aplicam esse limite total. batch_bytes, prefetch, limite individual
de BLOB planejado e orçamento de cache têm finalidades diferentes e permanecem.
As estatísticas ainda contabilizam bytes, sem afirmar ganho de desempenho medido.

Chamadas antigas com max_result_bytes geram ConfigurationError/unknown option;
FB_MAX_RESULT_BYTES deixa de ser uma variável reconhecida. Como os demais nomes
FB_* desconhecidos, ela é ignorada. Não há alias ou opção obsoleta silenciosa.

O padrão wire_crypt continua required, com True equivalente a required e False
a disabled. Enabled negocia criptografia e permite conexão sem ela; não é uma
garantia de tráfego criptografado. Não há fallback automático a partir de required.
O erro nativo 335545064 preserva SQLSTATE/códigos e passa a explicar a combinação
incompatível e o significado de enabled.

O harness usa três bancos distintos no mesmo container descartável, com políticas
WireCrypt Required, Enabled e Disabled. São bancos de teste próprios porque o
Firebird rejeita configurações diferentes para aliases do mesmo arquivo físico.
Os testes usam MON$WIRE_ENCRYPTED para verificar a negociação efetiva nos três
modos e seus atalhos booleanos. Nenhuma configuração do servidor operacional do
usuário foi inspecionada ou alterada.

Referências: [contrato de typing do Pyright](https://github.com/microsoft/pyright/blob/main/docs/typed-libraries.md)
e [WireCrypt do Firebird](https://www.firebirdsql.org/docs/html/en/refdocs/fbconf/firebird-configuration-reference.html).
