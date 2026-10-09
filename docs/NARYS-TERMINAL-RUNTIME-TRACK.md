# LR-9 — Operational Terminal & Cognitive Trace Runtime

**Estado:** EM ANDAMENTO — LR-9A/B/C em PASS, encerradas, auditadas e integradas; LR-9D IMPLEMENTAÇÃO CANDIDATA, aguardando auditoria independente da Luna.
**Origem:** promovida em 08/10/2026 a partir da trilha futura NARYS-TERM,
registrada originalmente durante a PERF-1B em 07/10/2026.  
**Posição:** pré-SpecialistAgents; deve preparar a infraestrutura comum consumida
posteriormente por LR-10/Copilot e LR-11/Codex.

## 1. Visão

A LR-9 transforma a área central da Economy Shell em uma **superfície operacional**
da Narys.

Ela combina duas capacidades diferentes, que podem aparecer juntas na interface
sem serem confundidas internamente:

1. um **terminal Linux real**, baseado em PTY, utilizável normalmente pelo humano;
2. uma **superfície de observabilidade passiva** para acompanhar trabalho,
   progresso, comandos, eventos e saídas naturalmente expostas por Cognitive
   Providers, workers, TaskGraph e SpecialistAgents.

A Conversation continua responsável pela comunicação de alto nível: pedidos,
respostas, perguntas, approvals, decisões, alertas que exigem intervenção e
relatórios finais.

O Terminal concentra atividade operacional: shell humano, stdout/stderr,
comandos/processos, routing/fallback/retry útil, subtarefas/workers, ações de
ferramentas e traces concorrentes de múltiplas inteligências.

O Terminal não é a fonte da verdade da tarefa e não substitui o Luna Core.

## 2. Invariantes

### 2.1. Passive Observability

> observar nunca cria trabalho cognitivo adicional para a inteligência observada.

Adapters podem transportar somente aquilo que a fonte já produz naturalmente em
seu endpoint, SDK, CLI, protocolo ou runtime.

É proibido:

- acrescentar prompt pedindo que um modelo narre o trabalho apenas para o Terminal;
- gastar nova inferência para fabricar progresso;
- exigir mensagens extras de SpecialistAgents para preencher a UI;
- inferir ou fabricar "pensamentos" que o backend não expôs;
- tentar extrair private chain-of-thought oculto.

Se um backend expõe apenas started/completed, é isso que existe. Se expõe
reasoning summaries, agent messages, tool events ou progress deltas próprios,
esses itens podem ser apresentados conforme policy.

### 2.2. Execution Independence

> um consumidor lento de observabilidade nunca pode retardar o executor.

UI fechada, Headless, frame lento, scroll pesado ou subscriber ausente não podem
aplicar backpressure bloqueante a provider, worker, agent ou processo.

### 2.3. Bounded by Design

Toda fila, buffer, scrollback e janela de replay possui limite explícito de
eventos e/ou bytes. Não existe coleção de trace que cresça indefinidamente.

### 2.4. Source Fidelity

A Narys pode transportar, agrupar, coalescer e renderizar informação recebida.
Não pode transformar ausência de evidência em descrição factual.

Eventos críticos não podem ser descartados silenciosamente por otimização visual.

### 2.5. Terminal is not Authority

A superfície visual não define admission/policy ou execution authority agentiva.
Autoridade de processo permanece no Execution Broker. Na LR-9C, a boundary
humana específica permite à main controlar o shell humano durante uma attachment:
é uma ampliação intencional da trust boundary, registrada em SECURITY, sem
conceder authority a agentes/providers ou transformar Presentation em Core.

## 3. Três planos

~~~text
                 NARYS CORE
                     │
       ┌─────────────┴─────────────┐
       │                           │
 Cognitive Plane             Execution Plane
       │                           │
 providers/workers           Execution Broker
 planner/agents          ┌─────────┴─────────┐
                         │                   │
                 Structured Exec           PTY
                         │                   │
                         └─────────┬─────────┘
                                   │
                                  OS
                                   │
                            Observation Plane
                                   │
                          Operational Trace Bus
                                   │
                            Terminal Surface
~~~

**Cognitive Plane:** decide, planeja e escolhe recursos.

**Execution Plane:** executa efeitos reais através de uma fronteira controlada.

