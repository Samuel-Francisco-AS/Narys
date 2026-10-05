# LR-8E — evidência automatizada local E–L

LR-8 permanece aberta. Este documento registra evidência automatizada de
semântica, sem falsificar execução humana E–L, sem aprovar merge e sem implementar
produto, LR-8.5 ou PERF-1. Fechamento/reconciliação documental cabem à Luna.

## Identificação e ambiente

- Data: 05/10/2026.
- Branch exclusiva: `lr-8e-operational-panel-final-gate`.
- HEAD auditado de partida: `ea8668f857cfb813feecfa5ebd3d9c3f321b38b5`.
- Base original da branch: `b8a89c1943c232a3ac793a897f06d68593970d95`.
- Não houve merge/rebase de main nem alteração do planejamento posterior.
- Ambiente: Fedora Linux 44, Linux x86_64; Rust/Cargo 1.98.1,
  Node 24.18.0, npm 12.0.2. Dependências já instaladas; nenhuma adicionada.
- HEAD de evidência dos testes: `aa9a0ac05c85a7725051f263cb12b1840c3d8132`.
  O commit documental subsequente não modifica código; o HEAD de entrega identifica
  esse registro no histórico da mesma branch.

A–D humanos foram informados como PASS pelo usuário. Em D, o hardware real
mostrou `activeCalls=2/2`, `queueDepth=1/64`, `pendingReservations=3` e drenagem
`2/2+queue1 → 2/2+queue0 → 1/2+queue0 → 0/2+queue0`. São observações fornecidas
pelo usuário, não chamadas executadas pelo agente nesta tarefa.

## Arquitetura e isolamento

O módulo `cognition/lr8e_gate_tests.rs` é registrado somente com `#[cfg(test)]`.
Todas as chamadas passam pelo Scheduler, AdmissionController, RateLimitManager,
ResilienceManager, TelemetryStore e TaskBudget reais. K usa o TaskGraph runtime,
Orchestrator/parser de PlanV1, workers, channel e persistência reais. L destrói o
runtime e constrói outro sobre o mesmo SQLite em diretório temporário, com as
migrations do projeto; não usa SQLite `:memory:`.

`ControlledProvider` é a única implementação registrada pelo harness. Ele
confirma `InvocationObservation::started_unless_cancelled()` antes de enviar a
ack de entrada. Channels/oneshot comandam output, conclusão e reconhecimento de
cancelamento. Nenhum sleep sincroniza cenários. Fake clock governa rate/health;
backoff inesperado causa panic em vez de espera real. Os watchdogs mantêm os
bounds já usados pelos harnesses existentes: 3 s em admission, 10 s em TaskGraph.
Timeouts de produção não mudam.

F cruza também HTTP real **exclusivamente em 127.0.0.1**, com servidor TCP Tokio
local, confirmação de request recebida, resposta vazia e cliente `no_proxy()`.
O endpoint vem somente de `TcpListener::bind("127.0.0.1:0")`; nenhum domínio,
URL remota, header Authorization ou credential é necessário.

E–J/L não consultam SecretStore. K precisa do preflight real do TaskGraph:
reutiliza `TestKeys`, identidade sintética e helpers channel/collect de
`task_graph_runtime_tests`. O SecretStore é novo, em diretório temporário único,
com `UnlockKeyStore` test-only e um batch explícito de credenciais sintéticas.
Os IDs `gemini`, `groq`, `cloudflare` em K são aliases exigidos pelo catalog real,
registrados com **ControlledProvider**, não adapters comerciais. Nenhuma key,
Account ID ou Stronghold real do usuário é lido. Os demais cenários usam IDs a/b.

O error contract não devolve SchedulerUsage quando a chamada termina cancelada.
Para distinguir Selected de committed provider call, há uma única ack adicional
no ponto real `PendingSchedulerAttempt::commit`, também sob `#[cfg(test)]`.
O recorder é isolado pela alocação de cancellation de cada tarefa e removido por
RAII. Não muda decisões, orçamento, erros ou comportamento de produção; não
existe em debug sem testes/release. Não adiciona endpoint ou controle à UI.

Em cada estado pertinente, `stable()` captura `Scheduler::operational_snapshot()`
cinco vezes. Compara todos os fatos/contadores/constraints/estado, exceto os dois
metadados naturalmente temporais `capturedAtUnixMs`/`updatedAgeMs`. As autoridades
não são globalmente atômicas; a estabilidade vem das acks que mantêm as chamadas
controladas bloqueadas, não de megamutex ou mudança da arquitetura do snapshot.

## Matriz por bloco

