# LR-9E — Concurrency, Security & Final Gate

Estado: **LR-9E IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna**. LR-9A/B/C/D estão
PASS, auditadas e integradas. A LR-9 inteira aguarda auditoria independente/final.
Base autorizada: `d11d5834aacaa97bf642a24e6aeee7e1628c4617`.
Branch: `lr-9e-concurrency-security-final-gate`.

E0–E4 são checkpoints internos desta subfase, não novas fases do roadmap.
Não há approval engine, AgentExecutionAuthority, ExecutionGrant, novo executor
agentivo, sandbox, migration, dependency production ou plugin.

## E0 / E1 — método e causa

O baseline foi capturado antes da alteração de `bus.rs`. O driver
`scripts/lr9e-trace-benchmark.py` executa um binário de testes já compilado,
três vezes por cenário, sem cargo/compilação no intervalo medido. A fixture
LR-9D permanece com dez producers: duas Conversations, quatro Workers,
Scheduler Summary, Summary real e duas operações Codex reais com backend fake.
Cada execução compara requests, outputs, usage, calls e turn starts entre
no-op, bus headless e bus com subscriber de 64 eventos nunca drenado.

Stress: 27.086 fatos fonte / 12.090 OperationalEvents. Carga representativa
compatível: 1.436 fatos fonte / 690 OperationalEvents, incluindo 600 STREAM
e 90 STATE/CRITICAL, com routing/retry/fallback e dois ou mais Workers.
O Summary real mantém sua fixture upstream; a exposição continua metadata-only.
Os 22 provider calls e dois Codex turn starts são constantes em cada modo.
Tokens 77/91 correspondem aos sete Scheduler resultados; o Summary real adiciona
20/30, totalizando 97 input e 121 output nesta fixture. Não são tokens cobrados.

`scripts/lr9e-trace-profile.py` compila uma cópia temporária do bus com timers
`cfg(test)` usando somente std/rustc. Mede lock, timestamp, criação do evento,
retenção inclusiva, recount/snapshot, busca/remoção de vítima, estimated_bytes e
try_send. Timers aninhados não são somáveis; lock wait com dez producers é tempo
agregado de espera, não wall time. O microprobe de adapter mede projeção,
formatação/allocations e dispatch no-op, sem atribuir toda essa duração ao allocator.
Não há instrumentation nem logging por evento em production.

O recount completo da deque era a causa material: com um producer, ~968 ms dos
995 ms de retenção debug e ~70 ms dos 75 ms release no probe causal. A alteração
mantém contagem e bytes por classe dentro de Storage. O incoming ocupa uma cópia
virtual; cada eviction atualiza a deque e os contadores persistentes; insert
contabiliza somente eventos aceitos. Um drop não contabiliza o incoming, mas
preserva evictions anteriores, como no algoritmo original.

Reservas, ceilings, prioridade, FIFO por classe vítima, sequence, replay, perdas,
provenance e ordering não mudaram. O oracle congelado da base LR-9A recomputa
independentemente 24.000 publicações determinísticas STREAM/STATE/CRITICAL/mistas,
incluindo limites de bytes, comparando conteúdo, counters, receipts, holes e
highest_lost_sequence a cada operação e replay paginado periodicamente.
Não houve aumento de budgets, sampling, queue adicional ou thread produtora.

A busca FIFO continua linear. Depois dos counters, seu custo absoluto no probe
foi ~18–19 ms debug e ~0,9–2,8 ms release para 12.090 publicações; esse resíduo
não justifica alterar a arquitetura do storage. Os números causais instrumentados
são distintos dos números da fixture cognitiva não instrumentada.

### Before/after na mesma máquina

Medianas de três execuções, em ms; duração da fixture completa, sem compilação.
No-op não cria OperationalEvents. Custo por evento inclui trabalho cognitivo fake
fixo, threads e processos: não equivale ao custo exclusivo de publish.

