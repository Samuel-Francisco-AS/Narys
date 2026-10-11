# LR-10 — GitHub Copilot SpecialistAgent

**Estado vigente (10/10/2026): LR-10A PASS FINAL; NARYS-SERVER-1A–D PASS DELIMITADO; LR-10B PASS FINAL DELIMITADO após FIX-1 e [reauditoria independente](LR-10B-INDEPENDENT-AUDIT-2026-10-10.md). Próxima etapa operacional: LR-10C — Authority, Approval Policy, Sandbox & YOLO. LR-10D/E/F e LR-11 ainda NÃO IMPLEMENTADAS.** [Meta Narys 0.1 agentiva até 17/10/2026](NARYS-01-AGENT-TOOLS-DELIVERY.md): ainda não alcançada. Nenhuma inferência/ferramenta Copilot está liberada pela LR-10B.
**Sequência vigente:** LR-9 PASS → LR-10A PASS → SERVER-1 PASS → LR-10B PASS → **LR-10C próxima** → LR-10D/E/F (gates próprios) → LR-11 executor Codex em trilha separada.
**Base histórica:** a referência antiga de `main` abaixo era marco de entrada da trilha e não representa o HEAD pós-LR-10B. A validação e os commits da etapa B estão no [relatório de entrega](LR-10B-DELIVERY-REPORT.md) e no [parecer final](LR-10B-INDEPENDENT-AUDIT-2026-10-10.md).
**Plano de entrada:** [LR-10A — SDK & Runtime Feasibility POC](LR-10A-FEASIBILITY-POC.md).

## 1. Tese e resultado de produto

Integrar o **GitHub Copilot como SpecialistAgent de engenharia**, usando seu *motor agentivo operacional* (ferramentas de arquivos, comandos, programação, testes, sessões e eventos) **dentro da interface da Narys**. O Copilot não deve ser reimplementado como gerador simples de texto, nem substituir a autoridade, o estado durável ou a identidade da Luna.

**Decisões do usuário (09/10/2026):**
- Narys apresenta conversa, atividades operacionais, aprovação e resultado. Copilot e, futuramente, Codex trabalham como especialistas dentro dessa interface; nenhuma TUI nativa é requisito do produto.
- Três perfis: **Assistido**, **Autônomo isolado**, **YOLO por ativação humana explícita**. Nenhum modo expansivo é default.
- Runtime Copilot **on-demand**: não iniciar com o aplicativo nem manter processo ativo sem trabalho. Encerrar com cleanup verificado após a última atividade elegível.
- Preferência pela integração **SDK oficial Rust**, condicionada a POC; elevação do MSRV proposta para **Rust 1.94.0** mantendo a Edition **2021** da Narys.
- Preservar autonomia de desenvolvimento e ferramentas nativas do Copilot, com segurança, quota, cancelamento e observabilidade sob controle da Narys.

**Aceitação de produto:** uma solicitação de engenharia, feita na conversa da Narys, gera execução operacional real no workspace aprovado, acompanha eventos e decisões, produz diff/resultado + evidências de testes e consumo, permite cancelamento e finaliza sem processo órfão; não é necessário abrir a TUI do Copilot.

## 2. Estado herdado, limites e fronteiras

A LR-9 fornece:
- `ExecutionBroker` process-wide: Structured Exec, PTY humano, lifetime/cancel/reap, limites e trace;
- `OperationalTraceBus` e `AgentTraceSink`: observabilidade passiva, bounded, sem private reasoning;
- Terminal/Activity lazy e Headless/reentrada; hardening de IPC release;
- `AgentBackend`, `AgentRegistry` e Codex planner read-only anteriores à LR-10;
- Scheduler, TaskGraph, políticas LR-8.5 de recursos/quota e safe handoff.

**Baseline histórico anterior à LR-10A:** faltavam `CopilotAgentAdapter`, autoridade agentiva, sandbox, approvals e integração Copilot. **Após LR-10A:** o crate separado `narys-core/` executou um `CopilotBackend` textual real no Fedora headless, sem integrar a factory de production Tauri; execução de ferramentas, sandbox comprovado, approvals e Supervisor completo continuam pendentes. Em produção, `ExecutionAuthority` permite `HumanLocal` apenas para `ExecutionOrigin::Human`. `WorkspaceScope` verifica cwd, mas **não é sandbox de filesystem**. O Codex de LR-7D0.5 é *planner* read-only; LR-11 permanece responsável pelo executor Codex.

