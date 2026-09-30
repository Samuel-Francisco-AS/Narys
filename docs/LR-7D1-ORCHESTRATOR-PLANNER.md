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

`run_orchestrator_planning` recebe um objetivo, usa contexto técnico mínimo,
invoca o Scheduler com timeout persistido por provider e retry da policy, e
retorna provider, usage e somente o plano validado. Não há execução automática.
O módulo aceita cancelamento antes da aceitação do resultado e não aceita plano
depois de um cancelamento vencedor.

## PlanV1 e structured output

Gemini/Groq anunciam somente `text_stream()`: não há capability nativa de
structured output. O prompt exige JSON cru, sem markdown ou texto adicional.
O parse é estrito e falha fechado; não existe reparo mágico. A validação única
de `agents::planner::PlanV1` impõe tamanho, cardinalidade, IDs, dependências,
ciclos, capabilities e consistência de perguntas.

## Contexto, eventos e segurança

O objetivo é limitado a 2048 bytes e pelo budget persistido. Histórico e memória
não são enviados ao planejamento diagnóstico; secrets nunca entram em SQLite ou
na resposta. O Core mantém autoridade sobre schema, budgets, capabilities,
permissões e cancelamento. O resultado mostrado na UI é o plano já validado.

## Testes e gate humano

Testes locais cobrem migration/reopen, seed, policy e parse estrito; não usam
internet, quota ou credenciais. O gate humano deve configurar Gemini na policy,
salvar, executar o objetivo controlado, verificar provider e plano, reiniciar,
trocar somente para Groq, salvar e repetir. Deve confirmar que Conversation,
Summary, identidade e histórico permanecem intactos e que nenhum passo é
executado.

Fallback chain/Auto/score/affinity/task graph, ferramentas, LR-8 e expansão do
Codex permanecem fora de escopo.
