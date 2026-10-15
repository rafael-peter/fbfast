# Primeiro alvo e bootstrap

Confirmado pelo autor: macOS arm64 deste computador. O `python3` global é 3.15,
fora da matriz 3.11–3.14 do plano. O primeiro artefato usa CPython 3.14 com GIL;
não se afirma compatibilidade com 3.15, abi3, free-threaded ou outros alvos.

Toolchain fixado: Rust 1.91.1, PyO3 0.27.2, maturin 1.10.2. O bootstrap da sessão
instalou Rust em `.tools/` e Python packages em `.venv/`, sem editar o perfil do
shell. Código e scripts continuam utilizáveis com instalações convencionais.

Cliente local: Firebird 5.0.3 (framework oficial instalado). Copiamos os recursos
do cliente, dependências ICU/tommath/tomcrypt e charset/timezone; não incluímos
servidor, engine embedded, security database, configurações/credenciais do host.
As referências Mach-O são reescritas para `@loader_path` e reassinadas ad hoc.
O carregador valida que um único caminho canônico foi selecionado no processo.

O layout `_runtime/lib` respeita a descoberta pelo endereço da própria dylib
implementada no [Firebird 5.0.3](https://github.com/FirebirdSQL/firebird/blob/v5.0.3/src/yvalve/config/os/darwin/config_root.cpp).
O verificador exige que o root informado pelo cliente esteja dentro da cópia.

A wheel é construída por maturin; a etapa de bundle adiciona runtime e manifesto
e usa WheelFile para regenerar RECORD. A pasta entregue é extraída dessa wheel
final, evitando duas variantes do pacote. Não há downloads durante build/import.

O primeiro teste de import revelou `mis-aligned LINKEDIT string pool` no dyld.
Mantemos `strip = "none"` nos perfis Cargo: a remoção de debug info pelo Rust
produziu Mach-O desalinhado com o linker instalado. É o defeito documentado em
[rust-lang/rust#157750](https://github.com/rust-lang/rust/issues/157750).
Isso preserva release/otimizações; tamanho de binário não prevalece sobre import.

O consumidor temporário comprova distribuição. A homologação no projeto real do
autor continua separada; nenhum projeto consumidor específico foi informado.