| Profile / carga | No-op antes → depois | Bus headless antes → depois | Subscriber cheio antes → depois |
| --- | --- | --- | --- |
| debug / 12.090 eventos | 48,459 → 49,990 | 3.394,576 → 134,480 | 3.434,953 → 182,214 |
| release / 12.090 eventos | 21,505 → 21,075 | 118,389 → 23,393 | 117,764 → 24,341 |
| debug / 690 eventos | 43,863 → 46,933 | 51,832 → 46,045 | 54,619 → 53,379 |
| release / 690 eventos | 23,838 → 25,958 | 29,198 → 20,901 | 23,459 → 19,259 |

Stress headless: ganho absoluto 3.260,096 ms debug / 94,996 ms release, razões
25,24× / 5,06×. Subscriber cheio: 3.252,739 ms / 93,423 ms, 18,85× / 4,84×.
Custo total por OperationalEvent headless cai de 280,78 para 11,12 µs debug e
9,79 para 1,94 µs release; cheio, 284,12 para 15,07 µs / 9,74 para 2,01 µs.
Não foi inventado um percentual universal de PASS.

Na carga menor a dispersão da fixture é comparável ao overhead; algumas medianas
ativas ficam abaixo de no-op por variância de scheduling/processos. Isso não
significa custo negativo. Retenção mantém 690 eventos (~212 kB), zero eviction/drop;
subscriber cheio perde 626 entregas live, recuperáveis pelo replay bounded.
Stress mantém 922 eventos (~283 kB), 11.168 evictions STREAM, zero eviction
STATE/CRITICAL e zero drops de retenção; subscriber cheio perde 12.026 live.
As pequenas diferenças de bytes decorrem de IDs e interleaving, não de budget.

## E2 — IPC, capabilities e boundary

O inventário derivado e machine-readable separa handler, AppManifest, capability
e consumer frontend em `LR-9E-FINAL-GATE-EVIDENCE.json`. O gate automático é
`python3 scripts/test-lr9e-release-security.py --bundle`.

Na base, `start_mock_task` estava registrado e permitido em release. As outras
oito permissões DEV residuais também estavam na capability comum, embora seus
handlers/manifest release já estivessem ausentes. Agora os nove commands DEV
estão fora das três superfícies release: `start_mock_task`,
`start_mock_cognition_task`, `cognition_provider_status`, `security_test_store_secret`,
`security_test_delete_secret`, `lr4_status`, `lr4_import_private_bootstrap`,
`lr4_create_diagnostic_conversation` e `lr4_get_recent_conversation`.
O relatório `perf1c_ui_report` também saiu do manifest comum: só é conhecido
no build opt-in `perf1c-probe`, cujo handler/capability já eram feature-gated.

`debug-diagnostics.json` está fora do diretório de capabilities estáticas. O
setup `cfg(debug_assertions)` instala a ACL dinâmica oficial Tauri somente em
`main`, com exatamente as nove permissions necessárias. A versão resolvida já
possui dynamic-acl; nenhum plugin/dependency foi acrescentado. Probes Codex e
Groq usados por settings são funcionalidades de produto e foram preservados;
o AppManifest passa a declarar três handlers de produto já existentes, antes
omitidos: `probe_codex_planner`, `probe_codex_planner_preflight`, `start_task_graph`.

Settings general/AI não ganham terminal, diagnostics, shell, fs, process ou
authority. CSP/origins permanecem iguais, sem remote scripts, unsafe-eval,
unsafe-inline production, frames ou remote workers. O bundle production não
contém nomes DEV proibidos. Prompt builders, requests/schemas, dependency graph
e migrations também são comparados à base pelo script.

| Origem com HumanLocal nativo | Resultado |
| --- | --- |
| Human | permitido |
| SpecialistAgent | AuthorityDenied |
| Worker | AuthorityDenied |
| CognitiveProvider | AuthorityDenied |

Doctests compile-fail comprovam que JSON não desserializa ExecutionAuthority e
que seu constructor humano não é público. TaskId, PID, declared origin e AgentTrace
correlation não cunham capability. O gate revalida canonical cwd, symlink escape,
root substituída, Controlled environment e ausência de herança de secrets.
WorkspaceScope é validação de diretório, **não sandbox**.

