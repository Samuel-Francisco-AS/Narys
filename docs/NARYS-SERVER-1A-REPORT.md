# NARYS-SERVER-1A — relatório único para auditoria independente da Luna

**Resultado: PASS técnico da SERVER-1A, candidato à auditoria independente.** A fundação Rust, ownership SQLite, IPC local e ciclo de vida foram implementados e verificados. Este resultado não declara PASS de Conversation real, provedores integrados, ferramentas agentivas, SERVER-1B ou SERVER-1D. Nenhuma dessas etapas foi iniciada automaticamente.

## Controle da execução

| Registro | Valor |
|---|---|
| Repositório | `Samuel-Francisco-AS/Narys` |
| Etapa | SERVER-1A / sequência 1A–1D |
| Main local e remota verificadas na abertura | `553b51182bb477d0b777093da5bfda93239fddf9` |
| Branch exclusiva | `narys-server-1-headless-runtime`, criada da main verificada |
| Abertura real, sem retroatividade | **2026-10-10T15:03:12-03:00 — America/Recife** |
| Prazo SERVER-1, 48 horas | **2026-10-12T15:03:12-03:00 — America/Recife** |
| Checkpoint de abertura | `2492285b786ef261951f0759f151b893ed69037a` |
| Commit de implementação e evidências funcionais | `81751db34ef146388003342ac49766256434ec2d` |
| Registro deste relatório | 2026-10-10T15:46:45-03:00; restante **47h16min26s** |

Workspace inicial limpo. Main permanece na base indicada. Não houve merge, rebase, force-push ou commit na main. O HEAD documental final e a igualdade com a branch remota são informados na mensagem de entrega após o push; o commit de implementação acima ancora o código auditado e o binário instalado.

As seis fontes obrigatórias foram lidas integralmente antes da implementação: arquitetura Core-First de 10/10, plano SERVER-1, entrega Narys 0.1/agent-tools, closure LR-10A de 10/10, LR-10 Copilot Specialist Agent e README do Core. Autorização LR-10A permanece encerrada. Não houve inferência Copilot, resume real, compra, overage ou fallback pago nesta execução.

## Inventário e decisão de arquitetura

O desktop concentrava Conversation/Context, Scheduler, ProviderRegistry, TaskGraph e políticas, agentes, persistência, trace, SecretStore e ExecutionBroker. O bootstrap Tauri compunha e possuía serviços, canais e tasks; o Core reaproveitava parte do código por `#[path]` para fontes dentro de `src-tauri`. Dependências de canais/runtime Tauri, enum de apresentação no módulo gráfico e wrapper Stronghold impediam uma composição operacional independente. Existiam duas bases SQLite e IDs numéricos sobrepostos entre tarefas de produto e LR-10A.

A decisão foi extrair a implementação operacional para **`narys-domain`**, usando um monólito modular. `narys-core` é a raiz operacional do processo servidor, dono de inicialização, banco, recovery e encerramento. Tauri mantém comandos, apresentação e adaptação de eventos; seus módulos operacionais reexportam os mesmos contratos. Não há cópia permanente dos mecanismos cognitivos extraídos.

| Área inspecionada | Arquitetura resultante |
|---|---|
| Conversation, Context, histórico e sessões | `narys-domain::cognition`; lógica de sessões extraída dos comandos; conexão ao servidor reservada à 1B |
| Scheduler, providers/registry, admission, rate, resilience, routing e políticas | Domínio independente; `ProviderRuntime` real composto pelo Core, ainda com registry vazio |
| TaskGraph, handoff, checkpoints, continuations, TaskRegistry/TaskId | Implementação única no domínio; Core semeia o próximo ID de produto a partir do histórico persistido |
| AgentRegistry, contratos/planner e adaptador Codex | Domínio; registry do produto disponível na composição, sem agente habilitado automaticamente |
| ExecutionBroker, authority, OS, PTY e terminal humano | Domínio; broker compartilhado no processo Core; HumanLocal continua exclusiva do terminal humano nativo |
| Persistência, migrations e configurações | Domínio; Core conserva o handle Database e a lease durante sua vida operacional |
| SecretStore | Domínio; wrapper Stronghold upstream preservado com licença, sem dependência Tauri; Core usa modo existente e somente leitura |
| Canais/runtime/trace | Subscribers transport-neutral e Tokio; adaptador Tauri converte Channel nativo; TraceBus efêmero separado dos eventos duráveis |
| Bootstrap desktop | Recusa runtime operacional após takeover; aguarda adaptação dos comandos para cliente IPC |

Trade-off explícito: a extração aumenta o diff e exige a transição do desktop para cliente do serviço. Isso elimina a autoridade concorrente e o acoplamento gráfico. O desktop legado **não está operacional após o takeover**; sua compilação e seus adaptadores foram preservados, mas não se declara smoke de GUI ou cliente IPC concluído. Binários desktop antigos que desconhecem a fence não devem ser executados; a migração e o atualizador recusam processos conhecidos desses binários ativos.

