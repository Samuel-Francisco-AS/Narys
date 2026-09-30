# LR-7D1 — Orchestrator/Planner configurável

Estado: **candidata à auditoria independente e ao gate humano**.

## Arquitetura

Orchestrator é um `CognitiveRole` da Luna e usa o `ProviderRegistry`/Scheduler
existente. Gemini e Groq continuam `CognitiveProvider`s; Codex continua um
`AgentBackend` separado e não é selecionável nesta policy.

## Persistência e runtime

A migration 008 reconstrói `cognitive_role_policies` para aceitar
`orchestrator`, preservando Conversation/Summary, adicionando
`context_max_bytes` e sem armazenar credenciais. O default é `Fixed(gemini)`,
4096 tokens de saída, 8192 bytes de contexto e duas chamadas máximas. A policy é
carregada a cada operação, validada contra catálogo, capability e SecretStore,
e sobrevive a reopen/restart.

`start_orchestrator_planning` registra uma tarefa no `TaskRegistry` compartilhado,
emite eventos pelo `Channel<TaskEvent>`, usa contexto técnico mínimo, invoca o
Scheduler com timeout persistido por provider e retry da policy, e remove a
tarefa em todos os caminhos terminais. `cancel_task(TaskId)` usa a mesma flag
compartilhada pelo Scheduler; a resolução de corrida em `TaskRegistry::finish`
faz o cancelamento vencedor impedir a publicação do plano.

## PlanV1 e structured output

Gemini/Groq anunciam somente `text_stream()`: não há capability nativa de
structured output. O prompt incorpora a representação serializada de
`agents::planner::output_schema()` e explicita as invariantes que o JSON Schema
não cobre (IDs, dependências, ciclos e consistência entre `needsUserInput` e
`questions`), além de um exemplo mínimo apenas de formato. A resposta aceita é
JSON cru, sem markdown ou texto adicional; o parse é estrito e falha fechado,
sem reparo mágico. Falhas são classificadas apenas pelos códigos sanitizados
`orchestrator_json_syntax_invalid`, `orchestrator_plan_shape_invalid` ou
`orchestrator_plan_semantic_invalid`. A validação única de
`agents::planner::PlanV1` impõe tamanho, cardinalidade, IDs, dependências,
ciclos, capabilities e consistência de perguntas.

## Contexto, eventos e segurança

O objetivo é limitado a 2048 bytes. `context_max_bytes` é o orçamento
determinístico de input do planejamento: limita os bytes UTF-8 da instrução
enviada ao provider (instruções fixas mais objetivo); não pretende representar
bytes HTTP nem serialização interna do `ContextBundle`. Histórico e memória não
são enviados ao planejamento diagnóstico; secrets nunca entram em SQLite ou na
resposta. O Core mantém autoridade sobre schema, budgets, capabilities,
permissões e cancelamento. A UI acompanha `TaskStarted`, provider/retry,
`ProviderOutputObserved` sem conteúdo parcial, `OrchestratorPlanReady` (somente após validação), e exatamente um terminal
`TaskCompleted`, `TaskCancelled` ou `TaskFailed`.

## Testes e gate humano

Testes locais cobrem migration/reopen, seed, policy, routing por provider,
budgets/timeouts, lifecycle do registry e parse estrito; não usam internet,
quota ou credenciais. O gate humano deve configurar Gemini na policy, salvar,
executar o objetivo controlado, verificar provider e plano, reiniciar, trocar
somente para Groq, salvar e repetir. Deve confirmar que o TaskId é cancelável,
que a UI recebe eventos factuais e que Conversation, Summary, identidade e
histórico permanecem intactos e nenhum passo é executado.

O gate humano confirmou a persistência da policy após restart, o routing real
para Groq, a geração real de `PlanV1` válida por Groq, o terminal `completed` e
que nenhum passo/ferramenta foi executado. O mesmo gate revelou que o catálogo
não expunha um default Gemini, que `settings-ai` não tinha
`allow-cancel-task`, e uma perda de responsividade ao iniciar o planejamento.

As correções desta rodada fazem o catálogo derivar o default Gemini da
constante canônica da integração, adicionam somente `allow-cancel-task` à
capability `settings-ai` e movem o preflight SQLite/SecretStore do início
síncrono de `start_task` para `spawn_blocking` após o registro da tarefa. O
preflight continua validando policy, catálogo e timeouts; seus erros seguem o
fluxo factual de histórico e evento terminal, e o cancelamento verificado antes
do planejamento continua impedindo `OrchestratorPlanReady`.

Foi adicionada cobertura de lifecycle de `start_task` para retorno do `TaskId`
antes do preflight, falha de preflight, cancelamento durante preflight e
cancelamento após o provider iniciar. Os testes verificam eventos, histórico,
ausência de plano publicado, chamadas ao provider e remoção do registry; não há
novo comportamento funcional nesta cobertura.

O gate humano precisa ser repetido apenas para confirmar a troca Groq ↔ Gemini,
o cancelamento pela janela `settings-ai` e a responsividade no início. O estado
permanece **CANDIDATA À REAUDITORIA / GATE HUMANO**.

Fallback chain/Auto/score/affinity/task graph, ferramentas, LR-8 e expansão do
Codex permanecem fora de escopo.
