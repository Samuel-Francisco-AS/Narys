# LR-9D — Cognitive / Agent Trace Adapters

**Estado:** **PASS TÉCNICO + AUDITORIA INDEPENDENTE — encerrada em 08/10/2026.**
Os gates locais e a auditoria independente estão concluídos; nenhuma FIX-1
bloqueante foi necessária.

Base confirmada após workspace limpo e `git fetch origin --prune`:
`46da34e0cf316fb11f4ed01a0f516b627641991f`. Branch exclusiva:
`lr-9d-cognitive-agent-trace-adapters`. Main preservada; sem PR, merge,
rebase, force-push ou reset destrutivo. LR-9A/B/C estão em PASS, auditadas e
integradas. O cabeçalho e a próxima ação stale da LR-9C no track foram
normalizados nesta branch, sem reescrever registros históricos.

## Arquitetura

```text
TaskEventKind ────────┬──→ Channel / TaskEventSink existentes
                     └──→ TaskTraceAdapter
SchedulerEvent ──────┬──→ callback funcional existente
                     └──→ SchedulerTraceAdapter (contexto local por invocation)
Summary process ────┬──→ reserva tardia de ID / persistência existentes
                    └──→ SummaryTraceAdapter
AgentEvent ─────────┬──→ sink funcional com ACK existente
                    └──→ AgentTraceSink::Lifecycle
Codex notification → inspeção fail-closed existente → display delta permitido
                                                        ↓
                   PassiveTracePublisher → OperationalTraceBus::process_wide()
                                                        ↓
                                    transporte / Activity LR-9C existentes
```

A camada está em `operational_trace/adapters/{task,scheduler,agent}.rs`, com
publisher comum em `mod.rs`. Mapping é centralizado, com taps pequenos nos
emitters Conversation, Orchestrator, TaskGraph e Worker. Publication acontece
antes e independentemente da entrega funcional; nenhuma rejeição de trace
participa do retorno dessa entrega. Os callbacks Scheduler continuam com a
mesma assinatura e semântica de erro.

`TracePublisher` é substituível por bus isolado, no-op ou fixture que retorna
erro. `PassiveTracePublisher` descarta erros de validação/construção/publish,
exhaustion e até unwind de publisher defeituoso. Não faz retries, I/O, logging
por delta ou chamadas cognitivas. O bus LR-9A continua único em produção, com
sua própria sequence e budgets intactos. Não foi otimizado/reaberto nesta fase.

## Responsabilidade única / deduplicação

- Task adapter: lifecycle de Core, TaskGraph e Worker; counts e result markers.
- Scheduler adapter: única publicação de admission, routing, retry/fallback e
  provider output, inclusive quando a fonte também gera um TaskEvent funcional.
- Agent adapter: lifecycle e conteúdo explicitamente autorizado pelo backend.
- ExecutionBroker: integração LR-9B original, sem segunda publicação.

Task ignora antes de alocar provenance: ProviderQueued, ProviderAdmitted,
ProviderSelected, ProviderChunk, ProviderRetry, ProviderFallback,
ProviderOutputObserved, SubtaskRetry e SubtaskOutputObserved. Tests contam
Selected/Chunk/Retry/Fallback de uma execução real do Scheduler com sua projeção
TaskEvent recebida simultaneamente. O cenário TaskGraph conta apenas
SubtaskStarted, provider_selected, output_observed e SubtaskFailed, apesar de
3000 projeções funcionais de output.

## Mapping TaskEvent

| Fonte factual / evento | Classe / kind / code | Payload |
| --- | --- | --- |
| Core Conversation/Orchestrator/mock: TaskStarted | STATE / Started / task_started | vazio |
| TaskGraph root: TaskStarted | STATE / Started / task_started | vazio |
| StepStarted / StepCompleted | STATE / Checkpoint / step_started, step_completed | prepare ou verify |
| TaskPaused | STATE / Checkpoint / task_paused | vazio |
| TaskCompleted | CRITICAL / Completed / task_completed | vazio |
| TaskCancelled | CRITICAL / Cancelled / task_cancelled | vazio |
| TaskFailed | CRITICAL / Failed / task_failed | vazio, sem detail original |
| ContextBuilt | STATE / Checkpoint / context_built | memory_count e recent_message_count |
| TaskResultReady / OrchestratorPlanReady / TaskGraphResultReady | STATE / Checkpoint / result_ready | vazio |
| TaskGraph: TaskPlanned | STATE / Planning / task_planned | step_count |
| TaskGraph: SubtaskWaiting | STATE / SubtaskLifecycle / subtask_waiting | vazio |
| Worker: SubtaskStarted | STATE / SubtaskLifecycle / subtask_started | vazio |
| TaskGraph: SubtaskCompleted / SubtaskFailed | STATE / SubtaskLifecycle / subtask_completed, subtask_failed | vazio |