O trust boundary LR-9C permanece: main controla somente a PTY humana aberta;
input/resize exigem attachment vigente; nenhum DTO fornece program/argv/cwd/env,
PID para ação genérica ou authority. Não existe execute_command equivalente.
ExecutionResult conserva ExecutionRequest internamente, sem serde/IPC; trace e
Terminal DTO fazem projeção explícita de metadata.

`subscribers_disconnected` não tem consumidor de health no código atual. Seu
comentário agora explicita total de registrations removidas, incluindo Drop/detach
normal; não foi criado refactor/API apenas para renomear essa métrica.

### Release IPC derivado (subset)

Capability abaixo é a estática de `main`; consumers/settings e definições
conhecidas de permission estão discriminados no inventário JSON completo.
Definição autogerada conhecida não concede acesso.

| Command | Handler | AppManifest declarado | Capability main | Finalidade |
| --- | --- | --- | --- | --- |
| cancel_task | yes | yes | yes | product |
| cognition_provider_status | no | no | no | DEV/fixture |
| get_provider_operational_snapshot | yes | yes | yes | product |
| groq_probe | yes | yes | no | product |
| lr4_create_diagnostic_conversation | no | no | no | DEV/fixture |
| lr4_get_recent_conversation | no | no | no | DEV/fixture |
| lr4_import_private_bootstrap | no | no | no | DEV/fixture |
| lr4_status | no | no | no | DEV/fixture |
| open_human_terminal | yes | yes | yes | product |
| probe_codex_planner | yes | yes | no | product |
| resize_terminal | yes | yes | yes | product |
| security_test_delete_secret | no | no | no | DEV/fixture |
| security_test_store_secret | no | no | no | DEV/fixture |
| send_terminal_input | yes | yes | yes | product |
| set_terminal_activity | yes | yes | yes | product |
| start_conversation_task | yes | yes | yes | product |
| start_mock_cognition_task | no | no | no | DEV/fixture |
| start_mock_task | no | no | no | DEV/fixture |

### Activity recolhida

A calibração física debug encontrou diferença material no burst: Activity recolhida
processando trace consumiu ~37,2% de um core, versus ~10,8% com Terminal ausente;
a árvore chegou transitoriamente a ~1.264 MiB RSS. A carga representativa foi
~15,4% versus ~12,7%, diferença menor. São amostras locais curtas, não universais.

Foi implementada suspensão da assinatura visual: collapse remove o subscriber,
limpa o store visual e estaciona o consumer existente em Condvar; expand cria
nova assinatura, replay bounded e live. A thread estacionada continua contada
até detach/Headless/Quit, que a acorda e encerra. Não há polling, novos workers,
queues ou mudança nos publishers/Core history. A PTY e attachment permanecem.

O command de produto `set_terminal_activity` apenas controla essa assinatura e
exige main/attachment vigente; settings não recebem sua permission. Epochs de
entrega impedem ACK antigo de confirmar replay novo ou interromper a PTY. Testes
cobrem overflow enquanto recolhida, gap factual ao expandir, dez ciclos, epoch
exhaustion fail-closed e Quit acordando o consumer estacionado. WebKit cobre o
store visual/replay, além do lifecycle real nativo.

## E3 — composição das provas

A prova é composta por matriz nativa Rust, regressões funcionais A–D e lifecycle
Tauri/WebKit. Evita um teste monolítico que precise abrir rede comercial.

`lr9e_integrated_concurrency_fault_and_hygiene_matrix` sobrepõe PTY humana real
com flood >6MiB, input/resize após overflow, Exec normal/cancelado/timeout,
quatro Scheduler tasks (duas Workers), retry/fallback, Summary real metadata-only,
duas operações Codex fake reais e TaskGraph com dois workers efetivamente paralelos.
Exec e adapters usam o mesmo bus isolado; o TaskGraph real usa seu adapter
process-wide existente. A probe do app usa o bus process-wide real para todas
as fontes. Callbacks de bridge reentram locks deliberadamente; se callback fosse
executado com producer/attachment lock retido, a prova travaria.