Comando exato compartilhado: `cargo test --manifest-path src-tauri/Cargo.toml lr8e_gate`.
Cada resultado abaixo corresponde às assertions dos testes nomeados, não a uma
inferência de outra suite. Os 14 testes do filtro passaram, incluindo na global
paralela; os gates completos estão registrados ao final.

| Bloco | Testes (`lr8e_gate_…`) | Cenário/assertions críticas | Resultado |
|---|---|---|---|
| E | `e_cancel_queued_before_boundary_and_commit` | A/B em execute; cap 2; C confirma Queued; snapshot 2/2+queue1, requests=2, reservations=3. C cancelada: Selected=1, commits=0, sem Admitted/HTTP/retry/fallback; queue0, A/B continuam, reservations=2, reserved=0, probes=0. A/B terminam: calls=1 cada, requests=2, todos os guards zerados. | PASS |
| F | `f_cancel_after_loopback_http_keeps_accounting` | Servidor confirma GET loopback; requests=1, activeCalls=1, consumed=1. Provider reconhece cancellation e aguarda Finish. Cancelled final mantém request/consumed/commit=1; B=0; sem retry/fallback, falha health ou guard leak. | PASS |
| G | `g_background_waits_without_foreground_preemption`; `g_admitted_background_survives_new_foreground` | Background confirma queue sob dois foregrounds: background=1, active2, queue1. Liberação de um permit admite Background, sem interromper peer. Na direção inversa, Background admitida permanece viva quando foreground preenche cap e outro foreground aguarda; liberar foreground não preempta Background. | PASS |
| H | `h_local_capacity_is_pre_http_terminal_for_preferred` | Local Provider/RPM capacity1: primeira chamada factual consome1. Preferred(A,B) seguinte retorna exatamente RateCapacityExceeded antes de admission/HTTP/commit; requests permanecem1/0, localBlocks+1; sem retry/fallback nem degradação health. | PASS |
| I | `i_rate_limited_fallback_cooldown_without_health_failure`; `i_unavailable_fallback_does_not_recover_source`; `i_fixed_never_falls_back`; `i_output_forbids_retry_and_fallback` | Preferred A→B: requests e commits=2, fallback1/retry0, cooldown A=5000. Retry hint factual=7777 separado e histórico após cooldown expirar. RateLimited mantém health failures0; Unavailable(Some) mantém failures1 mesmo após sucesso B. Fixed não usa B; chunk confirmado seguido de Timeout não retry/fallback. | PASS |
| J | `j_integrated_circuit_single_probe_recovery_and_reopen` | Três Timeouts factuais abrem circuit threshold3/remaining30000/reason allowlisted. Open bloqueia antes de Selected/rate/admission/provider/factual/commit. Boundary fake não transita por leitura. Duas tentativas disputam HalfOpen: somente1 probe/active/reservation; outra NoProvider sem events/commit. Success fecha/failures0/recovery1; novo ciclo Unavailable+probe Timeout reabre com ProbeTimeout; sem leaks. | PASS |
| K | `k_real_taskgraph_workers_provenance_and_consolidation` | Planner real valida PlanV1 com dois steps independentes. Dois workers coexistem nos providers controlados groq/cloudflare: active1/request1/reservation1 em cada. Plain e StructuredOutput respeitam contratos reais. Provenance por unidade e SQLite preservada; workerUsage calls2/retry0/fallback0/providersUsed coerentes; consolidation ordenada; conclusão única e guards zerados. | PASS |
| L | `l_restart_keeps_window_daily_budget_and_resets_health`; `l_daily_request_budget_is_durable_and_enforced`; `l_durable_uncertainty_never_becomes_known_credit` | SQLite real reaberto restaura policy/RPM/request DailyBudget/consumed. Bloqueios exatos RateCapacityExceeded e DailyBudgetExceeded não invocam provider ou novo HTTP/commit. Cooldown/failure transitórios voltam Closed/0. Unbounded timeout preserva marker unresolved/remaining None após restart; tentativa com prova sintética total5 é recusada por RateStateUnavailable, sem tornar uncertainty crédito. | PASS |

G utiliza Scheduler com `TrafficClass::Background` para testar a mesma
AdmissionController usada por Summary (o request real em `summary.rs` usa essa
classe). Não cria acoplamento ao lifecycle SQLite/session do SummaryWorker nem
altera UIP-6C. As suites globais mantêm a cobertura existente de Summary/deferral.

Em L, request telemetry reinicia em zero: é memória do novo runtime. O consumed
local durável permanece1. Isto preserva a distinção entre requests factuais deste
runtime e accounting persistido. A prova de tokens pertence apenas ao provider
sintético bounded de verificação. Adapters sem TokenUpperBound conservam unknown;
o teste não afirma que todas as novas tentativas unbounded são bloqueadas.