**Invariante central:** `ExecutionOrigin::SpecialistAgent`, TaskId, IDs de sessão, provenance, callbacks da WebView e texto produzido por LLM **não conferem autoridade**. Nunca reutilizar `HumanLocal` nem abrir IPC de shell genérico para agentes.

## 3. Topologia proposta

~~~text
Narys UI: Conversation (objetivos/aprovações/decisões/relatório)
           Terminal Activity (eventos/saídas/progresso, bounded)
                          │ comandos tipados
                          ▼
Luna/Narys Core: TaskGraph + task state + LR-8.5 + AgentRegistry
                          │ intenção + contrato de execução tipado
                          ▼
CopilotAgentAdapter (Rust, sem chamada direta da WebView)
    ├─ AgentRuntimeSupervisor: on-demand, owner, refcount/leases, shutdown/reap
    ├─ Copilot SDK Client/Session + handlers/hooks, eventos, usage/quota
    ├─ AgentApprovalPolicy: Assistido / Isolado / YOLO explícito
    ├─ workspace + sandbox/authority boundary (prova operacional exigida)
    └─ Result/Evidence + terminal task state
                          │
            Copilot CLI runtime / native agent-tool loop
                          │
            tool calls, edits, commands, test runs
                          │
           eventos observáveis (passivos e sanitizados)
                          ▼
OperationalTraceBus → Activity  /  resultado validado → Conversation
~~~

O SDK é uma interface para o CLI sobre JSON-RPC; **o próprio CLI executa seu loop de ferramentas**. O fato de a Narys lançar/supervisionar o processo principal **não prova** que seus comandos passam pelo `ExecutionBroker` nem que suas ações foram confinadas. A LR-10C deverá estabelecer e testar explicitamente a política de autorização + sandbox do runtime e de suas ferramentas. Se integração nativa com Broker for inviável, usar supervisor agentivo separado com um boundary verificável, **sem alegar mediação pelo Broker**. Nenhum executor agentivo bypassa os gates de segurança por herança de origem.

**Separação dos planos:** Core decide e autoriza, Copilot trabalha, Observation Plane observa, UI apresenta. O futuro AI-Native Runtime mantém sua própria fronteira e não é antecipado nesta fase.

## 4. Contratos do plano (lifecycle LR-10B candidato; autoridade/resultados LR-10C/D pendentes)

Contratos recomendados, sujeitos ao POC:

| Contrato | Campos/semântica essenciais |
| --- | --- |
| `CopilotTaskInvocation` | TaskId, correlation, objetivo, capacidades requeridas, workspace aprovado, modelo/Auto, modo de autonomia, ceiling/limites, timeout |
| `AgentPermissionDecision` | operação/ferramenta/argumentos normalizados, sessão, decisão `approve_once/deny/ask`, origem humana e prazo de validade |
| `AgentExecutionLease` | responsável pelo runtime, tarefas ativas e prompts pendentes, estado de cancelamento, no-start-after-stop |
| `AgentSessionReceipt` | identificador opaco de sessão, TaskId associado, lifecycle, checkpoint/recovery admissível, política de retomada |
| `AgentWorkEvidence` | estado real, diff/inventário de mudanças, comandos/testes com exit status, limitações, usage observado e lacunas |
| `AgentUsageObservation` | sinal, fonte, unidade, escopo (sessão/conta/modelo), timestamp, frescor, nullable, cobrança confirmada vs estimada |

Os contratos devem ser **agent-neutral onde possível**, para não refazer o núcleo na LR-11, e **Copilot-specific nas peculiaridades do protocolo**. Manter retrocompatibilidade: hoje `AgentRequest` só contém `objective` e `required_capabilities`, `AgentResult` só contém `output`, e a implementação Codex read-only é restrita. Evoluir por extensão/adapters sem alterar silenciosamente as garantias de Codex, TaskGraph e LR-8.5.

