# Cancelamento independente e sinais do processo

Alvo verificado: macOS arm64, CPython 3.14 com GIL, cliente/servidor Firebird 5.0.3.

Cada worker ativo tem no máximo uma thread de controle, uma operação armada e
um comando de cancelamento. Não há fila ilimitada de cancelamentos ou thread
por chamada. O identificador de geração impede que um pedido atrasado atinja
a consulta seguinte. Um gate nativo mantém o attachment vivo durante
`IAttachment::cancelOperation(fb_cancel_raise)` e espera seu retorno antes de
cleanup/detach. O executor normal não segura esse gate durante a consulta.

O controlador pode vencer o prazo sem polling do consumidor. O timeout nativo
do statement também é habilitado; seus códigos oficiais são mapeados para
`QueryTimeoutError`. A primeira origem do cancel define o motivo e evita
contagem dupla. Após uma tentativa nativa de cancel, a conexão é descartada
conservadoramente. Nenhuma consulta é repetida automaticamente.

O deadline usa o mesmo instante para execução e entrega. Conversão Python faz
checkpoints por células e antes/depois do resultado. A espera síncrona por
consulta e por close libera o GIL e verifica sinais a cada fatia de até 25 ms;
isso é uma cadência de verificação, não promessa de latência máxima física.
Uma conversão individual ou chamada nativa pode durar mais.

O teste de Ctrl+C revelou que o cliente oficial instala um multiplexador POSIX
e tenta desligar todos os seus subsistemas quando recebe SIGINT/SIGTERM. Foi
registrado o callback oficial `fb_shut_confirmation`, vetando apenas
`fb_shutrsn_signal`. O shutdown de exit/unload continua permitido. A assinatura
e constantes são compiladas contra o SDK.

O multiplexador preserva handlers chamáveis, mas não encadeia SIG_DFL/SIG_IGN.
Para conservar término padrão, antes da primeira entrada no dispatcher a ponte
substitui somente SIG_DFL de SIGINT/SIGTERM por uma função que reinstala SIG_DFL
e reenvia o sinal. Ela usa apenas operações POSIX async-signal-safe, sem Python,
Firebird, alocação ou locks. Handlers personalizados são encadeados pelo cliente;
SIG_IGN permanece sem efeitos porque o shutdown é vetado. O aplicativo pode
instalar outro handler depois. Esta política é process-wide, como o carregador
único do cliente, e foi testada também em subprocessos com término real por sinal.

Evidências automatizadas:

- Cancel nativo em consulta pesada com timeout do servidor desativado; gerações
  antigas e handle de controle após o fechamento não atingem nova operação.
- Pool respeita teto/FIFO, descarta attachment cancelado e aceita nova consulta.
- Deadline autônomo, panic do worker com substituição segura e attach TCP sem
  resposta mantendo capacidade ocupada depois de aquisição/close expirarem.
- Ctrl+C preserva o tipo Python e não inutiliza o cliente; SIGINT/SIGTERM nos
  modos padrão/ignorado/personalizado exercitados em seis subprocessos.
- Deadline durante conversão Decimal lenta e Ctrl+C durante close de attach
  pendente, com liberação real após o peer ser fechado.
- `benchmarks/run_cancel_probe.py` registra dez amostras de interrupção do
  chamador e liberação observada do slot, com build_id e consulta de recuperação.

Limite: partição de rede pode prender attach, cancel ou cleanup. A capacidade
permanece ocupada e as threads continuam contadas; não há detach concorrente,
liberação antecipada de memória nem substitutos ilimitados. Isto não homologa
todos os cenários de falha de rede, nem os caminhos async ainda ausentes.

Fontes primárias:

- [Contrato de cancelamento](https://github.com/FirebirdSQL/firebird/blob/v5.0.3/doc/README.fb_cancel_operation).
- [Callback de shutdown](https://github.com/FirebirdSQL/firebird/blob/v5.0.3/doc/README.fb_shutdown).
- [Dispatcher e inicialização dos sinais](https://github.com/FirebirdSQL/firebird/blob/v5.0.3/src/yvalve/why.cpp).
- [Multiplexador POSIX](https://github.com/FirebirdSQL/firebird/blob/v5.0.3/src/common/os/posix/isc_ipc.cpp).
