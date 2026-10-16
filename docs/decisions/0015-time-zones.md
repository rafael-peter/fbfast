# UTC e zona preservados nos tipos temporais

O decoder aceita TIME WITH TIME ZONE e TIMESTAMP WITH TIME ZONE pelas estruturas
ISC_TIME_TZ/ISC_TIMESTAMP_TZ do SDK. A ponte copia a mensagem para armazenamento
alinhado e usa IUtil para obter a zona e a representação local oficial. Os campos
UTC são retidos separadamente; não são reconstruídos a partir de horário local.
Buffers, larguras e terminador são verificados; padding enviado ao cliente é
zerado explicitamente. Nenhum ponteiro escapa para os sinks.

Timestamp em linhas vira datetime aware com datetime.timezone para offset fixo
e ZoneInfo para região. A conversão parte de UTC, preservando fold em horários
repetidos. O resultado Python é comparado à data/hora local do cliente oficial:
zona indisponível, overflow ou divergência das regras geram DataConversionError.
Não há fallback silencioso para UTC ou offset quando isso descartaria a região.
Arrow/JSON devem preservar UTC e zona independentemente da disponibilidade de
ZoneInfo; JSON já segue esse caminho, Arrow permanece pendente.

TIME WITH TIME ZONE usa o dataclass público TimeWithZone(utc_time, zone), frozen
e com slots. utc_time é time sem tzinfo que representa o relógio UTC, precisão
de 100 microssegundos, fold=0. Isso evita atribuir data ou regras de verão a um
valor que não contém data. A conversão local feita pelo servidor usa a referência
2020-01-01 definida pelo Firebird; esse detalhe não vira uma data no objeto.

No bind, timestamp aware aceita ZoneInfo com chave e datetime.timezone. O Python
retém instante UTC e horário local esperado. O worker resolve a zona pelo codec
oficial e grava os campos UTC na estrutura do SDK; não recodifica um horário local
ambíguo, o que perderia fold. Reconverter e comparar o horário local rejeita gaps
de DST e divergências de tzdata. Offset fixo exige minutos inteiros e domínio
de -14h a +14h. UTC fora do domínio e precisão menor que 100 microssegundos
geram erro. TimeWithZone faz bind diretamente de UTC e zona; time aware comum
exige conversão explícita do usuário, pois não fornece uma data para DST.

Os campos de timezone na representação nativa de timestamp ficam num Box;
outros escalares não passam a pagar por esse payload na enum. O orçamento de
lotes inclui a estrutura própria e o nome de zona, além da mensagem de fetch.
JSON escreve objetos {utc, zone} e {utc_time, zone} diretamente por célula,
sem construir datetime/time/TimeWithZone Python. Timestamp UTC termina em Z;
TIME conserva somente o relógio UTC, sem uma data inventada.

Testes exercitam valores armazenados, offsets fracionários, virada de dia, datas
extremas, NULL/vazio, retenção após fechamento, snapshots e streams sync/async.
Casos de Nova York e Lord Howe distinguem duas ocorrências de horário local e
rejeitam horários inexistentes. Regras ausentes/divergentes no Python são
injetadas para verificar erro em linhas e continuidade do caminho JSON.
O codec é exercitado em todos os minutos de offset fixo suportados, nos limites
de relógio, além de largura inválida de buffer, padding e zona inválida.

Referências primárias:
[tipos temporais Firebird 5](https://www.firebirdsql.org/file/documentation/chunk/en/refdocs/fblangref50/fblangref50-datatypes-datetime.html),
[layout dos valores](https://www.firebirdsql.org/file/documentation/html/en/firebirddocs/firebirdinternals/firebird-internals.html)
e [utilitários do SDK](https://github.com/FirebirdSQL/firebird/blob/v5.0.3/doc/Using_OO_API.md).