**Não** inferir sucesso de `session.idle` ou de texto do especialista. Exigir estado terminal apropriado e evidência pós-execução; conclusão relatada pelo Copilot não é prova independente de arquivos/testes. Nenhum replay de etapa com efeitos desconhecidos após crash.

## 5. Políticas de autonomia

| Perfil | Autorização | Boundary necessário | Disponibilidade |
| --- | --- | --- | --- |
| **Assistido (default)** | aprovações humanas explícitas por operação sensível; negar sem aprovador/timeout | workspace + política por ferramenta, permissões de menor privilégio; controles de escape | depois da prova LR-10C |
| **Autônomo isolado** | aprova automaticamente apenas o que o contrato permite | **sandbox de OS validado** para operações e ferramentas, limites de paths/rede/segredos, conta e processos; sem sandbox funcional, recusar modo | depois da prova LR-10C |
| **YOLO explícito** | equivalente de permissão abrangente por sessão, acionado somente pelo usuário; nunca via prompt/tool/modelo ou fallback | informar quando não há isolamento; gerenciamentos institucionais prevalecem; escopo, risco e autorização contextual explícitos | depois de threat model + gate destrutivo |

**YOLO não significa Autopilot**: YOLO expande permissões; Autopilot estende a execução/continuidade do agente. Podem coexistir **apenas dentro dos limites autorizados**. Também não significa sandbox nem confere aprovação irrestrita para sempre. A interface deve declarar sem ambiguidade quando o YOLO está atuando sobre o host sem isolamento. Não persisti-lo automaticamente após restart/resume, não habilitá-lo por default, não acioná-lo por instrução recebida de conteúdo não confiável.

Política de falha segura:
- Approval pendente sem humano disponível: negar ou pausar, **não** promover a YOLO.
- Managed/org settings que exijam aprovação não podem ser contornadas pela Narys.
- Bloquear por default `sudo`, acesso a chaves/secret stores, rede exfiltrante e caminhos externos nos modos que prometem escopo controlado.
- Apenas validar `cwd` e hooks não é isolamento: subcomandos, links simbólicos, subprocessos, plugin/MCP e ferramentas próprias do CLI podem furar uma allowlist ingênua.
- Mudança de perfil requer checagem de sessão e do escopo; não escalar autoridade implicitamente por handoff, fallback ou reinício.
- No modo YOLO sem isolamento, nenhuma afirmação de confinamento deve ser exibida.

**A política real (SDK PermissionHandler + pre-tool hooks + sandbox + capabilities disponíveis) deve ser testada contra o CLI e versões fixadas**. Se faltar cobertura para uma classe de ferramentas, bloquear o modo dependente dessa cobertura até corrigir.

## 6. Runtime on-demand e economia operacional

~~~text
Dormant (sem processo Copilot sob ownership da Narys)
   └─ primeira tarefa admitida → Starting → Ready/Active
       ├─ execuções concorrentes/leases → Busy
       ├─ aprovação pendente → PendingApproval (conta como atividade)
       ├─ cancel/erro → Cancelling/Cleanup → IdleEligible
       └─ último lease encerra + filas vazias → Stopping
           ├─ desconectar/persistir refs necessárias
           ├─ client.stop() + deadline + kill/reap se necessário
           ├─ verificar filhos/descendentes e recursos
           └─ Dormant / Faulted com diagnóstico sanitizado
~~~

