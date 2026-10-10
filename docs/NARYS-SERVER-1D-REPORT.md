# NARYS-SERVER-1D — gate integrado final e fechamento

**EM EXECUÇÃO / checkpoint anterior ao reboot humano. Não é PASS da SERVER-1D nem fechamento da trilha.** Os gates locais abaixo foram executados; novo cold boot, senha humana e conversa/reconexão pelo celular continuam pendentes. Este é o relatório único da etapa e será atualizado com a evidência posterior, sem SERVER-1E/FIX. Auditoria independente da Luna ainda não realizada.

## Controle da execução

- Branch exclusiva: `narys-server-1-headless-runtime`; HEAD inicial local/remoto conferido: `2b7c64cb7f69339167d014ac0f78f11e03454e66`.
- Main remota preservada: `553b51182bb477d0b777093da5bfda93239fddf9`. Sem merge, force-push ou declaração de conclusão da main.
- Primeira observação da 1D: **10/10/2026 18:03:07 America/Recife**. Prazo absoluto original: **12/10/2026 15:03:12 America/Recife**; a contagem não foi reiniciada.
- Checkpoint do host pronto para retomada: **10/10/2026 18:09:57 -03**; aproximadamente **44h53min** até o prazo. HEAD remoto de cada entrega deve ser conferido com `git ls-remote`, sem inventar o SHA do próprio commit documental.
- Lidos os relatórios e auditorias 1A/B/C, plano SERVER-1, arquitetura Core-First, CLI.md, IPC.md, plano Narys 0.1, planejamento operacional e contexto LR-10. Nenhuma funcionalidade 1A–C foi reimplementada e nenhum binário/unidade instalado foi substituído nesta preparação.

## Matriz dos critérios e limites de aprovação

| Etapa / critério | Situação | Evidência / limite |
|---|---|---|
| 1A: Core independente de GUI, authority única, SQLite/migração, IPC/lifecycle | PASS técnico ratificado por Luna | [Auditoria 1A](NARYS-SERVER-1A-INDEPENDENT-AUDIT-2026-10-10.md); conferência instalada e dados atuais abaixo |
| 1B: Conversation/Scheduler/providers, Stronghold existente, resultados/recovery | PASS funcional ratificado por Luna | [Auditoria 1B](NARYS-SERVER-1B-INDEPENDENT-AUDIT-2026-10-10.md); duas conversas reais históricas continuam preservadas |
| 1C: CLI, consultas/paginação, unlock humano encapsulado | PASS técnico-funcional ratificado por Luna | [Auditoria 1C](NARYS-SERVER-1C-INDEPENDENT-AUDIT-2026-10-10.md); unlock sintético anterior não substitui humano pessoal |
| 1D: Fedora atual em multi-user.target; GDM ausente; linger; autostart habilitado; readiness/socket/SQLite | PASS do estado observado | [Host atual](evidence/server-1d/host-ready-for-reboot.json); ainda não é novo boot frio |
| 1D: Termux/SSH humano, doctor e credentials status | PASS das consultas pré-reboot, por relato do operador | [Registro sanitizado](evidence/server-1d/human-preboot.json); logind no host confirma sessões Remote=yes/sshd/tty/user, sem atribuir independentemente qual é o celular |
| 1D: novo cold boot e senha digitada exclusivamente pelo humano em SSH direto | **PENDENTE** | Keyring já unlocked; nenhum unlock pessoal simulado, senha solicitada ao agente ou alteração global |
| 1D: conversa Groq nova, recibo, resultado e histórico pelo celular | **PENDENTE** | Autorização específica: máximo 2 chamadas Groq Free, quota confirmada, sem overage/retry/fallback pago; **0 usadas** neste checkpoint |
| 1D: desconexão/reconexão SSH/Termux durante tarefa nova | **PENDENTE** | Clientes locais novos e independence do serviço provados; não substituem a ação do operador no celular |
| 1D: histórico/estado após shutdown e interrupção/restart | PASS no serviço instalado e nos testes isolados | [Lifecycle](evidence/server-1d/lifecycle.json), [paginação integral](evidence/server-1d/host-ready-for-reboot.json), [8 testes](evidence/server-1d/protocol-tests.txt) |
| 1D: cancelar tarefa elegível sem replay | PASS limitado a tarefa preparada lr10a:3 | [Cancelamento real](evidence/server-1d/cancel-prepared.json). Não houve submit nem agente iniciado. Cancelamento de inferência de produto ativa nova não é demonstrado por esse caso |
| 1D: segurança e recursos proporcionais | PASS das observações delimitadas | Socket/UID, cofre imutável, logs sanitizados e CPU/RSS abaixo; sem alegação de sandbox ou benchmark prolongado |
| SERVER-1 inteira / Narys 0.1 / main concluída | **NÃO APROVADOS** | Gates humanos pendentes e auditoria independente 1D necessária; ferramentas agentivas continuam indisponíveis |