**Observation Plane:** expõe eventos e saídas já produzidos sem se tornar
dependência para a execução.

## 4. Execution Broker

A LR-9 cria a fundação comum que SpecialistAgents posteriores devem consumir para
chegar ao Linux.

~~~text
ExecutionRequest
- execution_id
- task_id
- owner/origin
- workspace_scope
- cwd
- capability
- mode
- payload

ExecutionResult
- execution_id
- exit/status
- stdout/stderr refs ou bounded output
- timestamps
- provenance
~~~

### 4.1. Structured Exec

Preferido para comandos não interativos e automação controlável, como
`git status`, `cargo test`, `npm run build` e `rg`.

Deve preservar owner, cwd, PID/lifecycle, exit code, stdout/stderr, cancelamento
e timeout.

### 4.2. PTY Execution

Usada quando a semântica exige terminal interativo: shell, REPL, vim/nano, TUI
ou processo que aguarda input.

A PTY não é atalho para contornar policy.

### 4.3. Fronteira pré-especialistas

LR-9 constrói Broker e primitivas, mas **não entrega shell irrestrito ao Codex ou
Copilot antecipadamente**.

LR-10 e LR-11 continuam responsáveis por habilitar capabilities reais,
approvals, sandbox/workspace e gates de seus SpecialistAgents.

## 5. Terminal Runtime humano

~~~text
TerminalSession
- session_id
- owner/origin
- cwd
- shell/process
- cols / rows
- state
- created_at
- process/pty handle
~~~

Capacidades mínimas:

- criar sessão;
- abrir shell do usuário;
- input/output incremental;
- resize;
- consultar estado;
- encerrar sessão;
- exit code/signal;
- destruir/recriar UI sem processo órfão.

Persistir processo após restart completo da aplicação não é requisito inicial.

A sessão humana e sessões agentivas são distintas. Agentes não digitam
silenciosamente na PTY humana. Compartilhamento futuro exige handoff explícito de
ownership.

## 6. Operational Trace

Contrato provider/agent-neutral:

~~~text
OperationalEvent
- event_id / sequence
- task_id
- subtask_id?
- source_type
- source_id
- source_instance?
- kind
- priority
- timestamp
- correlation_id?
- bounded payload
~~~

Fontes possíveis: core, scheduler, task_graph, cognitive_provider, worker,
specialist_agent, execution_broker, terminal_process e human.

### Classes mínimas

**CRITICAL:** approval required, security/policy block, failed, cancelled,
completed quando necessário à integridade.

**STATE:** started, planning, worker/subtask lifecycle, tool/command lifecycle,
retry/fallback, checkpoint e state transition.

**STREAM:** stdout/stderr chunks, provider deltas, agent-message deltas,
reasoning/summary deltas expostos pelo backend e progress fragments.

CRITICAL e estado necessário à reconstrução têm precedência sobre STREAM.

## 7. Ingestão e performance

~~~text
sources
  ↓
trace adapters
  ↓
Operational Trace Bus
  ↓
bounded ingest buffers
  ↓
batch / coalesce / priority
  ↓
UI stream batches
  ↓
virtualized trace surface
~~~

- muitos eventos podem atravessar IPC em batch;
- deltas do mesmo item podem atualizar uma entrada viva;
- agrupar transporte não autoriza resumir conteúdo com LLM;
- usar limites por fonte/tarefa e limite global em itens/bytes;
- overflow deve ser determinístico e observável;
- STREAM antigo pode ser evictado/coalescido antes de CRITICAL;
- traces estruturados devem renderizar somente janela necessária;
- PTY mantém stream/emulador próprio, sem OperationalEvent por byte.

Sob carga crescente: batches maiores → coalescing maior → cadence visual menor →
eviction bounded de STREAM antigo. A execução não desacelera para acompanhar UI.

## 8. Headless e reentrada

Em Headless, tarefas, workers, agents e Execution Broker continuam. Ausência de
subscriber visual não é erro.

Ao reabrir uma Presentation, entregar snapshot atual + janela recente bounded,
nunca replay ilimitado de deltas acumulados.

## 9. Segurança e autorização

Shell arbitrário é capability de alto impacto.