- Proibido iniciar no startup, por polling da UI, por abertura de Settings ou pela simples exibição de quota; iniciar mediante operação que necessite realmente do runtime ou probe manual explícito.
- Runtime é propriedade do Rust Core, não da aba/janela; fechar a WebView não o desconecta de tarefa ativa.
- Concurrency limitada e leases por tarefa; uma tarefa ativa, aprovação pendente ou cleanup aberto impede shutdown prematuro.
- Não usar timeout arbitrário como condição suficiente de `idle`. Deixar tempo de graça configurável após último trabalho e validar custo/perf; compromisso é **zero processo permanente sem demanda**.
- Cancelamento solicitado deve interromper sessão/turno, finalizar subárvore de processos relevante, impedir commit posterior e preservar TaskGraph/continuation sem duplicar efeitos. Failures de shutdown não viram sucesso silencioso.
- `client.stop()` é o caminho graceful; `force_stop` somente recovery/fallback, documentando perda de garantias de flush. Medir CPU/RSS/startup/tempo para liberar processos no Fedora sem GPU e em Headless.
- Sessões duráveis pertencem ao provider; Narys persiste **refs opacas e evidências próprias**, sem confiar que `resume` revalida authority automaticamente. Reavaliar profile, workspace, modelo, quota e consentimento em cada retomada.

## 7. Recursos cognitivos, Student e limites

- Identidade de autenticação: preferir conta já logada no CLI e credential store; **nunca ler/copiar tokens, auth JSON ou secrets para logs/frontend/SQLite**.
- Modelo **Auto** como default, compatível com benefícios Student. Não presumir modelo, allowance ou preço fixos; consultar capabilities e catálogo atuais do runtime.
- Observação: `account.getQuota`; `assistant.usage` e demais sinais `session.usage.*` quando presentes. Registrar unidades explícitas, timestamp, origem e lacunas; ausência ou falha de quota **não significa zero saldo nem saldo infinito**.
- `sessionLimits.maxAiCredits` é **limite soft após chamada**, sujeito a ultrapassagem por uma resposta; NÃO substitui um orçamento/admission da Narys, nem autoriza cobrança extra.
- Definir contrato de escassez/eleição LR-8.5 por especialista; não assumir que créditos e tokens de Copilot correspondem aos de um `CognitiveProvider`. Tarifa e disponibilidade são facts consultados, não constantes inventadas.
- Primeira tarefa real com quota deve ser pequena, explicitamente autorizada e com fixture reprodutível. POC de login/handshake é separado do teste consumindo modelo.

## 8. UI unificada e autoridade

- **Conversation**: intenção, perguntas ao humano, approvals e justificativas, decisões e relatório final. No modelo UX, todos os especialistas usam os mesmos padrões visuais, com identificação clara de source/model/custo quando conhecido.
- **Terminal / Activity**: eventos naturalmente observáveis de ferramenta, stdout/stderr permitido e sanitizado, progresso e subtarefas, replay bounded, gaps identificados. Não replicar a TUI do Copilot.
- Nada de converter `Activity` em ferramenta de execução nem criar IPC genérico de subprocesso. `Settings` pode configurar política declarativa, mas não carregar authority mutável enviada da WebView.
- Private chain-of-thought, raw JSON-RPC, credenciais, prompts internos, filesystem completo ou texto de tool sem política de minimização **não** entram em traces. Failures de observador não falham uma execução funcional válida.
- A presença 3D e a UI devem permanecer opcionais; execução precisa funcionar em Economy/Headless com reentrada.

## 9. Decomposição e gates

### LR-10A — SDK & Runtime Feasibility POC
**Saída:** decisão GO / FIX-AND-RETEST / NO-GO, evidências de ambiente e alternativa se necessário.
- Fixar release/SDK/CLI; comprovar Rust >=1.94 e edition da app 2021; avaliar cargo tree, MSRV declarado e regressão de Tauri.
- Comparar SDK Rust bundled/unbundled/runtime com `CliProgram::Path`. O SDK Rust não faz varredura genérica de `PATH` por default em runtime gerenciado: caminho/compatibilidade explícitos importam.
- Validar autenticação existente **sem extrair credenciais**, modelos/Auto, capabilities, quota, criação de sessão, streaming, cancelamento e shutdown/reap. POC com teste read-only sem gastos quando possível.
- Medir cold start, CPU/RSS incremental, build size, dependências e processos, inclusive stop em falha/Headless.
- Teste com modelo real separado, mínimo e consentido, e política documentada para indisponibilidade/limite de quota.

Referência: [roteiro verificável da LR-10A](LR-10A-FEASIBILITY-POC.md).

