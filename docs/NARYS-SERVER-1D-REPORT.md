# NARYS-SERVER-1D — relatório final do gate integrado

**Gate de execução concluído nas capacidades servidor abaixo; candidato à auditoria independente da Luna.** O Fedora iniciou sem GUI, o Core ficou pronto antes do primeiro SSH, o humano desbloqueou o Keyring em terminal privado e usou a CLI pelo Termux para conversar, sair, reconectar e recuperar resultado/histórico. Uma chamada Groq foi usada. Persistência, cancelamento elegível e recuperação foram verificados. **A trilha não está definitivamente fechada, a main não foi alterada e Narys 0.1 agentiva não está aprovada.** Relatório único da quarta e última etapa; não há SERVER-1E/FIX.

## Controle, prazo e publicação

- Branch exclusiva: `narys-server-1-headless-runtime`. HEAD esperado e inicial local/remoto: `2b7c64cb7f69339167d014ac0f78f11e03454e66`.
- Checkpoint pré-reboot: `bd59fddfb7c84943f0e002b3d8f6b12919d45fae`, igual ao remoto na retomada e antes da publicação final. HEAD da entrega final será registrado na publicação abaixo.
- Main remota conferida intacta: `553b51182bb477d0b777093da5bfda93239fddf9`. Sem merge ou force-push.
- Abertura da trilha: **10/10/2026 15:03:12 America/Recife**; primeira observação 1D: **18:03:07**. Prazo absoluto preservado: **12/10/2026 15:03:12 America/Recife**.
- Auditoria final do host: **10/10/2026 18:36:06 -03**; última amostra ociosa: **18:36:50**. Restavam aproximadamente **44h27min** na auditoria, sem reiniciar a contagem após reboot.
- Lidos planejamento geral, relatórios/auditorias 1A/B/C, CLI.md, IPC.md, arquitetura Core-First, critérios Narys 0.1 e contexto LR-10. Sem reimplementação das etapas aprovadas, alteração de código de produção ou reinstalação na 1D; mudanças limitadas a instrumentos de gate, evidências e documentação.

## Matriz dos critérios SERVER-1A/B/C/D

| Etapa / critério | Resultado | Evidência e alcance |
|---|---|---|
| 1A: Core sem GUI, authority única, SQLite/migração, IPC/lifecycle | PASS ratificado pela Luna | [Auditoria 1A](NARYS-SERVER-1A-INDEPENDENT-AUDIT-2026-10-10.md); instalação/baseline reconferidos |
| 1B: Conversation/Scheduler/providers, Stronghold, resultados/recovery | PASS ratificado pela Luna | [Auditoria 1B](NARYS-SERVER-1B-INDEPENDENT-AUDIT-2026-10-10.md); resultados históricos preservados |
| 1C: CLI, consultas/paginação e unlock humano encapsulado | PASS ratificado pela Luna | [Auditoria 1C](NARYS-SERVER-1C-INDEPENDENT-AUDIT-2026-10-10.md); fluxo humano real complementado na 1D |
| 1D: boot real multi-user.target, linger/autostart, readiness, ausência de GUI/workers/Copilot residente | PASS observado no host | [Pós-boot](evidence/server-1d/host-postboot.json), [estado final](evidence/server-1d/host-final.json); Core pronto 9,396s antes do primeiro SSH |
| 1D: Termux/SSH direto, doctor, credenciais e senha sem eco exclusivamente humana | PASS por relato humano e estado final no host | [Registro humano](evidence/server-1d/human-postboot.json); agente não operou TTY nem recebeu senha |
| 1D: acesso Core ao Stronghold existente sem alteração | PASS | Probe humano pós-reboot, conversa autenticada e snapshot/metadados imutáveis; migration/writes/secret_values_returned=false |
| 1D: criar/selecionar/consultar/retomar sessões, conversa Groq, recibo/resultado duráveis | PASS | Sessão39/task193/mensagens101–102; [resultado](evidence/server-1d/first-groq-result.json), [sessão](evidence/server-1d/final-session-recovery.json) |
| 1D: sair/reconectar pelo celular e recuperar resultado/histórico | PASS do fluxo executado | Humano confirmou receipt→exit→reconexão→consulta; logind confirma troca de sessão. A tarefa terminou **antes** do logout |
| SSH fechado enquanto uma tarefa real ainda executava | NÃO DEMONSTRADO por esta chamada | Limite temporal explícito; clientes desconectados cobertos por testes locais, sem substituir prova remota ativa |
| 1D: tarefas/eventos por cursor, histórico após restart sem nova inferência | PASS | CLI/SQLite comparados integralmente; task193/sessão39 iguais após restart e espera |
| 1D: cancelamento elegível sem replay | PASS limitado a tarefas legadas preparadas 3/4 | CLI oficial, cancelled_before_send e repetição idempotente, sem submit; não prova cancelamento de inferência ativa nova |
| 1D: shutdown limpo, interrupção/restart e recuperação | PASS | Stop/start instalado, SIGKILL ocioso/restart automático, reboot humano e 8 testes isolados incluindo uncertain→interrupted sem replay |
| 1D: socket/UID, segurança, CPU/RSS proporcionais | PASS das observações delimitadas | Unix 0600/dir 0700/peerUID, sem TCP público Narys, cofre preservado, logs sanitizados e medições abaixo |
| Fechamento definitivo SERVER-1 / merge main | AGUARDA auditoria independente 1D | Luna deve avaliar relatório e limites; nenhuma aprovação antecipada |
| Narys 0.1 com agentes/ferramentas reais | NÃO APROVADA | Tools/approvals não integrados; LR-10B–F/LR-11 são backlog prioritário |

