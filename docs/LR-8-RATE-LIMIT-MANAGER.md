# LR-8 — Rate Limit Manager completo

Estado: **EM EXECUÇÃO — LR-8A e LR-8B encerradas em PASS; LR-8C liberada.**
Pré-requisito: `main@71c9a650ffc92459811309d7749593d2244b9d46` ou posterior, contendo o fechamento da LR-7D3.
Subfase corrente: **LR-8C — implementação candidata; LR-8D bloqueada**.

## Objetivo

Transformar o Scheduler multi-provider já validado na LR-7 em um runtime capaz de
administrar capacidade real de providers antes, durante e depois das chamadas,
sem depender de descobrir limites apenas por HTTP 429/503.

A LR-8 deve fechar:

- RPM e TPM;
- RPD/TPD quando o provider expuser ou o usuário configurar limites reais;
- concurrency;
- token bucket/admission control;
- fila;
- parsing factual de headers de quota/rate limit;
- `Retry-After`;
- exponential backoff + jitter;
- cooldown;
- circuit breaker;
- budget por tarefa e budget diário;
- telemetria por provider;
- painel operacional por provider.

Nenhuma quota, preço, reset, capacidade ou saúde deve ser inventada. Estado
desconhecido é representado como desconhecido.

## Regra de execução da trilha

Para reduzir FIXes tardias, a LR-8 é dividida em subfases sequenciais:

1. o agente implementa somente a subfase corrente;
2. executa os gates técnicos definidos nela;
3. Luna realiza auditoria independente do diff e dos contratos;
4. achados bloqueantes são corrigidos ainda na mesma subfase;
5. somente após PASS técnico + auditoria a próxima subfase é liberada;
6. gate humano real é exigido quando a subfase altera comportamento operacional
   observável com providers reais.

Uma subfase não pode antecipar contratos pertencentes às seguintes apenas para
"deixar pronto", salvo refatoração estritamente necessária e auditável.

## Princípios arquiteturais

- Luna Core continua sendo a autoridade. Provider, quota header ou modelo não
  decide permissões, identidade, task state ou policy.
- O Rate Limit Manager pertence ao runtime, não aos adapters.
- Adapters somente traduzem fatos específicos do provider para contratos
  genéricos: usage, headers, retry hints e erros tipados.
- O Scheduler consulta estado de capacidade, mas não deve conter regras
  comerciais por marca.
- Fixed/Preferred/Auto e affinity continuam respeitando escolhas explícitas do
  usuário e capabilities reais.
- Sem fallback/retry depois de output parcial, preservando o contrato atual.
- Telemetria não pode registrar prompts, respostas, reasoning, credenciais ou
  headers sensíveis.
- Ausência de metadados do provider não autoriza inferir quota ou custo.
- Custo permanece `unknown` até existir preço configurado/factual; não hardcode
  tabela comercial mutável no Core.
- Accounting medido e accounting conservador permanecem separados.
- Cancellation e channel failure continuam tendo precedência sobre novas
  esperas/filas.

---

## LR-8A — modelo de quota + telemetria factual

### Objetivo

Criar a linguagem interna única para representar capacidade, usage e saúde sem
mudar ainda a decisão operacional do Scheduler.

### Entregas

- contratos genéricos para snapshots de rate limit/quota por provider;
- dimensões independentes para requests, input/output/total tokens quando
  disponíveis, concurrency e limites diários;
- valor conhecido/desconhecido explícito;
- reset/retry timestamps ou durações somente quando observados;
- origem/provenance do dado: provider header/response, configuração do usuário
  ou medição local;
- ledger local de requests/tokens concluídos;
- telemetria in-memory por provider com snapshot seguro para Settings/DEV;
- extensão de `ProviderUsage`/resultado apenas quando necessária, sem quebrar
  o accounting D3;
- contrato de headers normalizado nos adapters que realmente expõem fatos úteis;
- testes para overflow/saturação, valores ausentes, headers inválidos,
  monotonicidade do ledger e isolamento entre providers;
- documento da matriz factual de suporte de cada adapter.

### Não entra em 8A

- bloquear chamadas por RPM/TPM;
- token bucket;
- fila;
- prioridade;
- semaphore/concurrency enforcement;
- circuit breaker;
- jitter/backoff novo;
- mudança de routing/score;
- persistência de janelas de quota;
- UI operacional completa;
- preços inventados.

### Gate

O runtime deve conseguir responder, para cada provider, "o que eu sei e de onde
sei" sobre usage/capacidade sem alterar qual chamada seria executada.

---

## LR-8A — implementação e fechamento (03/10/2026)

**PASS técnico + auditoria independente da Luna em 03/10/2026.**
Branch de implementação: `lr-8a-rate-model-telemetry`, base
`977caacb76f0b867432cbaab8e54b469e6e9ee26`.
Candidata final auditada em `1622b80c6d50a967d22a07eeccab89426b8d7133`.
**LR-8B está liberada.** A 8A permanece deliberadamente observacional: nenhum
admission control, fila, token bucket ou circuit breaker foi ativado.

### Arquitetura e ownership

`cognition/telemetry.rs` contém a autoridade única `TelemetryStore`, pertencente
ao `Scheduler` do `ProviderRuntime` compartilhado no composition root.
Conversation, Summary, Orchestrator e Workers continuam usando esse mesmo
Scheduler; não possuem ledgers de telemetria próprios. Os schedulers mock de
DEV e os fixtures têm stores isolados, como já tinham runtimes isolados.

O store usa um único `Mutex<BTreeMap<provider_id, State>>`, inicializado somente
com todos os IDs registrados. O Registry é a autoridade dos IDs já expostos em
status/configuração; não há filtro exclusivo da telemetria nem desaparecimento
silencioso de providers registrados.
Nenhum update cria providers arbitrários. A leitura clona todos os providers sob
um único lock, com um instante de captura comum; depois libera o lock.
Os updates de usage por amostra são atômicos entre dimensões. Nenhum guard
atravessa `.await`; o store não conhece UI, requests HTTP, credentials ou textos.

`InvocationObservation` é um handle efêmero por tentativa, com deduplicação local
de início e amostras cumulativas. Seu pequeno mutex não é outro estado por
provider: só protege a tentativa e escreve no store central. A ordem de locks
é sempre tentativa → store; snapshots não acessam mutexes de tentativas.
Não há reservations, permits, waiting, fila ou admission.

### Contratos

- `Fact<T>`: `unknown` ou `known { value, provenance, observedAtUnixMs }`.
- `Provenance`: `provider_header`, `provider_response`, `user_configuration`,
  `local_runtime`; nenhuma origem aceita strings remotas arbitrárias.
- `UsageDimension`: requests, input/output/total/thought tokens independentes.
- `UsageCounter`: soma observada, `reportingRequests` e `saturated`.
- `QuotaDimension`: RPM, TPM, RPD, TPD e concurrency; extensível adicionando
  dimensões, sem obrigar adapters a fornecer dados.
- `QuotaScope`: `Provider` ou `Model { model }`, extensível sem campos comerciais.
  `ScopedQuotas` associa um scope a um mapa de dimensões; `quotas` é uma lista
  desses grupos no snapshot JSON (tipo TypeScript atualizado). O scope Provider
  começa desconhecido e nenhum scope Model é promovido a Provider. Scope ausente
  significa unknown, nunca herança automática de outro scope. Cada modelo
  conserva suas próprias observações; os IDs são dos targets locais validados,
  sem account/project IDs remotos.
- `QuotaSnapshot`: limit, remaining e reset independentes, cada um com seu
  próprio estado/origem/timestamp.
- `Timing`: duração `delay_ms` ou timestamp `unix_ms`; nenhuma janela inventada.
- `ProviderTelemetrySnapshot`: provider, captura, idade monotônica da última
  atualização, usage, quotas, último retry hint e último resultado do provider.
- `Outcome`: succeeded ou failed com código já allowlisted de `ProviderError`.
  Isso descreve o transporte/provider, não a saúde, nem o resultado posterior
  da validação de PlanV1/budget pelo Core.

Timestamps de fatos usam UTC epoch ms (null se o relógio não for representável); `updatedAgeMs` usa `Instant` e não depende
de ajustes do relógio de parede. Timestamps são observacionais, nunca relógios
de enforcement. Retry hints são a última observação, não uma ordem de espera nem
promessa de que ainda sejam atuais.

