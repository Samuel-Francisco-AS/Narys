# LR-10B — Copilot Adapter & On-Demand Supervisor

**ESTADO VIGENTE: LR-10B ENCERRADA — PASS FINAL DELIMITADO.** [Auditoria independente](LR-10B-INDEPENDENT-AUDIT-2026-10-10.md) e [fechamento/PR #26](LR-10B-FINAL-CLOSURE-2026-10-10.md). O restante deste documento preserva a arquitetura e a cronologia da candidata.
Base verificada: `main` / `origin/main` em `21be6382d146c99056d8dc99e6a13e2fa0dbe489`.
Branch: `lr-10b-copilot-adapter-supervisor`. Sem PR, merge ou PASS definitivo.

Este documento descreve a implementação; resultados e inventário completos estão
no [relatório de entrega](LR-10B-DELIVERY-REPORT.md) e em
[evidências](evidence/lr10b/). Não amplia os encerramentos da LR-10A ou SERVER-1.

## Arquitetura e decisão

`RuntimeServices` compõe um `CopilotAgentAdapter` no `AgentRegistry` compartilhado,
um `CopilotLifecycle` e um `AgentRuntimeSupervisor`. O Copilot é SpecialistAgent;
não entra no ProviderRegistry, não recebe recursos cognitivos do Scheduler e não
substitui Luna. O Codex mantém seu backend e capabilities originais de planner
read-only (planning/structured_output, zero ferramentas).

O contrato legado `AgentRequest` não contém TaskId, autorização financeira ou
escopo de efeitos. Sua chamada ao Copilot falha fechada (`Unavailable`); pedidos
de ferramentas falham como `UnsupportedCapability`. O registro Copilot tem
`enabled=false` para **eleição de planner**: texto não passa a ser um gerador de
PlanV1 validado. O adapter possui a capacidade textual declarada separadamente de
tools/permissions/inference admission; nenhum novo envio é admitido nesta entrega.
As operações integradas de lifecycle são reais chamadas do SDK, sem prompts.
LR-10C/D deverão conectar os gates de autoridade/admission antes de habilitar
texto ou ferramentas nas rotas de tarefas de engenharia.

```text
CLI / Unix IPC v1 → Core / RuntimeServices / AgentRegistry
                       └─ CopilotAgentAdapter + CopilotLifecycle
                            ├─ TaskId compartilhado / TaskGraph lifecycle
                            ├─ SQLite autoritativo / server_events
                            ├─ OperationalTraceBus (SpecialistAgent)
                            └─ AgentRuntimeSupervisor (máximo 2 demandas)
                                 └─ github-copilot-sdk = 1.0.17 / stdio
                                      └─ owner Python privado (filho do SDK)
                                           └─ guardian subreaper
                                                └─ CLI nativo 1.0.95 / RPC3
```

O SDK oficial executa o protocolo e suas operações de sessão. O código Python
não interpreta JSON-RPC, não implementa um agente nem recebe comandos de shell
por IPC. Ele está embarcado no binário (`include_str!`, Python `-I -c`), não depende
do checkout instalado e só gerencia filhos próprios. A escolha cobre a limitação
Linux do SDK pinado: sua process-tree crash-safe é específica de Windows, enquanto
Linux depende do filho direto. A topologia acrescenta ownership de descendentes,
sem alegar sandbox ou mediação das ferramentas nativas pelo ExecutionBroker.
[Referência oficial do SDK Rust](https://github.com/github/copilot-sdk/tree/main/rust).

## Contratos e estado

`narys-domain::agents::lifecycle` acrescenta referências opacas, operações
Create/Resume, estados de runtime/sessão, capabilities e nomes dos perfis futuros.
Assisted/Isolated/ExplicitYolo são **somente vocabulário**, não permissões. Nenhuma
operação wire permite selecionar perfil, authority, origin, programa, env ou prompt.

O supervisor mantém:

| Estado | Fato / próximo passo |
|---|---|
| Dormant | Sem processo próprio, cleanup comprovado; status não cria demanda |
| Starting | Primeira demanda admitida inicia uma geração; outras aguardam o mesmo lock |
| Ready | SDK/versão/protocolo verificados, antes de iniciar operação |
| Busy | Um ou dois leases; nenhuma parada enquanto outro lease está ativo |
| Stopping | Último lease termina; SDK stop com deadline, guardian confirma reap |
| Faulted | Falha registrada; cleanup incerto bloqueia novas inicializações |

A admissão mantém no máximo **duas tarefas**, incluindo startup e cleanup, sem fila
ilimitada. Startup/start/stop são serializados por um único mutex assíncrono.
O refcount só diminui após detach/erro da operação. O último lease faz shutdown;
leases anteriores podem terminar com `cleanup_verified=false` enquanto outro
trabalho ainda utiliza o processo. O registro de ownership confirma posteriormente
cleanup para todos os runs daquela referência de runtime. O estado funcional
terminal não é reescrito por essa atualização de evidência.

Nenhum worker, SDK, CLI, health polling ou quota RPC é criado no boot, por status,
Settings, listagem de modelos ou consulta de sessões. Não há timer de idle.
Somente durante recuperação/cleanup há espera bounded de 10–20ms, não em ociosidade.
Uma nova demanda explícita pode iniciar outra geração depois de cleanup verificado;
nenhum cancelamento ou resultado incerto inicia retry/restart automático.

## Ownership e cleanup

Cada runtime recebe diretório privado 0700, linha `agent_runtime_owners` antes do
launch e recibos 0600 com PID/start_ticks. SDK possui o owner principal; seu
subreaper filho possui o CLI. Ambos são subreapers privados e monothreaded.

- pidfd do pai sinaliza morte do Core/owner sem polling;
- pidfd do filho sinaliza término do runtime/guardian;
- somente filhos **diretos e ainda não reaped**, verificados antes/depois de
  `pidfd_open`, podem receber sinal;
- `setsid`, reparenting e netos são tratados por adoção iterativa;
- `waitpid(-1, WNOHANG|__WALL)` + `ECHILD` e inventário vazio comprovam exaustão;
- não se usa nome de processo, PID numérico recebido por IPC ou process group
  como autoridade. Nenhum processo externo é sinalizado;
- stop SDK recebe 5s; em falha/deadline, `force_stop` aciona recovery. Cada reaper
  tem cleanup de 4s e o Core espera prova/ausência das identidades por até 6s;
- sucesso funcional exige detach; shutdown não gracioso é relatado como
  `sdk_shutdown_recovered`, mesmo quando o reap foi comprovado;
- falha/panic de backend não abandona o caminho de cleanup. Falha de observador
  é contida e registrada como gap.

O `owner_json`/`cleanup_json` sanitizado fica no SQLite. Até 32 diretórios recentes
com prova de cleanup são mantidos; prune só remove diretórios registrados e
privados do próprio runtime. Sessões/histórico/SQLite não são apagados por prune.
Estado de processos em `agent-status` combina recibo com leitura de `/proc/stat`
por identidade, distinguindo alive/zombie/not_present/identity_changed. Não lê
cmdline, environ, tokens ou protocolos.

**Limite:** matar simultaneamente os dois reapers fora da unidade systemd não é
contenção comprovada. O serviço mantém `KillMode=control-group`; nenhum sandbox,
proteção contra adversário do mesmo UID ou garantia de filho hostil que escape do
cgroup é declarada. Esse hardening e execução de ferramentas pertencem à LR-10C/F.

## Sessões, persistência e recovery

Schema021 aditivo acrescenta `agent_sessions`, `agent_runs`,
`agent_runtime_owners`. Usa a mesma `Database`, migrations e WriterLease do Core.
O processo do SDK/reapers nunca abre SQLite. O TaskId é reservado no TaskRegistry
existente; boot inclui max(agent_runs) para impedir colisão com Conversation.
`tasks`, `task-get` e `task-cancel` continuam no namespace `product`.

As sessões ficam em `core/copilot/sessions/cs-<128 bits aleatórios>`, não /tmp.
ConfigDir é privado por sessão, SDK baseDir é privado e estável. O Core persiste
separadamente o ID do SDK e um hash do evento raiz (`session.start` + sessionId).
IDs provider/diretórios/anchor não são enviados ao cliente. A sessão pública usa
somente sua referência opaca; autenticação do socket continua same UID.

Create registra Creating antes de efeitos e termina Detached após SDK create,
histórico e detach. Resume só admite sessão Detached com provider ID, anchor e
diretório próprio ainda válido. Depois do resume, SDK deve retornar o mesmo ID
**e o mesmo anchor de histórico**; missing/history mismatch falha sem create fallback,
transcript recovery, send ou replay. Create sem evento raiz pode terminar como
operação de metadata válida, mas não ganha prova para resume.

Estados Cancelled/Failed/Interrupted/Closed não autorizam retomada. No restart,
Pending/Running tornam-se Interrupted com `restart_never_retries`, e
Creating/Resuming tornam-se Interrupted. Recovery de ownership verifica os recibos,
sem iniciar CLI nem sinalizar PIDs antigos. Outro boot ID prova que os processos do
kernel anterior desapareceram; na mesma inicialização do kernel, falta de prova
continua Faulted. Recovery tem limite global de 7s e não impede Conversation/IPC
de operar quando apenas o especialista está faulted.

Attach é referência de observação, limitada a 32 clientes. Detach/desconexão SSH
não cancela a tarefa; attachments desaparecem no restart. Close é explícito,
recusa sessão com operação ativa e bloqueia resume. Não afirma apagar sessão remota.
Cancelamento é cooperativo (abort/detach) com escalada forçada no shutdown bounded.
Startup em andamento resolve dentro do deadline antes de cancelar/detachar uma
sessão eventualmente criada. Um mutex serializa cancelamento aceito com commit
terminal; falhas de cleanup/segurança prevalecem sobre cancelamento concorrente.

## Observabilidade e autoridade

Progress/terminal têm TaskId, correlation aleatória própria e source
SpecialistAgent/copilot. TraceBus continua bounded; `server_events` conserva
retention4096/páginas128/gap. Eventos têm códigos fixos, sem texto do agente,
private reasoning, payload de permissão, prompt, stderr ou credencial. Lag/cap de
observação é registrado em `observation_gaps`. Falha de observer/store de eventos
após admissão não invalida uma operação válida nem transforma falha real em sucesso.
Admissão durável e commit do estado continuam obrigatórios; erro de commit fecha
admissão e preserva incerteza para recovery, nunca duplica a operação.

DenyAll + availableTools=[] + MCP{} + hooks/skills/discovery/extensions/host Git
false, infinite sessions false, no custom instructions. Ambiente do filho usa
allowlist HOME/PATH/LANG/runtime/DBus e opções fixas, sem tokens/env de SSH ou Python
pessoal. CLI hash é verificado antes de launch. SDK/CLI não foram atualizados.

HumanLocal permanece exclusivamente humano; ExecutionBroker/Authority não foram
ampliados. Approvals, native agent tools e generic shell continuam recusados.
`submit` LR-10A retorna `lr10a_authorization_closed`; diagnósticos experimentais
retornam `legacy_copilot_diagnostic_closed_use_agent_lifecycle`. Prepare/cancel/
result/histórico permanecem legíveis. Worker/harness históricos não são o caminho
novo do serviço, e os testes antigos continuam isolados/sintéticos.

## Operação, manutenção e rollback

```sh
narys agent status             # local, lazy, zero metadata RPC
narys agent new                # admite create SDK SEM prompt/ferramentas
narys task ID --wait           # acompanhar pelo recibo; Ctrl-C não cancela
narys agent get cs-HEX
narys agent attach cs-HEX      # obter ca-HEX; observação, sem lease de execução
narys agent detach ca-HEX      # nunca cancela tarefa
narys cancel ID                # product, cooperativo + fallback bounded
narys agent resume cs-HEX      # explícito, exige prova; sem create/send fallback
narys agent close cs-HEX
narys agent recover            # reconciliar ownership, SEM launch/retry
narys agent stop               # fechar admissão/drenar apenas especialista
```

Não executar `new`/`resume` como polling. Não executar inferência/ferramenta para
validar esta entrega. O diagnóstico real, caso registrado no relatório, limita-se
a lifecycle/metadata; não consome autorização da LR-10A.

Build/test offline com COPILOT_SKIP_CLI_DOWNLOAD=1, CARGO_BUILD_JOBS=2 e
CARGO_TARGET_DIR=src-tauri/target. Atualizador verifica ausência de trabalho por
IPC antes de parar somente narys-core.service, preserva binário/unidade/CLI e
snapshot SQLite antes de schema021. Não altera boot/SSH/GNOME/Keyring/Stronghold.

Rollback operacional imediato: `narys agent stop` mantém Conversation/servidor
ativos e preserva dados. Para rollback de binário, **não** apontar binário schema020
para a autoridade schema021 nem baixar user_version. Usar release de recuperação
que entenda schema021/max agent TaskId, ou reconciliar dados antes de restauração
humana de snapshot. Backups não autorizam descartar mensagens/tarefas novas.
Instalação anterior continua preservada; não há rollback destrutivo automático.

## Gates seguintes

LR-10C: política por operação, approvals, sandbox validado, perfis reais e native
tools. LR-10D: integração de tarefas de engenharia, quota/AI Credits/admission e
result/evidence. LR-10E: UX/gate real de engenharia autorizado. LR-10F: stress,
endurance, falhas simultâneas/adversariais, MSRV exato e portabilidade. LR-11 mantém
gate independente de executor Codex. Nenhum desses gates recebe PASS por esta entrega.

## LR-10B FIX-1 — Startup Ownership & Recovery Safety

A auditoria da candidata `45bc5ad0cabfd49ccf1967a8ba7fda5a9d4fea75`
identificou retornos antecipados após INSERT e classificação de cleanup por lista
negativa de códigos de erro. A FIX-1 remove essa inferência e preserva os demais
contratos, pins, wrapper, schema021 e política de autoridade.

`RuntimeFactory::start` retorna `StartupFailure`, com erro original sanitizado,
`runtime_ref`, erro de segurança secundário e `StartupSafety` explícito:
NoProcessLaunched, CleanupVerified, CleanupUnverified ou PersistenceUncertain.
Somente as duas primeiras permitem outra geração. O supervisor não consulta o
nome do erro para decidir a segurança do startup; o mesmo código é testado com
as quatro classes. O IPC adiciona `runtime.startup_failure` sem alterar intents.

O journal version1 em `owner_json.startup` usa o SQLite autoritativo existente:

| Fronteira | Estado persistido / autorização |
|---|---|
| Antes do INSERT | Diretório temporário RAII; nenhum SDK/CLI invocado |
| preparing | INSERT em transação Immediate, depois de rejeitar ownership pendente |
| preparação | workspace/logs/sdk-state/options, todas as saídas finalizadas pela guarda |
| launch_intent | Commit obrigatório antes de invocar Client::start; um crash aqui é incerto |
| startup parcial | SDK/owner podem existir; falha exige stop, prova kernel e commit terminal |
| ready | IDs de owner/guardian/CLI válidos, owner principal corresponde ao SDK e commit pronto |
| failed_before_launch | Prova sdk_launch_not_invoked; stopped/verified após commit válido |
| failed_after_launch | stopped/verified somente com prova kernel e commit, senão faulted/unverified |

Uma falha de persistência permanece PersistenceUncertain, inclusive se o cleanup
físico depois funcionar. O erro original não é substituído pelo erro de cleanup.
A guarda Drop registra faulted/unverified e conserva a última fronteira; nunca
certifica limpeza em destructor. Quando o banco rejeita também esse registro,
conserva-se o último journal durável; o snapshot mantém o erro e o run vinculado
registra o diagnóstico quando seu commit é possível. Falha geral de armazenamento
preserva a operação como incerta para o recovery, sem novo launch.

Admissões seguintes validam certificados terminais, e não apenas o bit verified.
Recibos ausentes/inconsistentes são marcados faulted/verified=false. Artefatos sem
linha correspondente bloqueiam lançamento e recovery; não são adotados/removidos
para liberar o serviço. Ownership + atualização dos runs no cleanup são atômicos.
O finish de tarefa não herda um bit antigo para uma falha de startup incerta.

Recovery valida vínculo ref/path/boot, journal e certificado. Preparing e
failed_before_launch válidos, sem recibos/IDs contraditórios, provam a barreira
anterior ao launch e podem ser concluídos sem subprocessos. Launch_intent, ready,
falhas posteriores e rows legadas sem journal precisam de prova kernel ou de
outro boot válido. JSON/boot inválidos não ganham prova por conveniência.
Recovery é idempotente, não sinaliza PID de recibo e não recria sessão, TaskId ou
inferência. Ao remover uncertainty, apenas permite uma demanda futura explícita.

Não existe migration nova. Campos JSON são compatíveis com schema021; rows antigas
com prova completa continuam aceitas. Rows legadas incompletas não são presumidas
prelaunch. Uma intenção interrompida sem prova permanece bloqueante no mesmo boot,
mesmo se a fixture sabe que não invocou SDK: esse conhecimento não sobrevive ao crash.
Resultados, testes e operação constam da seção FIX-1 no relatório de entrega.

Recuperação operacional: `narys agent status`, consultar tasks/events, depois
`narys agent recover` para reconciliar evidência existente, sem launch. Se persistir
incerteza, manter o especialista parado; reparar armazenamento/recibos somente com
proveniência e backups verificáveis. Não apagar rows, subir verified nem reenviar
operações como procedimento de recovery. Não reiniciar host/serviços externos nesta
FIX. Outro boot legítimo futuro é evidência distinta, não um retry do Core.
Rollback imediato continua `narys agent stop`, preservando Conversation. O schema
não mudou; o binário anterior é preservado, mas reinstalar a candidata auditada
reintroduz o defeito e não deve reabilitar lifecycle Copilot sem a FIX-1.
