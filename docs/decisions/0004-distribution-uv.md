# Distribuição versionada com uv/pip

Em 15/10/2026 o usuário propôs repositório público e instalação por uv/pip,
substituindo a cópia manual como caminho principal. O objetivo funcional P0–P6
permanece. O repositório público e a primeira tag ainda aguardam criação/endereço.

O backend PEP 517 `build_backend.py` reutiliza o mesmo empacotamento do wheel
com runtime, manifesto e licenças. `uv add` de checkout/Git compila em release;
o consumidor precisa de Rust, C++ e SDK Firebird durante esse build. Instalação
de wheel pronto dispensa essas ferramentas. A pasta copiável é extraída desse
mesmo wheel e continua servindo à verificação de relocação.

Recomendação: fontes e tags no Git, wheels versionados para os ambientes
homologados. A origem Git pode ser fixada por tag/commit no projeto e no lock,
conforme a [documentação do uv](https://docs.astral.sh/uv/concepts/dependencies/#git).
Nenhum comando deste projeto publica pacotes ou cria releases remotas.

Wheels continuam dependentes de SO/arquitetura e, por enquanto, da versão menor
de CPython. `abi3` permitiria compartilhar a extensão entre versões de Python,
com restrições/possível custo que precisam ser medidos antes da decisão; não
elimina a matriz de SO/arquitetura. Ver [PyO3 0.27.2](https://pyo3.rs/v0.27.2/building-and-distribution#py_limited_apiabi3).

`scripts/verify_install.py` usa um Git temporário local, um commit fixo e dois
consumidores uv independentes (wheel e fontes). O processo final recebe PATH
sem Rust e sem variáveis de loader/Firebird e deve identificar o cliente e seu
root dentro do pacote instalado. Isso não prova publicação/instalação de uma
URL pública ainda inexistente, nem homologa plataformas não testadas.