A extensão aditiva `Provider::execute_observed` preserva `execute` e os contratos
`ProviderUsage`, `SchedulerUsage` e `ProviderResponse`. O default apenas delega a
`execute`, sem marcar request ou usage: preflight de um provider futuro não pode
fabricar fatos. Providers/fixtures que desejem telemetria devem instrumentar
explicitamente a fronteira de invocação. Todos os quatro de produção a sobrescrevem.
`execute` direto mantém o caminho original com observação desabilitada; os
consumidores runtime continuam entrando pelo Scheduler, sem caminho alternativo.

### Lifecycle de accounting

1. O Scheduler resolve targets, modo, cooldown, budget e eventos exatamente como
   antes. Uma seleção/evento não incrementa a telemetria.
2. O adapter valida modo/configuração, credencial, payload e construção do
   request HTTP. Erros aqui não registram request remota.
3. Ao efetivamente pollear o ramo de envio HTTP, depois de verificar cancellation,
   registra exatamente um início. URLs rejeitadas antes de HTTP não contam;
   DNS/connect/timeout após início contam como invocação iniciada, sem alegar
   que o servidor recebeu/processou o request.
4. Cada retry/fallback real recebe outro handle. Targets inelegíveis, em cooldown
   ou nunca invocados continuam sem consumo.
5. Amostras normalizadas de usage, já validadas pelos parsers dos adapters, são
   observadas no SSE. Groq non-streaming extrai usage tipado do envelope HTTP
   de sucesso antes de aceitar finish_reason/conteúdo. Gemini extrai usage tipado
   dos estados terminais reconhecidos de interaction.completed antes de retornar
   incomplete/requires_action/cancelled/failed. A validação cognitiva não é
   relaxada: uma rejeição posterior conserva o consumo observado.
   Amostras cumulativas repetidas não são somadas novamente; crescimento debita
   apenas o delta e uma dimensão conta um reporter por tentativa. Valores menores
   subsequentes não retiram consumo observado. O campo `ProviderUsage.calls` não
   incrementa requests (teste usa propositalmente `calls=99`).
6. O Scheduler registra o último resultado do provider. Cancelamento/falha
   iniciado conserva a request; sem usage recebido não inventa tokens.
   Usage observado antes de erro/validação Core permanece factual.

Tokens desconhecidos não viram zero: antes de qualquer amostra a dimensão é
`unknown`. Uma amostra válida de zero é `known(0)`. `reportingRequests` mostra
quantas invocações forneceram cada dimensão; a soma é apenas desse subconjunto,
não estimativa do consumo total das chamadas sem usage. Requests locais começam
em `known(0)` porque o runtime sabe que nenhuma invocação começou desde sua criação.

O budget e `SchedulerUsage.provider_calls` preservam a semântica anterior de
tentativas do Scheduler; requests na telemetria têm a fronteira de envio descrita
acima, inclusive quando uma tentativa falha no preflight local.

Usage e quota não se reconciliam nesta fase. Nenhum saldo é calculado a partir de
usage. `output_tokens_measured` e `output_tokens_accounted` da D3 permanecem
inalterados: a telemetria contém medição, não a reserva conservadora. Não há
contabilização duplicada entre adapter e Scheduler; o Scheduler só registra
resultado, não soma tokens no novo store.

Os contadores do store são inteiros, saturantes em `2^53−1` com flag explícita,
para manter serialização JSON exatamente representável no frontend. Esse é um
limite numérico local, não uma quota comercial. Metadata acima desse bound,
remaining > limit, reset inválido e total reportado menor que input + output
são ignorados. A soma para validar usage amplia u32 para u64 antes de operar.
Parsers existentes rejeitam negativos, frações e valores fora de u32. Updates
normalizados inválidos de quota preservam o último fato válido; ausência num
update válido é `unknown`, nunca default comercial.

### Matriz factual de produção

| Adapter | Requests locais | Usage aceito | Retry hint HTTP | RPM/TPM/RPD/TPD/concurrency, remaining/reset comercial | Custo |
|---|---|---|---|---|---|
| Gemini Interactions | início real de envio | input/output/total e thought opcional em terminal reconhecido, inclusive incomplete, com usage válido | Retry-After opcional | unknown | unknown |
| Groq streaming | início real de envio | input/output/total quando usage válido | Retry-After opcional | RPD e TPM: limit/remaining opcionais por modelo; demais dimensões/reset unknown | unknown |
| Groq JsonSchema non-streaming | início real de envio | input/output/total quando usage válido, mesmo com finish_reason/conteúdo rejeitado | Retry-After opcional | RPD e TPM: limit/remaining opcionais por modelo; demais dimensões/reset unknown | unknown |
| Cloudflare Workers AI | início real de envio | input/output/total opcional; ausência preservada | Retry-After opcional | unknown | unknown |
| Mistral | início real de envio | input/output/total opcional; ausência preservada | Retry-After opcional | unknown | unknown |

O adapter Groq normaliza exclusivamente os seguintes headers opcionais:

| Headers | Dimensão genérica | Scope |
|---|---|---|
| `x-ratelimit-limit-requests`, `x-ratelimit-remaining-requests` | RequestsPerDay (RPD) | Model do target chamado |
| `x-ratelimit-limit-tokens`, `x-ratelimit-remaining-tokens` | TokensPerMinute (TPM) | Model do target chamado |
| `x-ratelimit-reset-requests`, `x-ratelimit-reset-tokens` | reset unknown | Não há parsing de duração |