## Fedora, instalação e credenciais

Fedora Linux **44 Workstation**, alvo existente `multi-user.target`, alvo multi-user ativo, GDM inativo, `sshd` ativo. `narys-core.service` é de usuário, enabled, Type=notify, linger=yes e executado pelo user manager. O runtime indica `graphical_environment_present=false`, tools=0, agent_execution_authority=false e zero workers/tarefas ativos. Os processos selecionados não incluem GNOME Shell, Xorg/Xwayland ou Copilot residente. A instância Codex que conduz esta sessão de desenvolvimento não é um executor agentivo da Narys.

Keyring de usuário existente ativo e **unlocked**, com backend GNOME50 compatível. O operador retornou `doctor` e `credentials status` do terminal privado do Termux, confirmando Core alcançável e unlock_transport=private_authenticated_ssh_tty. As propriedades logind das sessões SSH 2 e 3 são compatíveis com Remote=yes, Service=sshd, Type=tty, Class=user e UID1000. Nenhuma senha pessoal foi solicitada, conhecida, inserida ou capturada pelo agente. Não se executou `credentials unlock` para fingir bloqueio/desbloqueio; o teste legítimo aguarda cold boot.

O Core abriu o Stronghold original por leitura em um probe existente, que retorna somente `existing_snapshot_opened=true`, `writes=false`, `migration=false`, `secret_values_returned=false`. SHA-256 e inode/tamanho/permissões/mtime/ctime permanecem iguais à 1C. A autorização LR-10A continua encerrada e seus metadados preservados. Não se criou coleção, snapshot, chave ou arquivo de senha. O acesso usa o cofre existente; não houve consulta remota a billing.

Instalação preservada da revisão `06ef4076380ed2723b9c567f5323104ec596e846`, com hashes conferidos iguais à 1C:

| Artefato | Caminho / SHA-256 |
|---|---|
| CLI | `~/.local/bin/narys`, modo0700; `5719709444be4a62503a6ddfbdd6ecb9a3049944541fd26d1a9030e9a9032328` |
| Core | `~/.local/lib/narys/narys-core`; `6a68ae32e967eb2d196b890424e9ac9114ebe5a0e9de6636d7be97d97c98c8bd` |
| Unidade | `~/.config/systemd/user/narys-core.service`; `df7b3205ebfcf5f673e9c72fd8aea4c0bba2c697d3f15a76feaaf4028b1a0810` |

Nenhuma configuração global de boot/GDM/SSH/Keyring foi alterada; nenhum sudo usado. [Baseline de retomada](evidence/server-1d/resumption-baseline.json) registra alvo, hash GDM e hashes das unidades efetivas. `sshd_config` não é legível sem privilégio, o que foi preservado como limite sem pedir elevação.

## Persistência, tarefas e recuperação