### LR-10B — Copilot Adapter & On-Demand Supervisor
**Implementação candidata:** [arquitetura e operação](LR-10B-COPILOT-ADAPTER-SUPERVISOR.md) · [relatório/evidências](LR-10B-DELIVERY-REPORT.md). Sem inferências novas, tools ou PASS definitivo.
**Saída:** adapter registrado (não exposto como CognitiveProvider), contrato de sessão/lifecycle, events/control.
- `CopilotAgentAdapter` e registry com compatibilidade com o Codex read-only.
- `AgentRuntimeSupervisor` process-wide: limites, leases, concorrência, refcount, cleanup, failure recovery e shutdown.
- Sessões new/resume e outputs tipados, attach/detach da UI, cancelamento cooperativo e forçado, zero lançamento por Settings/startup.
- Testes de corrida start/stop, cancel/completed, sink fechado, reentrada e stop concorrente.

### LR-10C — Authority, Approval Policy, Sandbox & YOLO
**Saída:** prova por ferramentas e execução do boundary; só então habilitar efeitos reais.
- Políticas Assistido, Autônomo isolado, YOLO explícito, validação central e UX de confirmação.
- Hooks e permission handler; inventário das ferramentas nativas/MCP, subprocessos e rede.
- Decidir *com evidência* entre agent-owned execution mediada pelo Broker e CLI runtime com sandbox/supervisor: não assumir mediação.
- Testes negativos (traversal/symlink, comandos aninhados, shell, FS externo, ambiente/segredos, rede, approvals negadas, políticas gerenciadas, tentativa de auto-YOLO, process escape).
- YOLO real e sem sandbox só mediante opt-in inequívoco e com aviso de risco; jamais fallback automático.

### LR-10D — TaskGraph, Quota, Handoff & Trace
**Saída:** TaskId/provenance/evidence, orçamento e recovery coesos com LR-8.5 e LR-9.
- Integrar resultado e consumo ao estado durável existente sem igualar `AI Credits` a tokens.
- Observabilidade passiva e bounded; eventos permitidos e gaps, sem raw reasoning e sem gasto cognitivo extra para tracing.
- Cancelamento tardio, resumptions/checkpoints e safe handoff sem duplicar efeitos.
- Quota desconhecida, rate limit, falta de entitlement e erro de rede geram estados diferentes; nenhum fallback automaticamente pago/YOLO.

### LR-10E — Narys UX & Real Engineering Gate
**Saída:** tarefa completa observável na UI unificada, sem TUI nativa.
- Fluxo Conversation → approval → Activity → relatório final com diffs, testes, falhas e custo observado.
- Teste real em **workspace descartável aprovado**, com modificação pequena, compilação/teste, interrupção/cancelamento e reteste pós-restart/Headless.
- Testar os três perfis, gate de isolamento e ausência de autoridade implícita; execução YOLO real somente com aprovação separada e ambiente controlado.
- Nenhuma tarefa usa diretórios pessoais/produção por default nem faz push remoto sem autorização específica.

### LR-10F — Concurrency, Security & Final Gate
**Saída:** auditoria independente; PASS somente se todos os invariantes forem provados.
- Regressão de LR-7D Codex planner, LR-8/8.5 allocation/handoff, LR-9A–E trace/execution/Headless, UI e release IPC.
- Matrizes concorrentes de PTY humano + Agent + Worker + CognitiveProvider; processos do Copilot não herdam `HumanLocal`.
- Stress de cancel/timeout/crash/reattach/quit e zero orphan/duplication; bounded memory/trace; recursos devolvidos em idle; versão de SDK/CLI e medições registradas.
- Verificação de falsos `completed`, aprovação bypassada, leak em logs/telemetry, re-run de efeitos, alteração de security posture do release.
- Gate humano explícito em tarefa real, e documentação de limitações; não aprovar por número de testes apenas.

Cada subfase: branch curta baseada na `main` verificada, implementação/testes/evidências, auditoria independente, fechamento antes da próxima etapa. Não preemptar a LR-11 nem modificar arquivos de código neste registro de planejamento.

## 10. Critérios de PASS final (todos obrigatórios)