Subscriber cheio não é drenado; outro subscriber/bridges somem com batch pendente.
Uma Conversation usa RejectPublisher em paralelo e compara requests/usage/output
à Conversation observada. Cancel de root Graph não interfere nas tasks independentes.
Exec cleanup preserva PTY; close PTY e shutdown broker são provados também nas
regressões LR-9B. Dez reconnects replay/live terminam com zero subscribers/workers.

As regressões A–D cobrem Channel funcional fechado, passive Codex observer rejection,
security failure antes de cancel, cleanup failure, timeout/cancel nunca success,
dedup, task/provenance/correlation isolation, raw reasoning e forbidden payload.
Falha observacional não cria/mascara falha funcional; falha funcional continua
visível e interrompe inference segundo o contrato existente.

Markers distintos de input, context, Summary transcript/output, environment,
raw reasoning, forbidden Codex payload e PTY humana não aparecem na projeção
OperationalTrace. ProviderText Conversation naturalmente gerado permanece
permitido. PTY bytes podem aparecer no xterm humano; imprimir não os publica em
trace nem os envia à Conversation/provider/outro task.

## E4 — infraestrutura

`lr9e-probe` reutiliza PERF-1C, LR-9B e LR-9C; suas ações são fixas e nativas,
ativadas explicitamente por feature/ambiente. Não há novo command WebView de
execution. `scripts/lr9e-native-probe.py` usa DBus/vault/HOME temporários, fake HTTP
SSE local para Conversation/Summary reais e fake providers em memória para
Scheduler/Workers. As observações Agent no app são passivas; protocolo Codex
concorrente real com backend fake é provado pela matriz Rust.

O probe verifica startup Conversation/Terminal lazy, shell humano, seis fontes
Activity, burst, PTY responsiva, Close com batches in-flight, zero WebViews/bridges,
Core/PTTY preservados, single-instance reopen, mesma sessão/PID, replay/gap,
input/resize, cancel, exit/reap, dez detach/reattach e Quit com Summary, task,
PTY, trace e Structured Exec simultâneos. Shutdown registra counters factuais.
O deadline cooperativo existente de cinco segundos e fallback existente não
foram aumentados. Caso excedido, agora há diagnóstico agregado sem conteúdo.

Medições físicas são amostras curtas locais, CPU percentual de um core e RSS
somado da árvore (pode duplicar shared pages), com PSS quando /proc permite.
Compilação/dev server não entram no custo da app. Native foi executado em debug/probe e release/probe, ambos com frontend
production embarcado. O lifecycle debug final usa amostras diagnósticas de um
segundo; a calibração debug before/after usa dez segundos. O lifecycle release
final usa dez segundos após warmup de dez segundos nos cenários idle. O build
release normal e seu handler são verificados separadamente do opt-in probe.
Nenhum resultado debug é apresentado como native release.

## Dívidas e limites preservados

| Item | Classificação |
| --- | --- |
| Windows/macOS | dívida futura de validação |
| PTY persistence após restart | fora de escopo |
| Rust 1.77.2 global | dívida futura, não atestado; toolchain exata ausente, não instalada |
| daemonização hostil / execução escapando de processo gerenciado | risco aceito fora de sandbox |
| Isolation Pattern Tauri | hardening futuro |
| SpecialistAgents Copilot/Codex executores; approvals/grants | LR-10/11, fora de escopo |
| sandbox real | fora de escopo |
| endurance de muitas horas / allocator e WebKit caches | dívida futura; gate curto não prova endurance |
| source-map-js transitivo DEV / chunks frontend grandes | dívida frontend herdada, sem atualização ampla de graph |
| fmt global / dead_code histórico | dívida de manutenção; somente Rust alterado foi formatado |



## Resultados nativos e físicos

A probe Tauri/WebKit completa passou em debug e release. Na release, os nove
invokes DEV foram efetivamente negados com `not allowed by ACL`, sem confundir
validação de argumentos com negação. Startup/reopen exibem Conversation e não
carregam Terminal antes da seleção; abrir Terminal não abre shell automaticamente.
Activity recebeu seis fontes reais dos adapters (Core, Scheduler, Provider,
Worker, Agent, TaskGraph); Agent no app é observação passiva, não execução.