Antes das mutações legítimas do gate, SQLite online backup privado com WAL preservou **38 sessões, 99 mensagens, 191 tarefas de produto, 39 subtarefas e 2 tarefas legadas**. Backup fora do Git: `~/.local/state/narys/core/backups/server-1d-o7b9amba/authority.sqlite3`, modo0600, SHA-256 `8acfe11831e1b059fcda3de09c7726ffd9cefbacb371ec921b9304ff7d1af145`. Diretório0700; backup/integridade/fsync confirmados. O primeiro probe de auditoria encontrou `/proc/PID/fd` protegido pelo Core nondumpable depois de criar o backup; o script foi ajustado para registrar essa limitação sem ampliar privilégios.

[Auditoria final pré-reboot](evidence/server-1d/host-ready-for-reboot.json) compara todas as linhas originais de sessões/mensagens/tarefas/runs/subtarefas/identidade/memórias e tarefas LR-10A com esse backup: **todas preservadas exatamente**. CLI instalada pagina todas as sessões/mensagens/tasks; resultados e eventos por cursor são iguais ao SQLite autoritativo sem exportar conteúdo privado. Schema20, quick_check=ok e zero foreign key errors.

Alterações novas e factuais: sessão **39**, entrada técnica de cancelamento, tarefa **product:192**; e tarefa preparada **lr10a:3**, cancelada antes de submit. Totais atuais **39 sessões, 100 mensagens, 192 TaskRecords, 4 runs, 3 tarefas legadas**, 52 eventos. Tasks189/190 completed e191 cancelled mantêm resultados/hash originais.

A tentativa product:192 foi admitida com todos os providers temporariamente desabilitados, garantindo **zero envio remoto**. Falhou com `free_provider_authorization_required` antes de o próximo processo CLI poder cancelar; cancel retorna already_terminal, sem resultado fabricado. [Registro](evidence/server-1d/cancel.json): eligible_cancel_pass=false. Isso é uma corrida legítima entre término e cancelamento, não evidência de cancelamento ativo. A permissão Groq foi restaurada em finally; os outros providers permaneceram desabilitados.

Para verificar cancelamento elegível sem gastar outra chamada, preparou-se **lr10a:3** pelo contrato legado local e cancelou-se pela CLI oficial `narys cancel 3 --namespace lr10a`: eventos prepared→cancelled_before_send, estado cancelled e repetição already_terminal. Nenhum `submit`, recibo financeiro novo, runtime Copilot, ferramenta ou inferência executado. Não é uma tarefa agentiva concluída nem reabertura da LR-10A.

[Lifecycle real](evidence/server-1d/lifecycle.json): shutdown via systemctl --user stop removeu socket/MainPID e informou stopped; start criou PID70468. Depois, SIGKILL **somente ao Core ocioso** levou a PID70500 por Restart=on-failure, NRestarts=1. Runs/resultados e contagens foram idênticos antes/depois dos dois ciclos. Nenhum trabalho importante, tarefa ativa ou worker existia no preflight. Testes isolados cobrem crash com tarefa uncertain→interrupted uma única vez/restart_never_retries, sem injetar dados artificiais no banco pessoal.

## CPU/RSS e investigação do pico

[Amostra isolada](evidence/server-1d/memory-isolated-preboot.json), sem compilação ou inferência concorrentes:

| Fase | Duração | RSS observado | CPU adicional do Core |
|---|---|---|---|
| Idle antes | 10,007s | máximo29228KiB, cerca de28,5MiB | 0,00s |
| Leitura Stronghold existente | 1,715s | pico553556KiB, cerca de540,6MiB; volta a29228KiB | 1,46s |
| Idle após | 10,007s | máximo29228KiB; final29136KiB | 0,00s |

Polling a cada50ms; o pico observado é amostrado, não garantia do máximo absoluto. Cgroup MemoryPeak547401728 bytes, abaixo de MemoryHigh600MiB e MemoryMax900MiB. Eventos high/max/oom/oom_kill=0, PSI de memória do serviço=0. Host observado: 8007528KiB RAM, mais de7 milhõesKiB disponíveis. Não houve impacto funcional ou indício de pressão nessa amostra. O pico da 1B é reproduzível e transitório, distinto de idle.