Fonte oficial: [Groq Rate Limits](https://console.groq.com/docs/rate-limits),
consultada em 03/10/2026. A documentação associa limites à organização e apresenta
limites por modelo; os valores são os headers dinâmicos daquela chamada, nunca
copiados das tabelas de planos. O scope Model restringe a observação ao target
local chamado no contexto de credenciais do runtime. Não afirma quota exclusiva
daquela chave, nem impede compartilhamento remoto entre modelos. Nenhuma quota
model-scoped é usada como provider-wide; nenhum identificador de organização,
conta, projeto ou chave é armazenado para definir scope.

O parser aceita apenas decimal inteiro sem sinal dentro de `2^53−1`. Ausência,
frações, negativos, duplicidade, overflow e texto inválido produzem campo unknown;
remaining > limit invalida o update do par e conserva a última observação válida.
Campos parciais são independentes: limit conhecido não inventa remaining. Resets
ficam unknown porque os exemplos oficiais não definem gramática formal de duração.
Não há logging ou persistência de headers. Gemini/Cloudflare/Mistral mantêm quotas
unknown. `observe_quota` continua aceitando fatos normalizados por scope, sem UI
ou configuração de quota ligada nesta fase.

`Retry-After` permanece opcional, conforme
[RFC 9110, §10.2.3](https://www.rfc-editor.org/rfc/rfc9110.html#name-retry-after),
consultada em 03/10/2026. `factual_retry_after` conserva delay-seconds como
`Timing::DelayMs` (multiplicação checked) e HTTP-date como `Timing::UnixMs`,
inclusive datas passadas representáveis. Fatos maiores que sete dias são
preservados dentro do contrato numérico local. Ausência, sintaxe inválida,
ambiguidades/duplicidades ou overflow ficam unknown. O parser operacional
`retry_after_ms()` continua intacto: a política existente clampa a sete dias e
não usa timestamps/hints factuais para decidir cooldown, espera ou retries.

### Exposição read-only

`Scheduler::telemetry_snapshot()` e a extensão aditiva `telemetry` em
`get_ai_settings` fornecem leitura por provider para Settings/DEV/testes.
`src/settings/providerTelemetry.ts` tipa o contrato sem criar painel novo.
O campo é capturado antes do trabalho bloqueante de Settings; `capturedAtUnixMs`
identifica esse instante. Nenhuma nova permissão/comando Tauri foi necessária.

Cooldown permanece exclusivamente no mapa existente do Scheduler e na exposição
`ProviderStatus.cooldown_ms`. Não foi duplicado no store nem unificado com quota;
essa unificação pertence à 8D. Não há estado `health`, circuit breaker ou custo
monetário no snapshot: sem fonte de preço/configuração, custo continua desconhecido.

### Testes e gates

A suíte `telemetry_tests` inclui isolamento, request único, retry/fallback reais,
provider não chamado, deduplicação cumulativa, ausência de usage, cancellation
antes/depois do início, falha de sink/preflight, unknown e zero factual,
metadata inválida, saturação, snapshot concorrente coerente, routing intacto em
Fixed/Preferred/Auto com quota zero, cooldown, ausência de fallback após output,
ledger conservador D3 e serialização sem conteúdo/headers sensíveis.

Fixtures HTTP locais exercitam os quatro adapters com usage válido, ausente,
negativo, extremo e impossível; credenciais ausentes/build inválido não contam;
cancelamento sincronizado após recebimento HTTP conta sem inventar tokens;
429 válido/inválido mantém cooldown e normaliza provenance. Groq structured
conta uma vez mesmo quando o Core rejeita posteriormente o budget. Retry-After
tem fixtures numéricas, de data com relógio fixo e ambíguas. Os testes existentes
de Planner/Workers permanecem parte da suíte completa. Nenhum teste novo usa
internet ou credencial real.

Gates da candidata inicial no estado auditado `2fc8af5b8c9a972766addc54494c6948889163de`
(histórico; resultados da FIX abaixo):

| Gate | Resultado |
|---|---|
| `npm run typecheck` | Concluído, exit 0 |
| `npm run build` | Concluído, exit 0; aviso de chunk acima de 500 KiB |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Concluído, exit 0 |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | 331 passaram, 0 falharam, 2 ignorados/manual Codex |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | Concluído, exit 0 |
| `git diff --check` e `git diff --check main...HEAD` | Sem erros |

Os 22 testes novos passaram na suíte final. Os 10 testes TaskGraph afetados pela
execução paralela também passaram individualmente (`--exact --test-threads=1`).
A primeira execução paralela terminou com 316 sucessos, 11 falhas e 2 ignorados:
10 timeouts TaskGraph, com operações SecretStore de vários segundos nos logs,
e uma fixture nova que aumentava input sem ajustar total. A fixture foi corrigida
após a validação de total impossível ter sido acrescentada; não era flake.
A classificação dos 10 timeouts como contenção do harness é sustentada pela
reprodução individual e pela suíte serial completa, não presumida pelo nome do
teste. Nenhum timeout do harness foi aumentado para esconder falha.

Na candidata inicial, Rust emitiu 15 warnings no check debug e 41 no release. Quatro correspondem aos
contratos novos ainda sem fonte de produção (`UserConfiguration`, `UnixMs`,
`Timing::valid`, `observe_quota`); os demais são de código existente. Não houve
supressão de warnings. O bundle frontend manteve o aviso acima de 500 KiB.

### Autoauditoria do diff

Revisados os caminhos streaming/non-streaming e os invariantes exigidos:
nenhuma quota/preço comercial hardcoded; nenhuma regra por marca no Core;
unknown sem conversão em zero; início e usage deduplicados; preflight não
contabilizado; nenhum lock atravessando await; nenhuma consulta da telemetria
por routing/score/affinity; ledger medido/conservador D3 preservado; aritmética
checked/saturante e casts bounded; nenhum novo log/header/body sensível;
nenhuma migration/persistência; nenhuma implementação 8B/C/D/E.
A autoauditoria corrigiu a classificação involuntária de erros de build HTTP,
preservando os mappers anteriores, e separou o clamp operacional de Retry-After
da leitura factual. Isso não substitui a auditoria independente da Luna.

### Limitações e decisões adiadas

- In-memory, desde a criação do runtime; restart descarta o ledger. Sem migration.
- Usage local agrega por provider; quotas distinguem provider e modelos, sem
  identidade de conta/projeto nem definição de janelas. Observações são históricas,
  não saldo disponível ou estimativa de cobrança. Trocas de credencial/organização
  no mesmo runtime não invalidam automaticamente fatos históricos: conferir origem
  e timestamp; não reutilizá-los como capacidade atual em futura admission. A
  identidade local de contexto de conta e sua invalidação ficam para desenho futuro.
- Usage só é extraído de envelopes de sucesso/terminais tipados e validados, nunca
  de corpos arbitrários de erro HTTP ou inferido do output. Envelope truncado,
  inválido ou rejeitado pelo guard incremental pode impedir validação independente;
  os guards/limites cognitivos existentes não foram relaxados para colher métricas.
- Sem configuração de quota/preço ligada. Resets Groq permanecem desconhecidos.
- Nenhuma alteração de routing/score/affinity, backoff, policy de cooldown,
  limites de tarefa, identidade/memória/avatar, ferramentas ou executores.
- Admission/fila/concurrency enforcement: 8B; buckets/budgets/janelas/restart
  safety: 8C; unificação cooldown/health/circuit breaker: 8D; painel: 8E.
- Auditoria deve conferir especialmente a fronteira do send nos quatro adapters,
  a extensão aditiva do trait e a distinção entre somas reportadas, reporters,
  requests iniciadas e accounting conservador D3.

### FIX após auditoria de `2fc8af5b8c9a972766addc54494c6948889163de`

**IMPLEMENTAÇÃO CANDIDATA — aguardando reauditoria independente da Luna.**
**LR-8B bloqueada.** FIX restrita aos seis achados: scopes explícitos e isolados;
headers dinâmicos Groq RPD/TPM; Retry-After factual independente do clamp; IDs
registrados sem filtro divergente; default do trait sem instrumentação presumida;
usage válido conservado em rejeições semânticas posteriores Groq/Gemini.

A mudança do formato JSON de `quotas` (mapa único → lista por scope) é necessária
para corrigir o contrato ainda candidato da 8A. O tipo TypeScript foi atualizado;
Settings não renderiza um painel operacional. `SchedulerUsage`, ProviderUsage,
routing, affinity, cooldown, retry, D3 e ownership do store não foram reescritos.
Cloudflare/Mistral só passam a usar o novo retorno factual Timing de Retry-After.
Não há migration, persistence, novo log HTTP nem comportamento 8B+.

Dez testes `lr8a_fix_*` cobrem: dois modelos e scope Provider no mesmo ledger;
ID registrado antes rejeitado; preflight pelo default; delay maior que sete dias,
HTTP-date absoluto e overflow com clamp operacional intacto; dois modelos Groq
no mesmo Scheduler com HTTP real local; headers ausentes/parciais/inconsistentes,
negativos/fracionários/extremos/duplicados; Groq length/tool_calls/conteúdo inválido
com usage válido; usage inválido/corpo de erro sem métricas; Gemini incomplete com
usage válido/inválido; Retry-After numérico/data por HTTP preservando erro/cooldown.
Fixtures de headers também conferem RPD (não RPM), TPM, provenance, reset unknown,
privacidade e ausência de contagem dupla. Os testes anteriores foram adaptados
somente ao contrato de scope e à nova semântica factual de datas.

Gates da FIX sobre o código final (03/10/2026):

| Gate | Resultado |
|---|---|
| `npm run typecheck` | Exit 0 |
| `npm run build` | Exit 0; aviso de chunk acima de 500 KiB |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0; 12 warnings |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Exit 0; 341 passaram, 0 falharam, 2 ignorados; doc-tests sem falhas |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | Exit 0; 38 warnings |
| `git diff --check` | Sem erros |
| `git diff --check main...HEAD` | Sem erros |

Todos os dez testes novos passaram na execução completa final. Os dois ignorados
já existentes são gates Codex locais/manuais (`real_app_server_handshake` e
`manual_final_codex_agent_bridge_gate`), fora da FIX. As duas execuções completas
paralelas passaram sem contenção/falhas de SecretStore; não houve necessidade de
reprodução isolada ou suíte serial nesta FIX. Uma rodada dirigida inicial teve
9 sucessos e 1 falha na fixture que exigia scope existente mesmo quando ambos
os updates inválidos haviam sido ignorados. A asserção foi corrigida: ausência
significa unknown, sem herdar valores de outro scope. Não foi classificada como
flake. A suíte final inclui as validações adicionais de whitespace HTTP, bound
numérico, zeros à esquerda e envelope sem a estrutura tipada esperada.

Warnings Rust são unused/dead-code existentes e o contrato `UserConfiguration`
ainda sem fonte de produção; nenhum warning foi suprimido. A compilação de testes
emite também dois warnings de campos existentes. O aviso de bundle acima de
500 KiB permanece sem redesign do frontend.

Autoauditoria da FIX: scopes não se sobrescrevem; headers mapeados RPD/TPM apenas
no adapter; nenhum número comercial/preço embutido; unknown sem default zero;
parser operacional inalterado; Registry permanece autoridade; default não gera
request; extraction de usage independente não altera rejeição cognitiva;
observação deduplicada; store único; locks síncronos; nenhuma leitura por seleção
ou admission; nenhum header/body/credencial no snapshot. A validação de usage do
terminal completed Gemini conserva a propagação de erro do parser anterior,
inclusive em lote SSE, evitando alteração de chunks/fallback.


### Fechamento auditado da LR-8A

**Resultado: PASS. Integrada à `main` pela PR #14 em 03/10/2026, merge
`3e89fe67a28e31c928ef09e3378ea39fbeab0978`.** A reauditoria independente
confirmou os seis pontos da FIX:
quota com scope explícito provider/model, headers dinâmicos Groq normalizados como
RPD/TPM sem hardcode comercial, Retry-After factual separado do clamp operacional,
Registry como autoridade dos IDs, default conservador de `execute_observed` e
preservação de usage factual em respostas Groq/Gemini posteriormente rejeitadas.

A suíte final reportada pelo agente fechou com **341 testes Rust aprovados, 0
falhas e 2 ignorados/manual Codex**, além de typecheck, build, checks debug/release
e `git diff --check` verdes. O repositório não possui CI/status check associado
ao HEAD desta subfase; a auditoria independente foi estática sobre diff, contratos
e testes presentes no remoto.

#### Dívida formal para LR-8C — geração de contexto de credencial/quota

Os snapshots da 8A são históricos e in-memory. Quotas model-scoped observadas em
headers são válidas no contexto de credenciais/projeto vigente no instante da
chamada, mas a troca de API key, conta, organização ou projeto durante o mesmo
runtime **não invalida automaticamente fatos anteriores**.

Isso não bloqueia 8A nem 8B porque telemetria ainda não governa admission. Porém,
**antes de LR-8C usar quota para bloquear, esperar ou autorizar chamadas**, o
runtime deve impedir que fatos de um contexto de credencial antigo sejam tratados
como capacidade atual. A 8C deve implementar uma estratégia explícita, sem
persistir segredos, como epoch/generation local do contexto de credencial ou
invalidação equivalente, e cobrir rotação/restart com testes determinísticos.

Até essa dívida ser resolvida, nenhuma quota histórica da 8A pode ser promovida
a autoridade de enforcement após mudança de credencial/contexto remoto.

**Próxima etapa oficial: LR-8B — admission control + fila + concurrency.**

---

## LR-8B — admission control + fila + concurrency

### Objetivo

Introduzir uma fronteira única antes de toda chamada cognitiva real.

### Entregas

- admission controller compartilhado por Conversation, Summary, Orchestrator e
  Worker;
- limite de concurrency factual/configurável por provider;
- fila cancelável e bounded;
- semáforo/reserva liberados em todos os caminhos terminais;
- prioridade por classe de trabalho:
  - foreground interativo/explicitamente solicitado;
  - trabalho cognitivo da task raiz/Workers;
  - background oportunista (Summary);
- FIFO estável dentro da mesma classe, salvo razão factual documentada;
- cancellation remove espera sem consumir chamada;
- queue timeout separado de request/stream timeout;
- telemetria de queue depth, active calls e queue delay;
- testes determinísticos de starvation, cancellation, channel failure e leak de
  permit.

### Gate

Sob contenção sintética, foreground progride antes de background, nenhuma
chamada ultrapassa concurrency configurada e nenhuma tarefa cancelada "vaza" da
fila para o provider.

---

## LR-8B — implementação e fechamento (03/10/2026)

**PASS técnico + auditoria independente + gate humano real em 03/10/2026. Integrada à `main` pela PR #15, squash `cb1f0fd30f8cbddcb91e09f87674f2488120ab8e`.**

Base obrigatória:
`main@78b966db5a4f76ff0a41c5fc346cf056d6a3ccf5`, com LR-8A integrada pela PR #14.
Branch de implementação: `lr-8b-admission-control`. Candidata auditada em
`69577a2dd55f8e828b7aa4b4ccd227af1bded583`. **LR-8C está liberada após a
integração desta subfase.**

### Ownership e configuração local

`ProviderRuntime → Scheduler → AdmissionController → estado por provider`.
Todos os consumidores de produção usam o Scheduler compartilhado. O mapa de IDs
é inicializado a partir do Registry, fica imutável e não possui lock global. Cada
provider tem seu próprio `Mutex<State>` e `Notify`; fila de um não impede admission
de outro. O controller não conhece marcas, modelos, prompts, credentials, quota,
routing ou callbacks de adapters. Gemini/Groq/Cloudflare/Mistral não ganharam
semáforos nem prioridade. As mudanças nos arquivos Gemini/Groq são só fixtures de
teste que agora explicitam a classe do request do Scheduler.

`AdmissionConfig::default()` centraliza exclusivamente limites locais da Luna:

| Campo | Default | Significado |
|---|---:|---|
| `max_concurrency_per_provider` | 2 | Tentativas cognitivas simultaneamente admitidas por provider |
| `queue_capacity_per_provider` | 64 | Tickets aguardando por provider; proteção de memória/runtime |
| `queue_timeout_ms` | 60.000 | Timeout exclusivo de espera, iniciado no enqueue monotônico |
| `max_priority_bypasses` | 8 | Máximo de ultrapassagens por prioridade superior |

O concurrency 2 acompanha o paralelismo mínimo existente da LR-7D3; não afirma
nenhum limite comercial de provider. `Scheduler::with_admission_config` permite
contratos menores em testes. Concurrency/timeout zero e números fora do bound
JSON-safe são rejeitados; capacidade de fila zero permite somente admission
imediata, bypasses zero resulta em FIFO entre todas as classes. Sem migration,
persistência, edição na UI ou configuração externa nesta subfase.

### Prioridade e fairness

O Core preenche `ProviderTaskRequest.traffic_class` explicitamente:

- `ForegroundInteractive`: Conversation e diagnóstico solicitado, incluindo Groq probe;
- `ForegroundTask`: Orchestrator/Planner e TaskGraph Workers;
- `Background`: Summary automático/oportunista.

`ProviderRequest` não carrega a classe para os adapters. A fila guarda somente
identificador interno (identidade de `Arc<()>`), classe, contador de bypasses e
`Instant` de entrada; a ordem do `VecDeque` é a ordem de chegada. Não guarda
prompt, resposta, reasoning, contexto, segredo ou headers.

Ao haver vaga, ganha o ticket protegido mais antigo (`bypasses >= limite`); na
ausência de protegido, ganha a maior classe, com a chegada mais antiga como
desempate. Cada admission aumenta o contador dos tickets anteriores de classe
inferior que ultrapassou. Somente ultrapassagens efetivas por admission contam,
nunca chegada/seleção. Tickets anteriores da mesma classe têm contador pelo
menos igual ao dos posteriores: proteção não quebra FIFO da classe. Novos
requests entram na fila se existir qualquer waiter, mesmo com vaga livre.

Após oito ultrapassagens, o ticket antigo ganha a próxima oportunidade elegível
antes de novas ultrapassagens. Tickets protegidos anteriores podem vir primeiro;
a fila é finita e novos tickets não entram à frente deles. Não há weighted
scheduling, aging por wall-clock ou preemption. Uma chamada Background admitida
continua normalmente quando chega Conversation.

### Lifecycle, cancellation e erros

O Scheduler decide o target pelos contratos anteriores, constrói sua invocação
e adquire o permit na fronteira imediatamente anterior a `execute_observed()`.
Emite `Selected → [Queued] → Admitted → invocation`; chamadas imediatas omitem
`Queued` e têm delay zero. `Queued.queue_depth` é o tamanho factual no enqueue,
não promessa de posição futura; `Admitted.queue_delay_ms` mede tempo monotônico.
Os eventos atravessam os canais Core e o diagnóstico Groq sem conteúdo privado.

O slot representa **tentativa cognitiva admitida pelo Scheduler**, incluindo
preflight do adapter; não comprova socket HTTP aberto. O permit não é clonável e
seu Drop libera exatamente um slot. É explicitamente descartado logo após
retorno do Provider, antes de observar resultado/processar resposta, retry,
backoff, fallback ou consolidação Core. RAII também cobre retorno antecipado,
channel failure, cancellation, unwind e descarte do future. Cada retry/fallback
adquire um novo permit, e source/destination não ficam presos simultaneamente.

`AdmissionQueueFull` (`admission_queue_full`) e `AdmissionTimeout`
(`admission_timeout`) são erros locais do Scheduler. Encerram diretamente sem
RateLimited/Unavailable/QuotaExceeded, cooldown, retry ou fallback automático.
Timeout de fila não consome timeout de request/connect/stream. Começa quando o
ticket entra em espera, sem relação com nascimento da task raiz.

Cancellation mantém o `AtomicBool` existente. É checada antes do enqueue, sob
lock antes de admission e de novo pelo Scheduler antes da invocation. Em espera,
a detecção usa polling assíncrono bounded de 25 ms, como o backoff existente,
porque esse contrato não tem wake handle de cancelamento. Release/mudança de fila
acorda por Notify imediatamente. Cancellation observada vence vaga/timeout;
permit concedido não agenda execução futura, e cancellation que vence antes da
fronteira de invocation impede Provider. Cancellation depois dessa fronteira
continua pelos caminhos atuais do adapter, sem preemption por prioridade.

O ticket possui guard RAII: erro de sink, cancellation, timeout ou abort do future
remove a espera e acorda peers. Não há concessão de permit em Drop/release;
apenas o próprio waiter elegível, sob lock, pode remover seu ticket e incrementar
active. `Notified::enable()` precede a checagem de estado para impedir lost wake
na race release/espera. Nenhum lock atravessa await; callbacks e notify ocorrem
fora dos locks. Drop só faz trabalho local, bounded pela capacidade da fila.
`EventSinkClosed` em Queued/Admitted encerra sem invocation e limpa ticket/permit.

### Routing, LR-8A, Summary e TaskGraph

Não há leitura de admission no ranking/score/affinity/cooldown. Fixed espera no
escolhido; Preferred conserva a ordem autorizada; Auto conserva score e affinity.
Saturação e fila não criam fallback. Somente os erros remotos já autorizados
podem fazê-lo. Quota zero e Retry-After factual da 8A não governam admission.

`TelemetryStore` permanece semanticamente intacto. **Queued ≠ request; admitted
≠ request; fronteira real de envio HTTP do adapter = request factual.** Espera
cancelada/timeout/full/sink fechado antes de envio conservam requests zero para
a tentativa. Usage/reportingRequests, quota provider/model, retry hints,
accounting conservador D3 e privacidade de credencial permanecem os anteriores.

Summary conserva `foreground_provider_tasks` como fast-path para não iniciar
trabalho oportunista novo enquanto Conversation está registrada. Não é uma
segunda autoridade de prioridade: uma tentativa Summary que já entrou em
admission segue a fila Background e fairness central, sem cancelamento por
chegada de foreground. Summary já executando também não é interrompido. Falha
local de admission deixa o resumo pending e encerra o drain, aguardando kick
futuro; não faz retry imediato ou fallback por capacidade.

TaskGraph conserva PlanV1, compilação, dependências, no máximo duas subtarefas
independentes, distribuição de targets e consolidação determinística. Com duas
Workers no mesmo provider, cap 1 serializa suas tentativas e cap 2 admite ambas.
Os testes verificam também ordem da consolidação e provenance persistida das
duas subtarefas.

### Snapshot read-only

`Scheduler::admission_snapshot()` e campo aditivo `get_ai_settings.admission`
expõem por provider: ID, max concurrency local, active, queue depth/capacity,
queued por classe (inclusive zeros), admissions, total que entrou em espera,
delay acumulado/recente e amostras, queue full/timeout counts e saturação numérica.
Cada snapshot de provider é coerente sob seu lock; providers são capturados em
sequência, sem alegar captura atômica de todo o runtime. Delay/amostras contam
somente tickets que esperaram e foram admitidos; cancelados/timeout permanecem
em totalWaited, sem amostra de admission. Admissions incluem permit liberado
antes de HTTP por preflight/cancellation/channel failure.

Contadores saturam em `2^53−1` com flag, gauges são bounded pela configuração
validada. O snapshot não tem request, prompt, output, reasoning, contexto,
credential, headers ou quota remota. `providerAdmission.ts` e os eventos
TypeScript tipam leitura; nenhum painel operacional LR-8E foi construído.

### Testes e gates da candidata

24 testes novos: 18 em `admission_tests`, dois internos do controller, dois de
Summary e dois TaskGraph. Usam channels/notifies/oneshots para controlar admission
e completion; waits temporizados possuem deadline de harness. Apenas o teste de
timeout usa espera real curta para demonstrar independência dos timeouts de 1 ms
da invocação. A race cancellation/release executa 40 ciclos em runtime multithread.

Cobertura inclui cap 1/2/terceiro waiter, isolamento por provider, FIFO, ultrapassagem
Interactive/Task sobre Background, nenhuma preemption, bypass limits 1/2,
cancellation queued e após Admitted, abort de espera/permit, queue full/timeout
terminais, todos os códigos de erro Provider, preflight Authentication sem request,
sink fechado em Queued/Admitted, retry readquirindo após backoff, fallback liberando
source, quota zero sem gate, ranking/modes/score/affinity/cooldown preservados,
privacidade, aritmética JSON-safe, integração foreground/Summary e Workers cap 1/2
com consolidação/provenance. A suíte completa inclui os testes LR-7D3 e LR-8A.

Gates sobre o código final (03/10/2026):

| Gate | Resultado |
|---|---|
| `npm run typecheck` | Exit 0 |
| `npm run build` | Exit 0; aviso de chunk acima de 500 KiB |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0; 12 warnings |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Exit 0; 365 aprovados, 0 falhas, 2 ignorados; main/doc-tests sem falhas |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | Exit 0; 38 warnings |
| `git diff --check` | Sem erros |
| `git diff --check main...HEAD` | Sem erros no commit candidato |

Os dois ignorados são gates Codex locais/manuais preexistentes
(`real_app_server_handshake`, `manual_final_codex_agent_bridge_gate`), fora da 8B.
A compilação de testes tem também dois warnings de campos existentes. Warnings
Rust são unused/dead-code preexistentes e `UserConfiguration` observacional da
8A; não houve supressão. O frontend conserva o aviso de bundle acima de 500 KiB.

A primeira execução completa terminou com 364 aprovados, uma falha e dois
ignorados. A falha `fix5_event_coalescing_hundreds_of_chunks_and_scheduler_byte_guard`
foi reproduzida isoladamente: esperava dois eventos e recebeu três por causa do
novo Admitted. A asserção foi adaptada para exigir exatamente
Selected → Admitted → OutputObserved, mantendo coalescing e proteção de bytes.
A reprodução isolada corrigida passou; depois a suíte completa paralela final
passou, incluindo os 24 testes novos, em 205,90 s. Não foi classificada como flake,
nenhum timeout foi aumentado e não foi necessário recorrer à suíte serial.
Fixtures HTTP/SecretStore existentes acima de 60 s concluíram com sucesso.
O gate frontend inicialmente detectou o switch TypeScript que ainda não tinha
os dois eventos aditivos; o consumidor foi completado antes dos gates finais.

### Gate humano real da LR-8B

O gate operacional foi executado com **Groq real** usando Orchestrator e Workers
em `Fixed(groq)` e TaskGraph com duas subtarefas independentes simultâneas. Os dois
Workers ocuparam os dois slots locais do provider. Uma terceira operação
`ForegroundInteractive` pelo **Diagnóstico Groq** foi disparada durante a execução
e permaneceu aguardando enquanto os Workers estavam ativos; o resultado só apareceu
após a liberação dos slots, sem fallback para outro provider.

Também foi validado o cancelamento real de Conversation durante contenção:
`cancel_task` foi aceito pelo runtime, a UI encerrou a resposta como cancelada e
os Workers do TaskGraph continuaram até seus terminais sem serem interrompidos.
Execuções seguintes continuaram funcionais, sem evidência de permit preso.

A diferença visual entre conclusão dos dois Workers foi mínima, o que é compatível
com execução paralela real sob `max_concurrency_per_provider = 2`; o gate não usa
essa diferença temporal como prova de fairness.

Os logs mostraram operações de SecretStore em torno de 1,5–1,8 s e waits maiores
em algumas chamadas. Essa latência pertence ao SecretStore/preflight e não é
interpretada como queue delay do AdmissionController.

**Resultado do gate humano: PASS.**

### Autoauditoria e limitações

Revisão direcionada de locks/await, callbacks sob lock, Drop/Notify/lost wake,
ownership único dos permits, active/depth em terminais, race cancellation,
FIFO/proteção bounded, Summary e ausência de preemption; nenhuma dependência de
quota/score/routing na fronteira local, nenhum request factual por fila/admission,
nenhum permit durante backoff/fallback/consolidação, nenhum dado privado em fila,
snapshot ou novos eventos. Auditoria independente deve conferir especialmente
a linearização cancellation/admission/invocation, fairness entre protegidos,
as amostras de delay e a proteção oportunista do Summary.

Estado in-memory e por ID de provider, compartilhado entre modelos desse ID.
O override de configuração é de construção, sem atualização dinâmica. Cancelamento
na fila não tem notificação própria; polling conserva o contrato existente e pode
ser atrasado por scheduling do executor. Fairness pressupõe progresso do executor
e término/cancellation das chamadas em execução; não interrompe um Provider travado.
O slot inclui preflight e não mede sockets. Nenhum teste novo usa credencial ou
provider comercial real; gate operacional humano continua posterior.

A dívida de invalidação/versionamento do contexto credencial/quota permanece
intacta e pertence à LR-8C antes de quota governar admission. RPM/TPM/RPD/TPD,
buckets/reservations/janelas/budgets diários/persistência ficam para 8C;
jitter/circuit breaker/health e unificação cooldown para 8D; painel/gate final
para 8E. Capacity-aware routing e configuração externa permanecem adiados.
**LR-8C liberada após fechamento/integração da 8B.**

---

## LR-8C — rate accounting + token buckets + budgets

### Objetivo

Evitar chamadas que o próprio runtime já sabe que excederiam uma janela real.

### Entregas

- buckets/janelas independentes para RPM/TPM e, quando conhecidos, RPD/TPD;
- reserva conservadora pré-call e reconciliação pós-usage;
- accounting desconhecido nunca vira crédito fictício;
- integração do ledger conservador da D3 com saldo real/estimado disponível;
- budget diário e por tarefa como contratos distintos;
- escolha entre esperar/falhar/fallback apenas segundo policy explícita;
- resets observados/configurados sem assumir meia-noite local;
- restart safety/persistência somente para estado necessário a não exceder
  limites conhecidos;
- testes de boundary, reset, clock, restart e provider isolation.

### Gate

Com limites sintéticos pequenos, o runtime impede excesso conhecido antes do
HTTP e volta a admitir chamadas após reset válido, sem reduzir artificialmente
providers sem quota conhecida.

---

## LR-8C — implementação candidata

**IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna**

**LR-8D bloqueada.** Branch `lr-8c-rate-accounting`, base obrigatória
`main@0b1a1e821523e6033874b5febdfdfca80feb1024`. Esta implementação não fecha
PASS nem libera gate humano, LR-8D ou LR-8E.

### Ownership e fronteiras

`ProviderRuntime → Scheduler → RateLimitManager`. O composition root constrói o
Scheduler de produção com o Database compartilhado e conecta o observer de
contexto ao SecretStore antes de iniciar Summary/expor o runtime. Conversation,
Summary, Orchestrator e Workers passam pela mesma autoridade. Schedulers de
testes/DEV continuam isolados e podem usar armazenamento efêmero explicitamente.

`cognition/rate.rs` mantém uma única autoridade com operações síncronas sob
mutex. Nenhum bucket fica em adapter. O Scheduler preserva integralmente
Fixed/Preferred/Auto, ordem autorizada, scores, affinity e cooldown existente.
O fluxo é seleção/routing → reservation → admission → execução → reconciliation
→ processamento de resposta/retry/fallback. O gate inicial não adquire permit
quando bloqueia. Não há espera por reset, sleep novo, refill comercial presumido,
capacity-aware routing, jitter ou circuit breaker.

`TelemetryStore` continua sendo ledger factual da LR-8A. Updates normalizados
de quota alimentam o RateLimitManager sob o lock do store. Usage, requests e
outcomes não são recalculados nem apagados por rate accounting. O ledger medido
e conservador D3, TaskBudget e SchedulerUsage não foram reescritos.

### Constraints externas versus policy local

As quatro dimensões RPM/TPM/RPD/TPD combinam constraints Provider e Model do
target: todas precisam permitir a reserva. Model A nunca governa Model B e
nenhum scope Model é promovido a Provider. Concurrency continua na LR-8B.

`ConstraintSource` distingue `ExternalFact`, `LocalPolicy` e `DailyBudget`.
ExternalFact aceita somente provenance ProviderHeader/ProviderResponse da 8A;
telemetria com UserConfiguration/LocalRuntime não configura uma quota externa.
Policy local usa o novo contrato `RatePolicy`, nunca headers nem valores de
planos. Ausência de config significa nenhum limite local nessa dimensão.
Um daily budget não preenche quotas remotas na telemetria.

Remaining factual fornece o teto externo; limit isolado fornece somente um teto
do consumo local desde o fato, sem alegar conhecer remaining comercial anterior.
Sem nenhum dos dois, a dimensão permanece não enforceable. `unknown != zero`
e `unknown != unlimited`: unknown é falta de fato suficiente, não um número.
Nada é inventado para Gemini, Cloudflare, Mistral ou futuros providers.

Cada novo remaining pode substituir a evidência anterior. Respostas concorrentes
são tratadas conservadoramente: o manager conserva débitos vivos, limita crédito
quando existem outras chamadas iniciadas e descarta observações de tentativas
anteriores a uma observação já aceita de tentativa posterior. Header ausente ou
limit isolado não prova refill de um teto já esgotado. O snapshot de rate pode
portanto conservar um fato utilizável anterior enquanto o snapshot observacional
de telemetry mostra a última resposta parcial/unknown. Provenance do campo
retido acompanha a constraint. `capacity` é um teto interno conservador derivado
da evidência e dos débitos vivos, não outra quota comercial reportada. Requests da
própria resposta com remaining não
são debitadas novamente: seu início precede o fato de remaining. Tokens podem
continuar sendo gerados após headers e mantêm a reserva conservadora.

Groq continua fornecendo RPD e TPM por modelo. Seus resets permanecem unknown:
zero bloqueia requests subsequentes naquele scope até evidência nova suficiente
ou invalidação legítima. Não há minuto alinhado, rolling window, refill contínuo,
meia-noite UTC/local nem quota comercial hardcoded.

### Context generation e credenciais

`TelemetryStore::invalidate_provider_quotas` muda a geração opaca local e retorna
quotas/retry hints daquele provider a unknown. Preserva usage factual, requests,
outcomes e outros providers; também preserva todos os budgets/limites locais.
Snapshots expõem apenas o contador JSON-safe de geração. Nenhuma credencial,
fingerprint/hash, account/project ID ou header raw acompanha essa geração.
Esgotamento numérico da geração encerra novas tentativas com erro local, em vez
de reutilizar uma era antiga.

`CredentialContextObserver` recebe somente os tipos fixos de SecretKey. Toda
mutação durável que altera valores pelo SecretStore (set/delete, individual ou batch) notifica antes
de liberar o lock da operação do vault. Isso cobre os comandos Gemini/Groq/
Mistral e Cloudflare, inclusive mudança somente de token ou somente de Account ID.
Uma atualização Cloudflare combinada produz uma única geração. Falha antes da
gravação durável não invalida. Se a gravação ocorreu e um check posterior de
permissões falhar, a invalidação já ocorreu. O observer usa Weak, sem ciclo de
ownership Scheduler → adapter → SecretStore → Scheduler.
Regravar uma credencial idêntica não é rotação e não apaga quota esgotada. A
comparação efêmera ocorre exclusivamente dentro da operação do vault, sem
fingerprint, hash, valor ou identificador derivado enviado ao observer/persistência.

O Scheduler também invalida o cooldown remoto legado dessa era. Publicação de
cooldown verifica geração sob a mesma ordem de locks: um 429/503 tardio da era
antiga não volta a impor cooldown à credencial nova. A política de cooldown,
seus prazos e regras de retry/fallback permanecem os anteriores; sua unificação
ao manager e políticas temporais novas continuam na LR-8D.

InvocationObservation captura a geração antes da reserva. Mudança de contexto
durante fila/preflight impede o início HTTP daquela reservation. Headers/retry
hints tardios de uma chamada na era antiga são ignorados; usage/outcomes dessa
chamada continuam factuais. Uma chamada nova começa com quota remota unknown.
Mudanças feitas externamente no arquivo de vault durante o mesmo processo não
têm um watcher; a API do SecretStore é a fronteira autorizada de mutação.

### Reservations, cancellation e reconciliation

O check e a criação da `RateReservation` são atômicos sob o mesmo mutex, sem
check-then-act. Requests reservam exatamente uma unidade por constraint aplicável.
Tokens reservam o upper bound explícito quando existir. A classe/prioridade da
8B não entra na chave de quota: foreground e background compartilham capacidade.

O guard RAII possui o rollback/reconcile; InvocationObservation recebe somente
um handle não proprietário. `started()` reutiliza a fronteira HTTP instrumentada
pela 8A (`started_unless_cancelled`) e retorna um sinal que os quatro adapters
devem honrar. O início transforma
reservas em débitos; ele não é inferido de Selected ou AdmissionPermit.
Há revalidação na fronteira de HTTP se quota/contexto/reset mudou durante fila:
uma falha local libera o permit e o Scheduler devolve o erro local tipado.
O retorno interno do adapter nesse caso não é publicado como erro remoto/outcome.

Drop devolve integralmente reservas sem HTTP em cancellation, queue full/timeout,
EventSinkClosed, preflight, unwind ou abort do future. Após HTTP, request permanece
consumida em sucesso, erro, cancellation ou abort. `final_usage` declara um total
terminal normalizado/validado; só esse total pode devolver excesso de tokens.
Samples cumulativos comuns continuam no ledger factual e podem aumentar débito,
mas não provam crédito devolvível. Ausência de usage conserva o bound integral.
Usage maior que reserva aumenta débito de forma saturante, sem underflow/wrap.
Esse aumento é imediato e durável quando há policy local, impedindo outra
reservation de usar capacidade que o runtime já sabe ter sido consumida.
Um débito saturado não recebe refund cuja precisão não possa ser provada.

Epochs internas das constraints impedem refund em janela/contexto posterior.
Reservations puramente locais atravessando um reset são transferidas à janela
nova e revalidadas; chamadas iniciadas antes da boundary pertencem à janela de
início. Não se transfere seu refund para a janela nova. Isso é semântica de
accounting local, não uma afirmação sobre o momento de billing de um provider.

Cancellation é verificada sob lock antes da reserva, pela admission e novamente
antes de invocation/envio, inclusive ao concluir o commit local síncrono de
accounting. Cancellation que vence durante essa etapa devolve o débito à
reservation pendente, sem request factual; seu Drop faz o rollback durável.
Quando observada antes de HTTP, vence; após o início,
consumo já factual permanece. Nenhuma reservation de retry futuro é segurada no
backoff. Cada retry real passa pelos dois gates novamente. Fallback remoto já
autorizado libera source e reserva somente destination; bloqueio local de rate
encerra diretamente, sem fallback/retry/cooldown remoto.

### Token upper bounds e precisão

`TokenUpperBound::explicit_total` é um contrato numérico de prova para aquela
invocação completa, obtido pelo hook genérico `Provider::token_upper_bound`.
Inclui entrada, protocolo/schema/overhead e toda saída contabilizável, inclusive
reasoning quando aplicável. O default é None. Bound não é uma estimativa de score
Auto, nem uma conversão de context bytes em tokens.

Os quatro adapters atuais não têm tokenizer/contagem pré-call capaz de provar
esse teto para todos os modelos/thinking/protocolos que aceitam. Por isso mantêm
None: **TPM/TPD e daily token budget não fazem enforcement pré-HTTP nessas
invocações sem prova**, mesmo que exista um saldo observado. Usage total factual
continua debitando após a chamada; chamadas sem bound e sem total definitivo
incrementam `unaccountedTokenCalls`, nunca são apresentadas como zero conhecido.
`effectiveRemaining` fica unknown enquanto há consumo de tokens incompleto,
inclusive em uma chamada ativa sem bound/total terminal. `capacity` conserva o
teto factual anterior como constraint, sem alegar saldo utilizável conhecido.
Um novo remaining factual substitui a incerteza das chamadas já encerradas;
um reset válido também inicia uma nova janela de accounting.
RPM/RPD/daily requests continuam enforceable normalmente. Fixtures com prova
explícita exercitam enforcement e reconciliation das dimensões de tokens.

Não se soma input + output para fabricar total quando o provider omite total ou
há reasoning não incluído nessas parcelas. Tokens da quota e do daily budget
são total tokens; input/output/thought continuam independentes no ledger 8A.
Não há preço, custo financeiro ou alteração retroativa do ledger D3.

### Resets, janelas locais e clock

`RateClock` injeta leitura monotônica e UTC opcional. SystemRateClock usa Instant
para progresso no mesmo processo. DelayMs vira deadline checked a partir da
observação; UnixMs vira delay uma única vez usando UTC representável e depois
deadline monotônica. Timestamp passado reseta imediatamente. Timestamp inválido,
wall-clock ausente para UnixMs ou overflow não inventam refill. Um reset externo
é one-shot: restaura limit conhecido; se limit continuar desconhecido, a capacidade
volta a unknown, sem fabricar quantidade. Não é uma janela recorrente presumida.

`FixedWindow { periodMs, anchorUnixMs }` implementa a alternativa determinística
a token bucket permitida nesta fase: refill integral somente nas boundaries
declaradas. A semântica está na configuração, não no nome RPM/RPD. Pode, por
exemplo, haver uma constraint RPD com janela curta sintética nos testes. Depois
da inicialização, boundaries avançam pelo clock monotônico. A aritmética de
deadline/end/epoch é checked antes de limpar consumo; overflow é erro local.

`DailyBudgetPolicy` contém anchor UTC explícita, max requests opcional e max
accounted total tokens opcional, com período declarado de 86.400.000 ms.
Sem campo configurado, não há budget naquela dimensão. Não usa timezone local,
TaskBudget, preços ou quota comercial. Modificar capacity na mesma janela/scope
preserva consumo. Alterar/remover a semântica da policy é mudança explícita de
configuração; mudanças com reservations vivas retornam RatePolicyBusy.
Não se reconstitui retrospectivamente consumo anterior à ativação de uma policy
quando o runtime não possui ledger temporal suficiente para prová-lo.

### Persistência e restart safety

Migration 012 / schema 12 cria `cognitive_rate_state`: apenas provider ID local,
policy explícita (incluindo model ID quando configurado), deadline UTC, débitos,
saturação e incomplete-accounting counts das janelas locais. O formato persistido
é separado do snapshot e não contém remote remaining/headers/reset/context,
credenciais, fingerprints, prompts, respostas ou reasoning.

Reservation local faz write-ahead do débito conservador antes de admission. O
início HTTP confirma o débito na janela real, inclusive se a fila cruzou boundary.
Rollback/refund gracioso atualiza o estado durável. Crash com reservation pendente
conserva o débito; pode gastar capacidade local sem ter enviado HTTP, em favor
de restart safety. Não se alega request factual por esse débito conservador.
Falha de armazenamento impede envio ou futuras reservas daquele provider; refund
não gravado nunca cria crédito no estado em disco. State malformado impede
startup de produção com RateStateUnavailable.

Restart carrega exclusivamente policy/janelas locais. Quotas remotas e geração
anterior não são reutilizadas, pois não há prova de identidade do contexto remoto.
UTC mapeia o deadline persistido ao novo clock monotônico. Relógio de parede
recuando não apaga consumo antes do deadline salvo. Este contrato pressupõe uma
instância ativa do runtime; não é um coordenador entre processos simultâneos.
Nenhum registro novo tenta identificar remotamente chave, conta ou organização.

### Locks, Settings e erros

Ordem aninhada: operação do vault → cooldown legado (quando aplicável) →
observação/telemetry → rate → SQLite.
Na leitura de usage/quota, observação → telemetry → rate. A confirmação de início
usa observação → rate, solta rate e só depois escreve telemetry. Nenhum caminho
rate/SQLite chama telemetry/vault; admission não aninha seus locks com os demais.
Callbacks de UI e awaits ficam fora dos guards. I/O SQLite é síncrono e bounded
pelo busy timeout existente; pode atrasar trabalho enquanto há policy local
durável. Não há espera por reset segurando AdmissionPermit.

`get_ai_settings.rate` / `Scheduler::rate_snapshot` expõem constraints/scopes,
source/provenance, capacidade known/unknown, consumed/reserved/effective remaining,
reset, geração, policy/daily budget, pending reservations, blocks, saturação e
falha de persistência. Snapshots são coerentes sob o mutex do manager e registram
captura UTC opcional. Não contêm conteúdo privado ou HTTP metadata raw.
Tipos TypeScript foram acrescentados, sem painel 8E.

`update_provider_rate_policy` é API explícita de configuração, com persistência
e validação central, restrita à capability settings-ai. Nenhum default comercial
é ativado. A UI atual ainda não cria controles para editar essa policy.

Novos códigos locais: RateCapacityExceeded (`rate_capacity_exceeded`),
DailyBudgetExceeded (`daily_budget_exceeded`), RateContextChanged,
RateStateUnavailable, InvalidRatePolicy e RatePolicyBusy. São distintos de 429,
QuotaExceeded remoto, AdmissionQueueFull/Timeout e BudgetExceeded por TaskBudget.
Summary mantém erro local transitório pending e encerra drain até kick posterior,
sem retry imediato; prioridade/fairness da 8B permanece na admission queue.

### Testes, gates e autoauditoria

38 testes novos em `rate_tests.rs` usam clock falso, barriers, channels e guards.
Cobrem unknown/zero/capacidade nas quatro dimensões; provider/model/isolation;
oversubscription concorrente de requests e tokens; rollback/cancellation/abort,
inclusive cancellation que chega durante o commit local antes do sinal HTTP;
queue full/timeout/sink/preflight; HTTP sucesso/erro; retry e ausência de reserva
no backoff; fallback independente; bound presente/ausente; total menor/igual/
maior/unknown/cumulativo parcial; saturação; boundaries/past/invalid/overflow;
sem refill desconhecido; rotação efetiva pelo SecretStore e Cloudflare token/
account/batch/no-op; histórico preservado e cooldown da era antiga invalidado;
restart local/remoto/write-ahead/UTC;
falhas de storage; TaskBudget separado; routing/ranking/cooldown/fallback intactos;
provenance separada e privacidade dos snapshots/SQLite.

Os testes 8A/8B de quota zero foram adaptados somente para o enforcement agora
exigido: o ranking/score/affinity continuam iguais e a tentativa selecionada termina
localmente, antes de admission/HTTP. Assertions de schema passam de 11 para 12.
Fairness/cap da 8B, TaskGraph D3 e fixtures dos quatro adapters continuam na suíte.
Um teste adicional de regressão em `telemetry_tests.rs` verifica a leitura completa
do body HTTP separado dos headers na fixture Groq de dois modelos.
Nenhum timeout foi aumentado. Nenhuma credencial/provider real foi usado.

Gates técnicos da candidata (04/10/2026):

| Gate | Resultado |
|---|---|
| `npm run typecheck` | Exit 0 |
| `npm run build` | Exit 0; chunk de 666,22 kB acima do aviso de 500 kB |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0; 15 warnings |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Execuções paralelas intermediárias falharam conforme registro abaixo; final serial autorizada pelo plano de gates |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | Exit 0; 404 aprovados, zero falhas, dois ignorados; 670,74 s; main/doc-tests sem falhas |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | Exit 0; 41 warnings |
| `git diff --check` | Sem erros |
| `git diff --check main...HEAD` | Sem erros no commit candidato |

Os dois ignorados são gates Codex locais/manuais preexistentes:
`real_app_server_handshake` e `manual_final_codex_agent_bridge_gate`.
A compilação de testes registra dois warnings existentes. Os warnings Rust
continuam unused/dead-code, sem supressões: incluem APIs de teste/compatibilidade
`ProviderRuntime::new` e `InvocationObservation::started`, e o construtor de prova
`TokenUpperBound::explicit_total`, ainda não utilizado pelos adapters de produção.
O aviso de tamanho de bundle do frontend permanece. Nenhum gate humano real ou
provider comercial foi executado nesta subfase.

As execuções completas paralelas intermediárias reproduziram timeouts no
harness TaskGraph de 10 s. A primeira terminou com 386 aprovados, 12 falhas e
dois ignorados: 11 timeouts e uma assertion de schema 11 corrigida para 12.
A repetição terminou com 389 aprovados, 12 timeouts TaskGraph e dois ignorados.
O grupo TaskGraph isolado passou em paralelo: 13 testes, 74,23 s. Os logs das
falhas completas registraram unlock de Stronghold em torno de 4,4–6,3 s por
operação durante contenção, antes de múltiplos preflights da mesma task.
Isso motivou a validação completa serial, mantendo todos os timeouts originais.
Não se classificou a falha como flake nem se aumentou timeout.

Uma execução serial intermediária terminou com 402 aprovados, uma falha e dois
ignorados em 696,61 s. A falha foi o teste de headers Groq de dois modelos,
com Provider(Unavailable); sua execução isolada passou. A investigação encontrou
uma fragilidade real na fixture HTTP: ela fechava o socket após ler somente os
headers, deixando o body eventualmente pendente. Isso pode produzir reset TCP.
A reprodução determinística com headers/body em segmentos separados falhou com
o leitor antigo, mostrando que todo o body ficava sem leitura, e passou após
consumir o Content-Length completo. O teste HTTP original corrigido também
passou isoladamente. A última execução serial inclui essa correção, o novo teste
de fixture e os refinamentos finais de cancellation/snapshot.

Após a mudança para remaining efetivo unknown em accounting incompleto, uma
asserção do teste de saturação ainda esperava zero. A reprodução isolada falhou
deterministicamente com None versus Some(0). A expectativa foi corrigida,
preservando débito MAX_FACT_VALUE, flag de saturação, ausência de refund mesmo
com total terminal menor e bloqueio de uma nova reservation com bound. O teste
isolado corrigido passou antes da execução completa final.

Autoauditoria direcionada: unknown não vira zero; remote remaining nunca é
persistido; mutações duráveis invalidam contexto; quotas tardias não reentram;
reserva atômica; RAII em terminais e abort; nenhum crédito por ausência/prefixo;
request somente no início HTTP e preservada após erro; retry/fallback liberam
guards antes da próxima tentativa; nenhum gate muda routing ou cria cooldown;
locks sem await/ciclo; scope de modelo isolado; reset exclusivamente factual/
configurado; UTC explícito; nenhum hardcode comercial, header raw ou segredo novo
em logs, snapshots ou SQLite. A auditoria corrigiu o refund inseguro de prefixos
e a limpeza de consumo antes de validar overflow de janela; também impediu usar
uma regravação idêntica de credencial como falsa invalidação de contexto.
Débito adicional de usage que excede o bound também passou a ocorrer na
observação, antes de qualquer reservation concorrente, sem aguardar completion.
Cancellation passou a ser rechecado ao concluir o commit local, antes de marcar
request factual. Cooldown remoto legado também foi separado por era, impedindo
que uma resposta tardia reaplique fatos do contexto anterior.
Snapshots de tokens com accounting incompleto passaram a expor remaining
efetivo unknown, preservando separadamente a constraint factual anterior.

Pontos para auditoria independente: semântica conservadora de headers concorrentes
e partial facts; fronteira started/rotação/cancellation; revalidação após fila;
assertion de total terminal nos adapters; epochs de refund; write-ahead/recovery
SQLite; limites da prova de tokens ausente em produção; custo de I/O síncrono;
pressuposto de uma instância ativa; distinção entre débito local e ledger factual.

## LR-8D — backoff, jitter, cooldown e circuit breaker

### Objetivo

Transformar falhas transitórias em estado operacional controlado, evitando
rajadas e insistência inútil.

### Entregas

- exponential backoff com jitter bounded e testável;
- preservação de `Retry-After` como sinal mais forte quando válido;
- cooldown integrado ao Rate Limit Manager;
- circuit breaker por provider com estados explícitos
  `closed/open/half-open`;
- thresholds configuráveis e defaults conservadores;
- sucessos/falhas usados somente nas classes de erro elegíveis;
- probing half-open bounded;
- nenhum circuit breaker por erro de autenticação/input do usuário;
- interação definida com Fixed/Preferred/Auto sem fallback oculto;
- telemetria factual de reason/transition;
- testes com relógio controlado.

### Gate

Sequências sintéticas e gate real controlado mostram que um provider degradado
é temporariamente retirado apenas pela regra configurada, pode ser sondado e
recupera-se sem tempestade de retries.

---

## LR-8E — painel operacional + integração final

### Objetivo

Tornar o Rate Limit Manager observável e fechar a LR-8 em cenário real.

### Painel por provider

Mostrar, quando factual:

- enabled;
- health;
- circuit state;
- active calls/concurrency;
- queue depth;
- requests;
- tokens;
- quota known/unknown por dimensão;
- reset/retry/cooldown;
- uso recente;
- custo known/unknown;
- origem dos limites relevantes.

O painel não deve mostrar precisão falsa nem derivar "saúde" de uma única
métrica sem contrato explícito.

### Integração/gate final

Executar cenário real com pelo menos dois Cognitive Providers independentes:

- workload concorrente;
- foreground + background;
- fila observável;
- provider com capacidade/limite reduzido ou condição real equivalente;
- fallback/espera segundo policy;
- cancelamento durante queue e durante request;
- TaskGraph preservando provenance;
- restart sem ultrapassar limite persistido conhecido;
- nenhuma regressão em Conversation streaming, Orchestrator e Worker.

Fechamento exige gates Rust/frontend, auditoria independente e gate humano.

---

## Decisões já tomadas para iniciar 8A

Nenhuma decisão de produto adicional é necessária para iniciar a LR-8A.

Para 8A:

- telemetria é factual e não normativa;
- nenhuma chamada será bloqueada por ela ainda;
- quota ausente permanece `unknown`;
- limites comerciais não serão hardcoded;
- custo ausente permanece `unknown`;
- contratos novos devem ser provider-agnostic;
- persistência de janelas e política de espera pertencem às subfases posteriores.

Qualquer decisão nova descoberta durante a implementação que altere experiência
do usuário, prioridade entre trabalhos, persistência ou política de gasto deve
ser registrada para decisão antes de ser ativada.

## Sequência

`LR-8A → auditoria → LR-8B → auditoria → LR-8C → auditoria → LR-8D → auditoria → LR-8E → auditoria + gate humano → LR-8 PASS`.