## Persistência e migração sem destruição

Fonte de verdade operacional: `~/.local/state/narys/core/db/luna.sqlite3`.

Fonte desktop preservada: `~/.local/share/br.com.assistente3d.app/luna.sqlite3`. O inventário confirmou identidade, memórias, sessões, mensagens, histórico de tarefas/subtarefas e configurações; o Core anterior mantinha duas tarefas LR-10A e configurações de bootstrap. A precedência escolhida é: configurações desktop tornam-se as configurações ativas; configurações anteriores do Core permanecem integralmente no backup. Bases com dados de domínio já populados de ambos os lados são recusadas como conflito, sem escolher silenciosamente um vencedor.

Migrations 001–018 foram movidas sem alteração de conteúdo. A nova **019_server_runtime.sql** é aditiva: preserva/cria `headless_tasks` e acrescenta `server_migrations`, `server_events` e índice de eventos por tarefa. `user_version` autoritativo é 19; schemas futuros/desconhecidos são recusados.

O takeover ocorre antes de recovery/readiness, sob leases independentes do Core e desktop. Verifica UID, arquivos regulares privados sem symlink/hardlink, schema e integridade. Usa SQLite online backup, incluindo commits em WAL, para guardar ambos os bancos antes de publicar o candidato. Aplica 019, importa tarefas LR-10A com IDs, estado, paths, objetivos e resultados exatos, registra receipt de migração em transação e verifica foreign keys. Faz fsync, checkpoint do WAL anterior e rename atômico. Os originais e backups são preservados em falha; não existe rollback automático que sobrescreva dados novos.

Uma fence `luna.sqlite3.core-owned` bloqueia abertura operacional da base desktop. O receipt publicado permite completar a fence após crash sem recópia. `WriterLease` usa flock não bloqueante, O_NOFOLLOW/O_CLOEXEC e arquivo 0600; handles do mesmo processo compartilham a lease, outro processo é recusado. As raízes de composição mantêm Database vivo: connections não devem sobreviver ao handle proprietário.

TaskIds não foram renumerados. O IPC explicita namespaces **`product`** e **`lr10a`**; comandos legados continuam exclusivos de LR-10A. Identidade, memória, histórico e recibos de autorização mantêm sua identidade. Os recibos ficam nos paths originais, sem renovação ou reinterpretação.

Evidência no host ([antes](evidence/server-1a/pre-update.json), [após](evidence/server-1a/host-final.json)):

- As **18 tabelas desktop** têm contagem e SHA-256 de linhas iguais ao snapshot; a base desktop fonte também permanece igual ao backup.
- Preservados **1 registro de identidade, 4 memórias, 37 sessões, 94 mensagens, 188 tarefas e 39 subtarefas**.
- As **duas tarefas LR-10A**, incluindo IDs e resultados, têm conteúdo igual ao snapshot Core anterior. A task 2 segue completed, resultado histórico `5`.
- Banco autoritativo e fonte passam `quick_check`; fence e receipt presentes.
- Backup SQLite: `/home/sam/.local/state/narys/core/backups/server-1a-L0o8Y4`.

Stronghold pessoal não foi aberto, migrado, salvo ou alterado nesta etapa. Comparação de inode/tamanho/modo/mtime/ctime do snapshot pessoal e de `closed.json` da autorização anterior é idêntica antes/depois. `SecretStore::existing` é lazy, não cria arquivos/chaves, não faz chmod/migração/save e recusa operações mutáveis ou o caminho legado de unlock. Acesso futuro ocorre somente pelo contrato Keyring/Stronghold aprovado, por demanda. Testes de segredos usaram apenas cofres sintéticos.

## IPC local, ciclo de vida e segurança da execução

Contrato em [narys-core/IPC.md](../narys-core/IPC.md), tipos em `src/ipc.rs`. Envelope v1, request_id validado e correlacionado, commands/intents tipados e rejeição de campos desconhecidos. Respostas possuem envelope success/failure e categoria; payloads dos endpoints legados seguem JSON, sem serializar autoridade de execução. request_id não é receipt de autorização nem garantia de exactly-once.

Socket Unix no runtime do usuário, diretório0700/socket0600, SO_PEERCRED mesmo UID verificado por servidor e cliente. Sem listener TCP. Limites: request16KiB, response256KiB, 32 conexões, read/write5s; timeout cliente265s para diagnósticos legados. Saturação fecha conexão; não há retry automático de mutações.

Status/capabilities, eventos, prepare/result/cancel e controles legados seguem disponíveis. Submit mantém consentimento financeiro anterior e autorização encerrada; sua existência não autoriza novos envios. Não foram executados credentials/stronghold/session-check/resume/inferência pessoais para validar esta entrega.

