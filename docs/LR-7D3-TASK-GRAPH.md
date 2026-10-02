# LR-7D3 — Task graph mínimo + subtarefas independentes

Estado da branch: **CANDIDATA COM FIX-3 IMPLEMENTADA; validação final e gate humano pendentes**.
Branch: `lr-7d3-task-graph`.
Base: `main@b447836ab84224cada5cf2e689d7aab9cf1f45ab`.

Este documento registra a implementação candidata. **Não é registro de PASS**:
typecheck/build, gates Rust e gate humano com providers reais ainda precisam ser
executados no Fedora antes do fechamento da LR-7.

## FIX-3 — accounting e fronteira de confiança

O accounting conservador de output divide o saldo pelos provider calls ainda
possíveis, incluindo a tentativa que está prestes a começar. Uma falha sem
usage debitável consome sua reserva; retry só é anunciado quando há chamada e
saldo de output para iniciá-lo. `output_tokens` permanece a medição observada e
`output_tokens_accounted` permanece o ledger conservador limitado ao budget.

Na compilação D3, IDs de subtarefa aceitam somente ASCII alfanumérico, `_` e
`-` (até 64 bytes). Essa validação é local ao TaskGraph; o contrato global
PlanV1 não foi alterado. A instrução interna do Worker é estática e não incorpora
IDs, descrições ou outros dados do Planner. O Core continua validando
deterministicamente o `subtaskId` retornado contra o ID esperado.

Esta atualização registra a implementação candidata da FIX-3, sem declarar
PASS técnico final nem aprovação do gate humano com Groq e Cloudflare reais.

## Objetivo

Transformar um `PlanV1` validado pelo Orchestrator em um grafo executável mínimo,
capaz de distribuir trabalho cognitivo independente entre Cognitive Providers
sem criar ainda o sistema multiagente completo, ferramentas operacionais ou o
Rate Limit Manager da LR-8.

## D3A — contratos e Task Graph Core

A D3 mantém `PlanV1` como fonte única do plano. O Core compila seus
`steps/dependsOn` em `TaskGraph` e rejeita em modo fechado:

- plano inválido ou cíclico;
- `needsUserInput=true`;
- capabilities operacionais ainda fora de escopo:
  `repository_read`, `file_write`, `command_execution` e `tool_use`.

Nesta fase, somente `planning` e `structured_output` são elegíveis.

`PlanCapability::StructuredOutput` continua sendo uma capability declarativa do
passo do `PlanV1`; ela **não promove** Groq/Cloudflare para
`ProviderCapabilities::structured_output=true`. Como definido na D1, esses
adapters continuam anunciando apenas `text_stream()`; nenhum suporte nativo a
structured output é inventado. Na D3, `StructuredOutput` exige um envelope
mínimo de resultado Worker (`subtaskId`, `text`) parseado como JSON estrito,
sem campos extras, fences, prefixos ou sufixos. `planning` continua aceitando
resultado textual. `requiredCapabilities` vazio é rejeitado especificamente na
compilação do TaskGraph; a validação geral do `PlanV1` da D1 permanece separada.

A migration 011 adiciona o papel cognitivo persistido `worker` e
`task_subtask_records`. O default Worker é:

- `Preferred`;
- Groq `openai/gpt-oss-20b`;
- Cloudflare Workers AI `@cf/zai-org/glm-4.7-flash`;
- `max_provider_calls=4`;
- `max_output_tokens=4096`;
- retry 1, backoff 750 ms;
- `context_max_bytes=16384`.

O Worker é configurável na mesma UI das demais policies. Ele não reutiliza nem
altera a policy do Orchestrator.

## D3B — executor distribuído

Existe exatamente uma `TaskId` raiz. Os IDs dos `PlanStepV1` são os IDs das
subtarefas; não são registradas Tasks raiz artificiais.

O executor:

1. carrega snapshots das policies Orchestrator e Worker;
2. valida providers e credenciais antes do trabalho remoto;
3. captura uma snapshot da identidade real do Luna Core;
4. solicita um `PlanV1` ao Orchestrator;
5. compila o task graph;
6. libera somente unidades cujas dependências terminaram;
7. executa no máximo duas subtarefas independentes simultaneamente;
8. usa o ranking do Scheduler para distribuir unidades prontas;
9. fixa cada unidade no provider atribuído;
10. encerra novas ondas se qualquer worker falhar.

Depois da atribuição não existe fallback cruzado dentro da subtarefa na D3.
Retry pode ocorrer no mesmo provider segundo a Worker policy. Essa restrição
mantém provenance inequívoca e evita antecipar LR-8.

### Contexto dos workers

A identidade atual é capturada uma vez no preflight e compartilhada como
snapshot imutável. Workers não recebem automaticamente memórias nem histórico
de conversa. Uma unidade dependente recebe somente os resultados necessários
das dependências, limitados por `context_max_bytes`.

A sessão de conversa não é escrita nem encerrada pelo task graph. O teste de
preservação mantém uma sessão ativa antes/depois da execução e exige estado,
`updated_at` e quantidade de mensagens inalterados.

### Cancelamento e eventos

O mesmo `AtomicBool` da Task raiz é observado por Planner e workers.
`cancel_task(root)` interrompe a onda em execução e impede novas ondas.

Eventos adicionais são factuais:

- `task_planned`;
- `subtask_waiting`;
- `subtask_started`;
- `subtask_retry`;
- `subtask_output_observed`;
- `subtask_completed`;
- `subtask_failed`;
- `task_graph_result_ready`.

Falha do Channel tem precedência sobre o cancelamento propagado usado para
interromper workers irmãos; portanto é registrada como `failed/channel_closed`,
não como falso cancelamento do usuário.