## Fedora, boot e participação humana

Fedora Linux **44 Workstation**, kernel `7.2.8-200.fc44.x86_64`, aproximadamente 8 GiB RAM sem GPU dedicada. Alvo existente/default e ativo `multi-user.target`, GDM inativo, sshd ativo. `narys-core.service` de usuário enabled/active, Type=notify, linger=yes. Estado final: Core PID2461, pai user manager1340, Keyring PID1391, zero execution_workers/product_active_tasks/tools, graphical_environment_present=false, Copilot on_demand. Não observados GNOME Shell, Xorg/Xwayland, workers órfãos ou Copilot residente. Codex nesta sessão de desenvolvimento não foi executor agentivo da Narys.

Reboot feito **pelo humano**, depois de backup, preflight sem trabalho ativo, evidências/plano persistidos no checkpoint. Boot ID mudou de `299dc161-7036-4c0f-b012-82b1150b528c` para `371dd203-7e49-4156-bdf8-6c8bb9f3af8a`. Core pós-boot PID1392 pronto às **18:18:44** (monotonic 19314756 µs); primeiro SSH às **18:18:53** (28710608 µs), **9,396s depois**. Autostart anterior à conexão SSH, sem GUI.

Operador atestou reconexão Termux no celular, doctor, credentials status e narys-core stronghold. Executou pessoalmente **`narys credentials unlock` em SSH direto**, recebeu prompt de senha **sem eco**, digitou pessoalmente e concluiu com sucesso. O agente não solicitou a senha, não a conheceu/capturou/registrou/inseriu, não operou terminal humano e não fez unlock por tmux/pipe/script/IPC. Procedimento é evidência humana; boot, unlocked/backend compatível e SSH Remote=yes/sshd/tty/UID1000 são evidências do host. Logind não identifica independentemente Termux. Estado já unlocked pré-reboot registrado sem simulação; prova legítima posterior ao reboot real.

Nenhuma configuração global boot/GDM/SSH/Keyring alterada, nenhum sudo usado ou novo reboot solicitado. Hash GDM, alvo e unidades Core/Keyring iguais ao checkpoint. sshd_config ilegível sem privilégio, sem escalada. Não houve recusa incorreta de SSH humano que exigisse correção.

## Keyring, Stronghold e instalação

Keyring existente ativo/unlocked, backend GNOME50 compatível. Probe pós-reboot: ok=true, existing_snapshot_opened=true, migration=false, writes=false, secret_values_returned=false. Conversa autenticada confirma acesso funcional do Core ao provider autorizado. SHA-256/inode/tamanho/permissões/mtime/ctime do snapshot iguais à 1C. Nenhuma coleção/snapshot/chave/arquivo de senha criado ou token exportado. Metadados da autorização antiga LR-10A imutáveis e **encerrados**.

Instalação preservada da revisão **`06ef4076380ed2723b9c567f5323104ec596e846`**, hashes iguais à 1C:

| Artefato instalado | SHA-256 |
|---|---|
| `~/.local/bin/narys`, modo 0700 | `5719709444be4a62503a6ddfbdd6ecb9a3049944541fd26d1a9030e9a9032328` |
| `~/.local/lib/narys/narys-core` | `6a68ae32e967eb2d196b890424e9ac9114ebe5a0e9de6636d7be97d97c98c8bd` |
| `~/.config/systemd/user/narys-core.service` | `df7b3205ebfcf5f673e9c72fd8aea4c0bba2c697d3f15a76feaaf4028b1a0810` |

## Conversa, desconexão e autorização delimitada

Humano autorizou até 2 chamadas **Groq Free**, quota disponível declarada, sem overage/retries financeiros/fallback pago. Política específica Fixed Groq/modelo `openai/gpt-oss-20b`/medium, teto 512 tokens, maxProviderCalls=1, retryEnabled=false/maxRetries=0, demais providers desabilitados. **Uma chamada pessoal via CLI no Termux/SSH; zero submissões remotas pelo agente.** Nenhuma nova inferência para completar o relatório.

Recibo **task193/session39/pending/durable=true/disconnect_cancels=false**. Resultado recuperado após exit/reconexão: completed/error_code=null, **`SERVER1D-371dd203 78`**, input 252 + output 66 = **318 tokens**, providerCalls=1/retries=0/fallbacks=0. Humano confirmou `narys session 39 --after 100 --json` com 101 (user) / 102 (assistant). [Resultado e relato](evidence/server-1d/first-groq-result.json) comparados à autoridade SQLite, sem publicar histórico pessoal.

Eventos 57–62: conversation_admitted→task_started→context_built→provider_selected→provider_admitted→completed. Admissão **18:32:01.328**, conclusão **18:32:04.938**, intervalo **3,610s** incluindo preflight, não latência LLM isolada. Logout SSH2 às **18:32:17.667039**, nova sessão4 às **18:32:26** ([antes](evidence/server-1d/ssh-before-first-send.json), [depois](evidence/server-1d/ssh-after-first-send.json)). Gate comprova envio/receipt/saída/reconexão/recuperação; **não comprova tarefa ainda executando na ausência do SSH**, pois já concluída. Independência do cliente IPC coberta por testes direcionados, sem substituir esse limite da chamada real.

[Gate encerrado](evidence/server-1d/gate-closure.json) com **1 chamada utilizada/1 não utilizada**, sem transferir saldo de autorização. Política original restaurada exatamente via CLI: maxOutputTokens=null/maxProviderCalls=3/retryEnabled=true/maxRetries=1, sem disparar inferência. Limites específicos descrevem a chamada de gate, não mudança permanente da política humana.

## Histórico, baseline, cancelamento e recuperação

Backup SQLite online privado com WAL/integridade/fsync antes das mutações: `~/.local/state/narys/core/backups/server-1d-o7b9amba/authority.sqlite3`, dir 0700/arquivo 0600, SHA-256 `8acfe11831e1b059fcda3de09c7726ffd9cefbacb371ec921b9304ff7d1af145`. Fora do Git; nenhum conteúdo privado exportado.

[Auditoria final](evidence/server-1d/host-final.json): schema20/quick_check=ok/zero foreign key errors. **Todas as linhas originais das 38 sessões, 99 mensagens, 191 TaskRecords, 39 subtarefas, 3 runs, 2 tarefas legadas, identidade e memórias iguais ao baseline.** CLI paginou/consultou todas as sessões/mensagens/tarefas/eventos e comparou à autoridade, sem publicar conteúdo privado. Eventos ordenados/únicos e páginas iguais à base.

Totais finais legítimos: **39 sessões, 102 mensagens, 193 TaskRecords, 5 runs, 4 tarefas legadas, 39 subtarefas, 69 eventos**, 1 identidade / 4 memórias. Sessão 39 tem entrada técnica de cancelamento pré-reboot e par 101/102. Tasks 189/190 completed e 191 cancelled conservam resultados originais. Reboot: eventos 52→54 apenas lifecycle, sem inferência automática.