Conversation/Sessions, tasks de produto, providers/config, approvals approve_once/deny e ToolRequest/ToolResult possuem fronteiras tipadas. Operações sem integração retornam **`capability_not_integrated` antes de efeitos**. Intent de ferramenta inclui list/read/edit/build/test/diff, sem campo shell, origin, authority ou HumanLocal. O Core deverá validar receipts vinculados a tarefa/operação/escopo; texto de agente, peer UID e TaskRef não concedem autoridade. ExecutionBroker é reutilizável para essa integração futura.

Eventos sanitizados persistem sequência/cursor, namespace, task ID, código e timestamp; retenção4096 e indicação de gap. Mudança de estado legado e seu evento compartilham transação. TraceBus continua limitado e efêmero. Cancelamento é idempotente e não promete desfazer efeitos. Desconexão do cliente não cancela tarefa admitida. No restart, running vira interrupted com `restart_never_retries`, uma única vez; efeitos incertos não são reenviados.

Serviço systemd **--user**, Type=notify/READY=1 após ownership, migração, recovery e bind. Singleton é adquirido antes de tocar a persistência; um socket legado vivo também bloqueia inicialização. SIGTERM/ctrl-c fecha admissão, cancela, drena workers/conexões/broker, persiste stopped e remove socket; STOPPING=1 informado. KillMode=control-group, timeouts, limites e NoNewPrivileges foram preservados. Não depende de alvo gráfico, e o runtime Copilot continua on-demand.

O UID Linux continua sendo fronteira de confiança: **HOST_ASSISTED_NOT_SANDBOX**. Validação de cwd não é sandbox. Agentes não receberam HumanLocal nem acesso a ferramentas reais nesta etapa.

## Compilação e testes relevantes

A extração afetou os módulos do domínio; sua suíte é evidência de regressão desses módulos, sem executar suites históricas externas. Logs completos em [evidence/server-1a](evidence/server-1a/).

| Verificação | Resultado/evidência |
|---|---|
| Domínio extraído, suíte Rust | **1049 PASS, 0 falhas, 2 ignorados**, 355,04s — `domain-tests.txt` |
| SecretStore existente/somente leitura, ajuste final | **12 PASS**, incluindo 2 casos novos, 26,46s — `existing-secret-reader-tests.txt` |
| Core final | **30 unitários + 1 integração existente + 4 novas integrações Unix = 35 PASS** — `core-tests.txt` |
| Adaptador desktop: sessão/resume sem escrita prematura | **1 PASS** após o ajuste final — `desktop-session-tests.txt` |
| Desktop: apresentação/settings/políticas e migration | **8 PASS** — `desktop-presentation-tests.txt` |
| Desktop: evento nativo pelos adaptadores existentes | **1 PASS** — `desktop-event-tests.txt` |
| Desktop release cargo check | **PASS** — `desktop-release-check.txt` |
| Python: unidades e autorização financeira sintética | **6 PASS** — `python-tests.txt` |
| Core cargo fmt --check, git diff --check, systemd-analyze --user verify | **PASS**, conferidos novamente no fechamento |
| Dependency tree de produção Core | Sem Tauri/GTK/WebKit/wry — `core-dependencies.txt`; binário também sem bibliotecas gráficas via ldd |

A suíte ampla precede o último ajuste de SecretStore: não é apresentada como rerun completo posterior. O ajuste final foi validado pelos 12 testes focados, Core completo, compilação desktop release e teste de sessão desktop. Os dois testes ignorados exigem app-server Codex local/manual com autenticação/inferência; não foram executados.

Casos novos cobrem conflito de bases preservado, backup com WAL, colisões de IDs/namespaces e receipt preservado, fence completada em restart, lease/daemon duplicado, versão/campos/limites IPC, recusa de autoridade futura, cancelamento/eventos duráveis e SIGKILL com tarefa uncertain interrompida uma vez sem replay. Esses testes usam diretórios, cofres e peer SDK sintéticos.

Reprodução local sem inferência:

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/src-tauri/target" \
  /usr/bin/cargo test --offline --locked --manifest-path narys-domain/Cargo.toml --lib -- --test-threads=2
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/src-tauri/target" \
  /usr/bin/cargo test --offline --locked --manifest-path narys-core/Cargo.toml