TaskId usa o campo próprio. Os lifecycle subtask usam o identificador no campo
`subtask_id`. Source IDs fixos: `core`, `task_graph`, `worker`. Allocation,
selection, transitions, dependencies, handoff structs, checkpoints, corpo de
resultado, PlanV1, consolidated_text e usage não são copiados. Nenhum Debug de
objeto vira protocolo. Erros arbitrários permanecem somente no contrato funcional.

## Scheduler / exposure policy

`SchedulerTraceContext` existe exclusivamente no callback. Contém TaskId e
subtask opcionais, CognitiveRole, ExposurePolicy e correlation local. Não altera
ProviderTaskRequest, ProviderRequest, contexto cognitivo, prompt, persistência
ou accounting. Cada callback representa uma invocation; o estado termina junto
dela. Não há registry/acumulador ilimitado de traces.

| SchedulerEvent | Source | Classe / code | Projeção |
| --- | --- | --- | --- |
| Queued | Scheduler / scheduler | STATE / provider_queued | provider validado, queue_depth |
| Admitted | Scheduler / scheduler | STATE / provider_admitted | provider validado, queue_delay_ms |
| Selected | Scheduler / scheduler | STATE Routing / provider_selected | provider/model como IDs bounded, attempt, routing reason interno, score opcional |
| Retry | Scheduler / scheduler | STATE Retry / provider_retry | vazio; reason arbitrário não é copiado |
| Fallback | Scheduler / scheduler | STATE Fallback / provider_fallback | from/to como IDs validados |
| OutputObserved | CognitiveProvider / provider_id | STATE Checkpoint / output_observed | vazio |
| Chunk Conversation | CognitiveProvider / provider_id | STREAM ProviderText | texto exato |
| Chunk interno não vazio | CognitiveProvider / provider_id | STATE output_observed coalescido | nenhum texto |

Um model que não cabe/satisfaz TraceId aparece como `unprojected`, sem truncar
conteúdo textual autorizado. Routing reason usa enumeração local fechada;
valores desconhecidos viram `other`. IDs inválidos desativam aquela projeção,
sem modificar SchedulerError ou execução.

| Role | Policy | Conteúdo |
| --- | --- | --- |
| Conversation | ConversationOutput | somente Chunk que já passa ao usuário no fluxo funcional |
| Orchestrator | MetadataOnly | nenhum JSON/plan intermediário |
| Worker | MetadataOnly | preserva redução funcional a SubtaskOutputObserved |
| Summary | MetadataOnly | nenhum transcript, input, raw output, title ou summary |

A policy faz fail-closed também se alguém acidentalmente fornecer
ConversationOutput para um role interno. Há um boolean por invocation/selection
para output_observed; Selected reinicia esse marcador. Milhares de Chunk e
OutputObserved ocultos produzem no máximo um STATE por seleção/attempt. Não é
inferido output quando a fonte não o forneceu.

## Summary minimization

Summary publica Worker/summary com correlation local e sem TaskId. A posição
original de `reserve_background_id()` depois da chamada permanece idêntica.
Scheduler recebe a mesma correlation, mas nenhum dado de sessão.

Lifecycle: summary_started; summary_deferred nas decisões locais de defer;
summary_disabled quando a policy está desabilitada; summary_persistence_failed
se a escrita falhar; summary_unchanged se nenhum registro foi alterado.
summary_completed / summary_failed só aparecem depois de uma escrita factual
com `changed=true`. Transient mantém summary_deferred, sem completion inventada.
O retorno interno da closure de escrita passa a transportar `changed` apenas
para essa decisão; SQL, timing, TaskRecord e ProcessOutcome são preservados.

O comentário `Task telemetry must never contain transcript or raw provider
output` permanece. Tests exercitam sucesso, erro transient, erro terminal e JSON
inválido, comparando no-op, publisher falho, headless e subscriber cheio:
mesmo ProcessOutcome, inputs, número de chamadas e estado/metadados persistidos.
O stress inclui o próprio `SummaryWorker::process_observed` com 3000 chunks fake.

## Contrato agent-neutral / Codex