Product 192 foi admitida com providers temporariamente todos desabilitados: falhou free_provider_authorization_required antes do cancel, **zero envio remoto**, resultado nulo. Cancel already_terminal, [eligible_cancel_pass=false](evidence/server-1d/cancel.json); corrida não apresentada como cancelamento efetivo. Permissão Groq restaurada; demais providers continuaram desabilitados.

Cancelamento elegível real: **lr10a:3 e lr10a:4** preparadas localmente/nunca submetidas. CLI oficial `narys cancel ID --namespace lr10a`, cancelled_before_send, repetição already_terminal, sem replay/submit/inferência/agente ([task3](evidence/server-1d/cancel-prepared.json), [task4](evidence/server-1d/final-eligible-cancel.json)). Task4 cancelada **localmente por Codex**, não pelo celular. Não prova cancelamento de inferência de produto ativa nem reabre LR-10A.

[Lifecycle instalado](evidence/server-1d/lifecycle.json): stop limpo removeu socket/MainPID/informou stopped; start PID70468; SIGKILL **Core ocioso** levou a PID70500 por Restart=on-failure/NRestarts1. Dados/resultados iguais após ciclos e preflight sem trabalho importante/ativo. Testes isolados exercitam uncertain→interrupted uma única vez/sem retry/replay, sem injetar efeito incerto no banco pessoal.

[Recuperação final](evidence/server-1d/final-session-recovery.json): sessão39 fechada/reaberta sem sumarização automática,3 mensagens idênticas. Core restart **1392→2461**, Keyring permaneceu1391. Task193/sessão39 exatamente iguais antes/depois do restart e após espera; credenciais unlocked, zero workers e nenhuma nova inferência. Resultado recuperado, não recalculado.

## CPU/RSS e pico transitório da SERVER-1B

Medições curtas com polling 50 ms, sem classificação como benchmark prolongado. RSS amostrado; espera pelo humano não é latência da chamada.

| Fase | Duração | RSS máximo observado | CPU adicional Core |
|---|---|---|---|
| [Idle pré-probe](evidence/server-1d/memory-isolated-preboot.json) | 10,007s | 29228KiB (~28,5MiB) | 0,00s |
| Stronghold existente | 1,715s | 553556KiB (~540,6MiB), volta a29228KiB | 1,46s |
| Idle após probe | 10,007s | 29228KiB, final29136KiB | 0,00s |
| [Idle pós-reboot](evidence/server-1d/idle-postboot.json) | 10,005s | 28292KiB (~27,6MiB) | 0,00s |
| [Espera humana/preflight/Groq](evidence/server-1d/first-call-observation.json) | 104,639s, majoritariamente espera | 553476KiB (~540,5MiB), final31008KiB | 2,96s |
| [Idle final pós-restart](evidence/server-1d/idle-final.json) | 10,003s | 26472KiB (~25,9MiB) | 0,00s |

Pico 1B reproduzido pela abertura do cofre. Código local pinado: stronghold_engine2.0.1/snapshotv3/age→iota-crypto0.23.2/scrypt r=8/p=1, work factor **público** 19. Buffer principal estimado `128 * 8 * 2^19 = 536870912 bytes` (**512 MiB**). Associação é inferência apoiada por código/parâmetro/amostra, não profiler de alocações. Sem modificar KDF/snapshot. Custo transitório por demanda, não permanece em idle.

Cgroup peak isolado pré-boot 547401728 bytes, janela Groq 579256320 bytes, abaixo MemoryHigh 600 MiB / MemoryMax 900 MiB. high/max/oom/oom_kill=0, PSI serviço=0. Host final 8007520 KiB RAM / 7128672 KiB disponíveis e swap sem uso. Sem pressão/impacto funcional observado; concorrência de agentes requer headroom/observação futura. [Amostra concorrente com Cargo](evidence/server-1d/memory-preboot.json) marcada valid_for_isolated_host_idle=false; MemoryPeak pós-reboot inclui probe humano anterior, não pico idle. [Espera anterior sem envio](evidence/server-1d/awaiting-human-window.json): zero runs novos/CPU, não medição LLM.

## Segurança, testes e limites