Diagnóstico a partir do código local pinado: `stronghold_engine 2.0.1` abre snapshot v3 por age; `iota-crypto 0.23.2` usa scrypt, r=8, p=1. O **parâmetro público** work factor do snapshot existente é19; o buffer principal do `scrypt` ocupa `128 * 8 * 2^19 = 536870912` bytes (**512MiB**). A associação com o pico é inferência apoiada pelo caminho de código, parâmetro e amostra, sem profiler de alocações. Não se alterou KDF, work factor ou snapshot para reduzir consumo. Abrir o cofre é custo por demanda; o Core não mantém esse buffer como idle.

[Primeira amostra](evidence/server-1d/memory-preboot.json) coincidiu com Cargo e está explicitamente `valid_for_isolated_host_idle=false`; preservada apenas como observação concorrente. Nenhuma medição curta é classificada como benchmark prolongado. Picos de LLM ainda pendentes neste gate; nenhum SDK/agent foi iniciado para performance.

[Idle após recovery](evidence/server-1d/idle-after-recovery.json), sem probe do cofre: **26492KiB RSS (~25,9MiB), CPU adicional0,00s em10,004s**, PID70500 estável. Confirma o comportamento ocioso da instância recuperada; não é benchmark prolongado.

## Segurança, testes e fronteiras

- Unix socket0600 e diretório0700; SO_PEERCRED retorna UID1000 e PID igual ao MainPID. O servidor e cliente conservam as verificações auditadas. Sem password/authority/HumanLocal via IPC.
- Narys só faz bind de UnixListener em produção. Inventário TCP mostra serviços do sistema/SSH e listener Codex em loopback; não é API Narys. A atribuição dinâmica de todos os sockets ao PID é limitada por `/proc/PID/fd` inacessível devido ao Core nondumpable; isso está registrado, sem bypass/elevação.
- [Logs da 1D](evidence/server-1d/log-safety.json): entradas do Core conferidas como rótulos fixos/timings numéricos; sem assinaturas suspeitas detectadas e sem logs brutos exportados. Scanner não prova ausência de qualquer segredo concebível. Evidências contêm hashes/contagens/estados e texto técnico criado para o gate, nunca histórico privado ou senha/token.
- [Oito testes IPC direcionados](evidence/server-1d/protocol-tests.txt) PASS, incluindo singleton, limites/versionamento/authority, crash/recovery sem replay, permissões gratuitas, clientes desconectados, paginação CLI e recusa de unlock automatizado. Compilação offline/locked; warning herdado de método de telemetria não utilizado. Não se repetiram suítes históricas domínio/desktop não alteradas.
- Scripts novos de auditoria/métricas/cancelamento foram executados no host; py_compile e git diff --check passaram. São instrumentos operacionais, não novo runtime de produto.
- Mesmo UID é a fronteira de confiança; HOST_ASSISTED_NOT_SANDBOX. Cancelamento não desfaz efeitos remotos e não garante concluir antes de tarefa rápida. Ctrl-C/saída do cliente não cancela automaticamente workers do Core.

## Plano persistido de reboot e retomada

**Reboot deve ser humano e coordenado somente depois do push deste checkpoint.** Ele pode encerrar a própria sessão Codex. Não alterar boot/GDM/SSH/Keyring nem reiniciar o Keyring isoladamente. Não executar reboot enquanto compilação, tarefas pessoais ou outras operações importantes estiverem ativas. O preflight do Core desta entrega está ocioso; o humano confirma o restante do host.