`agents/trace.rs` define AgentTraceObservation e AgentTraceSink. O sink retorna
`()`, sem ACK ou erro de AgentBackend; `observe_passively` também isola unwind.
As observações possíveis são Lifecycle(&AgentEvent), AgentMessage(&str) e
DisplayReasoningSummary(&str). Não existe variant de private reasoning.

`AgentBackend::execute_observed` é uma extensão object-safe, com implementação
padrão que faz tap dos AgentEvents existentes, preservando `execute`. Mock prova
reutilização para LR-10; nenhuma integração Copilot foi feita. Codex e o probe
sobrescrevem essa extensão para evitar tap duplicado e permitir o side-channel
textual diretamente no worker, sem entrar na fila funcional com ACK.

| AgentEvent | Classe / code |
| --- | --- |
| SessionReady | STATE Started / session_ready |
| WorkStarted | STATE Started / work_started |
| OutputObserved | STATE Checkpoint / output_observed |
| CancellationRequested | STATE Checkpoint / cancellation_requested |
| Completed | CRITICAL Completed / agent_completed |
| Cancelled | CRITICAL Cancelled / agent_cancelled |
| Failed | CRITICAL Failed / agent_failed |
| Output { text } de mocks/genéricos | STREAM AgentMessage; Codex production continua sem emitir esse variant |

SourceType é SpecialistAgent, source_id `codex`. Os terminal events continuam
após a tentativa de cleanup existente. Pre-cancel/invalid request sem evento
natural não ganha lifecycle sintético.

### Conteúdo textual autorizado

Após `inspect_notification` ter aceitado a notification, com thread/turn iguais
à operação atual, `delta` string presente e não vazio:

- `item/agentMessage/delta` → AgentMessage;
- `item/reasoning/summaryTextDelta` → DisplayReasoningSummary, porque esse método
  explicitamente identifica summary destinada a exibição;
- `item/reasoning/textDelta` → **nenhum conteúdo textual**, podendo contribuir
  apenas para o OutputObserved funcional original.

Campos ausentes, tipos errados, texto vazio, thread/turn alheios e métodos
ambíguos não viram texto. `item/completed` não republica PlanV1/texto completo.
Não há derivação de summary, extração de reasoning ou preenchimento artificial.

Forbidden commandExecution, fileChange, mcpToolCall, dynamicToolCall, webSearch,
approval/request e itens desconhecidos continuam fail-closed pela inspeção
original, byte a byte inalterada. Trace recebe somente agent_failed seguro,
nunca params/JSON/payload. As fixtures verificam ausência de `/private/raw`,
private reasoning e markers de objective/unrelated/forbidden.

`production_config`, STATIC_INSTRUCTIONS, thread_config, thread_start_params,
turn_start_params e schemas são idênticos à base. Apenas planning e
structured_output; shell/tool/web/MCP/write/read adicionais permanecem desligados.

## UTF-8 / provenance / correlation

`fragments` é iterator lazy de slices: máximo 8192 bytes, corte em char boundary,
sem normalize/truncation/cópia do texto inteiro para um buffer auxiliar.
Concatenar fragments reproduz exatamente a fonte, incluindo whitespace e Unicode.
Cada fragment preserva source, task/subtask, correlation e coalescing identity;
ordem por producer é mantida. Perda eventual é a retenção/entrega contabilizada
LR-9A, distinta de truncation pelo adapter.

IDs locais monotônicos checked: provider-call-N, summary-call-N e agent-call-N.
Nenhum depende de objective, PID ou remote thread/turn. Exaustão fecha a projeção,
sem wrap nem erro funcional. Provider coalescing key inclui ordinal local da
seleção, separando attempts/fallback mesmo que o attempt remoto recomece em 1.
Tasks/subtasks diferentes e duas operações do mesmo Codex têm identities
separadas. Apenas a sequence do bus ordena OperationalEvents globalmente.

## Failure isolation / Headless

Validação de source/id/task, EventDraft, sequence exhaustion, publisher falho,
subscriber cheio/ausente não mudam resultado, cancellation, SchedulerError,
retry/fallback, provider/agent requests, usage ou persistência. Tests incluem
bus com sequence u64::MAX, IDs inválidos, publisher rejeitando, mocks genéricos,
Codex controlado e Summary real. Um Channel funcional morto mantém seu erro e
publica a falha segura independentemente; trace bem-sucedido não mascara erro.

Headless publica na mesma retenção bounded, sem exigir subscriber ou Terminal.
Reopen vê somente essa janela. Subscriber full perde live deliveries conforme
LR-9A; nenhum producer espera ACK visual ou tenta retry de observabilidade.