1. Copilot no `AgentRegistry` como `SpecialistAgent`; Codex planner ainda opera, sem aumentar suas capabilities acidentalmente.
2. Editar/testar um projeto descartável autorizado; diff e exit codes observáveis; sucesso sustentado por evidência, não por declaração da LLM.
3. Human approvals fazem bloqueio efetivo; falta de UI nega/pausa; Autônomo isolado só funciona com boundary comprovado; YOLO só opt-in e sem escala persistida.
4. Nenhum agente usa `HumanLocal`/generic-shell WebView; permission & sandbox gates respeitados em todas as ferramentas e subprocessos testados.
5. Runtime zero no idle inicial; startup on-demand; stop gracioso + reap / erro diagnosticado; Headless e reentrada preservam tarefa ativa.
6. Cancelamento propaga, não duplica efeitos/continuations e não publica sucesso tardio; failures não liberam authority.
7. Traces bounded e redigidos; pensamento privado e segredos não expostos; falha observacional isolada.
8. Quota/usage e limites são tipados e honestos, sem alegar hard cap inexistente; no unexpected paid consumption.
9. Stress concorrente, execução debug/release, regressões do projeto e medições locais passam sem bloqueantes.
10. Auditoria independente + gate humano; registro de versão/pin/limitações; integração na `main` somente após PASS.

## 11. Fora de escopo

- Reproduzir a TUI inteira do Copilot ou exigir terminal nativo como UX;
- engine Copilot reimplementado na Narys, ou Copilot como conversa trivial genérica;
- Codex executor antes da LR-11;
- autonomia global always-on/execução preventiva ao abrir a Narys;
- confiar em `--yolo` como sandbox; `sudo` automático; escalonamento implícito de permissões;
- criar um shell genérico pela WebView;
- emular ou burlar quotas/entitlements, alternar identidades para contornar limites, gastos sem autorização;
- prometer Windows/macOS no primeiro gate, reabrir NARYS-VOICE, alterar avatar;
- implementar AI-Native Runtime ou a trilha experimental NX pós-LR-11.

## 12. Riscos e pontos de decisão

| Risco | Decisão / validação |
| --- | --- |
| SDK Rust/CLI evoluindo rapidamente | pin compatível e probes de protocolo; rollback para adapter via JSON-RPC somente se viável e justificado |
| `rust-version` da Narys = `1.77.2`; SDK = `1.94.0` | proposta de elevar MSRV na LR-10A após build/testes; manter Edition 2021; custo financeiro zero, custo CI/compatibilidade avaliado |
| SDK Rust usa Edition 2024 internamente | Cargo permite editions por crate; não implica migrar Edition da Narys |
| feature default `bundled-cli` pode ampliar build/download | medir `default-features=false, features=["runtime"]` + caminho CLI explícito vs bundle; não prescrever até POC |
| CLI tool loop fora do Broker | provar supervisão/sandbox independentemente; nenhuma autoridade agentiva por provenance |
| approve-all/hooks não garantem confinamento | defesa em profundidade com controles de SO, teste adversarial, desabilitar modo não comprovado |
| quota `maxAiCredits` é soft | admission + limites próprios, UX explícita e observação de custos; não prometer limite rígido |
| `session.idle` não é conclusão comprovada | validar efeitos pós-execução, TaskGraph state e receipts |
| memória/RAM em Fedora 8 GiB | build/runtime sob demanda e medição release; evitar processo residente |
| instabilidade de autenticação/serviço ou restrições de organização | estados separados; não adotar bypass nem inferir saldo a partir de falha |

## 13. Evidências e referências oficiais (verificadas 09/10/2026)

