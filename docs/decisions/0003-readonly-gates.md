# Leitura: política lexical e TPB, além do tipo do statement

O teste real contra Firebird 5.0.3 mostrou que DML com RETURNING pode ser
classificada como `isc_info_sql_stmt_select`. Portanto, esse código de tipo não
prova ausência de escrita. A mudança de RETURNING é documentada nas
[notas oficiais do Firebird 5](https://www.firebirdsql.org/file/documentation/release_notes/html/en/5_0/rlsnotes50.html).

O executor valida a SQL antes de abrir transação/preparar, verifica o tipo
preparado e sempre usa TPB READ ONLY. O teste nativo também comprova rejeição
efetiva da tentativa de DML no servidor. A política lexical bloqueia DML,
locks/atualização e operações diretas de sequence conhecidas.

Isso não transforma SELECT em sandbox de funções externas ou rotinas
privilegiadas; permissões do usuário/role continuam fazendo parte do contrato.
Nenhuma consulta enviada é repetida automaticamente para recuperar falhas.