Distinguir ao menos observação, execução comum em workspace autorizado, ação
destrutiva, ação privilegiada, rede sujeita a policy, processo persistente e
delegação agentiva.

`sudo`, remoção destrutiva, instalação de pacotes, push/publicação,
credenciais, saída do workspace e daemons podem exigir approval.

Não usar allowlist textual ingênua como única barreira.

## 10. Relação com Codex, Copilot e providers

O bridge Codex atual já observa notificações de agent message e
reasoning/summary delta em seu protocolo, mas deliberadamente reduz isso a
eventos genéricos e não expõe protocolo bruto ao frontend.

LR-9 pode criar adapter sanitizado para eventos **naturalmente expostos**, sem
ampliar trabalho do Codex e sem habilitar execução real antecipadamente.

LR-10 preencherá o mesmo contrato com os eventos/capabilities realmente
disponíveis na integração Copilot.

Cognitive Providers entram somente com streaming, usage, routing,
retry/fallback, tool/progress ou outros eventos que a integração já exponha.
Não padronizar "pensamento" inexistente entre providers.

## 11. Conversation x Terminal

~~~text
Conversation
→ pedidos
→ respostas
→ perguntas
→ approvals
→ decisões
→ relatórios

Terminal
→ shell
→ comandos
→ traces
→ subtarefas
→ progresso
→ stdout/stderr
→ atividade agentiva
~~~

Evento operacional pode gerar alerta na Conversation se exigir decisão humana;
isso não transforma Conversation em console de log.

## 12. Decomposição formal

### LR-9A — Operational Trace Contracts & Passive Event Bus

**Estado:** **PASS TÉCNICO + AUDITORIA INDEPENDENTE — encerrada em 08/10/2026.**

Contratos nativos tipados, bus único em managed state sem dependência de UI,
retenção priority-aware com reservas, live best-effort bounded, replay com
detecção de gaps, batches, métricas e coalescer separado foram implementados.
32 testes específicos e suíte Rust integral registrada com 1023 aprovados,
2 ignorados preexistentes e zero falhas concluíram os gates técnicos. O stress
de cinco fontes sintéticas preservou todos os STATE/CRITICAL sem consumo do
subscriber.

A auditoria independente aprovou a implementação sem FIX bloqueante. Permanecem
somente dívidas não bloqueantes de medição do scan bounded sob carga real,
semântica de detach/disconnect para telemetria futura e medição física de RSS.

Contratos, budgets, evidências e fechamento:
[LR-9A — Operational Trace Bus](LR-9A-OPERATIONAL-TRACE-BUS.md).

Formalizar OperationalEvent, provenance/correlation, CRITICAL/STATE/STREAM,
bus assíncrono, buffers bounded, batching/coalescing e overflow determinístico.

Gate: rajadas sintéticas multi-source, producer não bloqueia consumidor
lento/ausente, limites respeitados e CRITICAL preservado sob pressão de STREAM.

### LR-9B — Execution Broker & Real PTY Runtime

**Estado:** **PASS TÉCNICO + AUDITORIA INDEPENDENTE — encerrada em 08/10/2026.**

Execution Broker process-wide, authority separada de provenance, Structured Exec
com drenagem concorrente/bounded, PTY humana real, input/replay/resize/lifecycle,
timeout, cancellation e shutdown coordenado foram implementados. Agents continuam
sem execution authority; não há generic shell IPC ou Terminal React.

35 testes específicos, stress Exec/PTY sem rede, suíte integral registrada com
1058 aprovados (zero falhas, dois ignored herdados), check debug/release,
typecheck/build e probe Tauri Close/Reopen/Quit passaram. A auditoria independente
aprovou a candidata sem FIX bloqueante.

Dívidas não bloqueantes: reattach/registry PTY para LR-9C, projeção IPC sanitizada
de resultados, revisão do timeout de 1 hora para sessão humana longa e MSRV global
1.77.2 ainda não atestado.

[Fechamento, recuperação, contratos, budgets, gates e limitações](LR-9B-EXECUTION-BROKER-PTY.md).
[Evidência nativa](LR-9B-NATIVE-EVIDENCE.json).

Criar ExecutionRequest/Result, Structured Exec mínimo, PTY real Rust, sessão
humana Linux, lifecycle, resize, owner/origin e cleanup.