## Gates técnicos e evidências

Execução local em 08/10/2026, Fedora/Wayland, Rust/Cargo 1.98.1, Node 24.18.0,
npm 12.0.2. Sem rede comercial. Evidência estruturada:
[`LR-9D-GATE-EVIDENCE.json`](LR-9D-GATE-EVIDENCE.json).

| Gate | Resultado |
| --- | --- |
| npm typecheck / build | verdes |
| cargo check debug / release | verdes |
| cargo test global (`--test-threads=4`) | 1101 passed, 0 failed, 2 ignored; 210,66 s |
| adapters puros / Scheduler / stress | 19 passed |
| filtro lr9d (Codex, Summary, Channel) | 9 passed |
| Activity native DTO fixture | 1 passed |
| overhead dedicado | 1 passed; nenhuma assertion de limite temporal |
| Security script / diff check / rustfmt módulos novos | verdes |
| Node LR-9C, Headless, Economy Shell, Presentation, provider operations/helpers e DOM, allocation settings, adaptive presentation e hydration | nove scripts verdes |
| WebKit Activity LR-9D e regressão DOM LR-9C | verdes |

### Zero-extra-inference / accounting

O gate executa o mesmo Scheduler com requests completos comparados como valores
estruturados (incluindo inputs, instructions, history/context, model/mode e
budgets), usando no-op, publisher rejeitando e bus headless. Cada execução:
3 provider calls, 1 retry, 1 fallback, mesmo output, input_tokens=11,
output_tokens=13 e output_tokens_accounted=13. O gate Worker conservador usa o
mesmo caminho de produção `run_with_retry_conservative_output`: três calls e
output_tokens_accounted=80 em ambos os modos, mantendo os mesmos requests e usage.

Summary real compara quatro outcomes (success, transient, fatal, parse-invalid)
em quatro modos, sempre uma chamada por execução, mesmo ProcessOutcome,
input e metadados/status persistidos. A fixture fornece usage 20/30, sem mudança
no caminho de accounting. Os testes Scheduler também cobrem o role Summary.
Codex fake compara no-op/reject/headless/subscriber cheio, incluindo 3000 deltas:
exatamente um turn/start por operação, mesmos requests/resultado, zero interrupts
no sucesso e mesmo cleanup; cancel mantém um interrupt e cleanup. Nenhum request
app-server foi acrescentado pelo observer. Os mesmos gates negativos provam que
os markers de INPUT não são copiados diretamente para trace.

### Stress multi-source / overhead reproduzível

Dez producers sincronizados: duas tasks Conversation-like, quatro contexts
Worker/subtask, um Scheduler Summary metadata-only, o próprio SummaryWorker real
e duas operações Codex controladas. São 3000 chunks por provider fake e 3000
deltas exibíveis por Codex (1500 AgentMessage + 1500 DisplayReasoningSummary).
Todos terminam; requests, resultados e usage são iguais entre os modos.
Total: 22 provider calls (21 com sete retries/fallbacks + uma Summary real),
dois turn/start. Nas sete invocations gerais, input_tokens=77,
output_tokens=91 e accounted=359, idênticos entre os modos; a Summary real
recebe a mesma usage fake 20/30. Nenhum segredo/private reasoning permanece no bus.

Contagem de 27.086 source events significa entradas autoritativas nos adapters,
excluindo projeções TaskEvent duplicadas e protocol notifications sem observação.
Summary Structured reduz seus 3000 chunks a um OutputObserved antes do adapter.
Tempos abaixo são de uma execução dedicada **debug**, não benchmark universal
nem garantia de latência; bytes são orçamento lógico LR-9A, não RSS.

| Modo | OperationalEvents | Retidos / bytes | STREAM evicted / dropped | Live delivery dropped | Tempo aproximado |
| --- | ---: | ---: | ---: | ---: | ---: |
| publisher no-op | 0 | 0 / 0 | 0 / 0 | 0 | 57 ms |
| ativo headless | 12090 | 922 / 282788 | 11168 / 0 | 0 | 3425 ms |
| ativo, subscriber cheio sem drain | 12090 | 922 / 282826 | 11168 / 0 | 12026 | 3488 ms |

Budgets: <=1024 events e <=2 MiB; STATE/CRITICAL eviction e drop na retenção
foram zero. Seis CRITICAL e os output_observed metadata-only sobreviveram.
Provenance task/subtask e correlations foram verificadas. Full live queue perde
deliveries conforme LR-9A, sem bloquear producer; STATE/CRITICAL continuam na
retenção para replay. Após detach, publicação headless também conclui.