- Socket Unix 0600/dir 0700, SO_PEERCRED UID1000/PID MainPID, verificações servidor/cliente mantidas. Sem password/HumanLocal/authority de execução via IPC.
- Produção bind apenas UnixListener; TCP final sem listener não-loopback UID1000. SSH/DNS do sistema fora da API Narys. `/proc/PID/fd` do Core nondumpable inacessível: atribuição completa por PID não realizada nem contornada com privilégio. Listener Codex de desenvolvimento local não é executor Narys.
- Cofre/credenciais preservados. [Logs pré-boot](evidence/server-1d/log-safety.json)/[pós-boot](evidence/server-1d/log-safety-postboot.json): rótulos fixos/timings numéricos, zero entradas Core desconhecidas/assinaturas suspeitas detectadas, sem exportar logs brutos. Scanner não prova ausência de todo segredo possível. Evidências contêm estados/hashes/contagens e somente texto técnico criado para gate.
- [8 testes IPC PASS](evidence/server-1d/protocol-tests.txt): singleton, limites/versionamento/authority, crash/recovery sem replay, permissões, clientes desconectados, paginação e recusa de unlock automatizado. Comando: `COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/src-tauri/target" /usr/bin/cargo test --offline --locked --manifest-path narys-core/Cargo.toml --test protocol`. Warning herdado de telemetria não utilizada, zero falhas; não repetidas suítes domínio/desktop sem alteração.
- Instrumentos Python auditoria/métricas/observação/cancelamento executados, py_compile/diff check e [consistência das evidências/JSON](evidence/server-1d/delivery-validation.json) conferidos. Não alteram runtime instalado.
- Fronteira mesmo UID, **HOST_ASSISTED_NOT_SANDBOX**. Cancelamento não desfaz efeitos remotos nem garante vencer tarefa rápida. Saída do cliente não solicita cancelamento automaticamente. Nenhum Copilot/Codex agentivo foi iniciado, nem ferramenta executada por agente da Narys.

## Dívidas, desktop e retomada prioritária

Nenhum procedimento humano obrigatório restante para os gates executados. **Auditoria independente 1D é a pendência de fechamento.** Limites para Luna: logout SSH durante tarefa real ativa não observado; cancelamento novo limitado a preparadas; medições curtas sem endurance. Se exigidos como gates adicionais de aceitação, declará-los pendentes, sem converter extrapolação em PASS.

Desktop legado cercado pelo takeover e **não operacional**; testes/compilação herdados não provam GUI funcional. Faltam thin client Tauri/IPC e smoke sem writer concorrente. Avatar opcional/não executado no servidor. Não abrir desktop legado contra base migrada como prova 1D.

Dívidas objetivas: GNOME50 pinado/interface interna não suportada; Python3/PyGObject/libsecret; paths supervisor/recibos Copilot; sessões SDK em /tmp não garantidas após reboot; isolamento adversarial/supervisor descendentes não provados; Gemini/Cloudflare/Mistral sem teste remoto atual/autorização 1D; MSRV1.94 não ensaiado. Pico KDF 512 MiB exige headroom com concorrência, sem OOM aqui. Tools/approvals CLI ainda capability_not_integrated.

Após auditoria e fechamento aprovado, preparar **retomada prioritária LR-10B–F e LR-11**, para [agentes com ferramentas reais até 17/10/2026](NARYS-01-AGENT-TOOLS-DELIVERY.md), sem retomada automática:

| Trilha / responsáveis após autorização | Entrega necessária |
|---|---|
| LR-10B / Codex; auditoria Luna | Adapter/supervisor Copilot on-demand e lifecycle sem residente |
| LR-10C / Codex; decisões humanas | Approval por tarefa/operação/workspace, tool gateway e sandbox onde prometido, sem HumanLocal |
| LR-10D / Codex | TaskGraph/quota/usage/trace/resultados duráveis |
| LR-10E / Codex + operador remoto | Tarefa real tool_call→autorização→efeito→diff→teste/build, Termux e desconexão durante execução comprovada |
| LR-10F / Codex; auditoria Luna | Gate integrado/cancel/reap/recovery/segurança factual |
| LR-11 / executor autorizado; auditoria Luna | Codex com autenticação/approvals/cancel/sandbox/status próprios, ferramentas reais e validação separada |

Luna deve conferir HEAD publicado/baseline/evidências, distinguir relato humano de observação do host e decidir aceitação dos limites. Sem merge/main ou fechamento definitivo antecipado. Nenhuma nova inferência autorizada por este relatório.