1. Reboot pelo humano; reconectar no Termux com `ssh sam@HOST` em shell interativo direto, fora de tmux/screen, sem `ssh HOST comando` ou gravação de terminal.
2. No host, executar `~/.local/bin/narys doctor` e `~/.local/bin/narys credentials status`. Informar somente Core alcançável e locked/unlocked/erro seguro.
3. **Se locked**, executar `~/.local/bin/narys credentials unlock` diretamente nesse terminal. A senha é digitada exclusivamente pelo usuário sem eco; nunca enviar ao Codex ou registrá-la. **Se já unlocked**, não simular unlock; registrar e investigar a origem legítima. Confirmar novamente credentials status.
4. Retomar Codex no mesmo checkout/conversa e informar “SERVER-1D pós-reboot”, estados e eventual código de erro, sem senha. Codex deve comparar novo boot_id com `299dc161-7036-4c0f-b012-82b1150b528c`, serviço, PID/linger, ausência de GUI, snapshot e backup; executar auditoria com `--baseline` apontando ao backup privado acima. Antes de exigir password, respeitar o estado reportado/observado; nunca operar a TTY privada do usuário.
5. Após o unlock legítimo, Codex prepara a política específica Fixed Groq, maxProviderCalls=1, retryEnabled=false, maxRetries=0, output≤512 e preserva a política original para restauração. **Ainda não enviar nada manualmente** até essa preparação estar confirmada.
6. Humano envia a primeira mensagem técnica na sessão39 pela CLI oficial, recebe TaskId/recibo, encerra SSH/Termux, reconecta e consulta task/session/events. Codex acompanha Core/processos/uso e registra resultado factual/ausência de replay. Segunda chamada consulta o marcador pelo histórico da mesma sessão; máximo total2 tentativas, sem repetição em erro incerto. Os prompts e IDs exatos serão informados durante a coordenação, sem usar conversa pessoal.
7. Reconfirmar persistência após restart sem inferência automática, restaurar política original e atualizar **este mesmo relatório** com matriz/horários/HEAD remoto finais e limites. Commit/push exclusivamente nesta branch. Entregar à Luna para auditoria; nenhuma main/merge antes dela.

## Desktop, dívidas e backlog para 17/10

Desktop legado permanece deliberadamente cercado pelo takeover e **não operacional**. Compilação/testes herdados não provam GUI funcional. Avatar/apresentação permanecem opcionais e não executados no servidor; faltam thin client Tauri/IPC e smoke sem writer concorrente. Não abrir o desktop antigo contra a base migrada para tentar validar a 1D.

Pendências da própria 1D: cold boot real, unlock pessoal humano SSH/Termux, duas conversas novas e reconexão celular, medições LLM, relatório final/push e auditoria Luna. Responsáveis: humano para reboot/TTY/senha/Termux; Codex para gate/evidências/correções locais; Luna para auditoria independente. Sem esses passos não fechar PASS.

Backlog prioritário após auditoria/fechamento: **LR-10B–F e LR-11**, conforme [meta Narys0.1](NARYS-01-AGENT-TOOLS-DELIVERY.md), para agentes com ferramentas reais até **17/10/2026**. B: adapter/supervisor Copilot sob demanda; C: authority/approval por tarefa/operação/workspace e sandbox onde prometido, sem HumanLocal; D: TaskGraph/quota/usage/trace duráveis; E: tarefa real de engenharia com tool_call→autorização→efeito→diff→teste/build, uso via Termux; F: gate integrado/auditoria/cancel/reap/recovery. LR-11 deve provar o executor Codex separadamente, com autenticação/approvals/cancel/sandbox/status próprios. Approval/ToolRequest da CLI ainda retornam capability_not_integrated; chat não satisfaz o marco agentivo.

Dívidas herdadas objetivas: GNOME50/backend pinado e interface interna não suportada; dependências Python3/PyGObject/libsecret; caminhos históricos supervisor/recibos Copilot; sessões de SDK em /tmp não garantidas após reboot; isolamento adversarial/supervisor de descendentes ainda não provados; providers Gemini/Cloudflare/Mistral integrados mas sem teste remoto atual/autorização deste gate; MSRV1.94 não ensaiado; desktop IPC pendente. O pico512MiB da KDF exige headroom e observação quando agentes concorrerem, mas não mostrou OOM/impacto aqui. Nenhum desses limites foi ocultado como PASS de Narys0.1.