Há overhead debug material ao ligar trace neste burst. O bus existente mantém
scans bounded em retain_event, com contenção entre publishers; o cenário é
evidência para análise na LR-9E. Não foi feito profiling causal nem medição release,
CPU/RSS físico ou otimização especulativa do bus nesta fase. Não se afirma que o
custo seja desprezível ou que todo o tempo medido venha dos scans.

### Activity / limites visuais

[`LR-9D-ACTIVITY-FIXTURE.json`](LR-9D-ACTIVITY-FIXTURE.json) é gerado pelo teste
Rust usando adapters reais + o mesmo DTO LR-9C. O harness WebKit 2.54.0 usa
frontend de produção e apenas IPC sintético local: nove linhas, seis sources
(Core, Scheduler, CognitiveProvider, TaskGraph, Worker, SpecialistAgent/codex),
STATE/STREAM/CRITICAL e os dois canais agentivos exibíveis. Zero provider calls
e zero agent requests. Screenshot inspecionado:
[`LR-9D-ACTIVITY.png`](LR-9D-ACTIVITY.png); assertions e ambiente em
[`LR-9D-ACTIVITY-EVIDENCE.json`](LR-9D-ACTIVITY-EVIDENCE.json).
É prova controlada de rendering do contrato, sem alegar integração end-to-end
com provider comercial/app-server autenticado. Frontend, UI, abas e xterm intactos.

Warnings herdados: 19 no cargo check debug, 42 no release; test build tem três
warnings. Vite continua alertando o chunk AvatarViewport 634,27 kB (>500 kB),
sem aumento/dependência frontend. Dois testes Codex autenticados ignorados;
não executados para respeitar o gate sem rede comercial.

Baterias locais agrupam os 54 casos solicitados, com assertions parametrizadas:

| Casos | Bateria / assertions |
| --- | --- |
| 1–14 | adapters task, result markers, subtask provenance, dedup, fragmentation, invalid IDs, publisher/exhaustion |
| 15–29 | adapters scheduler, routing/admission/retry/fallback, role policy, coalescing, correlation, real fake-provider equivalence |
| 30–35 | Summary real: lifecycle, minimization, routing, transient, persisted state e ProcessOutcome equivalentes |
| 36–42 | agent lifecycle puro e Codex controlado com cleanup/cancel |
| 43–49 | Codex display fixtures, malformed/unrelated/private/forbidden notifications e marker negativo |
| 50–52 | Codex no-op/failure/headless/full subscriber; 3000 deltas, mesmo turn/start/result/cleanup |
| 53–54 | Codex concorrente com IDs locais distintos; trait object MockAgentBackend reutilizável |
| Dedup / stress / overhead | execution Scheduler com projeção TaskEvent; TaskGraph-like lifecycle; dez producers concorrentes, incluindo Summary real e dois Codex fakes |
| Activity | DTO gerado por adapters nativos, renderer WebKit real LR-9C; sem provider comercial |
| Security | `scripts/test-lr9d-security.py`: igualdade dos prompts/schemas/boundary e ausência de expansões |

Reprodução:

```sh
npm run typecheck
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml --release
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml operational_trace::adapters -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml lr9d -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml multi_source_stress_and_reproducible_overhead_gate -- --nocapture
NARYS_LR9D_ACTIVITY_FIXTURE="$PWD/docs/LR-9D-ACTIVITY-FIXTURE.json" cargo test --manifest-path src-tauri/Cargo.toml activity_fixture_uses_real_adapters
python3 scripts/test-lr9d-security.py
node scripts/test-lr9c-terminal.cjs
# Servir dist localmente, sem provider; WebKit usa somente IPC sintético:
python3 -m http.server 4173 --bind 127.0.0.1 --directory dist
python3 scripts/perf1b-webkit-probe.py --test --lr9d --width 640 --screenshot /tmp/lr9d-activity.png
git diff --check
```

## Security / escopo / dívidas

Zero nova inference, prompt, provider call, token deliberado, request app-server,
execution authority agentiva, command Tauri, permission, capability, migration,
Rust/npm dependency ou alteração frontend. Não existe UI nova, aba nova, xterm
novo, persistência de trace, Copilot ou antecipação LR-9E.

- LR-9E: gates finais integrando execução/observation/Presentation, endurance,
  CPU/RSS físico e análise do scan bounded sob carga medida.