python3 -m unittest discover -s narys-core/tests -p 'test_*.py'
python3 narys-core/ops/server_1a_audit.py
```

Compilador verificado: rustc/cargo **1.98.1**, Fedora. MSRV declarado 1.94 não foi ensaiado em toolchain1.94. Logs contêm warnings de código/test fixtures, sem erro de compilação; não se declara build livre de warnings.

## Serviço efetivamente atualizado e evidência headless

O atualizador preservou backups privados de binário/unidade com hashes/fsync antes de parar **somente narys-core.service**. Instalação final usa o commit funcional indicado; [host-final.json](evidence/server-1a/host-final.json) registra SHA do binário `9f6ca5d2bb3dcd82a4d13efbe3d18d3a21e6e2623004f710a11627778cc4cd48` e SHA da unidade. Backups de atualização: `server-1a-ys9m3m69` e `server-1a-_x08gjz_` sob `~/.local/state/narys/core/updates/`.

Host observado: serviço active/running, Type=notify, PID48353, linger=yes, alvo existente multi-user.target, GDM inactive, Keyring user service active, GNOME0, Copilot0 e graphical_environment_present=false. Nenhuma alteração de GDM/GNOME/Keyring global/boot/SSH foi feita; sudo não foi usado.

[Primeira inicialização](evidence/server-1a/host-first-start.json), [reinício](evidence/server-1a/host-restart.json) e [composição final](evidence/server-1a/host-final.json) demonstram IPC e dados persistentes. PID46762→46798 no ensaio de restart; task2 manteve completed/resultado sem envio/resume. A atualização final passou pelo mesmo readiness e instalou a composição SecretStore somente leitura.

[Amostra idle final](evidence/server-1a/idle-final.json): PID48353, pai user manager1344, RSS15964KiB estável, **0,0s CPU em 10,000s**, mesmo PID após saída dos clientes CLI. Demonstra serviço independente desses clientes. Não é benchmark de longo prazo nem ensaio novo de boot frio, SSH real, Termux ou celular; os gates de SERVER-1D permanecem pendentes.

O script de auditoria produz apenas contagens/hashes de tabelas, estado de processo/serviço/protocolo e metadados de arquivos. Não imprime linhas do histórico, valores de credenciais ou conteúdo de recibos.

## Arquivos e ownership para revisão

Manifesto completo dos arquivos da base ao commit funcional: [changed-files.txt](evidence/server-1a/changed-files.txt). Os principais grupos são:

- `narys-domain/{Cargo.toml,Cargo.lock,README.md,src/**,migrations/**}`: implementação movida, neutralização Tauri, leases, modo de segredos e migration019.
- `narys-core/src/{lib,main,runtime,storage,ipc,server,policy}.rs`: composição, takeover/recovery, protocolo e lifecycle; removidos antigos módulos de inclusão por path.
- `narys-core/{Cargo.toml,Cargo.lock,IPC.md,README.md,tests/protocol.rs}`: contratos, dependências e evidência integrada.
- `narys-core/ops/{narys-core.service,update_user.py,server_1a_audit.py}`: readiness, atualização restrita com backup e auditoria read-only.
- `src-tauri/{Cargo.toml,Cargo.lock,src/**}`: facades/adaptação de Channel, bootstrap/fence, apresentação e testes com caminhos ajustados.
- `README.md`, plano SERVER-1, este relatório e `docs/evidence/server-1a/*`: estado de implementação e evidências.

## Critérios de aceite e entrega à SERVER-1B

| Critério SERVER-1A | Avaliação |
|---|---|
| Compilação/testes dos módulos modificados | PASS nas verificações acima |
| Core independente de GUI | PASS no dependency tree, binário e serviço headless real |
| systemd, inicialização/comunicação/encerramento | PASS de 1A; boot frio/SSH remoto de 1D ainda pendentes |
| IPC tipado/local/seguro | PASS de contratos/limites/autenticação/capability gating |
| Ownership e migração sem destruição | PASS de backups, testes de falha e comparação dos dados reais |
| Ausência de regressão conhecida no escopo validado | PASS dos mecanismos extraídos e controles legados; transição desktop explicitamente documentada |
| Fronteiras para Conversation/provedores/ferramentas | PASS arquitetural; não equivale a integração funcional |

Pronto para SERVER-1B: domínio operacional independente com Conversation/Scheduler/provedores originais, persistência histórica consolidada e IDs preservados, RuntimeServices real, SecretStore existente por demanda, IPC versionado com recusas explícitas, eventos e broker reutilizáveis. A 1B deverá conectar Conversation/Providers a essa composição e provar sessão/mensagens/tasks reais através do serviço, sob seus gates de autorização. Não deve iniciar outra implementação cognitiva nem liberar authority por equivalência.

Pendências/limites mantidos para auditoria: adaptação desktop ao IPC; integração de providers/agents/config/approvals; sandbox e execução agentiva futura; boot frio/SSH real e gate final de 1D. As dívidas anteriores do helper Keyring50/interface interna, workaround DNS somente do runtime, sessão legada/SDK pinado e tratamento de descendentes adversariais da LR-10A não foram reclassificadas como resolvidas. Autorização financeira fechada permanece bloqueio efetivo de novos envios.

SERVER-1B não foi executada. Auditoria independente da Luna não foi realizada por esta implementação.
