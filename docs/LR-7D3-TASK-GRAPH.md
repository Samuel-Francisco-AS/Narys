# LR-7D3 — Task graph mínimo + subtarefas independentes

Estado: **PASS completo em 03/10/2026; LR-7D3 e LR-7 encerradas.**
Branch de implementação/fechamento: `lr-7d3-task-graph`.
Base original: `main@b447836ab84224cada5cf2e689d7aab9cf1f45ab`.

A implementação passou revisão independente, gates locais e gate humano real
com dois Cognitive Providers independentes. O fechamento desta branch integra
a LR-7D3 à `main` e libera oficialmente a LR-8 — Rate Limit Manager completo.

## Fechamento — gate humano real de 03/10/2026

A Task raiz `166` executou o objetivo controlado de duas análises cognitivas
independentes sobre cache local. O Orchestrator estava `Fixed` em Groq
`openai/gpt-oss-20b`, thinking `low`; o Worker estava `Preferred` com
Groq seguido de Cloudflare `@cf/zai-org/glm-4.7-flash`, retry 1,
`max_provider_calls=4` e output global configurado em **8192 tokens**.

Evidência observada na execução real:

- Planner Groq: 1 chamada, 171 tokens de output medidos;
- `PlanV1` validado e compilado em TaskGraph pelo Luna Core;
- exatamente duas subtarefas independentes liberadas na mesma onda;
- `step-1 → groq` e `step-2 → cloudflare`;
- ambos os Workers concluíram trabalho útil sem fallback cruzado;
- Worker aggregate: 2 chamadas e 2414 tokens de output medidos;
- Cloudflare terminou com `done=true`, `finish=stop`,
  `usage_present=true`, `usage_valid=true`, `content_bytes=1942`;
- o Core produziu uma única consolidação determinística;
- a Task raiz terminou em `completed`.

A persistência foi verificada diretamente no SQLite. `task_records` contém
`task_id=166`, `kind=task_graph`, `state=completed`, iniciada em
`2026-10-03T03:40:07.923Z` e concluída em `2026-10-03T03:40:54.726Z`.
`task_subtask_records` contém:

| Subtask | Provider | Estado | started_at | finished_at |
|---|---|---|---|---|
| `step-1` | `groq` | `completed` | `2026-10-03T03:40:15.423Z` | `2026-10-03T03:40:20.379Z` |
| `step-2` | `cloudflare` | `completed` | `2026-10-03T03:40:15.423Z` | `2026-10-03T03:40:54.726Z` |

O mesmo `started_at` das duas unidades confirma a onda paralela também na
provenance persistida. O `finished_at` da raiz coincide com o término da última
unidade.

### Observação operacional de budget

O seed da migration 011 permanece em `max_output_tokens=4096`; o gate real
mostrou que esse valor pode ser insuficiente para o GLM-4.7-Flash quando o
accounting conservador reserva output entre duas subtarefas e possíveis retries.
Com 4096 globais, o Cloudflare chegou a `finish=length` com
`content_bytes=0`; `reasoning_effort=low` sozinho não eliminou a condição.
Ao configurar o Worker com **8192 tokens globais**, a mesma tarefa concluiu sem
retry Cloudflare e com output útil medido.

Esse resultado não transforma 8192 em constante universal. A policy continua
configurável; calibração de budget, quota e accounting dinâmico pertence à LR-8.
Para repetir especificamente o gate validado da D3 com este modelo e retry 1,
usar 8192 como referência operacional.

### Gates técnicos finais

Após FIX-7, o teste focado do payload Cloudflare passou e a suíte completa Rust
foi executada serialmente para evitar contenção conhecida do SecretStore:
**309 passaram, 0 falharam, 2 foram ignorados/manual-only**. `cargo check`,
`cargo check --release` e `git diff --check main...HEAD` passaram. Os três
testes TaskGraph que haviam expirado sob execução paralela passaram isolados e
na suíte serial; o comportamento foi classificado como flake de harness por
contenção do SecretStore, não regressão funcional. Typecheck/build já estavam
verdes antes das FIXes exclusivamente Rust/docs posteriores.

Cancelamento raiz, propagação a Workers, terminal único, channel failure e
preservação de sessão/identidade permanecem cobertos pela suíte automatizada
final. O gate humano de fechamento comprovou especificamente planejamento real,
paralelismo multi-provider, trabalho útil, consolidação e provenance persistida.

**LR-7D3 = PASS completo. LR-7 = PASS completo. Próxima etapa: LR-8.**

## FIX-7 — reasoning mínimo explícito no GLM-4.7-Flash

Implementação candidata de 02/10/2026, após novo gate humano real da FIX-6.
A FIX-6 removeu a rejeição por terminal duplicado, expondo a próxima condição
real do Worker Cloudflare:

`phase=terminal done=true finish=length usage_present=true usage_valid=true content_bytes=0 error=incomplete`.

O TaskGraph reserva o budget global de Worker entre subtarefas e o Scheduler
conservador ainda reparte a reserva de cada unidade entre tentativas possíveis.
No gate observado, o GLM encerrou por `length` sem emitir conteúdo textual.
Como `@cf/zai-org/glm-4.7-flash` é um modelo de reasoning e a documentação
atual da Cloudflare declara `reasoning_effort` com `low`, `medium` e
`high`, esta FIX envia explicitamente `reasoning_effort: "low"` somente
para esse target conhecido.

A mudança é model-specific no adapter Cloudflare. Outros modelos Cloudflare
continuam sem campo de reasoning inventado e preservam o default do provider.
Não há alteração em Scheduler, divisão de budget, retry, routing, TaskGraph,
capabilities, Worker envelope ou Conversation contract. O objetivo desta FIX
é testar a causa mais estreita antes de alterar o accounting global.

Teste local de payload exige `reasoning_effort="low"` para
`@cf/zai-org/glm-4.7-flash`, preserva streaming/usage e confirma ausência do
campo para um target Cloudflare arbitrário. Historicamente, o primeiro gate após
esta FIX ainda encerrou em `length` sem conteúdo com Worker 4096; o fechamento
posterior com Worker 8192 está registrado acima.

**Esta FIX isoladamente não declarou PASS; o PASS final veio do gate de fechamento.**

## FIX-6 — tolerância idempotente de terminal Cloudflare

Implementação candidata de 02/10/2026, após gate humano real da FIX-5. O gate
comprovou Planner Groq estruturado, compilação do TaskGraph, despacho paralelo
de dois Workers e conclusão do Worker Groq. O Worker Cloudflare falhou antes da
validação do envelope cognitivo com diagnóstico sanitizado:

`phase=terminal done=false finish=duplicate usage_present=true usage_valid=true content_bytes=0 error=duplicate_finish`.

A correção é deliberadamente estreita no adapter Cloudflare: um
`finish_reason` repetido com **o mesmo valor** é tratado como terminal
idempotente e não altera estado, usage, conteúdo ou decisão final. Um terminal
posterior conflitante continua retornando `provider_protocol_error`. Assim,
`stop → stop` preserva `stop`, enquanto `stop → length` ou
`stop → tool_calls` falha fechado. `length → length` e
`tool_calls → tool_calls` continuam chegando às classificações já existentes
(`provider_incomplete` e `provider_requires_action`) em vez de serem
promovidos a sucesso.

A regra não altera Scheduler, budgets, routing, TaskGraph, Worker envelope,
capabilities ou streaming de outros providers. Teste HTTP local reproduz a
sequência observada de terminal repetido no chunk posterior de usage e confirma
sucesso somente quando a duplicata é idêntica; um terminal conflitante continua
rejeitado.

A documentação atual da Cloudflare registra historicamente uma correção para
streaming que deveria impedir `finish_reason` duplicado no endpoint
OpenAI-compatible; apesar disso, o gate real de 02/10/2026 observou novamente
essa condição com `@cf/zai-org/glm-4.7-flash`. A compatibilidade local é,
portanto, defensiva e restrita à repetição semanticamente idempotente.

**Naquele checkpoint nenhum PASS novo foi declarado.** Os gates posteriores e
o fechamento final estão registrados no início deste documento.

## FIX-5 — Structured Planner Invocation & Runtime Diagnostics

Implementação candidata registrada em 02/10/2026. Naquele checkpoint LR-7D3,
LR-7 e o novo gate humano ainda estavam pendentes; o fechamento final posterior
está registrado no início deste documento.

### Contrato e compatibilidade

`ProviderTaskRequest` e `ProviderRequest` carregam `InvocationMode`: formato
`Text` ou `JsonSchema`, transporte `Streaming` ou `NonStreaming`. O contrato
estruturado carrega diretamente `agents::planner::output_schema()` e
`MAX_PLAN_BYTES`; não existe um segundo schema PlanV1. Orchestrator standalone
e TaskGraph exigem JSON Schema estrito non-streaming. O schema e a instrução
são dados confiáveis do Core; o objetivo permanece exclusivamente no input
não confiável. Respostas continuam não confiáveis até `PlanV1::parse/validate`
e, para D3, `TaskGraph::compile`. A instrução Worker continua estática; IDs
continuam machine-safe e nenhuma ferramenta/permissão foi habilitada.

O método `Provider::supports_invocation(target, mode)` pertence ao adapter.
O Scheduler consulta esse método e as capabilities antes de seleção, eventos
de execução e consumo de chamadas. A declaração global do Registry é a união
dos modos implementados, não uma promessa sobre todos os modelos do provider.
Não há model discovery remoto nem decisões por marca no Orchestrator/Scheduler.