- LR-10: implementar Copilot quando autorizado, consumindo o contrato passivo.
- LR-11: Codex SpecialistAgent completo com authority/approvals/sandbox próprios;
  o planner read-only desta fase não os concede.
- Model metadata usa TraceId conservador; IDs de modelo fora dessa sintaxe são
  omitidos do detail (`unprojected`). Conteúdo autorizado nunca é truncado.
- Sem garantia de replay completo de STREAM nem overhead universal. Budgets são
  lógicos, não RSS. Compaction/transporte e perda continuam os contratos LR-9A/C.
- MSRV 1.77.2 exato não atestado e formatação global histórica continuam dívidas
  herdadas; rustfmt restrito aos módulos novos e às inserções.
- Dois gates autenticados Codex ignored herdados permanecem fora do teste sem
  rede comercial. Auditoria independente da Luna ainda é necessária.


## Fechamento independente — 08/10/2026

A auditoria independente da Luna revisou o diff completo contra `main`, os
adapters passivos, taps nos runtimes, deduplicação Task/Scheduler, exposure
policy por CognitiveRole, Summary minimization, contrato agent-neutral, bridge
Codex, raw reasoning policy, gates de equivalência e stress concorrente.

**Veredito:** **PASS TÉCNICO DA AUDITORIA INDEPENDENTE.**

Nenhuma FIX-1 bloqueante foi necessária.

A revisão confirmou:

- adapters acrescentam observabilidade, não trabalho cognitivo novo;
- trace failure/sequence exhaustion/subscriber cheio não mudam provider calls,
  retries, fallbacks, cancellation, resultado, usage ou persistência;
- TaskTraceAdapter não republica fatos provider/Scheduler já autoritativos;
- Conversation pode expor somente ProviderText já funcionalmente exibido;
- Orchestrator, Worker e Summary permanecem metadata-only e fail-closed;
- Summary não copia transcript, prompt, raw provider output, title ou summary;
- Codex mantém planner read-only, sem tools/shell/web/write;
- `agentMessage/delta` e `reasoning/summaryTextDelta` são os únicos deltas
  textuais agentivos permitidos;
- `reasoning/textDelta` nunca possui variant de conteúdo visível;
- forbidden Codex items continuam fail-closed e não vazam payload;
- fragmentação UTF-8 preserva exatamente conteúdo autorizado;
- correlations locais separam invocations concorrentes sem usar objective,
  PID ou IDs remotos;
- zero command/permission/capability/migration/dependency/frontend novo.

### Dívida prioritária obrigatória para LR-9E

O stress debug dedicado mediu aproximadamente:

- publisher no-op: **57 ms**;
- trace ativo headless: **3425 ms**;
- trace ativo com subscriber cheio: **3488 ms**.

A auditoria confirmou que esse overhead não altera inferência, calls, tokens,
resultado ou cleanup, mas é custo síncrono real no caminho produtor sob burst.
A maior pressão observável está associada ao OperationalTraceBus LR-9A
(retention/eviction síncronos, lock global e scans bounded), não a trabalho
cognitivo adicional dos adapters.

Isso **não reabre LR-9D**, mas LR-9E NÃO pode encerrar a LR-9 sem:

1. profiling causal do hot path;
2. comparação em release;
3. CPU/RSS físico no hardware alvo;
4. carga representativa além do stress extremo;
5. otimização se o custo permanecer material.

Qualquer otimização deve ser orientada por evidência. Não introduzir fila
assíncrona, novos locks ou mudança de semântica do bus apenas por especulação.

### Estado de fechamento

~~~text
LR-9D — Cognitive / Agent Trace Adapters

IMPLEMENTAÇÃO                  PASS
AUDITORIA INDEPENDENTE         PASS
TASK TRACE                     PASS
SCHEDULER TRACE                PASS
PROVIDER EXPOSURE POLICY       PASS
TASKGRAPH / WORKER TRACE       PASS
SUMMARY MINIMIZATION           PASS
CODEX PASSIVE TRACE            PASS
RAW REASONING PROTECTION       PASS
DEDUPLICATION                  PASS
FAILURE ISOLATION              PASS
ZERO EXTRA INFERENCE           PASS
PROVIDER CALL DELTA            0
AGENT REQUEST DELTA            0
PROMPT DELTA                   0
EXECUTION AUTHORITY DELTA      0
BLOCKERS                       0
FIX-1                          não necessária
~~~

A próxima etapa é **LR-9E — Concurrency, Security & Final Gate**.
