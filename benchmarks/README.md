# Benchmarks locais do fbfast

Os comandos abaixo aceitam apenas o banco descartável identificado pelo harness
deste checkout. Não recebem DSN externo. `--prepare` cria uma tabela sintética de
1 milhão de linhas nesse banco; nunca usa o banco operacional do consumidor.

```sh
uv pip install --python .venv/bin/python -r benchmarks/requirements.txt
.venv/bin/python scripts/firebird_fixture.py start
.venv/bin/python scripts/verify_vendor.py --package dist/vendor/fbfast
.venv/bin/python benchmarks/run_comparison.py --prepare --samples 3
.venv/bin/python benchmarks/run_comparison.py --samples 3 --cases narrow1m
.venv/bin/python benchmarks/run_comparison.py --samples 3 --cases wide16 wide64
.venv/bin/python benchmarks/run_comparison.py --samples 3 --cases narrow100k \
  --formats tuples --fetch-sizes 256 1024 5000 16384
.venv/bin/python scripts/firebird_fixture.py stop
```

Não executar junto de builds, testes com heartbeat ou outros benchmarks. O runner
copia o artefato release para uma pasta temporária e verifica o hash do cliente.
Todos os drivers carregam esse mesmo cliente e runtime, usando uma conexão cada,
UTF8 e transações READ ONLY / READ COMMITTED READ CONSISTENCY / lock timeout 5 s.
A transação dos baselines é conferida em `MON$TRANSACTIONS` antes da medição.
O fixture exige criptografia. A configuração pública do fbfast não é modificada.

Cada amostra usa um processo novo, import/attach/prepare separados, um warmup com
a consulta inteira, seguido da consulta medida. A ordem dos drivers é invertida
nas rodadas alternadas. O resultado medido inclui todo o fetch, conversão e cleanup;
Polars termina com o DataFrame pronto. O baseline JSON usa orjson 3.11.3. Tuplas e
dicts continuam objetos Python. Os casos atuais só incluem inteiros, UTF8 e NULL,
portanto não pressupõem equivalência de políticas para Decimal/BLOB/fusos.

`firebird-driver` reutiliza conexão e statement, fecha cursor e faz rollback.
`firebird-driver-reset` também executa `ALTER SESSION RESET`, como o fbfast. O reset
usa a interface nativa do driver fixado em 2.0.3, pois `execute_immediate` público
abriria outra transação. Não se atribui ao fbfast vantagem sobre reconexão por
chamada. FDB usa sua própria preparação e fetchmany, sem reset adicional.

`fdb` executa o pacote original 2.0.4. Neste macOS arm64 ele falhou em
`isc_start_transaction` com `invalid database handle`. `fdb-arm64` corrige apenas a
declaração ctypes dessa chamada no processo de benchmark: três argumentos fixos e
cauda variádica, como o `ibase.h` do SDK. O pacote instalado não é editado. Esta
diferença é explícita e não autoriza afirmar suporte geral do FDB a Firebird 5.
Ver [ctypes no Apple arm64](https://docs.python.org/3/library/ctypes.html#calling-variadic-functions)
e [preparação/reuso no firebird-driver](https://firebird-driver.readthedocs.io/en/stable/usage-guide.html#prepared-statements).

A fórmula dos dados e o SQL estão em `fixture.sql` e `cases.py`; não há seed
aleatório. Após a janela medida, todas as linhas são comparadas com um oráculo
independente, incluindo tipos Python exatos, contagem, ordem, multiplicidade,
Unicode e NULL. A validação não usa `assert` e continua ativa com `python -O`.
São gravados checksums de todas as linhas e dos bytes JSON. Os tipos Arrow/Polars
também são conferidos; só o fbfast adiciona metadados de origem SQL.

O controlador amostra RSS externamente a cada 5 ms até o resultado estar pronto,
antes da validação. `ru_maxrss` é registrado separadamente e inclui import/warmup.
O pico amostrado inclui runtime e memória retida pelo allocator; não é uma soma
dos buffers, nem uma medição de alocações. Picos menores que 5 ms podem escapar.
CPU do processo é medida dentro da consulta; contadores cgroup do servidor são
lidos nas barreiras e incluem o pequeno overhead do próprio `docker exec`.
Memória do servidor é do container inteiro; seu pico é acumulado desde o start.

Cada execução preserva manifesto, versões, hashes do harness/fixture/artefato,
SQL/parâmetros, JSON/CSV brutos, falhas e relatório em `results/`. Três amostras
permitem medianas descritivas, **não p95/p99**. TCP local passa pela VM OrbStack;
esses números não provam desempenho em LAN/WAN, saturação, cancelamento ou tipos
modernos. As demais dimensões da seção 17 do plano continuam exigindo medições.
