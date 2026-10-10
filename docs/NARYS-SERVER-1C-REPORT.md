# NARYS-SERVER-1C — Remote Operations CLI & Secure Credential Unlock

**Resultado: PASS técnico-funcional da SERVER-1C, candidato à auditoria independente.** CLI instalada e serviço operacional; consultas reais de histórico/resultados verificadas após reinício. Desbloqueio validado no GNOME50 real com cofre sintético e controles de terminal testados separadamente. Não se declara novo desbloqueio pessoal via SSH, gate celular/cold boot, ferramentas agentivas, recuperação da GUI ou PASS da SERVER-1D. A SERVER-1D não foi iniciada.

## Controle e referências

Repositório `Samuel-Francisco-AS/Narys`; única branch `narys-server-1-headless-runtime`. HEAD inicial conferido, limpo e igual ao remoto: **`4ad89d14e1666647dd92b1737a81c716d057692d`**. Main local permanece **`553b51182bb477d0b777093da5bfda93239fddf9`**, sem alterações/merge/push. Implementação instalada e publicada na branch: **`45a15d1e3cdb0e8c6afe5340758fde602061521b`**. Commit posterior registra este relatório, evidências do host e um caso sintético adicional de pin incompatível; não muda o código de produção instalado. HEAD remoto final da entrega será conferido após esse push e informado na resposta final, como nas entregas anteriores.

Prazo absoluto preservado: **12/10/2026 às 15:03:12 America/Recife**. Abertura da trilha permanece 10/10 às 15:03:12; nenhum relógio foi reiniciado. Serviço instalado e verificado em **10/10/2026, aproximadamente 17:43 (-03)**, com mais de 45 horas restantes. Não foram criadas etapas extras.

Fontes revisadas: relatórios e auditorias independentes 1A/1B, arquitetura Core-First de 10/10, planejamento SERVER-1 atualizado, README e IPC v1. A solução conserva o domínio compartilhado, a Conversation Engine, Scheduler, SecretStore e o SQLite autoritativo. Não há nova camada cognitiva, novo banco, novo cofre ou migração de schema.

## Implementação e decisões

`narys-core/src/bin/narys.rs` é a CLI Rust oficial. `client.rs` consolida o cliente IPC v1, utilizado também pelo cliente legado `narys-core`. O cliente só apresenta/solicita operações: não lê SQLite, não registra workers nem guarda estado operacional. Verifica UID do servidor, diretório runtime privado, versão/request_id, request16KiB/response256KiB e timeouts. Uma tentativa por request; não repete mutações após falha incerta. Quando o shell SSH não exporta XDG_RUNTIME_DIR, descobre `/run/user/UID`, sem depender de HOME/checkout para consultar o serviço.

Comandos entregues, documentados em [CLI.md](../narys-core/CLI.md):

- Status, doctor e capabilities; doctor apenas compõe consultas locais, sem inferência.
- Sessions com páginas; criar, consultar, selecionar/reabrir e fechar sessão.
- Chat interativo: seleção/criação, páginas com `more`, envio, recibo durável, acompanhamento, resultado e comandos `/history [cursor]`, `/task ID`, `/cancel ID`, `/exit`.
- `send SESSION_ID` recebe texto por stdin, limitado a4096 bytes. A CLI verifica locked/unavailable antes do envio; o Core continua revalidando credenciais/permissões na execução.
- Tasks paginadas por namespace, task/result por ID, `--wait --timeout`, cancel idempotente, eventos com cursor/gap/retention.
- Providers com política, permissões, presença booleana, admission/rate/resilience/telemetry; models com defaults locais e aviso factual de que catálogo remoto não foi consultado.
- Provider enable/disable com confirmação gratuita exigida pelo contrato existente; política Conversation show/set por JSON tipado, validado pelo Core.
- Approval approve-once/deny, preservando **`capability_not_integrated`**; nenhuma execução simulada ou autoridade HumanLocal transmitida.
- Credentials status/unlock. JSON estruturado para operações pertinentes; exit1 em erro, sem duplicar envelopes de recusa. Texto não confiável recebe escape de controles de terminal.