## Budgets

O budget do Planner continua pertencendo à policy Orchestrator.

O budget dos workers é global para o grafo e vem da Worker policy. Antes de
iniciar uma onda, o Core reserva conservadoramente o pior caso de chamadas
considerando retry. A divisão de output da onda nunca excede o output restante.

Como o Scheduler não devolve usage parcial quando uma chamada termina em erro,
a D3 não agenda novas ondas depois de uma falha de worker. Isso preserva o teto
sem inventar consumo. Quando um provider conclui sem reportar output usage, o
Scheduler marca a medição como indisponível e debita no ledger o máximo
autorizado para aquela tentativa. O saldo desconhecido não é reutilizado em
ondas posteriores. `outputTokens` continua sendo a soma medida; quando uma ou
mais unidades não reportam usage, a UI marca a medição agregada como incompleta
e expõe a parcela medida e `outputTokensAccounted` separadamente. Accounting mais
sofisticado, filas e concurrency dinâmica continuam na LR-8.

## FIX-2 — fronteira provider/planner e invariantes

O contrato interno do Orchestrator é enviado pelo Core em um campo interno
opcional, separado do objetivo não confiável do usuário e anexado pelos adapters
de produção à instrução de sistema existente da Luna. Ele exige JSON cru PlanV1,
schema e invariantes e proíbe ferramentas e ações externas. `PlanV1` continua
sendo a única fonte do plano; o parser continua rejeitando markdown, prefixos,
sufixos, múltiplos objetos e formatos alternativos.

O adapter Groq agora valida exatamente um `finish_reason`: `stop` requer texto,
`length` é incompleto, chamadas de ferramenta exigem ação e valor ausente,
desconhecido ou conflitante é erro de protocolo. Esses erros mantêm a classe de
retry/fallback da LR-7D2. Nenhum adapter anuncia structured output nativo.
Diagnósticos DEV do Cloudflare incluem somente provider/model sanitizados, fase,
marcadores de terminal/usage, tamanho agregado e categoria de erro; nunca
conteúdo, prompt, reasoning, corpo remoto ou credenciais.

Eventos de seleção, retry, fallback e observação textual do Planner aparecem na
seção DEV LR-7D3 com provider/model/rota/tentativa e motivo. O texto parcial do
plano não é exibido. Nenhum evento de subtarefa é emitido antes de o Core
compilar o `TaskGraph`.

O teste de integração local executa os adapters de produção Groq e Cloudflare
por HTTP/SSE fragmentado, inclusive bytes UTF-8 divididos, passando pelo
Scheduler, `orchestrator::plan`, parser PlanV1, compilação e despacho de dois
workers. Fixtures locais também cobrem terminais inválidos do Groq, EOF e usage
inválido no Cloudflare, parser estrito PlanV1, resultado estruturado,
cancelamento no primeiro evento e preservação de worker concluído quando a irmã
é cancelada. Os testes usam credenciais sintéticas; o gate humano real de
Orchestrator Groq/Cloudflare e cancelamento em execução continua pendente e este
documento não registra PASS.

## D3C — resultado e provenance

Cada sucesso gera `TaskGraphSubtaskResult` com:

- `subtaskId`;
- provider efetivamente usado;
- texto;
- usage real devolvido pelo Scheduler.

O Luna Core consolida deterministicamente os resultados na ordem do plano.
Nenhuma terceira LLM é usada apenas para “recontar” trabalho interno.

O SQLite persiste atomicamente a Task raiz e metadados das subtarefas:
provider, estado, timestamps e erro. O texto bruto das subtarefas não é
duplicado na tabela de provenance.

Planner usage e Worker usage permanecem separados no resultado.

## Testes adicionados

A suíte candidata cobre, entre outros:

- duas unidades independentes na mesma onda;
- dependência aguardando predecessores;
- capability operacional rejeitada fail-closed;
- falha bloqueando dependentes sem falsificar o estado de irmãos;
- cancelamento sem conclusão fictícia;
- ranking Preferred/Fixed/Auto do Scheduler para distribuição;
- persistência atômica root + provenance;
- dois workers sobrepostos com Groq + Cloudflare sintéticos;
- cancelamento raiz propagado a workers paralelos;
- teto global de chamadas impedindo nova onda;
- falha do Channel com cleanup e precedência correta;
- snapshot de identidade real sem memória/histórico;
- sessão ativa inalterada pela execução.

## Fora de escopo

A D3 não adiciona:

- ferramentas;
- execução de `PlanCapability` operacional;
- Codex/Copilot como executores do task graph;
- diálogo entre agentes;
- retry de grafo;
- redistribuição de uma subtarefa já atribuída;
- queue/token bucket/RPM/TPM/RPD/TPD;
- circuit breaker completo;
- telemetria de custo;
- Luna Voice dedicada à consolidação.

Esses itens permanecem em LR-8 ou fases posteriores.

## Gates antes de PASS

Executar no mínimo:

```bash
npm run typecheck
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --release --manifest-path src-tauri/Cargo.toml
git diff --check main...HEAD
```

Depois dos gates sintéticos, executar o gate humano real:

1. confirmar Orchestrator configurado e Worker com Groq + Cloudflare elegíveis;
2. executar o objetivo controlado da seção DEV · LR-7D3;
3. observar duas subtarefas independentes;
4. confirmar providers distintos em trabalho útil;
5. confirmar resultado único e provenance;
6. repetir com cancelamento durante a onda;
7. confirmar uma única terminal, nenhum worker órfão e sessão/estado íntegros.

Somente depois desses gates, auditoria final e eventuais FIXes a LR-7D3 pode ser
marcada PASS. O PASS da D3 encerra a LR-7 e libera a LR-8.