Close com trace/PTY/chunks fake in-flight destruiu os WebKitWebProcess antigos;
zero janelas, zero WebViews, zero bridges e subscribers. Chunks continuaram e a
PTY manteve a sessão/PID. Reopen via single-instance manteve lazy Terminal;
selecionar Terminal recuperou a mesma PTY, replay parcial sinalizado, input e
resize reais. A sessão humana inicial release foi `1`/PID `41905`; após exit/reap,
um novo gesto humano abriu sessão `5`/PID `42131`, independente do reattach.
O flood de Structured Exec capturou 6MiB em cada stdout/stderr, truncado/bounded,
Completed/reaped. O sleeper independente permaneceu Running após exit da PTY.
Cancel da fixture durante trace deixou a Conversation real em andamento.

Quit sobrepôs fixture cognitiva, Conversation/Summary HTTP fake in-flight, PTY,
trace burst e Structured Exec. Debug: **0,415 s**; release: **0,366 s**; exit code 0.
Nenhum deadline cooperativo/Execution foi excedido. Ambos terminaram com
TaskRegistry active/workers 0, Summary stopped, ExecutionBroker active/workers 0,
Surface workers 0, subscribers 0 e nenhum PID controlado vivo/zombie.
Os deadlines existentes não foram aumentados.

Dez detach/reattach reais por profile não reiniciaram a PTY. RSS release oscilou
1.207/1.294/1.070/1.287 MiB nos primeiros ciclos e estabilizou em ~772–784 MiB
(PSS ~579–590 MiB); debug seguiu padrão semelhante. Não houve crescimento
monotônico inexplicável de Core/collections; a prova curta não atesta endurance.
A matriz Rust final passou três vezes por profile, sempre com zero subscribers,
workers/processos restantes e estados Completed/Cancelled/TimedOut corretos.

### Release/probe · custo físico local

CPU é percentual de um core, portanto pode exceder 100%; RSS é soma da árvore e
pode duplicar páginas compartilhadas. PSS está no JSON. Janelas observadas são
~11,7–12,1 s, pois os snapshots de probe também custam tempo; não são picos
científicos. Não houve compilador nem dev server durante as amostras.

| Cenário | CPU % / core | RSS observado MiB |
| --- | --- | --- |
| Conversation idle / Terminal ausente | 9,1 | 562–563 |
| Terminal + Activity idle / sem shell eager | 4,3 | 572 |
| Terminal humano idle | 9,9 | 583–585 |
| Cognition representativa / Activity expandida | 122,9 | 592–718 |
| Trace burst / Activity expandida | 61,0 | 733–1.256 |
| Cognition representativa / Activity recolhida | 9,6 | 732–735 |
| Trace burst / Activity recolhida | 8,7 | 731–732 |
| Cognition representativa / Terminal ausente | 9,1 | 732 |
| Trace burst / Terminal ausente | 8,4 | 731–732 |
| PTY burst >6MiB | 63,0 | 734–771 |
| Cognition + trace + PTY + Exec | 78,1 | 832–841 |
| Headless / PTY, Conversation e Exec ativos | 3,2 | 274 (PSS 197) |

Expandida permanece visualmente cara sob burst; não foi vendida como idle cheap.
A suspensão recolhida remove esse processamento sem tirar fatos do Core. Em
debug, burst recolhido caiu de ~37,2% para ~11,0–11,2%, comparável a view ausente
(~10,7–11,2%); release recolhido/ausente ficou ~8,7%/~8,4%. Os providers fake
representativos fizeram streaming finito; calibrar toda a app inclui renderer,
crypto/status polling herdados e scheduling, não apenas publish.

Trace native release acumulou 39.475 publicações no cenário multietapas; reteve
1.024 eventos / 326.357 bytes ao shutdown. Houve 37.948 evictions STREAM,
14 STATE, zero CRITICAL; drops de incoming STREAM 489, STATE/CRITICAL zero;
23.954 perdas de entrega live. A carga multietapas encheu a janela também de
metadata prioritária: STATE cede a CRITICAL quando STREAM já foi removido, conforme
oracle/budgets existentes. Isso é distinto da fixture extrema LR-9D, cuja
STATE/CRITICAL não sofreram evictions/drops. Replay sinalizou perdas factuais.