Novas operações v1 `tasks` e `models` são consultas aditivas, descritas em [IPC.md](../narys-core/IPC.md). Tasks junta run/histórico sem duplicar IDs e retorna somente ID/namespace/state; resultados/prompts permanecem em consultas específicas. Limites1–100, cursor e `has_more` são verificados. Cursors tasks/events recusam overflow de i64 SQLite. Contratos anteriores, autenticação e limites do servidor permanecem preservados.

## Credential Unlock Manager

A API padrão [Secret Service](https://specifications.freedesktop.org/secret-service/latest-single/) entrega prompts pelo serviço e não define um método para receber master password num TTY. O [GNOME50 upstream](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/daemon/gkd-main.c) mostra que `--unlock` pode desbloquear **ou criar** o login keyring e tratar toda a entrada stdin como senha. Não foi escolhido: introduzir criação/inicialização ou password pipeline seria inadequado para este contrato existing-only.

A extensão GNOME50 anterior foi preservada e encapsulada em `credential_manager.py`, **incorporada no binário por include_str**. O processo Python3 recebe somente código fixo/operação pública; `-I` e ambiente limpo impedem imports/configurações do checkout. O operador usa `narys credentials unlock`; o cliente legado também encaminha ao manager incorporado. `credential_status.py` é apenas wrapper legado, sem import dos experimentos. O serviço atualizado não usa esse wrapper em execução.

Status consulta alias login/Locked/metadados, sem abrir Secret Service session, ler itens ou solicitar segredo. Retorna locked/unlocked/unavailable e compatibilidade/erro seguro do backend. Unlock não existe no Command IPC; variantes unlock/credentials-unlock/password/authority são recusadas pelo protocolo.

Antes de qualquer senha, o manager exige os três streams no mesmo `/dev/pts`, UID e permissões privadas, foreground process group, invocação da CLI seguida de shell interativo direto, e sessão logind com Remote=true, Service=sshd, Type=tty, Class=user, UID e TTY correspondentes. Shell `-c`, scripts, ancestrais de agentes, redirecionamentos, PTYs genéricos, tmux/screen e sessão inválida são recusados. Não confia em SSH_CONNECTION ou flags de origin fornecidas pelo chamador.

O destino é o **nome D-Bus único** do serviço já existente, sem autoactivation. Valida alias/coleção login existente, UID/PID, executável `/usr/bin/gnome-keyring-daemon`, hash GNOME50 auditado e cgroup do serviço de usuário. Revalida antes do efeito. Backend incompatível exige revisão do pin e produz recusa recuperável; não há fallback gráfico, inicialização de daemon ou criação de coleção.

Quando já unlocked, informa o estado sem pedir senha. Caso locked, a senha vai diretamente do TTY para buffer nativo mlock, sem eco/ECHONL, sem senha em String/bytes Python. Libsecret recebe SecretValue nativo e codifica Secret com **DH + AES128-CBC**, exigido pelo manager. Somente ciphertext entra em GLib.Variant/Python e na chamada específica `UnlockWithMasterPassword`; nunca passa pelo socket IPC Narys, LLM, chat, argv, env, arquivo, log ou history. Buffer é zerado, SecretValue liberado, sessão fechada e termios restaurado em sucesso/erro/INT/HUP/TERM. Core dumps/dumpability são desabilitados. Não se promete apagar memória interna do daemon/libsecret além das garantias dos respectivos backends.

Senha incorreta, indisponibilidade, sessão inválida, backend/pin incompatível e erro de transporte produzem códigos sanitizados. Nenhum segredo, exceção bruta ou traceback é publicado pelo dispatcher. Não houve senha pessoal digitada, capturada, registrada ou reutilizada; o cofre pessoal já estava desbloqueado desde a 1B. Nenhuma nova coleção, chave, snapshot, Stronghold ou política de senha foi persistida.

**Fronteira real:** mesmo UID Linux continua a fronteira local da 1A/1B. Os controles de admissão recusam automação ordinária e origem agentiva; não provam humanidade contra processo malicioso do mesmo UID que controle um shell/TTY SSH legítimo. Não se reivindica sandbox ou impossibilidade criptográfica de impersonação humana. A interface interna GNOME permanece não suportada: a melhoria é isolamento, uso de memória nativa/transport cifrado, checks e falha segura; não uma API upstream nova.

## Persistência, reconexão e evidência real

[Auditoria instalada](evidence/server-1c/host-installed.json) e [nova reconexão após restart](evidence/server-1c/host-reconnected.json) foram produzidas pelo comando instalado, com clientes novos por consulta, cwd `/tmp`, sem HOME/XDG/DISPLAY no ambiente do cliente. Paginação completa comparada ao SQLite autoritativo read-only:

| Estado verificado | Resultado |
|---|---|
| Sessões | **38**, todas recuperadas por IPC |
| Mensagens | **99**, igualdade exata de ID/role/content/timestamp em todas as sessões, sem exportar conteúdo na evidência |
| Tarefas de produto | **191** listadas sem duplicidades |
| Task189 / Task190 | completed; resultados reais Groq da 1B iguais ao resultado JSON autoritativo antes/depois do restart |
| Task191 | cancelled; resultado nulo preservado; [cancel repetido](evidence/server-1c/cancel-persisted.json) retorna already_terminal, sem efeito remoto |
| Banco | Schema20; quick_check=ok; contagens e runs iguais ao baseline |
| Stronghold existente | Bytes SHA-256 e inode/size/mode/mtime/ctime **inalterados** |
| Credenciais | unlocked, serviço disponível, backend compatível; nenhum unlock pessoal invocado |

Não houve nova inferência, chamada de billing, retry, fallback, compra ou overage. Groq conserva sua permissão anterior; os demais continuam desabilitados. Os resultados reais usados como prova foram produzidos pela SERVER-1B, não atribuídos a esta etapa.

Tasks são workers do Core. Receipt e estado duráveis sobrevivem ao cliente; Ctrl-C interrompe acompanhamento e não envia cancel implícito. Restart preserva os estados terminais e marca execução incerta interrupted, sem replay. Testes sintéticos cobrem disconnect/admission/cancel/recovery e request remoto incerto; evidências 1B já cobrem cliente encerrado durante execução real. Não foi aberto novo canal SSH/celular nem feita nova inferência para repetir essa prova.

## Testes e segurança

| Gate | Resultado / evidência |
|---|---|
| Core Rust final | **33 unitários +1 control +8 protocol =42 PASS**, zero falhas; [log](evidence/server-1c/core-tests.txt) |
| Python final | **16 PASS**:3 CLI,7 manager de credenciais,6 operacionais herdados; [log](evidence/server-1c/python-tests.txt) |
| GNOME50 real, cofre descartável | Sessão cifrada nativa, locked/unlocked status, senha incorreta recusada, correta desbloqueia, unlocked sem reprompt, pin incompatível e daemon fora do user-service recusados, bytes preservados; [prova](evidence/server-1c/synthetic-keyring.json) |
| Build/format | build locked/offline dos dois binários e fmt PASS; [build](evidence/server-1c/build.txt), [fmt](evidence/server-1c/fmt.txt); git diff --check PASS |
| Serviço instalado | status/doctor/models/credentials/sessions/tasks/result/cancel e reconexão PASS; fontes dos adapters temporariamente ausentes sem afetar credentials; [independência do checkout](evidence/server-1c/checkout-independent.json) |
| Unlock instalado por pipe/automação | Recusado antes de senha, exit1, JSON único, stderr vazio; [prova](evidence/server-1c/installed-unlock-denial.json) |

Gates incluem versão/request_id, request/response/input limit, paginação, diretório runtime inseguro, recusa de provider sem confirmação gratuita, capabilities futuras bloqueadas, agente/pipe/PTY sem SSH, propriedades logind/shell incompatíveis, password sem eco, buffer zerado e termios restaurado após sinal. O chat completo usa peer IPC sintético; testes de transporte simulam resposta incerta e verificam **uma única tentativa** de mutação.

Para o cofre descartável, um bus e HOME/runtime separados hospedam daemon real e coleção criada **somente pela fixture**. A fixture fornece senha sintética em memória e substitui gates humanos/identity apenas no código de teste; produção não aceita bypass por env/argv/IPC. Controles TTY/SSH são testados separadamente. Isso não é prova de um humano desbloqueando o cofre pessoal em SSH.

Primeira rodada identificou runtime de teste sem modo0700 e expectativa antiga do helper; os fixtures foram corrigidos sem enfraquecer produção. Dois filhos deixados pelo teste antigo após panic foram identificados por pidfd/UID/órfão/cgroup de execução e encerrados; o fixture control agora usa RAII para cleanup em panic. [Registro](evidence/server-1c/test-cleanup.json). O preflight de instalação inicialmente recusou hardlink gerado pelo Cargo; fonte de build é agora copiada para staging antes de strip, mantendo proibição de hardlinks na instalação existente. Gates finais passam. Não foram repetidas suítes domínio/desktop sem alteração nessas camadas; permanece um warning herdado de método não utilizado no domínio.

## Instalação, processos e consumo

`ops/install_cli.py` instalou **`/home/sam/.local/bin/narys`**, modo0700, sem sudo, dotfiles ou alteração global. Se o shell não inclui `~/.local/bin`, use o caminho completo. CLI funciona no host acessado por SSH; não há build Android/Termux nesta entrega.

Updater validou hashes Core/unit anteriores e fez backup privado em `~/.local/state/narys/core/updates/server-1a-3vpjs550` (ver path exato no [log](evidence/server-1c/install.txt); prefixo herdado). Atualizou somente nosso Core, preservando Keyring, boot, unit/políticas e dados. [Manifesto instalado](evidence/server-1c/installed-artifacts.json):

- Revision instalada: `45a15d1e3cdb0e8c6afe5340758fde602061521b`; worktree limpa durante instalação.
- CLI SHA-256: `f549dd8fc92b467f35e742266f33cf78a930b9b99673c2d9ed276ed1a5136e98`.
- Core SHA-256: `d2b937de218897be9ca2250b5c4f849164b7f7124f23f6a8a425f4d68cbce4bd`.
- Unit SHA-256 preservado: `df7b3205ebfcf5f673e9c72fd8aea4c0bba2c697d3f15a76feaaf4028b1a0810`.

Serviço active, PID **67100**, linger=yes, Keyring original PID1395 preservado, nenhum processo GNOME Shell/Copilot e nenhum worker/tarefa ativa/órfão. [Processos](evidence/server-1c/processes.json). A [amostra ociosa isolada](evidence/server-1c/idle.json), após restart controlado e sem diagnósticos de cofre, durou **10,000s**, mediu **25.676KiB RSS (~25,1MiB)** e **0,00s CPU adicional**. Não é benchmark prolongado nem avaliação nova do pico transitório Stronghold observado na 1B.

## Limites e encaminhamento

- SSH privado humano completo, Termux celular real, cold boot e restart integrado continuam gates **SERVER-1D**, aguardando instrução explícita. Nenhum desses gates foi iniciado automaticamente.
- A extensão GNOME50 é pinada; outro binário/backend requer revisão de compatibilidade. Python3/PyGObject/libsecret são dependências de sistema existentes para credentials. Não há dependência de sessão gráfica.
- CLI/status/credentials removem dependência do checkout. Supervisor/receipts/CLI Copilot LR-10A ainda conservam caminhos históricos; não foram reabertos nem acionados para inferência nesta etapa.
- Ferramentas/approvals agentivas seguem capability_not_integrated; desktop segue fence do takeover. Esses limites não são simulados como capacidades entregues.
- Instalação/desbloqueio humano seguro e consultas/conversa terminal estão implementados; a prova conjunta pessoal em SSH permanece explicitamente pendente da 1D, sem alegação de auditoria independente nesta entrega.