| Integração/target | Texto streaming | JSON Schema estrito non-streaming |
|---|---|---|
| Groq `openai/gpt-oss-20b` | Sim | Sim, implementado nesta FIX |
| Outros modelos Groq | Caminho textual existente | Não anunciado nesta FIX |
| Cloudflare `@cf/zai-org/glm-4.7-flash` | Sim | Não comprovado; inelegível |
| Gemini/Mistral, adapters atuais | Caminho textual existente | Não implementado nesta FIX |

Groq envia `response_format.type=json_schema`, `json_schema.strict=true`,
`json_schema.name=PlanV1`, o schema existente e `stream=false`. Não envia
`stream_options` nesse modo. O limite de tokens e o thinking continuam vindo
da policy. Conversation, Summary e Workers preservam texto/SSE; nenhum
parâmetro Groq é enviado aos demais adapters. `None` de thinking significa
omissão do parâmetro, não raciocínio desativado.

A documentação Cloudflare expõe `response_format` no modelo, mas a documentação
JSON Mode não lista GLM-4.7-Flash, não promete aderência estrita e não suporta
streaming nesse modo. Nenhuma compatibilidade equivalente à Groq foi inventada.
Cloudflare continua utilizável nos Workers e na Conversation. Uma rota
Preferred/Auto ignora targets incompatíveis e escolhe os compatíveis, sem
consumir chamada ou registrar fallback fictício; uma rota Fixed incompatível,
ou sem target compatível, termina com `provider_mode_unsupported`. Preflight
continua exigindo registro/configuração/credenciais segundo a policy existente.
Uma policy antiga Fixed Gemini/Cloudflare precisa ser configurada explicitamente
com um target compatível para o Planner; a FIX não reescreve policies do usuário.

### Accounting e progresso

O Planner usa o ledger conservador: cada tentativa recebe uma parcela do saldo
pelas chamadas ainda possíveis. Falhas sem usage debitam a reserva inteira;
respostas sem usage também debitam sua reserva. Output medido não é inventado:
`output_tokens` registra somente medição recebida,
`output_tokens_accounted` é o ledger e `output_tokens_measured=false` quando
uma tentativa tem consumo desconhecido. Usage que excede a reserva da tentativa
falha fechado. Retry antes de fallback permanece inalterado; com duas chamadas,
um retry ainda pode esgotar o budget antes de um fallback.

Na resposta non-streaming, um guard incremental acompanha os bytes UTF-8
decodificados de `choices[0].message.content`, inclusive escapes JSON e pares
surrogate, e interrompe a leitura assim que superar `MAX_PLAN_BYTES`.
O envelope HTTP também é limitado a `6 * max_bytes + 64 KiB`, permitindo
escapes e metadados finitos. O guard só limita tamanho: não repara JSON,
não extrai respostas alternativas e não substitui o parser estrito final.
O Scheduler aplica defesa adicional antes de acumular chunks/resultados.
Excesso produz `provider_output_limit_exceeded` e não inicia Workers.

`SchedulerEvent::OutputObserved` representa um fato por tentativa, sem texto.
O Planner coalesce chunks; uma resposta non-streaming aceita pelo adapter
produz uma observação sem simular streaming. Seleção, retry e fallback
continuam factuais. O streaming normal dos demais papéis não foi convertido.

### Diagnostics sanitizados

Groq/Cloudflare em DEV distinguem `connect_timeout`,
`request_overall_timeout`, `stream_idle_timeout`, `http_408` e `http_504`.
Metadados: provider, modelo sanitizado, attempt, timeout configurado, duração e
status allowlisted. Nenhum prompt, output, reasoning, corpo remoto, credencial
ou header sensível é registrado. O erro público permanece `timeout`.
O timeout HTTP inclui leitura do corpo; idle é espera por fragmento HTTP depois
dos headers, não espera por conteúdo textual útil. Non-streaming usa timeout
total e cancelamento, sem aplicar idle de SSE. A presença de usage Cloudflare
é capturada antes de `unwrap_or_default()`.

Fixtures locais verificam payload/schema, respostas válidas e inválidas,
fronteira do TaskGraph, inelegibilidade Cloudflare, uso de dois providers Worker,
retry/accounting, expiração HTTP real (inclusive TLS de conexão), tamanho
incremental/UTF-8 e centenas de chunks coalescidos. Nenhum teste automático
novo depende da internet ou de credenciais reais.

Fontes oficiais consultadas em 02/10/2026:

- [Groq Structured Outputs](https://console.groq.com/docs/structured-outputs)
  — modo estrito do GPT-OSS 20B e incompatibilidade com streaming.
- [Groq API Reference](https://console.groq.com/docs/api-reference)
  — Chat Completions, formato, usage e reasoning.
- [Cloudflare GLM-4.7-Flash](https://developers.cloudflare.com/workers-ai/models/glm-4.7-flash/).
- [Cloudflare JSON Mode](https://developers.cloudflare.com/workers-ai/features/json-mode/).
- [Cloudflare API OpenAI-compatible](https://developers.cloudflare.com/workers-ai/configuration/open-ai-compatibility/).

### Gates locais FIX-5 — 02/10/2026

| Gate | Resultado |
|---|---|
| `npm run typecheck` | PASS |
| `npm run build` | PASS |
| `cargo check --manifest-path src-tauri/Cargo.toml` | PASS |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 310 testes: 308 passaram, 0 falharam, 2 ignorados/manual-only |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | PASS |
| `git diff --check` e `git diff --check main...HEAD` | Sem erros |

Execução direta: `cargo test --manifest-path src-tauri/Cargo.toml fix5_ -- --test-threads=4`
passou os 13 testes novos (structured Groq, capability/mode, fases HTTP,
accounting, byte limit, coalescimento e Planner inválido sem Workers).
O teste de fases inclui também non-streaming antes/depois dos headers e timeout
durante leitura do corpo de erro Cloudflare. A execução direta de
`cognition::task_graph_runtime_tests::production_` passou os três cenários de
integração com adapters reais e HTTP local, incluindo dois providers Worker.
Fixtures sintéticas existentes foram adaptadas ao contrato explícito; somente
backends de teste anunciam schema genérico para exercitar routing/fallback.
Os builds emitiram avisos de código não utilizado e bundle acima de 500 KiB,
sem falhar. Nenhuma chamada externa foi usada como teste automático.

A autoauditoria do diff confirmou schema único, gates de modo antes de HTTP,
ausência de parâmetros Groq nos demais adapters, input não confiável separado,
diagnostics sem conteúdo remoto, budgets de retry limitados e streaming dos
outros papéis preservado. Isso não substitui a revisão independente nem o gate
humano. O teto local interrompe a leitura; não comprova cancelamento de computação
ou cobrança no serviço remoto.

### Gate humano planejado na FIX-5 (registro histórico)

1. Revisar esta FIX e configurar Orchestrator Fixed Groq `openai/gpt-oss-20b`,
   thinking low, 4096 tokens, 2 calls, retry 1; manter timeouts 45000/15000 ms.
2. Configurar Worker Preferred Groq + Cloudflare, 4096 tokens e 4 calls.
3. Executar o objetivo controlado de duas análises independentes de cache local.
4. Exigir plano validado/compilado, exatamente dois Workers, providers distintos,
   terminal único, provenance persistida, medição/ledger identificados e no máximo
   uma observação do Planner por tentativa.
5. Executar Orchestrator standalone; depois testar rota Auto/Preferred com
   Cloudflare antes de Groq: nenhum request Planner Cloudflare deve ocorrer.
6. Revalidar conversa streaming Groq/Cloudflare, envelope Worker, cancelamento
   e preservação de sessão/identidade. Em qualquer falha, registrar somente
   metadados sanitizados; não capturar saída bruta.

## FIX-4 — alinhamento do contrato executável D3

O Orchestrator genérico continua usando o contrato PlanV1 consolidado da D1.
Somente a entrada usada pelo TaskGraph D3 acrescenta, em instrução interna
confiável do Core, as restrições que o consumidor realmente exige: `step.id`
machine-safe (ASCII alfanumérico, `_` ou `-`, até 64 bytes) e
`requiredCapabilities` não vazio limitado a `planning` ou
`structured_output`. O objetivo do usuário continua separado e não pode
alterar essas regras.

O diagnóstico DEV de retry também distingue agora bloqueio por `call_budget`,
`output_budget` e `retry_limit`, sem alterar a decisão operacional do
Scheduler. Esta FIX não declara PASS; gates locais e gate humano real continuam
pendentes.

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
passo do `PlanV1`; ela não promove automaticamente um target a suporte nativo.
Na FIX-5, somente o modo comprovado Groq GPT-OSS 20B anuncia saída estruturada
estrita, verificada por target/mode. Cloudflare continua somente text streaming.
Essa capability do Planner é distinta do envelope cognitivo do Worker. Na D3, `StructuredOutput` exige um envelope
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
- `max_output_tokens=4096` como seed histórico da migration 011;
- retry 1, backoff 750 ms;

O gate humano final foi validado com a policy Worker explicitamente ajustada
para `max_output_tokens=8192`; consulte a observação operacional do fechamento.
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
Scheduler, `orchestrator::plan_task_graph`, parser PlanV1, compilação e despacho de dois
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