As sete chamadas HTTP fake do fluxo (quatro Conversations / três Summary) são
solicitadas deliberadamente pelo harness, sem provider comercial; as duas finais
foram interrompidas pelo Quit. Native Scheduler fake totalizou 61 attempts
(inclui retry/fallback e operações canceladas), não chamadas comerciais. A
comparação no-op/trace da fixture LR-9D manteve 22 calls, 97/121 tokens de fixture
e dois Codex turn starts por modo, mesmos requests/outputs/usage. A matriz
concorrente manteve 16 calls e dois Codex starts, incluindo Graph/Summary reais.

Markers privados ficaram ausentes do trace, de Conversation e dos requests
HTTP para PTY/environment/Worker. PTY marker apareceu apenas no terminal humano.
Raw reasoning/forbidden Codex e transcript Summary foram testados pelos gates
Rust integrados/adapters/protocolo fake, sem inference extra.

### Testes e reprodução

- `npm run typecheck`, `npm run build`: PASS; nove scripts frontend: PASS.
- WebKit production: LR-9C em 1120/640px, incluindo suspensão/replay, e LR-9D
  adapter DTOs em 640px: PASS.
- `cargo check` debug/release: PASS.
- `cargo test` global debug: 1.107 PASS, 0 falhas, 2 ignorados, 201,69 s;
  release: 1.107 PASS, 0 falhas, 2 ignorados, 200,90 s; ambos mais dois doctests.
- Os dois ignorados exigem app-server Codex real/autenticado; fakes controlados
  de protocolo/correlação/observer rejection passaram.
- Compilação da suíte release: 8m23s, separada do elapsed dos testes e benchmark.
- Build release normal, build opt-in debug/release, release security gate,
  scoped rustfmt e `git diff --check`: PASS.
- Rust 1.77.2 não está instalado: check exato não executado, MSRV não atestado.

Reproduzir o gate nativo, depois de `npm run build`:

```bash
cargo build --manifest-path src-tauri/Cargo.toml --features lr9e-probe
python3 scripts/lr9e-native-probe.py --profile debug
cargo build --manifest-path src-tauri/Cargo.toml --release --features lr9e-probe
python3 scripts/lr9e-native-probe.py --profile release --binary src-tauri/target/release/assistente-3d
python3 scripts/test-lr9e-release-security.py --bundle
```

O synthetic driver aceita `--binary` do teste já compilado, `--profile` e
`--output`; executa três repetições dos dois workloads, sem cargo no intervalo.
O profiler causal temporário aceita `--output` e `--release`. Não exige profiler
externo, benchmark crate ou property/fuzz framework.

Houve correções de harness verificáveis: contagem Graph usa uma chamada planner
mais duas WorkerEntry reais; o primeiro stream longo foi finalizado antes de
calibrar novos cenários para respeitar seu timeout; envio reutilizável PERF-1C
agora usa bloco lexical, evitando redeclaração global de `const t`, e aguarda a
UI concluir Nova conversa. As execuções anteriores não são apresentadas como
lifecycle PASS; suas amostras físicas válidas permanecem identificadas no JSON.

Warnings: unused/dead-code Rust (19 debug / 48 release normal / 46 release-probe),
chunk AvatarViewport 634,27 kB >500 kB, advisory transitivo DEV `source-map-js`
herdado (audit completo: 1 high; audit production: zero). DBus/portal/AT-SPI do
ambiente isolado emitiram warnings de ativação; key store/fixtures são isolados e
as assertions de lifecycle passaram. Nenhum dependency graph foi atualizado.

Evidência machine-readable consolidada:
[LR-9E-FINAL-GATE-EVIDENCE.json](LR-9E-FINAL-GATE-EVIDENCE.json).

```text
LR-9A PASS
LR-9B PASS
LR-9C PASS
LR-9D PASS
LR-9E IMPLEMENTAÇÃO CANDIDATA

LR-9 aguarda auditoria independente/final da Luna.
```