Gate: shell real; cd/ls/git/cargo; programa interativo simples; structured exec
com provenance; close/reopen sem processo órfão.

### LR-9C — Terminal Surface & Stream Management

**Estado:** **PASS TÉCNICO + AUDITORIA INDEPENDENTE — encerrada em 08/10/2026.**

Home foi reutilizada como Terminal lazy, com registry humano process-wide,
reattach, Channels raw/batched, input/resize bounded, Activity virtualizada e
lifecycle Headless independente da UI. A auditoria independente confirmou
Presentation sem autoridade de execução, backpressure fora do Core, trust boundary
explícita e ausência de adapters LR-9D/execução agentiva.

Dívidas não bloqueantes: Activity recolhida ainda processa trace enquanto Terminal
permanece montado; o budget de 2 MiB do TraceStore é lógico e precisa de medição
física continuada; UX cross-platform permanece futura.

[Arquitetura, budgets, segurança, testes, evidências e fechamento](LR-9C-TERMINAL-SURFACE-STREAMS.md).

Substituir a Home vazia por superfície operacional com PTY, traces multi-source,
filtros, batches, coalescing, virtualização/scrollback bounded e cadence
adaptativa.

Gate: PTY responsiva + stress concorrente sem render por delta como requisito;
fechar view não afeta execução.

### LR-9D — Cognitive / Agent Trace Adapters

**Estado:** IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.
Gates locais e evidências em [LR-9D-COGNITIVE-AGENT-TRACE-ADAPTERS.md](LR-9D-COGNITIVE-AGENT-TRACE-ADAPTERS.md).

Adaptar TaskEventKind, Scheduler/TaskGraph/workers, providers quando houver
evento útil e bridge Codex atual como primeira prova agentiva passiva. Deixar
contrato pronto para Copilot.

Gate: provenance correta, zero trabalho extra para gerar trace, conteúdo não
exposto continua não exposto e sanitização não vaza segredo/protocolo bruto.

### LR-9E — Concurrency, Security & Final Gate

Consolidar execution + observation + Presentation lifecycle, paralelismo,
approvals, cancelamento, Headless/reentrada e stress final.

Gate mínimo: PTY humana, múltiplas fontes concorrentes, Structured Exec
controlado, burst alto de STREAM, CRITICAL preservado, UI descartável, zero
aumento deliberado de provider calls/tokens por observabilidade e nenhum processo
órfão.

## 13. Fora de escopo

- Copilot SpecialistAgent completo;
- Codex executor completo;
- shell irrestrito a agents;
- tmux completo;
- persistência de PTY após reboot;
- Windows/macOS obrigatórios no primeiro gate;
- sudo automático;
- private chain-of-thought;
- resumo de traces via LLM por padrão;
- substituir primitives estruturadas por shell;
- reabrir a antiga Luna Voice;
- misturar Presentation com autoridade de processo.

## 14. Roadmap

~~~text
PERF-1 PASS
   ↓
LR-9 Operational Terminal & Cognitive Trace Runtime
   ↓
LR-10 GitHub Copilot SpecialistAgent
   ↓
LR-11 OpenAI Codex SpecialistAgent
~~~

LR-10/LR-11 herdam Execution Broker, provenance, PTY/exec primitives, trace
contracts, observação bounded e uma superfície preparada para concorrência.

## 15. Antiga LR-9

O antigo escopo **Luna Voice e feedback natural** foi adiado em 08/10/2026.
Continua válido, mas sem posição fixa e sem bloquear LR-10/LR-11.

Registro:
[NARYS-VOICE — Unified Voice & Natural Feedback](NARYS-VOICE-FUTURE-TRACK.md).

## 16. Próxima ação

LR-9A, LR-9B e LR-9C estão encerradas em PASS, auditadas e integradas.
A LR-9D é IMPLEMENTAÇÃO CANDIDATA na branch
`lr-9d-cognitive-agent-trace-adapters`, com gates locais verdes e evidências
registradas. Próxima ação: auditoria independente da Luna; PASS definitivo
depende dessa auditoria.

LR-9E permanece sem implementação. Approvals, sandbox e execution authority
agentiva continuam nas fases seguintes; LR-9D observa somente fatos existentes.