- [Rust SDK — README/API, lifecycle, streaming e permission handlers](https://github.com/github/copilot-sdk/blob/main/rust/README.md)
- [Rust SDK — Cargo.toml, `rust-version=1.94.0`, Edition 2024, features](https://github.com/github/copilot-sdk/blob/main/rust/Cargo.toml)
- [SDK release v1.0.18 de 08/10/2026](https://github.com/github/copilot-sdk/releases/tag/v1.0.18) — referência de investigação, **pin efetivo ainda indefinido**.
- [Autenticação com conta GitHub já logada](https://github.com/github/copilot-sdk/blob/main/docs/auth/authenticate.md)
- [Usage / billing / `account.getQuota`](https://github.com/github/copilot-sdk/blob/main/docs/features/usage-and-billing.md)
- [Session limits são soft](https://github.com/github/copilot-sdk/blob/main/docs/features/session-limits.md)
- [SDK hooks e pré-tool](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/hooks)
- [CLI permissões, `--allow-all` / `--yolo`](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli/allowing-tools)
- [Persistência e retomada de sessões](https://github.com/github/copilot-sdk/blob/main/docs/features/session-persistence.md)
- [LR-9 — terminal e observação](NARYS-TERMINAL-RUNTIME-TRACK.md)
- [LR-8.5C — safe handoff](LR-8.5C-SAFE-HANDOFF.md)
- [LR-7D0.5 — Codex agent bridge read-only](LR-7D05-CODEX-AGENT-BRIDGE.md)
- [Plano operacional da Narys](PLANO-OPERACIONAL-LUNA.md)

**Registro autoriza apenas planejamento/documentação; não constitui PASS técnico, teste real, prova de isolamento, instalação do SDK nem autorização permanente de YOLO.**

## 14. Checkpoint obrigatório pós-LR-10A (10/10/2026) — PAUSA

**LR-10A encerrou em PASS FINAL técnico** para servidor headless com Copilot textual e zero tools, conforme [fechamento auditado](LR-10A-FINAL-CLOSURE-2026-10-10.md), [relatório](LR-10-LATEST-EXECUTION-REPORT.md) e [evidência real](../narys-core/evidence/final-integrated-operation.json). Implementado `narys-core/` sem Tauri/GTK/WebKit, systemd --user, Keyring manual, Stronghold existente, SDK1.0.17/CLI1.0.95 pinados, sessão/response real5, TaskGraph/SQLite/artifact/trace e resume real sem send novo. Uma inferência de três autorizadas; duas restantes revogadas. A aprovação é **somente da LR-10A**. Não classificar LR-10 inteira como PASS.

**Decisão atual:** LR-10B, C, D, E e F **PAUSADAS / NÃO INICIADAS**. A próxima prioridade de implementação é [NARYS-SERVER-1](NARYS-SERVER-1-HEADLESS-SERVER-RUNTIME.md), em até 48h da abertura com máximo de quatro etapas. Enquanto ela estiver ativa, não iniciar atividades LR-10B–F por inércia.

### Transferência explícita para retomada B–F

| Etapa posterior | Base que já existe (não reimplementar) | Demanda restante para Copilot operacional futuro |
| --- | --- | --- |
| **LR-10B** Adapter/Supervisor | `narys-core/src/{server,worker,authorization}.rs`; SDK real, sessão e resume, Core sob systemd, lifecycle on-demand, harness de ownership | Adapter de produção reunido ao Core servidor/Tauri, leases e concorrência, supervisor com crash recovery e persistência de sessão além de /tmp, processo sem órfãos em falha adversarial, sem duplicar SQLite |
| **LR-10C** Authority/Approval/Sandbox | DenyAll, zero tools, HOST_ASSISTED_NOT_SANDBOX, sem HumanLocal | Permissões por ferramenta e operação, aprovador humano, sandbox de SO e limites de rede/FS/segredos; perfis Assistido/Isolado/YOLO explícito; ferramentas SDK/CLI não passam automaticamente no ExecutionBroker |
| **LR-10D** TaskGraph/Quota/Handoff/Trace | Task2 completada, validação por arquivo/SQLite/TaskGraph, usage `totalPremiumRequests`, trace bounded, receipt durável de teste fechado | Unificar contratos de tarefa/reentrada e economia LR-8.5/trace LR-9 para fluxos de produto; unidades AI Credits vs requests, observação de quota e ausência de hard cap; políticas reutilizáveis por sessão, zero extrapolação da autorização encerrada |
| **LR-10E** UX e tarefa real de engenharia | CLI SSH permite status/prepare/submit/result/cancel; tarefa textual real sem ferramentas | Conversa/Activity/approvals na UI Narys e clientes servidor, progresso de edição/testes/diff em workspace, gates dos perfis, UX sem TUI Copilot |
| **LR-10F** Audit/Security/Concurrency final | 67 testes direcionados de LR10A, um cold boot, one send, resume e cleanup observados | Stress agentivo com ferramentas, cancel/completed race, crash/orphan, rede/credenciais/approvals, regressões produção; MSRV/Tauri quando de fato modificados |

**Dívidas concretas preservadas:** helper Keyring50 usa API interna não suportada e pede senha manual; CLI tem `RES_OPTIONS=no-aaaa` restrito ao processo; config.json Copilot teve drift sem autoria provada; soft `maxAiCredits` falhou no create e foi omitido; quota externa pode não refletir uso imediatamente; o Core mínimo tem DB separada da GUI, sessões em /tmp e ferramentas desativadas; host-assisted é sem sandbox; rustc1.98.1 foi compilador observado, não prova de MSRV1.94. **Nenhuma dívida é motivo para reabrir a LR-10A**. Enquadramento e destinação: [ledger de fechamento](LR-10A-FINAL-CLOSURE-2026-10-10.md).

**Continuidade de produto:** primeiro tornar toda a versão Narys-servidora utilizável na trilha NARYS-SERVER-1; depois retomar a execução agentiva do Copilot em B–F (e Codex em LR-11), sem confundir o sucesso de texto da LR-10A com edição/comandos. Reabrir a LR-10 exige decisão expressa e checkpoint atualizado; não há permissão permanente para inferência, overage ou YOLO.

## 15. Decisão de produto posterior de 10/10/2026 — ferramenta real até 17/10

O usuário aprovou [arquitetura Core-First](NARYS-CORE-FIRST-ARCHITECTURE-2026-10-10.md) com **liberdade para refatorar integralmente Conversation, Scheduler, Registry, persistência ou bootstrap Tauri** se for a opção melhor; não preservar a preferência conservadora por extração incremental como impedimento. [Plano da entrega 0.1](NARYS-01-AGENT-TOOLS-DELIVERY.md).

**A pausa LR-10B–F é temporária e circunscrita à execução SERVER-1.** O compromisso atualizado exige operacionalizar agentes e ferramentas reais **até 17/10/2026**, portanto após SERVER-1 a retomada dos componentes agentivos Copilot será prioridade, ao lado de LR-11/Codex. Permanecem gates de execução segura: tool authority por operação, aprovações quando necessárias, limites por workspace, processos supervisionados, quotas/cancelamento e provas de ferramenta realmente invocada pela LLM. Não classificar o modo host-assisted como sandbox; YOLO não é default. Nenhuma autorização financeira ou de inferência da LR-10A continua válida.

Critério de aceite da integração real: tarefa de engenharia disparada pela conversa/CLI Narys, ferramenta solicitada e efetivamente executada com permissão validada, edição observável, teste ou build executado, diff/artefato/trace, resposta e histórico recuperável. Os gates de Copilot e Codex são avaliados independentemente; não atribuir PASS a executores não testados.


## 16. Fechamento da LR-10B (10/10/2026)

**LR-10B: PASS FINAL DELIMITADO**, após correção FIX-1 de startup/ownership/recovery e [auditoria independente](LR-10B-INDEPENDENT-AUDIT-2026-10-10.md). Candidata final `13a75bebdac7ac1ce7f69a0b6fdfab861812d97a`, correção de código `eaba03e98b54f5cde2b5968fbb1cbc4808f20576`. Registros dos 1.170 testes aprovados e 2 gates reais ignorados no [relatório](LR-10B-DELIVERY-REPORT.md) e [matriz](evidence/lr10b/VALIDATION-MATRIX.md). O parecer revisou código remoto e evidências, sem executar testes no Fedora. Lifecycle autenticado, sandbox, inferência, ferramentas, endurance, transcript, custo externo e MSRV exato continuam pendentes ou NOT_VERIFIED. **A próxima fase é LR-10C, sem autorização implícita de modelo, ferramenta, YOLO ou cobrança.** Histórico anterior desta especificação descreve decisões tomadas na época e não substitui esta posição vigente.
