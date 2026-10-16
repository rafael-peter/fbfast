# DECFLOAT exato nas consultas e no bind

DECFLOAT(16) e DECFLOAT(34) usam IDecFloat16/IDecFloat34 do SDK oficial.
A ponte copia os bytes para estruturas alinhadas, sem interpretar manualmente
o formato decimal IEEE. O texto devolvido pelo cliente é limitado por buffer;
nenhum caminho usa float binário intermediário. As interfaces utilitárias são
emprestadas do runtime, usadas somente no worker dono da conexão.

O decoder de células compartilhado recebe um NativeReader para BLOB e DECFLOAT.
Linhas conservam o texto decimal em memória própria até criar Decimal na borda
Python; JSON escreve diretamente da célula. Prazo e cancelamento continuam no
executor comum. Nenhum handle decimal escapa para Python ou sobrevive ao runtime.

O bind de Decimal preserva texto, expoente, sinal de zero e valores especiais
até conhecer o tipo SQL preparado. Para NUMERIC/DECIMAL fixos, o core reescala
exatamente para INT128 e rejeita fração excedente, overflow e não finitos.
Para DECFLOAT, converte com o cliente e reconverte para comparar coeficiente e
expoente sem arredondamento. Rejeita perda numérica, underflow, overflow,
alteração de sinal, de NaN/sNaN ou de payload. Zeros também preservam o expoente;
um zero fora da faixa não é silenciosamente limitado pelo codec.

Coeficientes não nulos podem perder zeros finais para caber na precisão do tipo,
desde que mantenham exatamente o valor. Essa mudança de representação é explícita:
por exemplo, Decimal com quarenta zeros após a vírgula pode voltar com menos zeros.
Valores que já cabem conservam os zeros na leitura. Erros de bind mostram apenas
índice/tipo do parâmetro, sem incluir o conteúdo citado por erros do codec.

Python recebe Decimal independentemente da precisão e dos traps do contexto ativo.
JSON usa string por padrão; o modo number escreve representação decimal validada
com expoente, conservando zero negativo. NaN, sNaN e Infinity causam erro por
padrão, inclusive no modo string; json_non_finite="null" autoriza convertê-los em
null. O consumidor de JSON number pode perder precisão ao interpretar a saída.

Os 29 casos de integração incluem leitura de valores armazenados, extremos das
duas precisões, subnormais, NULL, zero negativo, NaN/sNaN com payload e sinal,
bind exato e rejeições, regressão de NUMERIC fixo, JSON sem construir Decimal,
streams e snapshots sync/async. Uma propriedade Rust cobre a reescala para INT128
com coeficientes aleatórios, escalas de -38 a 0 e zeros finais redundantes.

Isso fecha o caminho de linhas e JSON para DECFLOAT no alvo local. A representação
Arrow/Polars com metadados de origem continua pendente junto dos sinks analíticos.
Conversão e capacidades de outros clientes/servidores ainda precisam da matriz
de homologação prevista no plano.

Referências primárias:
[interfaces utilitárias do Firebird](https://firebirdsql.org/file/documentation/release_notes/html/en/4_0/rlsnotes40.html)
e [contrato DECFLOAT](https://www.firebirdsql.org/file/documentation/chunk/en/refdocs/fblangref50/fblangref50-datatypes-floattypes.html).