## Correções do próprio harness

A primeira execução dos 13 cenários passou em 11 e encontrou duas premissas
erradas nas fixtures novas, sem defeito de produção:

- K enviava JSON de StructuredOutput para um step `planning`, que solicitava
  texto simples. O runtime corretamente preservou esse texto. A fixture final
  diferencia um step plain e outro StructuredOutput pela instrução real do worker
  e responde conforme o contrato; mantém assertions de texto/provenance fortes.
- L esperava recusa de uma nova tentativa sem TokenUpperBound. A LR-8C mantém
  uncertainty e permite fluxo unbounded sem alegar crédito conhecido. O teste
  exato inicial reproduziu a espera indevida por Finish no harness. A fixture de
  verificação final fornece prova sintética total5 e exige RateStateUnavailable,
  preservando também o assert de marker1/remaining None antes/depois do restart.

Logs iniciais preservados nesta sessão em `/tmp/lr8e-el-focused-first.log` e
`/tmp/lr8e-el-uncertainty-harness-first.log`. Não se alterou accounting/worker,
não se ampliou timeout nem se substituiu uma falha de produção por PASS.

## Gates e autoauditoria

Resultados finais no código do HEAD de evidência acima:

| Comando exato | Resultado |
|---|---|
| `npm run typecheck` | PASS, exit 0 |
| `npm run build` | PASS, exit 0; frontend sem mudanças |
| `cargo check --manifest-path src-tauri/Cargo.toml` | PASS, exit 0 |
| `cargo test --manifest-path src-tauri/Cargo.toml lr8e_gate` | PASS, 14 aprovados, 0 falhas; 2,97 s de testes |
| `cargo test --manifest-path src-tauri/Cargo.toml operational` | PASS, 13 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml admission_tests` | PASS, 18 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml rate_tests` | PASS, 69 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml resilience` | PASS, 77 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml task_graph_runtime_tests` | PASS, 13 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml` | PASS, 542 aprovados, 0 falhas, 2 ignorados; 169,25 s de testes |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | PASS, 542 aprovados, 0 falhas, 2 ignorados; 595,25 s de testes |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | PASS, exit 0 |
| `git diff --check` | PASS, exit 0 |
| `git diff --check main...HEAD` | PASS, exit 0; repetido nos commits finais |

Main/doc-tests também passaram nas duas globais. Nenhuma global falhou nesta
bateria; a serial é evidência adicional. Os 14 testes novos passaram tanto no
filtro próprio quanto nas duas globais, sustentando os oito PASS E–L separados.
Dois ignorados preexistentes: `real_app_server_handshake` e
`manual_final_codex_agent_bridge_gate` (gates manuais Codex), não executados.

Warnings preexistentes: 15 unused/dead-code em debug, dois de fixtures na lib test,
41 em release. Vite mantém aviso de chunk principal >500 kB. Nenhum warning novo,
nova dependência ou crescimento do frontend: IA permanece 60,44 kB/16,72 kB gzip
e App principal 666,22 kB/168,98 kB gzip. Nenhum timeout foi aumentado.

Não há blocker de produção identificado pelos cenários. A fixture corrigida está
inteiramente test-only. Nenhum resultado humano E–L foi preenchido em
`LR-8E-FINAL-GATE.md`; ali foi adicionada somente a nota/link complementar.

Autoauditoria do código: somente um client HTTP, com endpoint loopback gerado
localmente e `no_proxy`; nenhuma instância de GroqProvider/CloudflareProvider/
GeminiProvider/MistralProvider, nenhum getenv/config do usuário, nenhum sleep de
sincronização, nenhum helper compilado em release, nenhuma feature de produto.
Callbacks da fixture não podem completar sem Action explícita. Cada bloco verifica
requests, guards e snapshots reais; E/F verificam também o commit real.
Não há alteração em frontend, permissões, providers, policy cognitiva, threshold,
fairness, accounting, credenciais ou planejamento futuro.

Limitações: em F, o GET loopback prova o início HTTP; o hold/ack de cancellation
mantém `execute_observed` pendente após essa fronteira, sem simular um stream
comercial longo. Evidência local prova semântica e boundary operacional, não SLA remoto,
latência comercial ou comportamento de uma conta real. G não repete UI/session de
Summary; K usa providers sintéticos e SQLite real. L simula encerramento/reabertura
graciosos com marker unresolved durável, não perda elétrica; crash write-ahead
continua coberto pelas suites LR-8C existentes. Nenhum teste exige o usuário
capturar estados transitórios por scroll. Regressões humanas M e decisão de
fechamento permanecem sob responsabilidade da Luna/usuário; LR-8 segue aberta.
