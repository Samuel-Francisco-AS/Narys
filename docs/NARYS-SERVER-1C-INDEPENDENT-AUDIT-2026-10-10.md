# NARYS-SERVER-1C — auditoria independente Luna (10/10/2026)

**DECISÃO: PASS TÉCNICO-FUNCIONAL no escopo da SERVER-1C.** Este parecer decorre de inspeção independente do GitHub, do código Rust/Python, dos testes e da evidência de host registrada pelo executor; **não** corresponde à execução pessoal da Narys no Fedora, tampouco aprova 1D, release 0.1, ferramenta agentiva ou desbloqueio humano SSH/Termux ainda não testado.

## Identidade e janela

- Commit candidato auditado: `f81f799c87b6eaf11160377a1c992cd28617e86d` na branch `narys-server-1-headless-runtime`.
- Base anterior auditada: `4ad89d14e1666647dd92b1737a81c716d057692d`. Comparação: 4 commits à frente, zero atrás; alterações de produção 1C publicadas.
- Main mantida no SHA `553b51182bb477d0b777093da5bfda93239fddf9`.
- Janela vinculante SERVER-1: aberta em 10/10/2026 15:03:12 America/Recife; encerra 12/10/2026 15:03:12. A auditoria não reinicia prazo.

## Critérios demonstrados

1. **CLI própria e instalada:** `narys-core/src/bin/narys.rs`, `src/cli.rs`, `src/client.rs` formam cliente IPC estrito e sem cópia de Scheduler, Engine ou SQLite. `~/.local/bin/narys` instalado em modo privado; `narys help` e [manual CLI](../narys-core/CLI.md). Status, doctor, sessions, chat, send, tasks/task/cancel, events, providers/models, policy e credentials disponíveis; approvals permanecem `capability_not_integrated`.
2. **IPC e segurança:** mesma identidade UID no socket Unix; versionamento v1, validação de request_id, limites de 16KiB/256KiB e 32 conexões do Core; extensão de consultas tasks/models paginadas. Clientes não repetem mutações após resultado de rede incerto; Ctrl-C no acompanhamento não cancela o serviço. Texto vindo do provider/DB passa por escaping de controles terminais no modo humano.
3. **Unlock de credenciais:** `narys credentials unlock` é operação local do cliente em TTY SSH humano, **não** Command IPC. `credential_manager.py` empacotado por `include_str!` e executado em Python isolado, sem checkout e sem senha em argv/env/log/file/chat/IPC. Verificações de TTY, foreground, shell pai, logind sshd Remote=true, UID e backend GNOME50 preso a binário/serviço de usuário; falha fechada. Buffer de senha nativo/memória travada, sem eco e com limpeza e recuperação termios, libsecret cifrado. **A admissão por UID/TTY não prova identidade humana contra comprometimento do mesmo UID**. O método GNOME `UnlockWithMasterPassword` permanece interface interna não suportada, sujeita a incompatibilidade.
4. **Testes sem cofre pessoal:** 42 Rust PASS (33 unit+1 control+8 protocol), 17 Python PASS (4 CLI+7 credentials+6 operacionais), build/format/install verificados, snapshot de cofre sintético e daemon GNOME50 real: senha errada rejeitada; correta desbloqueia; não re-prompt se já unlocked; bytes do cofre sintético intactos. Teste isolado substituiu gates de identidade/TTY somente na fixture, **não prova sessão SSH pessoal**.
5. **Persistência e serviço:** evidências `host-installed.json` e `host-reconnected.json` mostram 38 sessões, 99 mensagens, 191 tarefas, tasks189/190 completed e191 cancelled após restart, sem nova inferência. Stronghold pessoal e metadados/bytes comparados intactos. `processes.json` identifica um Core systemd active, linger yes, sem GUI/Copilot/órfãos ativos.
6. **Economia:** `idle.json` reporta 25440 KiB RSS (~24,8 MiB), CPU adicional zero em 10 s sem probes. Esta amostra **não invalida** o pico transitório de ~542 MiB durante diagnóstico Stronghold na 1B, que deve entrar no gate da 1D.

## Ressalvas obrigatórias e fronteiras de PASS

- **Não demonstrado:** cold boot completo; SSH/Termux real com cofre pessoal inicialmente bloqueado; desbloqueio humano digitado; encerramento do SSH durante tarefa real nova; integração/compatibilidade de novo cliente Android. Esses são gates 1D. Não pedir senha ao agente e não tentar automatizar entrada de senha.
- **Restrição de uso:** desbloqueio exige shell SSH interativo direto. Não funciona intencionalmente de dentro de `tmux`/`screen`, `ssh HOST comando`, pipe, redirect ou script. Usuário sai da sessão tmux para desbloquear e volta depois. Verificar que isso é viável no Termux real; caso o gate logind rejeite uma sessão SSH legítima, resolver durante a 1D sem desabilitar indiscriminadamente as verificações.
- **Outros limites herdados:** desktop ainda cercado pelo takeover, sem adaptador IPC; agentes com ferramentas/approvals reais permanecem indisponíveis até LR-10/LR-11; Copilot LR-10A não ganha autorizações novas. Não utilizar esse PASS como prova dessas capacidades.
- **Não houve reexecução independente:** números derivam de logs versionados, lidos e comparados ao código; sem acesso direto à máquina pelo auditor.

## Decisão e encaminhamento

**Ratifico SERVER-1C PASS técnico-funcional, sem FIX adicional.** Autoriza preparar SERVER-1D na **mesma branch** e dentro da janela original, com gate real de cold boot headless, cliente SSH/Termux, desbloqueio humano privado e Stronghold, chat Groq sob permissão/quota já concedidas e sem overage, estado/tarefas cancelamento e reconexão, screenshots/observações quando adequadas, performance de pico e idle, segurança e documentação de encerramento. Reboot, desconexão deliberada do terminal operacional, login privado e senha requerem cooperação/ação humana; Codex não deve tentar operar o shell privado nem simular sua presença. A etapa não deve alterar boot global sem autorização nem usar contas pagas.

**Meta pós SERVER-1:** Narys 0.1 agentiva até 17/10, através de LR-10B–F/LR-11; serviço conversacional + CLI por si só não satisfaz ferramentas reais.
