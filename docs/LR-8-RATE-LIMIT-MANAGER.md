# LR-8 — Rate Limit Manager completo

Estado: **PASS COMPLETO — LR-8A, LR-8B, LR-8C, LR-8D e LR-8E encerradas.**
Pré-requisito: `main@71c9a650ffc92459811309d7749593d2244b9d46` ou posterior, contendo o fechamento da LR-7D3.
Subfase corrente: **LR-8E — PASS técnico + auditoria independente + gate final aprovado em 05/10/2026.**
**LR-8 encerrada em PASS completo; autorizada para integração à `main`.**

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

**IMPLEMENTAÇÃO CANDIDATA — aguardando reauditoria independente da Luna**

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

Um remaining de uma autorização HTTP posterior realmente isolada pode substituir
a evidência anterior. Respostas de calls que participaram de overlap conservam
essa restrição mesmo depois do término das peers: o manager mantém a origem do
grupo no Attempt, limita o primeiro fato ao crédito anterior e trata os seguintes
como tightening-only. Uma nova call iniciada sem peers HTTP ativas não herda o
grupo. Ordem local de HTTP start não prova ordem de processamento no provider.
A ordem é uma sequência monotônica atribuída em `started`, depois da admission;
o ID da reservation serve somente para ownership/correlação. Headers de uma
autorização anterior, ou repetidos da mesma autorização, podem apenas reduzir o
teto vigente, nunca restaurar crédito, limpar uncertainty ou alterar reset/epoch.
Header ausente ou limit isolado não prova refill de um teto já esgotado. A regra
detalhada e sua proteção contra refunds futuros estão na FIX de ordering abaixo.
O snapshot de rate pode
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
mantêm `unaccountedTokenCalls` durável desde o commit anterior ao HTTP, nunca
são apresentadas como zero conhecido.
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

Gates técnicos da candidata original, anterior à FIX (04/10/2026):

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


### FIX pós-auditoria — uncertainty durável e prova de terminalidade

FIX sobre o HEAD auditado `e12d40ddf6bee0a3634ecd0e3e3d229f3b33ec19`, na
mesma branch. **IMPLEMENTAÇÃO CANDIDATA — aguardando reauditoria independente
da Luna**. **LR-8D bloqueada.** Os resultados da candidata original acima são
histórico; os gates desta FIX são registrados separadamente abaixo.

#### Modelo durável e lifecycle

Cada charge possui ownership de um marcador `uncertain` por bucket/epoch.
`reserve(None)` continua sem fabricar um débito numérico de tokens. Em
`started`, porém, cada constraint token aplicável sem bound recebe exatamente
um marcador `unaccounted` **antes** do commit SQLite e antes do sinal que permite
o transporte. Esse marcador usa o campo persistido já existente da migration
012, sem persistir dados da chamada nem exigir migration nova. O marcador
representa consumo ainda não reconciliado; não afirma uma quantidade zero.
A request conserva a instrumentação factual original da 8A.

Falha desse commit proíbe HTTP e restaura em RAM o estado anterior completo,
inclusive saturation e ownership. Cancellation é rechecado após o commit e
antes da autorização de transporte. Se venceu, o guard volta a ser puramente
local e seu Drop remove o marcador duravelmente. Crash entre commit e transporte
pode deixar um falso positivo conservador. Preflight sem `started` nunca cria
marcador. Drop de uma chamada iniciada sem total definitivo preserva o marcador
já salvo, sem incrementar duas vezes. Reabrir o DB enquanto o guard original
está vivo prova a proteção sem depender de `finish`/Drop executado na morte.

Usage cumulativo aumenta o débito factual imediatamente, sem remover marcador
nem refundar. Um `final_usage` validado, não regressivo e protocolarmente
comprovado persiste débito e remoção apenas do próprio marcador na mesma
escrita. Isso inclui total zero e total igual ao último sample; duplicação não
remove marcadores de outras chamadas. O guard devolve excesso de bound apenas
na reconciliation final. Uso maior que bound aumenta débito sem wrap.
Saturação conserva o marcador quando não há precisão segura para removê-lo.
Epoch antiga não debita/refunda/remove marcador da janela nova.

Para chamadas **com bound**, a persistência acrescenta ainda um marcador de
recuperação enquanto a chamada estiver iniciada sem total definitivo. Em RAM,
a prova explícita permite concorrência e enforcement pelo bound. Após crash,
sem o proprietário vivo, o marcador vira uncertainty. Essa proteção adicional
impede que violação do bound seguida de falha ao gravar consumo maior recupere
crédito conhecido no restart. Pode restringir conservadoramente uma janela
após crash mesmo quando o bound original era válido. O snapshot do processo
ativo distingue isso: somente uncertainty real sem bound esconde seu saldo;
o estado recuperado não possui prova de reconciliation dessas chamadas.

#### Admission em accounting unresolved

Snapshot e gate usam o mesmo contador por constraint/epoch: `unaccounted > 0`
torna `effectiveRemaining` unknown. Uma tentativa com bound, que exigiria
enforcement de uma constraint token de capacidade conhecida nesse estado,
termina com **RateStateUnavailable**, antes de admission. A mesma regra é
revalidada em `started` para reservations feitas antes de uma outra chamada
sem bound tornar o saldo desconhecido. Não se afirma DailyBudgetExceeded nem
se usa `capacity - consumed - reserved` como crédito nesse caso. Capacidade
numérica continua visível como teto anterior, não como saldo utilizável.

Tentativa sem bound continua sem enforcement pré-call daquela dimensão, pois
não existe prova de quantidade reservável. Constraints de requests continuam
normais; ausência de constraint ou quota remota unknown não bloqueia provider.
Unknown não vira zero/unlimited. Em LocalPolicy/DailyBudget, só reconciliation
das chamadas ainda reconciliáveis ou reset configurado legítimo recupera esse
conhecimento; rotação de credencial não apaga uncertainty local, nem budgets.
Mudança explícita da semântica/remover policy continua sendo configuração
autorizada, não consequência da rotação. Na mesma janela, reconfigurar capacity
preserva o marcador.

ExternalFact não vai ao SQLite. Fresh remaining normalizado da era corrente
pode substituir uncertainty de chamadas encerradas. Limit isolado/header ausente
não pode fazê-lo. Chamadas sem bound ainda em curso mantêm seu marcador mesmo
com fresh headers, pois tokens podem ser gerados depois deles. Reset factual
válido ou boundary configurada inicia nova epoch; validação checked ocorre
antes de limpar marcadores. Não há refill presumido nem espera automática.

#### Failure paths e restart

Falhas de write-ahead e `started` são locais e impedem transporte. Qualquer
falha de escrita em usage maior, reconciliation definitiva ou Drop ativa
`persistenceFailed` e impede novas chamadas desse provider. Se a remoção do
marcador não puder ser persistida, ele é restaurado em RAM, conservando o maior
débito factual já observado. O último estado durável conserva débito/uncertainty
seguro, mesmo que seja mais restritivo que RAM. Uma escrita posterior completa
bem-sucedida pode salvar conhecimento mais recente; o latch de falha continua
até reconfiguração explícita bem-sucedida pelo Core. Restart lê somente estados
locais e não reutiliza remaining remoto. Este contrato continua supondo uma
única instância ativa; abertura paralela nos testes simula estado após morte,
não oferece coordenação multi-processo.

Dados persistidos continuam apenas policy, provider/model IDs de configuração,
deadline UTC, débito, saturation e contadores opacos. Não há secret, hash de
credencial, Account ID, header raw, prompt, output ou reasoning novo. Ordem de
locks, RAII, Scheduler/Admission/Telemetry, prioridade/fairness, routing/Auto/
affinity, TaskBudget, measured/accounted D3 e retry/fallback não mudam.

#### Matriz de terminalidade e fontes oficiais

Fontes consultadas em 04/10/2026. Fixtures verificam condições do parser; não
estabelecem garantias comerciais/protocolares por si mesmas.

| Adapter / protocolo | `usage` observacional | `final_usage` autorizado | Evidência / condição |
|---|---|---|---|
| Gemini Interactions SSE | Fatos normalizados da invocação | Sim, usage validado de `interaction.completed` | [Streaming interactions](https://ai.google.dev/gemini-api/docs/streaming#interactioncompleted) define esse evento como fim da interação com estatísticas finais. Não se promove `step.stop` nem `[DONE]`. |
| Cloudflare Workers AI Chat Completions SSE | Qualquer usage validado | Somente chunk final comprovado | [Changelog de 17/02/2026](https://developers.cloudflare.com/workers-ai/changelog/#2026-02-17) vincula finish_reason ao usage chunk final. Parser exige no mesmo envelope choices de um único index 0, delta vazio, finish_reason stop/length/tool_calls e usage completo consistente; `[DONE]` deve seguir sem outro evento JSON. |
| Groq Chat Completions não-streaming | Usage validado da resposta | Sim, objeto final completo validado | [API reference](https://console.groq.com/docs/api-reference#chat-create) define usage da completion request na resposta completa. Envelope precisa ter uma choice/message válida, finish_reason reconhecido e não conter error. |
| Groq Chat Completions SSE | Sim, inclusive total cumulativo | **Não** | [API reference](https://console.groq.com/docs/api-reference#chat-create), [guia de text generation](https://console.groq.com/docs/text-chat) e [tipo oficial ChatCompletionChunk](https://github.com/groq/groq-python/blob/main/src/groq/types/chat/chat_completion_chunk.py) não fornecem garantia explícita suficiente de total terminal num sample específico. `[DONE]` só prova terminação do stream. |
| Mistral Chat Completions SSE | Sim, inclusive usage habilitado no stream | **Não** | [Referência oficial Chat](https://docs.mistral.ai/api/endpoint/chat) descreve progressão parcial e `[DONE]`, sem garantia explícita de total final para o sample aceito pelo adapter. Conservadoramente, include_usage não autoriza refund. |

A [configuração oficial Cloudflare](https://developers.cloudflare.com/workers-ai/configuration/open-ai-compatibility/)
confirma o endpoint `/accounts/{account_id}/ai/v1/chat/completions` usado pelo
adapter. A condição escolhida é deliberadamente estrita: usage com choices vazio,
finish_reason em outro chunk, delta de conteúdo/tool, index diferente, reason
unknown ou JSON posterior continua apenas observacional. Um novo formato não
comprovado não recebe refund. `[DONE]` sem o envelope final comprovado nunca
promove usage anterior a definitivo.

Limitação: todos os adapters de produção continuam `TokenUpperBound=None`.
Groq/Mistral SSE encerrados normalmente podem deixar daily token budget unknown
até reset legítimo, mesmo que o ledger factual mostre total observado. Tokens
medidos/D3 não são alterados para esconder essa limitação. Não há tokenizer,
estimativa otimista, quota comercial default, fallback por rate block ou LR-8D.

#### Testes e gates da FIX

Foram preservados os 39 testes da candidata original: 38 em `rate_tests.rs`
e a regressão HTTP em `telemetry_tests.rs`. Nenhum foi removido. A assertion
original de saturação agora espera RateStateUnavailable, pois o saldo está
unknown, mantendo todas as verificações de débito/saturação/ausência de refund.

19 testes adicionais: 13 no manager e seis nos adapters. Cobrem crash com guard
vivo sem bound; total definitivo zero/menor/maior persistido antes de Drop;
término sem usage ou com prefixo; rollback/preflight/cancellation, inclusive
cancellation durante o commit durável; bloqueio de saldo stale em reserve e no
início de uma reservation queued; fresh remaining/reset factual; uncertainty
ativa após fresh headers; rotação/local uncertainty/epoch velha; reset exatamente
na boundary e overflow sem limpeza antecipada; storage failures em write-ahead,
started, prefixo, final total/zero e Drop; bound violado + write failure/restart;
marcadores múltiplos, ausência de double debit/refund; fixtures de terminalidade
nos quatro adapters e ausência de refund/promover por `[DONE]`. A fixture Groq
não-streaming também exige finish_reason reconhecido; envelope incompleto
permanece apenas observacional. Clock falso, barriers/channels da candidata e
triggers SQLite determinísticos substituem sleeps para essas verificações.

| Gate da FIX | Resultado |
|---|---|
| `npm run typecheck` | Exit 0 |
| `npm run build` | Exit 0; aviso de chunk 666,22 kB preservado |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0; 15 warnings preexistentes |
| `cargo test --manifest-path src-tauri/Cargo.toml rate_tests` | Exit 0; 51 aprovados em paralelo, zero falhas; 32,97 s |
| Fixtures `rate_fix_` | Seis aprovadas em paralelo; a suíte global também inclui o refinamento final do envelope Groq |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Exit 0; 423 aprovados, zero falhas, dois ignorados; 205,37 s; main/doc-tests sem falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml task_graph_runtime_tests` | Exit 0; 13 aprovados em paralelo, zero falhas; 58,42 s |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | Exit 0; 423 aprovados, zero falhas, dois ignorados; 697,11 s; main/doc-tests sem falhas |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | Exit 0; 41 warnings preexistentes |
| `git diff --check` | Sem erros na revisão final |
| `git diff --check main...HEAD` | Sem erros; verificado também no commit da FIX |

A paralela global desta FIX não repetiu os timeouts TaskGraph anteriores.
Nenhum timeout foi aumentado. Dois testes Codex manuais continuam ignorados;
test compilation mantém dois warnings preexistentes. Não houve tráfego com
credenciais/providers comerciais reais.

Durante desenvolvimento, uma execução dirigida intermediária aprovou os 49
testes de biblioteca então existentes, mas o launcher de main retornou
Permission denied enquanto outra compilação relinkava o mesmo binário de teste.
Execuções posteriores sem compilação concorrente passaram biblioteca e main,
inclusive os gates direcionados/global. Isso não foi classificado como flake
nem usado para dispensar gate. A assertion de saturação com o antigo erro local
foi reproduzida e atualizada à semântica unknown exigida pela FIX.

Autoauditoria pós-FIX: marker durável antes do sinal HTTP; nenhuma recovery de
budget cheio após crash unbounded; snapshot e gate concordam sobre unknown;
marcadores sobrevivem restart e término incompleto; somente total definitivo
comprovado remove ownership; falhas de storage conservam marker/debito e latch;
Drop não incrementa nem refunda duas vezes; old epoch não limpa janela nova;
reset validado antes de limpeza; credential rotation não remove locais;
remaining remoto nunca entra na estrutura persistida; `[DONE]` não promove
sample anterior; sources oficiais sustentam os três protocolos autorizados,
Groq/Mistral SSE são somente observacionais. Diff não modifica Scheduler,
AdmissionController, TelemetryStore, policies/routing/Auto/affinity/TaskBudget,
TaskGraph/Workers, credenciais, migrations ou tipos de frontend. Regressões
8A/8B/D3 permanecem cobertas pela suíte completa.

Pontos para reauditoria da Luna: conservadorismo de recovery markers também
para bounds conhecidos; semântica de permitir invocações sem bound sem fabricar
enforcement token; condição Cloudflare deliberadamente estrita e sensível a
mudanças protocolares; windows/epoch e remoção de um único marcador; latch de
persistenceFailed e recuperação explícita; nenhuma coordination multi-processo.
Status continua candidato; nenhum gate humano ou LR-8D foi liberado.

### FIX residual — ordering de observations concorrentes

FIX sobre o HEAD auditado `d0b813e6e1424267a0d02d786c6614738edec417`,
exclusivamente na branch `lr-8c-rate-accounting`.
**IMPLEMENTAÇÃO CANDIDATA — aguardando reauditoria independente da Luna**.
**LR-8D bloqueada.** Sem merge nem alteração da FIX anterior.

#### Autoridade de ordering

`HttpStartSequence` é uma sequência monotônica local por provider, atribuída sob
o mutex do RateLimitManager somente após `started` completar accounting durável
e a última verificação de cancellation. A sequência identifica a autorização
efetiva de transporte na fronteira factual instrumentada pela 8A, após admission.
Tentativas canceladas/falhas antes dessa autorização não recebem sequência;
repetir `started` não cria outra. Overflow falha com RateStateUnavailable antes
de permitir HTTP. Reservation ID permanece apenas chave de lookup e proprietário
dos charges, sem comparação numérica para ordenar fatos.

Cada constraint ExternalFact Provider/Model mantém sua `observation_floor`.
A geração de credencial é verificada antes da correlação/ordering. Somente uma
invocação ainda viva e autorizada pode publicar headers correlacionados. Um fato
normalizado independente de uma invocação estabelece uma barreira na sequência
de início corrente: respostas das chamadas já iniciadas não podem apagá-lo.
Sequências/barreiras são efêmeras, não expostas como identificadores em snapshots
e não persistidas. Restart continua carregando somente estado local; quota
remota volta a unknown. Rotation invalida os fatos/barreiras externos e preserva
budget/uncertainty local. Nenhum wall-clock participa do desempate.

Sequência maior que a barreira, sem histórico de overlap restringindo aquele
scope, segue a reconciliação factual existente: remaining pode substituir a
evidência anterior e conserva todos os charges vivos. A FIX de overlap histórico
abaixo estende a restrição mesmo após remoção das peers. Requests já refletidas
no próprio remaining não são debitadas
duas vezes; tokens conservam seu bound/consumo factual, pois podem ser gerados
depois dos headers. Partial headers não provam refill nem limpam uncertainty.
Só reset factual normalizado válido/configuração explícita inicia nova janela.

Sequência menor ou igual **não é prova de que o valor remoto seja antigo**:
o runtime não conhece a ordem de processamento no provider nem consumo externo.
Portanto esse fato ainda pode apertar um teto conhecido, mas não aumentá-lo.
Mantêm-se consumed, charges, reservation ownership, epoch, reset, barreira e
uncertainty. O fato só pode reduzir capacity, com provenance do campo utilizado.
Não se converte capacidade unknown em zero ou em crédito utilizável.

Para um teto recebido `C = remaining`, ou `limit` quando remaining é ausente,
a atualização conservadora é:

`capacity = min(capacity anterior, consumed não reembolsável + C)`.

Em requests, consumo iniciado não é reembolsável. Em tokens, desconta-se de
consumed todo excesso ainda potencialmente reembolsável dos charges iniciados
da epoch corrente: `max(charge - usage factual já observado, 0)`. Essa parcela
usa a regra anterior de usage validado/não regressivo. Assim, até depois de todos
os refunds terminais comprovados e rollbacks de reservations pendentes, o crédito
não ultrapassa o teto atrasado mais restritivo. A atualização não repete nenhum
débito nem executa refund. Um total terminal continua devolvendo somente o excesso
provado, pelo guard proprietário e na epoch correta. Fato atrasado com reset
imediato não pode acelerar a janela nem aplicar novamente DelayMs.

Isso não depende de ordem de completion do Tokio: a sequência ordena autorizações
HTTP, e fatos que não podem substituir a evidência corrente só podem restringi-la.
Overlaps podem subutilizar capacidade de propósito; não se alega reconstruir o
saldo comercial a partir dos débitos da Luna. Uma nova observação elegível ou
reset legítimo recupera conhecimento conforme os contratos já existentes.

#### Regressões e autoauditoria

Cinco novos testes determinísticos preservam os 58 adicionados à LR-8C anterior:

- Scheduler + AdmissionController real, cap por provider = 1: reservation #1
  Background entra primeiro na fila, reservation #2 ForegroundInteractive
  ultrapassa, inicia HTTP primeiro e publica RPD=9/TPM=90; depois #1 inicia e
  publica RPD=4/TPM=40. O resultado respeita 4/40 e o débito token conservador.
- Scheduler real com cap = 2 para overlap: chamada Background iniciada antes
  responde depois da ForegroundInteractive. Header atrasado maior não restaura
  saldo; header menor restringe. Consumo permanece igual, reset imediato antigo
  não troca o deadline aceito e Drop não debita duas vezes.
- Unitário de reservations invertidas, barreira de fato independente e rejeição
  de observação sem proprietário vivo.
- Unitário de partial header repetido: não refilla, não aplica reset e não limpa
  uncertainty; bound futuro retorna RateStateUnavailable e scopes ficam isolados.
- Unitário TPM/TPD de teto atrasado mais restritivo seguido por final_usage menor:
  o refund comprovado ocorre uma vez e não ressuscita crédito além desse teto.

Os cenários usam clock falso e channels com acknowledgements, sem sleeps ou
inferência da ordem de completion. Com os novos testes e o manager do HEAD
auditado, quatro das cinco regressões falharam deterministicamente, incluindo
o cap = 1 que manteve capacity 9 em vez de 4; o teste de partial header passou.
O manager corrigido foi restaurado antes dos gates finais.
Não se aumentou timeout. Autoauditoria:
reservation ID não é relógio factual; overtaking não descarta saldo menor;
resposta anterior nunca aumenta crédito/reset/epoch; saldo restritivo permanece
seguro após refunds; geração antiga é rejeitada antes da comparação; scopes não
se misturam; schema 12, persistence/recovery markers, terminal usage, routing,
Auto/affinity, admission/fairness, TaskBudget/D3 e retry/fallback estão preservados.
Diff restrito ao manager, seus testes e esta documentação. Nenhum secret, Account
ID, header raw, prompt ou conteúdo de resposta novo é persistido/exposto.

#### Gates da FIX residual

Resultados sobre o código final (04/10/2026):

| Gate | Resultado |
|---|---|
| `npm run typecheck` | Exit 0 |
| `npm run build` | Exit 0; aviso preexistente de chunk 666,22 kB |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0; 15 warnings preexistentes |
| `cargo test --manifest-path src-tauri/Cargo.toml rate_tests` | Exit 0; 56 aprovados em paralelo, zero falhas; 32,73 s |
| `cargo test --manifest-path src-tauri/Cargo.toml task_graph_runtime_tests` | Exit 0; 13 aprovados em paralelo, zero falhas; 49,31 s |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Exit 0; 428 aprovados, zero falhas, dois ignorados; 237,10 s; main/doc-tests sem falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | Exit 0; 428 aprovados, zero falhas, dois ignorados; 701,37 s; main/doc-tests sem falhas |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | Exit 0; 41 warnings preexistentes |
| `git diff --check` | Sem erros |
| `git diff --check main...HEAD` | Sem erros, inclusive no commit final |

A global paralela não repetiu os timeouts TaskGraph anteriores. Nenhum timeout
foi aumentado nem falha nova dispensada. Os dois ignorados continuam sendo
`real_app_server_handshake` e `manual_final_codex_agent_bridge_gate`, gates
Codex locais/manuais preexistentes. Compilação de testes mantém dois warnings
preexistentes. Não houve tráfego com providers/credenciais comerciais reais.

Pontos para a reauditoria independente: sequência atribuída na autorização de
transporte, depois da admission; regra restritiva para fatos anteriores/repetidos;
desconto do potencial de refund no teto atrasado; overlap pode subutilizar quota,
pois início local não prova ordem de processamento remota. Continuam as
limitações anteriores de token bounds ausentes em produção e coordenação de
uma única instância. Nenhum gate humano ou LR-8D foi liberado.

### FIX residual — overlap histórico

FIX sobre o HEAD auditado `bc8d5e03a89ec15bb076f165816caaf69bfd2837`,
exclusivamente na branch `lr-8c-rate-accounting`.
**IMPLEMENTAÇÃO CANDIDATA — aguardando reauditoria independente da Luna**.
**LR-8D bloqueada.** Sem merge nem novas políticas da LR-8D.

#### Grupo de overlap e lifecycle

HTTP start order não equivale a provider processing order. A sequência maior
de B não prova que seu remaining foi calculado depois do de A. Verificar apenas
peers ainda vivas em `observe_external` perdia a evidência de overlap depois do
Drop de A. Agora cada Attempt possui `overlap_start`, origem de seu grupo,
expressa pelo menor HttpStartSequence daquele grupo. É identidade temporal local
do grupo, sem afirmar ordem remota e sem usar reservation ID ou wall-clock.

Após accounting durável e último check de cancellation em `started`, o manager
procura outras attempts HTTP-started da mesma geração/provider. Se houver,
atribui à nova e às peers ativas a menor origem histórica delas, ou o HTTP start
da peer ainda isolada. Essa operação ocorre sob o mesmo mutex da autorização de
HTTP. Reservations pendentes sem `started` não criam overlap. Falha/cancellation
antes de autorizar transporte não modifica os grupos das peers.

A origem acompanha cada membro até seu próprio Drop. Assim, B continua membro
quando A termina; C que começa enquanto B ainda está viva herda o grupo A/B,
mesmo que A já tenha terminado. Não há flag global permanente: depois que todas
as attempts HTTP-started terminam, uma nova call começa com overlap_start ausente.
Uma call da era antiga ainda viva não agrupa calls da nova credencial. Os checks
de generation anteriores à publicação de quota continuam rejeitando fatos antigos.

#### Autoridade factual e accounting

Cada constraint Provider/Model conserva sua observation_floor. A primeira
observação útil de um grupo naquele scope pode estabelecer sua baseline, mas
mantém o clamp conservador existente: seu teto nunca ultrapassa crédito conhecido
anterior e todos os charges vivos são conservados. Mesmo essa observação não
limpa uncertainty concluída nem saturation apenas por fornecer remaining.
Um reset factual novo válido nessa baseline continua sendo registrado normalmente.

Quando a floor já pertence ao grupo (`overlap_start <= observation_floor`),
todo fato correlacionado desse grupo segue a regra tightening-only da FIX anterior,
mesmo com HTTP start maior e nenhuma peer restante. Também permanece essa regra
para HTTP start anterior/repetido à floor. O fato pode reduzir capacity, incluindo
a proteção contra refunds terminais futuros; não aumenta teto, não limpa
uncertainty/saturation e não rebasa charges/epoch. A regra temporal definitiva
da FIX abaixo pode retirar a autoridade de uma deadline conflitante, sem
antecipá-la nem reaplicar um reset.
Consumed e ownership permanecem intactos; Drop reconcilia somente o excesso
terminal comprovado, uma vez e na epoch correta.

Um fato normalizado independente ainda estabelece barrier e conserva as regras
anteriores de fresh remaining. Reset factual já aceito/configuração explícita
continua avançando pela deadline monotônica válida. O grupo histórico não impede
esse reset legítimo quando não há evidência temporal conflitante, mas seus
headers posteriores não reaplicam um DelayMs antigo. A FIX temporal abaixo
retira auto-refill quando um reset ambíguo contradiz a deadline aceita.
Partial headers não provam refill. Os valores permanecem isolados por scope;
nenhuma quota de um modelo é aplicada a outro nem promovida a Provider.

Uma nova call realmente isolada, iniciada após as anteriores terminarem, não
herda a origem histórica. Com HTTP start posterior à floor, seu remaining volta
a possuir autoridade fresh normal. Portanto o conservadorismo não é permanente.
O marker não é serializado, persistido nem exposto em snapshots: é proteção de
quota externa efêmera. Restart já descarta essas quotas sem contexto comprovável.
Durable uncertainty, recovery markers, schema 12 e policies/budgets locais não
mudam; o formato persistido e a matriz terminal usage permanecem iguais.

#### Regressão, testes e autoauditoria

No HEAD auditado, o novo teste com OrderedHeaderProvider + Scheduler/Admission
reais, cap = 2, falhou deterministicamente: após A publicar RPD=4/TPM=40 e
terminar completamente, B ainda viva publicou RPD=9/TPM=90; capacity de requests
subiu de 4 para 9. O teste exige manter teto/debitos, depois inicia C isolada e
comprova fresh RPD=9/TPM=90 novamente.

Cinco testes adicionais, preservando os 63 anteriores da LR-8C:

- regressão A/B histórica + recuperação de C isolada, com duas traffic classes;
- caso inverso: B mais restritiva após A terminar, sem double debit/refund/reset;
- cadeia A/B → B/C preserva a origem mesmo após A/B terminarem; D isolada recupera;
- uncertainty da peer concluída permanece unknown; reset antigo de B é ignorado,
  enquanto a deadline factual já aceita continua válida exatamente na boundary;
- era antiga ainda viva não cria overlap na nova; headers antigos não alteram
  novo saldo ou reset.

Clock falso e channels com acknowledgements determinam os passos. Nenhum timeout
foi aumentado. Os 56 corpos anteriores de rate_tests ficaram byte a byte iguais;
adapters, TelemetryStore, Scheduler, admission, TaskGraph/D3, credenciais,
migrations e frontend não foram alterados. Uma expectativa nova do caso inverso
foi corrigida: o clamp anterior produz capacity 80 e saldo 60, não 70, antes do
header mais restritivo. Isso preserva o algoritmo existente, sem dispensar falha.

Autoauditoria: marker nasce somente na autorização HTTP; acompanha o membro
após Drop das peers; herança transitiva não usa ID de reservation; desaparece
com os membros, permitindo fresh isolado; contexto/provider separados; mesmos
scopes de constraints; nenhum refill, limpeza de uncertainty ou reset via header
tardio do grupo; nenhuma duplicação de charges/refunds; nenhum estado remoto,
secret, Account ID, header raw ou conteúdo de usuário novo em persistência/snapshot.

Limitações: o grupo é conservador por provider/era enquanto houver membro ativo,
inclusive em chains longas e no intervalo até reconciliation/Drop. Isso pode
subutilizar quota. O runtime não tenta reconstruir processing order remoto.
Continuam os limites anteriores de bounds ausentes em produção e uma instância
ativa. A reauditoria deve conferir baseline inicial versus tightening-only dos
demais membros, herança transitiva, cancellation e recuperação após quiescência.

#### Gates da FIX de overlap

Resultados sobre o código final (04/10/2026):

| Gate | Resultado |
|---|---|
| `npm run typecheck` | Exit 0 |
| `npm run build` | Exit 0; aviso preexistente de chunk 666,22 kB |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0; 15 warnings preexistentes |
| `cargo test --manifest-path src-tauri/Cargo.toml rate_tests` | Exit 0; 61 aprovados em paralelo, zero falhas; 32,71 s |
| `cargo test --manifest-path src-tauri/Cargo.toml task_graph_runtime_tests` | Exit 0; 13 aprovados em paralelo, zero falhas; 48,69 s |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Exit 0; 433 aprovados, zero falhas, dois ignorados; 200,82 s; main/doc-tests sem falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | Exit 0; 433 aprovados, zero falhas, dois ignorados; 699,27 s; main/doc-tests sem falhas |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | Exit 0; 41 warnings preexistentes |
| `git diff --check` | Sem erros |
| `git diff --check main...HEAD` | Sem erros, inclusive no commit final |

A paralela global não repetiu timeouts TaskGraph. Nenhum timeout foi aumentado.
Os dois ignorados continuam `real_app_server_handshake` e
`manual_final_codex_agent_bridge_gate`, gates Codex manuais preexistentes.
Compilação de testes mantém dois warnings preexistentes; nenhum warning novo foi
introduzido. Não houve tráfego com providers/credenciais comerciais reais.

Status mantido como candidata, aguardando reauditoria independente da Luna.
Sem merge, sem gate humano e sem implementação da LR-8D.

### FIX residual — resets factuais em observations ambíguas

FIX sobre o HEAD auditado `3dc0a82c06f9f53c92e0e29ead9fbf0a70e0c343`,
exclusivamente na branch `lr-8c-rate-accounting`.
**IMPLEMENTAÇÃO CANDIDATA — aguardando reauditoria independente da Luna**.
**LR-8D bloqueada.** Sem merge.

#### Regra temporal definitiva

Estratégia adotada: opção B conservadora, aplicada quando há conflito temporal.
HTTP start order continua não sendo prova de processing order remoto. Para
uma observation tightening-only, o manager normaliza o reset recebido com uma
única leitura do clock injetável e compara sua deadline monotônica com a aceita:

- deadline válida anterior ou igual: conserva a deadline aceita, sem antecipar;
- deadline válida posterior: retira a autoridade de auto-refill da constraint,
  definindo `deadline = None` e `end_unix = None`;
- reset conhecido sem conversão temporal segura, por ausência de wall-clock para
  UnixMs ou overflow monotônico: também retira essa autoridade;
- header parcial sem reset: não inventa nem reaplica deadline;
- constraint sem deadline: observation ambígua não pode criar/restaurar uma.

A retirada também transforma o reset do snapshot aceito pelo rate manager em
`unknown`; o TelemetryStore continua registrando o fato observacional normalizado
recebido, sem lhe conferir autoridade de enforcement. Não significa reset zero,
quota ilimitada ou fim presumido da janela. Nenhum estado remoto novo é persistido.

O tratamento ocorre **antes de `refresh()`**, sob o mutex já existente. Assim,
um header que chega na boundary ou depois da antiga deadline, antes de outro
refresh, impede que essa deadline libere crédito no processamento do próprio fato.
A alteração temporal não modifica capacity, consumed, epoch, charges, ownership,
uncertainty ou observation_floor. A redução de capacity continua seguindo a regra
tightening-only anterior, inclusive a proteção contra refunds futuros. Outros
scopes/dimensões/providers conservam suas próprias deadlines; janelas locais
configuradas e budgets duráveis permanecem inalterados.

Não há extensão incremental de DelayMs nem cache crescente de headers. Um fato
relativo duplicado, convertido mais tarde, pode deixar de ser compatível com a
deadline aceita e causar sua retirada conservadora. Repetições posteriores não
rearmam a deadline, não prolongam uma janela repetidamente, não alteram epoch e
não limpam uncertainty. Reset menor, inclusive DelayMs(0), não acelera a deadline.

Um trusted independent fact pode restabelecer autoridade temporal normal,
mantendo a barrier para calls já iniciadas. Uma nova call isolada, depois da
quiescência do grupo, também pode fornecer fresh remaining/reset normalmente.
Os checks de generation precedem o tratamento temporal: headers de credencial
antiga não antecipam nem retiram deadlines da era atual. HttpStartSequence e
overlap_start continuam com o mesmo lifecycle; reservation ID, wall-clock e
ordem de completion não são relógios de ordering remoto.

#### Regressão e testes

Antes da correção, o teste novo com OrderedHeaderProvider, Scheduler e
AdmissionController reais (cap=2, Background e ForegroundInteractive) falhou
deterministicamente no HEAD auditado. A/B ficam in-flight; A publica remaining=40
e DelayMs(10) para RPD/TPM e termina; B publica remaining=30 e DelayMs(100) e
termina. Ao avançar +10 ms, requests capacity subia incorretamente de 31 para 100,
ignorando a evidência temporal de B. O débito de uma request explica capacity 31
e saldo 30; TPM conserva os dois débitos de 10 e saldo 20, sem double debit/refund.

O teste corrigido exige que +10 ms e +100 ms não refillam automaticamente.
Depois inicia C isolada, aceita fresh remaining=90/reset=50 ms, e comprova o
reset único exatamente na nova boundary. Foram adicionados oito testes:

- regressão Scheduler/Admission real e recuperação temporal de C isolada;
- DelayMs/UnixMs posteriores, antes, exatamente na e após a deadline antiga;
  preservação de uncertainty e reconciliation na epoch original;
- resets menores/iguais conservam a deadline, inclusive DelayMs(0);
- partial headers não criam nem reparam uma deadline;
- repetição do mesmo DelayMs da mesma attempt não estende/rearma reset;
- fato independente recupera autoridade enquanto uma call do grupo ainda vive;
- reset da geração antiga não acelera nem retira deadline atual;
- ausência de wall-clock/overflow de conversão revoga auto-refill conservadoramente.

Os 61 corpos anteriores de rate_tests foram preservados integralmente. O fixture
existente ganhou apenas uma variante que recebe o fake clock. Os 68 testes
anteriores da fase (rate, adapters e telemetry) permanecem presentes, totalizando
76 com esta FIX. Channels/acknowledgements e clock fake determinam as etapas;
nenhum timeout foi aumentado.

#### Autoauditoria e limites

Auditados: reset maior tratado antes de refresh; reset menor não antecipado;
duplicação sem extensão/rearme; nenhum reset ambíguo cria epoch, limpa uncertainty
ou aumenta capacity; autoridade fresh recuperável por fato independente ou
quiescência; contexto e scopes separados; nenhum double debit/refund ou mudança
na persistência/schema 12, terminal usage, routing/admission/fairness, TaskBudget/D3,
retry/fallback/cooldown. Somente o manager de rate, seus testes e este documento
foram modificados. Nenhum secret, Account ID, header raw, prompt ou output novo
entra em snapshot, log ou persistência.

Limitação explícita: conflito temporal ambíguo, inclusive um DelayMs repetido
mais tarde, pode eliminar um reset que seria legítimo e subutilizar quota até
nova autoridade fresh. Mesmo ao passar o reset posterior informado pelo header
ambíguo, não há auto-refill. Isso evita presumir processing order e deduplicação
remotos. Se o teto já estiver esgotado, uma nova call não recebe bypass para
descobrir o reset: precisa de fato independente ou invalidação legítima do
contexto remoto conforme o contrato existente, sem apagar budgets locais.
Um fato recebido depois de um refresh já realizado não pode desfazer
retroativamente chamadas previamente autorizadas; a proteção passa a valer na
observação, conforme a evidência disponível ao runtime. Permanecem os limites
anteriores de bounds ausentes em produção e coordenação de uma instância ativa.

Pontos para reauditoria: comparação temporal antes de refresh, revogação sem
rebase da epoch, repeated DelayMs conservador, e recuperação temporal fresh.

#### Gates da FIX temporal

Resultados sobre o código final (04/10/2026):

| Gate | Resultado |
|---|---|
| `npm run typecheck` | Exit 0 |
| `npm run build` | Exit 0; aviso preexistente de chunk 666,22 kB |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0; 15 warnings preexistentes |
| `cargo test --manifest-path src-tauri/Cargo.toml rate_tests` | Exit 0; 69 aprovados em paralelo, zero falhas; 32,69 s |
| `cargo test --manifest-path src-tauri/Cargo.toml task_graph_runtime_tests` | Exit 0; 13 aprovados em paralelo, zero falhas; 49,84 s |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Exit 0; 441 aprovados, zero falhas, dois ignorados; 206,99 s; main/doc-tests sem falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | Exit 0; 441 aprovados, zero falhas, dois ignorados; 697,81 s; main/doc-tests sem falhas |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | Exit 0; 41 warnings preexistentes |
| `git diff --check` | Sem erros |
| `git diff --check main...HEAD` | Sem erros, inclusive no commit final |

A global paralela não repetiu os timeouts TaskGraph; nenhuma falha foi dispensada
nem timeout aumentado. Os ignorados continuam `real_app_server_handshake` e
`manual_final_codex_agent_bridge_gate`, gates Codex locais/manuais preexistentes.
Compilação de testes mantém dois warnings preexistentes. Não houve warning novo,
tráfego comercial real ou alteração de credenciais reais.

**IMPLEMENTAÇÃO CANDIDATA — aguardando reauditoria independente da Luna**.
**LR-8D bloqueada.** Sem merge ou liberação de gate humano.


### Fechamento auditado da LR-8C

**Resultado: PASS técnico + reauditoria independente da Luna em 04/10/2026. Integrada à `main` pela PR #16, squash `5ac28b93480c746ab1e7c519edf31c8ae06f4375`.**
Branch de implementação: `lr-8c-rate-accounting`. Candidata final auditada em
`9668ada86a0716a65917d264cd41b95195c87f24`.

O fechamento consolidou rate accounting central antes do admission, reservations atômicas e reconciliation conservadora, budgets locais persistidos sem hardcode comercial, geração/invalidação de contexto de credencial, durable uncertainty antes da fronteira HTTP, terminal usage apenas com evidência protocolar suficiente, `HttpStartSequence`, ordering conservador de facts concorrentes, grupos transitivos de overlap e revogação conservadora de auto-refill diante de resets ambíguos.

As FIXes finais eliminaram recuperação fictícia de crédito após crash, refunds baseados em usage não terminal, stale headroom com accounting unresolved, ordering por reservation ID, refill por resposta historicamente sobreposta e auto-refill antecipado por reset ambíguo.

Gates finais reportados pelo agente: `rate_tests` 69/0; `task_graph_runtime_tests` 13/0; suíte global paralela 441 aprovados, 0 falhas, 2 ignorados; suíte global serial 441 aprovados, 0 falhas, 2 ignorados; typecheck/build/check debug/release e ambos `git diff --check` em PASS. Não há workflow/status check remoto associado ao HEAD; os gates são execuções locais do agente auditadas contra o código remoto.

Limitações aceitas e não bloqueantes: adapters de produção ainda sem `TokenUpperBound` comprovável; ambiguity/overlap podem subutilizar quota até nova autoridade factual; estado local assume uma única instância ativa; I/O SQLite síncrono permanece dívida de performance, não de correção.

Nenhum gate humano específico foi exigido para a LR-8C pelo protocolo atual. **LR-8D está liberada.**

---

## LR-8D — implementação candidata

**IMPLEMENTAÇÃO CANDIDATA — aguardando reauditoria independente da Luna**

**LR-8E bloqueada.** Branch `lr-8d-resilience-circuit-breaker`, base obrigatória
`main@221d9c35bd31b0524a04de8ba687e52bae2ea076`. Sem merge, migration de health,
painel operacional ou gate comercial/real. A Luna decidirá o gate humano depois
da auditoria estática; esta implementação não declara fechamento da subfase.

### Ownership e autoridade única

`ProviderRuntime → Scheduler → ResilienceManager → estado por provider`.
`cognition/resilience.rs` pertence ao mesmo runtime de RateLimitManager e
AdmissionController. O mapa legado `Mutex<HashMap<String, Instant>>` foi
removido do Scheduler; cooldown e circuit têm uma única fonte de verdade. O
Registry continua fornecendo os IDs, e nenhum update inventa provider ou estado
por modelo. Gemini/Groq/Cloudflare/Mistral não conhecem breaker ou probes e não
receberam modificações.

Conversation, Summary, Orchestrator e Workers continuam usando o Scheduler
compartilhado. RateLimitManager conserva exclusivamente constraints,
reservations, accounting e budgets da 8C; AdmissionController conserva a fila,
concurrency e fairness da 8B. O novo manager não lê quotas, secrets, prompts,
outputs, headers, classes de prioridade ou preços.

### Configuração local, clock e jitter

`ResilienceConfig` é central, validada na construção e substituível por
`Scheduler::with_resilience_config` em testes:

| Policy local | Default |
|---|---:|
| `failure_threshold` | 3 |
| `open_duration_ms` | 30.000 |
| `half_open_max_probes` | 1 |
| `max_retry_backoff_ms` | 30.000 |

Threshold/duração de Open/probes zero e valores acima de `2^53−1` são rejeitados.
Cap de backoff zero é válido e desabilita a espera local. São políticas da Luna,
sem pretensão comercial, UI de edição ou configuração persistida.

O mesmo `Arc<dyn RateClock>` da 8C governa todos os deadlines por tempo monotônico.
A extensão aditiva `RateClock::sleep_ms` permite esperar com clock falso; a 8C não
passa a esperar e seus implementadores existentes preservam o default. Produção
usa `SystemRateClock` baseado em `Instant`, sem serializar `Instant`. Wall clock
continua restrito aos contratos de rate/persistência já existentes e não decide
health. O backoff conserva polling cancelável de até 25 ms, sem reserva ou permit
entre esperas. A responsividade depende também do progresso do executor.

`JitterSource::choose(lower, upper)` é injetável. Produção usa um PRNG xorshift por
runtime, semeado pela dependência `getrandom` já existente; nenhum dado de
credencial, generation, usuário ou conteúdo participa da seed. Testes fornecem
explicitamente extremos determinísticos, sem assertions contra random global
ou relógio de parede. O manager clampa inclusive uma implementação injetada que
retorne fora do intervalo.

Para retry de número `r`, preserva-se o cálculo saturante existente:

```text
exponential = initial_backoff_ms × 2^(r−1)
base = min(exponential, max_retry_backoff_ms)
base == 0 → delay = 0, sem consultar JitterSource
base > 0  → delay ∈ [base/2 + base%2, base]
```

O cap precede equal jitter; shift/multiplicação usam checked/saturating, inclusive
para retry extremo. Contadores/snapshots são JSON-safe. Overflow de deadline
monotônico é explicitamente sinalizado e mantém o gate fechado até invalidação,
em vez de fabricar expiração antecipada.

### Classificação e evidência factual

Somente `ProviderError::Timeout` e `ProviderError::Unavailable { .. }` são health
failures. Uma tentativa só pode incrementar failures ou provar sucesso se
`InvocationObservation::was_started()` confirmar a fronteira HTTP LR-8A. Esse
método lê o marcador deduplicado existente; Selected, admission e construção do
future não são interpretados como HTTP.

`RateLimited`, QuotaExceeded, Authentication, InvalidRequest, Fatal, Protocol,
Incomplete, RequiresAction, OutputLimitExceeded, UnsupportedMode,
RemoteCancelled, Cancelled e EventSinkClosed são neutros para health. Protocol
permanece deliberadamente fora do breaker. RateCapacityExceeded,
DailyBudgetExceeded, RateStateUnavailable, RateContextChanged, admission failures,
TaskBudget e erros/validação posteriores do Core também não degradam health.
Preflight sem transporte não incrementa failures nem fecha um probe.

Sucesso factual em Closed zera failures; em HalfOpen fecha e zera failures. O
resultado do provider é processado depois da reconciliação de rate e antes da
validação cognitiva/budget do Core: rejeição posterior não vira falha remota.
Cancellation observada e sink fechado liberam o guard sem novo health outcome.
Uso e resultados factuais da 8A continuam preservados, inclusive de chamadas
que terminaram tarde.

### Retry-After e cooldown

A publicação de cooldown exige HTTP factual e generation atual; o sinal
operacional continua sendo o erro tipado do adapter. `telemetry.retryHint`
permanece factual e nunca é promovido automaticamente a policy.

| Resultado | Cooldown operacional | Retry no mesmo provider | Breaker |
|---|---|---|---|
| RateLimited + hint | Hint, sem jitter | Não | Neutro |
| RateLimited sem hint | 3.000 ms, sem jitter | Não | Neutro |
| Unavailable + hint | Hint, sem jitter | Não | Failure, se HTTP factual |
| Unavailable sem hint | Nenhum novo | RetryPolicy + budget + backoff/jitter | Failure, se HTTP factual |
| Timeout | Nenhum novo | RetryPolicy + budget + backoff/jitter | Failure, se HTTP factual |

O mínimo operacional preexistente de 1 ms para hint zero permanece. Parsing e
clamp operacional dos adapters permanecem os anteriores. Registrar cooldown faz
`max(deadline_atual, novo_deadline)`, inclusive em resultados concorrentes; uma
observação menor nunca encurta a espera. Cooldown não altera remaining/reset,
consumo, budgets ou quota factual.

Cooldown e Open coexistem e ambos precisam permitir a tentativa. Cooldown é
checado antes de qualquer transição/probe: se Open expirar primeiro, nenhum probe
é consumido. Se cooldown expirar primeiro, Open continua bloqueando. Só quando
ambos permitirem o próximo trabalho pode adquirir um probe bounded.

### State machine e lifecycle de HalfOpen

- **Closed:** autorização normal; cada Timeout/Unavailable factual elegível
  incrementa failures. Ao atingir exatamente o threshold, transição atômica para
  Open com reason allowlisted e nova duração monotônica.
- **Open:** novas tentativas bloqueadas antes de TaskBudget/provider_calls,
  reservation, admission ou adapter. Expiração não agenda chamada e snapshot não
  faz transição; pode mostrar Open com remaining zero até o próximo trabalho.
- **HalfOpen:** a próxima autorização após a boundary tenta adquirir probe sob o
  mesmo mutex que faz Open → HalfOpen. Default um probe; outras tasks não chegam
  aos gates seguintes. Sucesso factual → Closed; Timeout/Unavailable factual →
  Open por outra duração completa; resultado neutro mantém HalfOpen e libera o
  slot para trabalho futuro.

`ResiliencePermit` possui RAII exclusivo; `InvocationObservation` recebe somente
handle não proprietário para revalidar a fronteira HTTP existente. Drop cobre
cancellation, abort/unwind, retorno local de rate/admission, falha de sink e
preflight. Não há concessão de chamada futura em Drop. O guard é descartado antes
de qualquer retry/backoff/fallback.

Cada transição substitui um token interno `Arc<()>` de epoch. Outcome/drop antigo
não fecha nem libera probes de um ciclo posterior, mesmo na mesma credential
generation. Calls já iniciadas não são canceladas quando um peer abre Open;
seus fatos/accounting permanecem. Outcomes de probes de ciclos anteriores não recuperam um circuit atual. Uma
call normal já running que termine depois de recovery participa normalmente
do contador se o estado atual for Closed; não substitui probe em HalfOpen. Cooldown operacional tardio ainda pode estender espera na mesma
geração, mas nunca em uma geração antiga.

### Integração Scheduler, routing, rate e admission

Fluxo: ranking autorizado → autorização atômica de resilience → reservation →
admission → execução HTTP instrumentada → rate reconciliation → health outcome
→ retry/fallback. Após fila, revalida-se health antes de invocar o adapter;
revalida-se novamente no ramo instrumentado de início HTTP, antes da autorização
write-ahead da 8C. A autorização nessa fronteira lineariza o início da tentativa;
Open não preempta trabalho que já cruzou essa autorização. O marcador factual
continua dependendo de sucesso dos checks/commit locais existentes.

Nenhum mutex de resilience atravessa rate, admission, SQLite, HTTP, callback,
backoff ou `.await`. Invalidação segue telemetry → rate (libera) → resilience
(libera). A observação usa attempt → resilience (libera) → rate (libera) →
telemetry. Resilience nunca chama telemetry/rate/admission; não há ciclo de locks.
O caminho tipado de recusa local da observação continua devolvendo NoProvider ou
RateContextChanged, sem fabricar erro remoto ou request factual.

Fixed preserva NoProvider local e nunca usa alternativa. Preferred conserva a
ordem explícita e pula gates operacionais bloqueados. Auto calcula/ordena pelo
score D2 original e aplica o gate aos candidatos nessa ordem; health não soma ou
subtrai pontos. Affinity não é removida por Open/cooldown e só é atualizada após
sucesso real conforme o contrato existente. Ranking/preflight read-only não
adquire probes.

Cada retry processa o outcome, libera os recursos anteriores, calcula delay,
espera cancelavelmente e readquire os três gates. Se um peer abrir Open ou criar
cooldown durante backoff, o retry não executa naquele provider; Preferred/Auto
podem avançar somente para os targets já autorizados. TaskBudget segue limitando
tentativas, sem reserva futura durante backoff. Como na 8A/B/C, uma tentativa já
selecionada que falha no preflight local não é um request HTTP factual.

Sem retry/fallback após primeiro output/chunk. Source e destination têm health
isolado: falha Groq não degrada Cloudflare e sucesso Cloudflare não fecha Groq.
Summary conserva Background e deferral transient/NoProvider; TaskGraph conserva
PlanV1, distribuição, consolidation e provenance existentes.

### Credential generations

TelemetryStore continua autoridade da geração opaca da 8C. Sua invalidação
sincroniza RateLimitManager e ResilienceManager antes de publicar a nova geração.
Troca efetiva de key/token/account context limpa cooldown, Open e failures e
retorna a Closed; não apaga limits/budgets locais ou o ledger factual. Rotação
invalida epochs/probes antigos. Late Timeout/Unavailable/sucesso/cooldown de outra
geração não modifica health atual. Regravar a mesma credencial continua sem
notificação/nova era, preservando cooldown/circuit e quota esgotada.

O observer do SecretStore permanece Weak e recebe somente os tipos de SecretKey,
sem valor/hash/fingerprint/account ID. Esgotamento da geração continua fail-closed
no contrato da 8C. Mutações externas do vault sem a API continuam fora do watcher
existente; a LR-8D não adiciona watcher ou mudança de secrets.

### Snapshot e observabilidade

`Scheduler::resilience_snapshot()` e `get_ai_settings.resilience` expõem circuit,
failures/threshold, Open remaining, probes active/max, cooldown remaining,
transition count/último reason, contadores de Open/HalfOpen/recovery e saturação.
`providerResilience.ts` e o tipo Settings são aditivos e read-only. ProviderStatus
continua expondo cooldown a partir da nova autoridade.

Reasons são enum allowlisted: failure_threshold_timeout,
failure_threshold_unavailable, open_duration_elapsed, probe_timeout,
probe_unavailable, probe_succeeded e credential_context_changed. Contadores de
transição são históricos do processo, não requests nem quota; rotação preserva
estes contadores históricos e reinicia o estado operacional. Não foram adicionados
SchedulerEvents: snapshot/counters/reasons fornecem observabilidade técnica sem
callbacks sob lock, eventos de leitura ou novo consumidor operacional LR-8E.
Snapshots são coerentes por autoridade; a leitura conjunta Settings não promete
uma transação atômica entre health, telemetry, rate e admission.

Nenhum snapshot novo contém prompt, output, reasoning, raw error/header/body,
secret, API key ou Account ID. O diagnóstico NoProvider usa somente provider local,
state enum e duração operacional. Não há mensagem remota nova em logs/eventos.

### Persistência e restart

**Restart limpa cooldown/circuit state.** Health remoto/transitório permanece
in-memory: não há prova de contexto remoto depois do restart e deadlines são
monotônicas. Não há migration ou persistência de health no SQLite.

Isso **não limpa DailyBudget, local rate windows nem durable uncertainty da
LR-8C**. Seus markers e recovery conservador permanecem no RateLimitManager.
O teste de restart cria um novo Scheduler no mesmo SQLite e verifica preservação
de accounting/uncertainty com health Closed, failures/cooldown/transitions zero.

### Testes determinísticos e matriz dos requisitos

A candidata original adicionou 71 testes em `resilience_tests` e um teste de preflight multi-role em
`catalog::route_tests`, filtro executado **`resilience`** (72 testes). Clock e
JitterSource falsos controlam boundaries e delays; Barrier/Notify/oneshot/channels
controlam overlap, entrada, conclusão, cancellation e abort. Nenhum teste novo
usa sleep arbitrário, random global em assertion ou credencial comercial. O
adapter Groq de produção também é exercitado com HTTP exclusivamente em loopback
e credencial sintética em storage temporário.

| Requisitos da auditoria solicitada | Cobertura |
|---|---|
| 1–5 | Base exponencial; extremos de equal jitter; cap anterior ao jitter; zero/extremos sem overflow |
| 6–11 | Cancel durante backoff; Retry-After sem RNG; deadlines max; default 3 s; 100 × 429 sem Open |
| 12–17 | Timeout/Unavailable ±hint; sucesso Closed; threshold exato/abaixo; HTTP factual/preflight |
| 18–22 | Open antes de reservation/admission/HTTP/budget; boundary exata; disputa atômica entre tasks |
| 23–28 | Recovery e reabertura de probe; todos os neutros; cancel/abort/drop em running e queued |
| 29–31 | RateCapacity/DailyBudget/RateState, queue full/timeout e sink em Selected/Queued/Admitted |
| 32–38 | Fixed/Preferred/Auto, score/affinity, isolamento e fallback source/destination |
| 39–46 | Output parcial, eras antigas/rotação/no-op real, cooldown/Open combinados e fallback por hints |
| 47–53 | Erros locais e todas as classes neutras, incluindo Authentication/InvalidRequest/Protocol/quota |
| 54–57 | Contratos Summary/TaskGraph e regressões existentes nas suítes 8B/8C/D3; retry/reconciliation novos |
| 58–60 | Snapshot allowlisted, restart durável e suítes completas anteriores |

Casos adicionais: RNG injetado fora do intervalo; wall-clock jump irrelevante;
config inválida/override/probes=2; epochs internas posteriores; call running não
preemptada; revalidação após fila e preflight; budget do Core depois de sucesso
HTTP; factual retryHint sem enforcement; cancel em Admitted; deadline extremo
fail-closed. Os testes anteriores permanecem sem relaxamento de timeouts.

### Gates técnicos da candidata original (antes desta FIX)

| Comando | Resultado no código final |
|---|---|
| `npm run typecheck` | exit 0 |
| `npm run build` | exit 0; aviso de chunk > 500 kB preexistente |
| `cargo check --manifest-path src-tauri/Cargo.toml` | exit 0 |
| `cargo test --manifest-path src-tauri/Cargo.toml resilience` | 72 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml rate_tests` | 69 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml admission_tests` | 18 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml task_graph_runtime_tests` | 13 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 513 aprovados, 0 falhas, 2 ignorados; 225,77 s de testes |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | 513 aprovados, 0 falhas, 2 ignorados |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | exit 0; 41 warnings |
| `git diff --check` | exit 0 |
| `git diff --check main...HEAD` | exit 0 |

O filtro utilizado é exatamente `resilience`. Os dois ignorados são os gates
Codex locais/manuais preexistentes `real_app_server_handshake` e
`manual_final_codex_agent_bridge_gate`; nenhum gate comercial/real foi executado.
O check debug registra 15 warnings de código não utilizado, mesma quantidade
documentada na base. O leitor direto `TelemetryStore::context_generation` agora
fica sem consumidor no runtime após remover o cooldown legado; a tentativa usa
`InvocationObservation::context_generation`. Builds de testes também mantêm
warnings de campos dos fixtures anteriores. O release registra 41 warnings,
mesma quantidade documentada na base (incluindo código diagnóstico condicionado
ao debug). Não há nova dependência ou alteração de limites de timeout para
ocultar race. Main/doc-tests também concluíram sem falhas nas duas globais.

A primeira global paralela teve 506 aprovados, dois timeouts TaskGraph e dois
ignorados (código então com 67 testes novos). Os dois testes afetados foram
executados por nome completo com `--exact --nocapture`, isoladamente; ambos
passaram. Os logs da global registraram unlocks scrypt de ~4,7–5,6 s versus
~1,5 s nas reproduções isoladas. Os providers sintéticos desses testes não
marcam HTTP e, portanto, não abrem o circuit; a falha ocorreu antes dos Workers,
na espera de eventos de preflight. Não foi dispensada como flake.

A fixture TaskGraph fazia quatro gravações individuais de credenciais, abrindo
o mesmo vault repetidamente e impondo três unlocks scrypt desnecessários por
fixture à suíte paralela. Foi trocada pela API batch existente, com os mesmos
quatro secrets sintéticos. Os testes de overlap/cancelamento também passaram a
segurar/liberar os Workers por channels/oneshot e a aguardar um acknowledgement
após Scheduler/validação do Worker antes de cancelar o sibling; foram removidas as suposições
de 60/150 ms e o sleep arbitrário de 50 ms. Os deadlines existentes de 10 s
para preflight/eventos e 5 s para overlap foram preservados. O acknowledgement tem hook somente `cfg(test)` no Worker e é keyed pela
alocação de cancellation da tarefa, sem callback sob lock. Nenhuma alteração no SecretStore ou timeout aumentado.
A primeira tentativa de usar SubtaskCompleted como acknowledgement foi
rejeitada pelo teste dirigido: o evento só é emitido depois do `join` da wave;
a sincronização correta é o retorno validado do Worker, antes desse join.

A global paralela seguinte reproduziu o problema de preflight em sete testes
TaskGraph (505 aprovados, sete falhas, dois ignorados; código com 71 testes
novos). A economia da fixture não eliminava dois unlocks sequenciais na tarefa:
`validate_policy(Planner)` e `validate_policy(Worker)` abriam o mesmo vault
separadamente, ultrapassando o deadline existente sob contenção global.
`catalog::validate_policies` agora valida os targets registrados das duas roles
e consulta presença em um único snapshot, usando a mesma API batch existente.
`validate_policy` permanece wrapper de uma role; ausência/indisponibilidade de
credencial mantém `provider_not_configured`, e adapters continuam revalidando
antes do HTTP. Nenhum resultado desse snapshot é cacheado como autorização.
Um teste reproduz deterministicamente o trabalho anterior (dois loads), prova
o novo caminho (um load) e verifica fail-closed. A única mudança de produção
TaskGraph adicional é essa consolidação de presença no preflight.


### Autoauditoria e limitações

A revisão direcionada cobre remoção de autoridade duplicada, aritmética/jitter,
classificação factual, max cooldown, lock ordering, probes atômicos/RAII,
read-only snapshots, geração/epoch, ausência de preemption, score/affinity,
Fixed/fallback/output parcial, budgets e ausência de persistência/sensíveis.
Resultados e event callbacks ficam fora do mutex de resilience. Não foi
introduzida dependência Rust nova ou mudança nos adapters/accounting da 8C.

Limitações: estado por provider, sem model-scoped breaker; thresholds são locais
por runtime e não configuráveis pela UI; neutral não prova recovery; expiry não
agenda probe; uma tentativa pending pode permanecer no slot de probe durante
admission/preflight até terminar/cancelar/ser descartada; polling de cancellation
depende do executor. Como a 8C, pressupõe uma instância ativa e clocks monotônicos
válidos. Falha de entropy na seed de produção tem fallback não criptográfico;
esta fonte serve exclusivamente a dessincronização, nunca segurança.

Pontos para auditoria independente: fronteira autorização HTTP/write-ahead,
consistência generation/epoch em late outcomes, RAII após abort e falhas locais,
semântica Open com remaining zero, contabilização anterior durante retry/fallback,
telemetry.retryHint versus hint operacional, manutenção de score/affinity e
isolamento do recovery do source. Isso não substitui a auditoria da Luna.

**IMPLEMENTAÇÃO CANDIDATA — aguardando reauditoria independente da Luna**.
**LR-8E bloqueada.**

### FIX — call budget pendente e cooldown factual

FIX sobre o HEAD auditado `ce22e3da636aa13462629ca8514cb2031501505c`, na mesma
branch `lr-8d-resilience-circuit-breaker` e base
`main@221d9c35bd31b0524a04de8ba687e52bae2ea076`.

**IMPLEMENTAÇÃO CANDIDATA — aguardando reauditoria independente da Luna**.
**LR-8E bloqueada.** A arquitetura geral foi aprovada pela auditoria independente;
os dois bloqueios de correção abaixo motivaram esta FIX. Nenhum merge, painel,
migration, gate comercial/real ou alteração de defaults/timeouts.

#### Scheduler attempt, provider call e commit de TaskBudget

`PendingSchedulerAttempt` representa o bookkeeping provisório de uma tentativa:
número por provider e intenção de fallback. `Selected.attempt` e
`ProviderRequest.attempt` usam esse número pendente; seleção não é prova de HTTP.
O check de TaskBudget ocorre antes de rate/admission, mas não debita o ledger.
Como a task executa uma tentativa por vez, a vaga verificada permanece disponível
até resolver essa tentativa. Não é uma reserva compartilhada de rate/admission.

Existe **um único commit**, consumindo o valor pendente depois da execução e da
reconciliação, antes de processar resposta/retry/fallback. Ele publica
`provider_calls`, número committed do provider, `retries`, `fallbacks` e
`providers_used`. A divisão conservadora de output usa as chamadas committed;
a tentativa pendente continua incluída no divisor, preservando o cálculo da 8C.

Recusa por resilience depois da fila descarta o pendente e libera os guards.
Recusa na fronteira instrumentada, com `was_started() == false` e erro local da
observação, também não faz commit. `NoProvider` avança na ordem de targets já
autorizados; `RateContextChanged` conserva o erro terminal fail-closed da 8C,
sem inventar fallback por rotação. Não há rollback/decremento ou débito duplo.
Depois de HTTP factual, o commit não é desfeito por erro posterior.

`Preferred(a,b)` com budget 1: A Selected/Queued, peer abre circuit ou publica
cooldown, A é recusada ao ser admitida, B Selected/HTTP/sucesso. O resultado tem
uma call, providersUsed somente B, zero retries/fallbacks remotos, requests
factuais A=0/B=1, accounting local A=0/B=1 e nenhum guard pendente. Se uma call A
real falhou e seu retry foi recusado no início HTTP, somente a primeira A e B
debitam budget; o retry recusado não entra em `usage.retries`.

O contrato conservador anterior permanece para resultados normais do adapter,
incluindo erro de preflight sem erro local da observação: eles podem consumir
call budget, mesmo com requests factuais zero. Isso limita retries e preserva
fixtures/providers legados sem instrumentação LR-8A. Portanto `provider_calls`
é o ledger de tentativas committed do Scheduler, **não substitui o contador
factual HTTP**. Nesta FIX, a exceção é a tentativa recusada localmente antes do
HTTP, cujo bookkeeping nunca é committed. Eventos Selected/Retry/Fallback
continuam semântica de seleção/intenção anterior; os counters do resultado só
incluem tentativas committed. A recusa inicial de A não fabrica erro remoto ou
evento Retry/Fallback.

#### Cooldown exige HTTP factual

`ResiliencePermit::finish` só publica qualquer outcome operacional se
`started == true` **e** a generation ainda coincide. O mesmo check protege
cooldown e health; epochs continuam protegendo o lifecycle dos probes. Preflight
RateLimited(None/Some) ou Unavailable(Some) retorna o erro tipado sem cooldown,
failure, recovery ou alteração de circuit. Drop libera o probe e a próxima task
pode testar novamente. `TelemetryStore.retryHint` não foi alterado.

Após HTTP factual, permanecem RateLimited(None) → 3.000 ms,
RateLimited(Some) → hint, Unavailable(Some) → hint + health failure elegível;
hints sem jitter e deadlines max. Timeout/Unavailable(None), classificação
neutra, generations, RAII, score/affinity, accounting/admission, isolamento,
Summary/TaskGraph e restart conservam os contratos anteriores.

#### Regressões e gates da FIX

As duas regressões foram executadas contra o código auditado antes da correção:
o caso Preferred/budget 1 não iniciava B e expirava o acknowledgement existente;
o preflight RateLimited registrava 3.000 ms em vez de zero. Não houve aumento
de deadline para fazê-las passar.

Cinco testes novos e ampliação de três testes anteriores cobrem cooldown na
fila; Open/cooldown na fronteira HTTP com budget 1; retry negado na fronteira
com budget 2; hints factuais em Closed/HalfOpen; hints locais em Closed/HalfOpen
com nova tentativa permitida; era alterada terminal antes do HTTP; ordem/números,
providersUsed, counters e reservations reconciliadas. Clock/jitter falsos e
channels/oneshot/Notify continuam controlando boundaries, sem sleeps arbitrários.

Na rodada dirigida intermediária, os dois testes novos de hints factuais
descartavam o receiver de eventos antes da execução; o callback do harness
falhava com SendError. O receiver passou a permanecer vivo até a conclusão.
O filtro resilience seguinte aprovou os 77 testes, sem alteração de timeout.

A primeira global da FIX teve 512 aprovados, seis falhas e dois ignorados. Todas
as seis falhas foram reproduzidas isoladamente com seus nomes completos e
`--exact`: fixtures antigos esperavam cooldown de MockProvider/SequenceProvider/
Synthetic sem marcar transporte. O default Provider e os mocks de produção
continuam sem fabricar HTTP. Os testes de cooldown remoto agora usam um wrapper
**somente no módulo de testes**, `SimulatedTransport`, que marca explicitamente
o início do transporte simulado; suas assertions de cooldown/routing/budget são
preservadas. O teste do runtime diagnóstico também verifica que o mock bruto
não publica cooldown, além de manter a regressão de cooldown persistente em um
Scheduler com transporte simulado. Não houve mudança em adapter, default
Provider, mock de produção, Scheduler para acomodar fixtures ou timeout.

| Gate | Resultado final da FIX |
|---|---|
| `npm run typecheck` | exit 0 |
| `npm run build` | exit 0 |
| `cargo check --manifest-path src-tauri/Cargo.toml` | exit 0 |
| `cargo test --manifest-path src-tauri/Cargo.toml resilience` | 77 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml rate_tests` | 69 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml admission_tests` | 18 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml task_graph_runtime_tests` | 13 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 518 aprovados, 0 falhas, 2 ignorados |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | 518 aprovados, 0 falhas, 2 ignorados |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | exit 0; 41 warnings |
| `git diff --check` | exit 0 |
| `git diff --check main...HEAD` | exit 0 |

Filtro usado exatamente `resilience`. As globais levaram 259,32 s (paralela)
e 678,72 s (serial) de testes; main/doc-tests também concluíram sem falhas.
Os dois ignorados são `real_app_server_handshake` e
`manual_final_codex_agent_bridge_gate`, gates Codex locais/manuais preexistentes.
Warnings mantidos: 15 no debug, 41 no release, dois campos de fixtures na
compilação de testes e chunk frontend de 666,22 kB (> 500 kB). Nenhum timeout
foi aumentado. Nenhum gate comercial/real foi executado.

#### Autoauditoria e limitações residuais

A revisão direcionada confirmou um único commit consumindo o pendente, sem
decrementos ou commit nos caminhos de recusa antes do HTTP. Os testes verificam
budget disponível na fila/HTTP/retry, numeração/providersUsed, ausência de
probe/reservation/permit leak, hints locais sem policy e hints factuais
preservados. Divisão de output, geração e parcial output conservam os contratos
anteriores; as globais cobrem LR-8A/B/C/D3. A FIX não muda rate/admission,
telemetry, adapters, preflight multi-role, tipos/frontend ou ownership de
resilience. Nenhum mutex/callback/persistência de health foi introduzido.

Limitações mantidas: provider-scoped/in-memory, sem painel/config persistida,
sem prova de recovery por resultado neutro. `RateContextChanged` segue terminal;
preflight normal mantém budget conservador; Selected é tentativa provisória,
e não contador HTTP. A reauditoria deve conferir especificamente a fronteira
de commit, divisão de output e a recusa entre admission e início instrumentado.

**IMPLEMENTAÇÃO CANDIDATA — aguardando reauditoria independente da Luna**.
**LR-8E bloqueada.**


### Fechamento auditado da LR-8D

**Resultado: PASS técnico + reauditoria independente da Luna em 04/10/2026. Integrada à `main` pela PR #18, squash `8f69d02a612233133a550a3a468915d00066505c`.**
Branch de implementação: `lr-8d-resilience-circuit-breaker`. Candidata final auditada em
`c4c49bfd6942fcf4517ad52b91634724df9812bd`.

A fase consolidou uma autoridade única `ResilienceManager` por provider e geração de
credencial, com exponential backoff + equal jitter, cooldown monotônico, circuit
breaker `Closed/Open/HalfOpen`, probes HalfOpen bounded e RAII, revalidação após
fila e na fronteira HTTP, isolamento entre providers e snapshot read-only.

A FIX pós-auditoria introduziu `PendingSchedulerAttempt`, tornando o accounting de
TaskBudget provisório até o ponto único de commit. Recusas locais por resilience
antes de HTTP não consomem `provider_calls`, retries, fallbacks ou
`providers_used`; chamadas que realmente cruzam HTTP nunca perdem o débito.
Cooldown derivado de `ProviderError` passou a exigir `started=true` e generation
atual, impedindo preflight local de fabricar estado operacional remoto.

Gates finais reportados pelo agente:
- `resilience`: **77 aprovados**;
- `rate_tests`: **69 aprovados**;
- `admission_tests`: **18 aprovados**;
- `task_graph_runtime_tests`: **13 aprovados**;
- suíte global paralela: **518 aprovados, 0 falhas, 2 ignorados**;
- suíte global serial: **518 aprovados, 0 falhas, 2 ignorados**;
- typecheck/build/check debug/release e ambos `git diff --check`: PASS.

Não existe workflow/status check remoto associado ao HEAD; os gates são execuções
locais do agente combinadas com auditoria estática independente do código remoto.

Limitações aceitas e não bloqueantes:
- health/cooldown continuam provider-scoped, in-memory e não persistidos;
- restart limpa health transitório sem apagar budgets/rate windows/uncertainty da 8C;
- resultados normais de preflight legado podem manter accounting conservador do
  Scheduler distinto do contador factual HTTP;
- nenhum gate comercial/real foi forçado nesta subfase.

O gate humano integrado permanece para a LR-8E, que está **liberada**.

---

## LR-8E — IMPLEMENTAÇÃO CANDIDATA

**IMPLEMENTAÇÃO CANDIDATA — aguardando reauditoria independente da Luna e gate humano final**

Branch `lr-8e-operational-panel-final-gate`, base obrigatória
`main@b8a89c1943c232a3ac793a897f06d68593970d95`. Sem merge e sem gate
comercial/real executado. LR-8A/B/C/D permanecem PASS; LR-8 continua aberta.

### Snapshot operacional e fronteira de leitura

`ProviderRuntime → Scheduler::operational_snapshot()` agrega os contratos
existentes `ProviderTelemetrySnapshot`, `AdmissionSnapshot`, `RateSnapshot` e
`ResilienceSnapshot`, em `cognition/operational.rs`. A captura UTC opcional comum
identifica o começo da leitura da telemetry, não uma transação global.

```rust
pub async fn get_provider_operational_snapshot(
    runtime: tauri::State<'_, std::sync::Arc<ProviderRuntime>>,
) -> Result<ProviderOperationalSnapshot, String>
```

O comando recebe somente o runtime em memória. Não recebe Database, SecretStore,
AppHandle, credentials ou filesystem; não chama catalog/presença de credenciais,
adapters, HTTP ou tarefas. A superfície Settings IA é a única capability que
recebe a nova permissão. Handlers debug/release e `build.rs` registram o comando;
Tauri gera o par allow/deny em `permissions/autogenerated`. Um worker blocking
faz a leitura em memória para não esperar locks concorrentes na thread da WebView;
falha do worker retorna somente o código genérico `worker_failed`.

**Cada autoridade fornece um snapshot coerente internamente (admission por
provider), mas a captura agregada não promete atomicidade global entre
telemetry/rate/admission/resilience.** Os locks são independentes e sequenciais,
sem megamutex, callbacks ou await atravessando authorities.

A leitura legada `RateLimitManager::snapshots()` aplica `State::refresh` no estado
vivo. O novo `read_only_snapshots()` clona os estados sob o mutex, solta o lock e
projeta as mesmas boundaries conhecidas somente nas cópias. Ambos reutilizam a
mesma construção de `RateSnapshot`; enforcement, persistência, reset legítimo e
accounting originais não mudam. O poll não incrementa counters, muda epochs,
remove uncertainty viva, transita circuit ou consome probes. Não executa
`persist` nem `Database::open`, mesmo quando existe uma policy durável. O teste
interno verifica consumed/epoch/deadline/blocks vivos invariantes na boundary.
O teste agregado também lê repetidamente com o diretório SQLite removido após
configuração e falha sintética de persistência; não recria o banco.

### Polling e performance

`operationalPolling.ts` mantém um único timeout de **1.000 ms depois da conclusão
da captura anterior**, um único IPC pendente e um bit de refresh adicional
coalescido. Não acumula amostras/histórico. Renderizações não recriam o timer.
`visibilitychange` pausa hidden e retoma visible imediatamente; cleanup remove
listener/timer e invalida respostas pendentes. O controller reutilizado no
cleanup/setup de React StrictMode preserva o latch de single-flight. Epochs
impedem publicar uma resposta da janela oculta/desmontada em outro lifecycle.
Atualizar agora e salvar policy durante um IPC aguardam uma captura posterior,
sem sobreposição. Somente `refreshManual()` (clique em Atualizar agora) controla
“Atualizando…”/disabled. `refresh()` automático ou após salvar policy não muda
o botão; feedback manual permanece até terminar a leitura fresh coalescida e
não publica depois de hidden/cleanup ou num lifecycle antigo. Falhas mantêm o
último snapshot válido, com aviso discreto,
sem console/log e sem aria-live em cada tick.

`get_ai_settings` permanece carga inicial e refresh explícito após adicionar ou
remover credencial pelo fluxo existente. `configured`/`enabled`/nomes são juntados
por providerId com `settings.providers`; o poll não tenta descobrir credentials.
A UI descarta os campos operacionais legados da resposta inicial de Settings;
conserva somente configuração estática e o último snapshot vivo do painel.
Nenhuma abertura periódica SQLite/Stronghold, chamada remota, event bus,
WebSocket, chart library, canvas/WebGL, dependência npm ou buffer temporal novo.
O custo é copiar/serializar as autoridades em memória e atualizar quatro cards.
Comparação de build com a base obrigatória em worktree temporária: chunk IA
35,82 kB / 9,81 kB gzip na base; candidata 59,60 kB / 16,49 kB gzip. O chunk
principal permanece 666,22 kB / 168,98 kB gzip; não carrega os componentes novos.
CSS Settings passa de 3,31 kB / 1,16 kB gzip para 4,78 kB / 1,48 kB gzip.
Sem bibliotecas novas ou renderer nesta superfície.
CPU/RAM percebidos e legibilidade na WebView real permanecem no gate humano;
não se afirma um benchmark do hardware alvo sem executá-lo.

### Semântica visual

`ProviderOperationsPanel.tsx` tem uma card por provider e resumo sempre visível:
configured/enabled, condições operacionais simultâneas, circuit, cooldown,
active/max concurrency, queue depth/capacity, requests/recência, último Retry-After,
DailyBudget, pending reservations/local blocks/persistence failure e custo.
`details` acessíveis separam uso/quota, admission, rate, resilience e edição local.
Tabelas têm caption/headers; foco e largura atual de Settings são preservados.
Warnings têm texto e borda, sem score/média composta ou semântica por cor.

- **Disponível para tentativa** exige configured/enabled, snapshots presentes,
  Closed, ausência de cooldown e de gate imediatamente restritivo conhecido.
  Não é garantia remota nem autorização substituindo o runtime; target/modelo,
  concorrência e nova evidência ainda precisam passar pelos gates originais.
- Open é mostrado como Open mesmo com restante zero, com próxima autorização
  podendo sondar. HalfOpen mostra probes ocupados/disponíveis, sem chamar saudável.
  Conditions simultâneas não são escondidas por uma label prioritária.
- Requests constraints Provider/Model permanecem rotuladas pelo scope e source.
  Restrição de um modelo não vira bloqueio global: o resumo indica que a
  disponibilidade depende do modelo. Fila cheia é indicada mesmo se um slot
  tiver sido liberado e ainda houver waiters pendentes.
  Token remaining zero/unknown não prova bloqueio pré-HTTP dos adapters sem bound.
- Unknown/null → **Desconhecido**, nunca 0, infinito, sem limite ou esgotado.
  Zero conhecido continua 0. Quota ausente é **Não informado pelo provider**.
- Usage desde o início deste runtime: observed/reportingRequests/saturated por
  requests/input/output/total/thought; tokens medem o subconjunto reportado e
  medição parcial é explícita. Não são consumo total da conta nem custo.
- Quotas Provider e Model(model) têm tabelas distintas com limit/remaining/reset
  independentes e provenance/timestamp por campo. Não existe herança de modelos.
- Constraints mostram source external_fact/local_policy/daily_budget, provenance,
  capacity/consumed/reserved/effectiveRemaining, resets e unresolved/saturated.
  Fatos externos retidos pelo manager são distintos da última observação telemetry.
- Admission mostra classes, admissions/waited/delay recente/acumulado/amostras,
  full/timeout/saturation; nenhuma média inventada ou inferência de velocidade.
- Resilience mostra circuit/failures/threshold/Open/probes/transitions/reason e
  contadores Open/HalfOpen/recovery/saturation. Reasons/outcomes usam allowlists
  locais; nenhum raw error é renderizado.
- **Último Retry-After observado** é histórico factual com origem, independente de
  **Cooldown operacional restante**, que representa a decisão atual da 8D.
- **Custo: Desconhecido · sem contrato de preço configurado.** Sem pricing web,
  preço hardcoded, inferência por tokens ou afirmação de gratuidade.

Provenance é somente apresentação: provider_header → Provider;
provider_response → Resposta do provider; user_configuration → Configuração local;
local_runtime → Runtime Luna. Enums originais permanecem no contrato TypeScript.

### Editor exclusivo da RatePolicy local

`LocalRatePolicyEditor.tsx` / `ratePolicyDraft.ts` editam somente o contrato
existente via `update_provider_rate_policy`. limits[] permite Provider/Model,
modelo explícito, RPM/TPM/RPD/TPD, capacity, periodMs e anchorUnixMs; não oferece
concurrency. DailyBudget tem opt-in, anchor UTC Unix ms obrigatório e dois máximos
nullable explicitamente visíveis. Novos valores começam vazios, sem defaults
comerciais ou conversão de timezone; zero e null não são intercambiáveis.

Formulário limpo acompanha a policy atual do snapshot. Ao editar, mantém draft e
base, sem auto-save. Salvar somente budget usa limits atuais; salvar somente
limits usa DailyBudget atual. Se uma seção editada mudou externamente, exige
recarregar formulário explicitamente para evitar sobrescrita silenciosa.
Valida Number.isSafeInteger/não negativo, período positivo, scope/modelo,
cardinalidade 64 e duplicatas; backend continua autoridade final. Após sucesso
pede refresh operacional; falha de refresh mantém a edição e sinaliza a limitação.
Errors remotos/IPC têm tradução allowlisted ou mensagem genérica.

Sem reset de consumption/telemetry/quota, perdão de uncertainty, mutation de
circuit/cooldown/generation, alteração Admission/ResilienceConfig ou botão de
bypass. `persistenceFailed` apresenta warning claro e desabilita gravação pelo
editor enquanto a falha estiver presente; não oferece limpeza do fail-closed.
Mudança/removal de policy conserva exatamente a semântica autorizada da 8C:
capacity na mesma janela preserva consumed/uncertainty; mudança de janela/remoção
é configuração explícita, não recovery implícita. RatePolicyBusy continua final.

### Privacidade e autoauditoria

JSON agregado reutiliza somente DTOs seguros das quatro authorities. Não contém
API key, bearer, Account ID, fingerprint/hash, headers/body/error raw, URL
autenticada, prompt, resposta/reasoning, conversation ou TaskGraph content.
Provider/Model IDs são os identificadores de configuração local já autorizados.
Testes deixam markers sintéticos em dados privados de um provider e em erro
SQLite e verificam sua ausência no JSON. Provider nunca é invocado pela leitura.

A auditoria dos caminhos novos cobre poll/cleanup/StrictMode/hidden/single-flight,
respostas antigas, unknown/zero, scopes, parcial usage, null remaining, fonte dos
limites, destaque de persistence failure, tradução de outcomes/reasons e erros
genéricos, nenhum log/content/raw stringify, merge das seções da policy e nenhum
bypass. O painel não importa renderer/Three.js nem APIs de secrets/storage.
A Settings inteira conserva seções preexistentes de credenciais e diagnósticos
reais: entrada digitada e resultado solicitado nelas não são incorporados ao
snapshot ou ao DOM do novo painel operacional. Nenhum gate real foi acionado.

Routing Fixed/Preferred/Auto, score/affinity, retries/fallback, concurrency/fairness,
thresholds, TaskGraph/Workers/Summary, credential rotation, PendingSchedulerAttempt,
D3 e durable uncertainty não têm mudança de comportamento. O diff de Scheduler
é aditivo pelo módulo operacional; a refatoração de rate somente reutiliza a
construção do DTO e adiciona leitura em cópia.

### Testes e gates

Nove testes Rust novos: oito no agregador, um na projeção read-only interna de
rate. Filtro `operational` inclui ainda quatro regressões existentes (13 no total).
Cobertura: todos os IDs registrados inclusive disabled, alinhamento entre as
quatro authorities, counters invariantes, Open inclusive após expiry sem
transition, HalfOpen/probes, cooldown, activeCalls=2/queueDepth=1/classes e cleanup,
constraints de sources distintos, unknown/partial usage/retry hint independente,
saturation, persistence failure, ausência de markers privados, Database
inacessível, boundary sem mutation viva, policy existente preservando consumed,
assinatura/capabilities e nenhuma chamada de provider (contador sentinel zero).
LR-8A/B/C/D e TaskGraph são verificados pelas suites originais sem relaxamento.

`node scripts/test-provider-operations.cjs` usa somente Node e o TypeScript já
instalado. Verifica unknown/zero/scopes/allowlist, merge e conflito de policy,
inteiros/duplicatas/novo budget explícito, timer único, manual pendente, hidden,
StrictMode, epochs/unmount, falha preservando último snapshot e recuperação.
`node scripts/test-provider-operations-dom.cjs` renderiza a card React real em
HTML estático com DTOs sintéticos; verifica markers privados ausentes, unknown,
usage parcial, scopes, warnings, captions/headers, controles desabilitados em
persistence failure e ausência de canvas/scripts/aria-live periódico. O harness
expõe a função privada somente no JS temporário emitido, sem alterar o produto.
Nenhum framework ou dependência de frontend nova. Browser não disponível nesta
sessão; inspeção visual física permanece para a checklist humana.

Resultados da candidata inicial auditada `0be42bafe4b042cc9b4b96687e781ab468990751` (04/10/2026):

| Gate | Resultado |
|---|---|
| `npm run typecheck` | Exit 0, inclusive após descarte dos snapshots legados |
| `npm run build` | Exit 0; IA 59,60 kB / 16,49 kB gzip; chunk main 666,22 kB |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0; repetido no IPC final em worker; 15 warnings |
| `cargo test --manifest-path src-tauri/Cargo.toml telemetry_tests` | 33 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml admission_tests` | 18 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml rate_tests` | 69 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml resilience` | 77 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml task_graph_runtime_tests` | 13 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml operational` | 13 aprovados, 0 falhas; repetido no comando final |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 527 aprovados, 0 falhas, 2 ignorados; global final 271,33 s de testes |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | 527 aprovados, 0 falhas, 2 ignorados; 671,73 s de testes |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | Exit 0; 41 warnings |
| `node scripts/test-provider-operations.cjs` | PASS; nenhuma dependência nova |
| `node scripts/test-provider-operations-dom.cjs` | PASS; HTML sintético sem markers privados |
| `git diff --check` | Exit 0 |
| `git diff --check main...HEAD` | Exit 0; verificado também no commit candidato |

As duas globais completaram main/doc-tests sem falhas. A primeira global também
passou (527/0/2; 274,10 s); a repetição verifica o IPC final em worker. Nenhuma
execução global falhou, nenhum timeout foi ampliado. Dois ignorados preexistentes:
`real_app_server_handshake` e `manual_final_codex_agent_bridge_gate`, gates Codex
locais/manuais. Debug/release mantêm os 15/41 warnings unused/dead-code da base;
a compilação de testes tem dois warnings de campos de fixtures preexistentes.
O aviso Vite de chunk > 500 kB continua no chunk da main, sem crescimento nele.
Nenhum gate comercial/real, startup do app com credentials reais ou gate humano
foi executado. Fixtures HTTP da suite são locais e usam secrets sintéticos.
A checklist humana A–M está em [LR-8E-FINAL-GATE.md](LR-8E-FINAL-GATE.md), sem
resultados antecipados: baseline/idle/foreground, concorrência/queue/cancellation,
Summary, capacidade local ou 429 natural, fallback remoto, circuit natural ou
suite determinística, TaskGraph/provenance, restart/accounting e regressões.

### Limitações residuais

Snapshots agregados não são globalmente atômicos; estados menores que um tick
podem não aparecer, mas contadores permanecem observáveis. Configured vem do
Settings estático até refresh explícito de credential UI. Circuit Open expirado
não transita por leitura. Rate projeta resets em cópia; o próximo enforcement
atualiza a autoridade viva. Respostas IPC não podem ser canceladas; são ignoradas
após hidden/cleanup, e nenhuma segunda começa antes da anterior terminar.

Persistem as limitações aceitas das 8C/8D: TokenUpperBound ausente nos adapters,
unknown token accounting, budgets locais conservadores, possível subutilização
por overlap/reset ambíguo, uma instância ativa, I/O SQLite em mutações reais,
health transient não persistido. A UI não cria preço, saúde composta ou outra
autoridade. Auditoria independente e gate humano real continuam pendentes.


### FIX LR-8E — polling visualmente silencioso

**IMPLEMENTAÇÃO CANDIDATA — aguardando reauditoria independente da Luna e gate humano final**.
LR-8 permanece aberta. Continuação exclusiva na branch
`lr-8e-operational-panel-final-gate`, sobre o HEAD auditado
`0be42bafe4b042cc9b4b96687e781ab468990751`.

O gate humano inicial conduzido externamente encontrou duas regressões de UX na
WebView real: feedback visual indevido do polling automático (o botão compartilhava
`busy` com cada captura) e timestamp epoch excessivamente ruidoso na superfície
principal. Esta FIX corrige ambas sem alterar o runtime/arquitetura aprovado.
O agente não executou gate comercial/real e não antecipa resultado do gate final.

`OperationalPoller.refresh()` continua silencioso e single-flight. Startup,
retomada, timeout automático e refresh após salvar RatePolicy usam esse caminho.
Somente o clique manual chama `refreshManual()` e ativa `manualRefreshing`.
Durante um IPC automático pendente, esse clique marca uma captura fresh adicional
no mesmo drain, sem iniciar IPC concorrente, e mantém feedback até o drain acabar.
Conclusões hidden/unmounted ou de epoch antigo não escrevem snapshot, erro nem
feedback manual. Retomada limpa feedback de uma ação manual interrompida antes
de iniciar a leitura visível. Intervalo continua **1.000 ms após a captura anterior**,
sem novo timer, listener, histórico, dependência ou mudança no comando memory-only.

A toolbar mantém o texto principal estável: **“Última captura recebida”**.
O `title` opcional apresenta data/hora ISO UTC legível e o Unix ms diagnóstico.
Timestamp ausente, não inteiro seguro, negativo ou fora do intervalo de `Date`
aparece como **“Última captura: Desconhecido”**, sem fabricar uma data.
Antes do primeiro snapshot: “Aguardando snapshot”. Nenhum aria-live/status é
adicionado à toolbar, e ticks automáticos não mudam texto/disabled do botão nem
largura do label. O aviso de erro já existente continua preservando o último
snapshot válido; não é uma mensagem anunciada a cada tick bem-sucedido.

Os checks existentes agora exercitam separadamente auto/manual, clique durante
IPC automático, captura fresh coalescida, conclusão do feedback manual, erro
silencioso, hidden/unmount e restart StrictMode durante ação manual. O harness
DOM renderiza também os controles reais e compara a superfície entre dois ticks:
somente o tooltip diagnóstico varia; timestamp principal não contém epoch/Unix ms,
o botão permanece habilitado e sem “Atualizando…”, e não há aria-live/status
periódico. Validação de timestamps inclui null, NaN, infinito, negativo, fração,
valor fora de `Date` e epoch zero válido. Todos os checks anteriores de privacidade,
unknown, scopes, usage parcial, warnings e tabelas continuam ativos.

Autoauditoria da FIX: nenhuma alteração Rust de produção, permissions/capabilities,
DTO, SecretStore/DB, requests de provider, RatePolicy editor, runtime A–D ou checklist.
O painel continua com uma única captura mais recente; não renderiza erro bruto,
conteúdo privado, preço ou estado “saudável”. A verificação em WebView real após
a correção permanece para a Luna e o usuário; checks sintéticos não a substituem.

A primeira global paralela desta FIX encontrou uma race preexistente na fixture
`fix5_http_timeout_phases_are_real_and_diagnostics_sanitized`: o servidor TLS
sintético encerrava o socket após um sleep de 200 ms. Sob carga, esse EOF podia
preceder a execução do timeout de conexão no cliente, produzindo `Unavailable`
em vez de `Timeout`. Resultado inicial: 526 aprovados, 1 falha, 2 ignorados.
O teste exato na fixture original passou isoladamente (28,54 s), caracterizando
a intermitência. Um novo teste de regressão mantém um cliente atrasado por
250 ms e verifica que a fixture permanece aberta: com a implementação antiga,
reproduziu deterministicamente a falha por EOF (0,25 s).

Corrigiu-se exclusivamente o helper de teste em `cognition/fix5_tests.rs`:
TLS stall agora aguarda EOF/reset pelo cliente, sob o deadline de fixture de
10 s já existente, em vez de provocar desconexão remota pelo timer de 200 ms.
O provider continua obrigado a produzir e diagnosticar seu próprio timeout.
Não se ampliou nenhum timeout, não se relaxou assertion de fase/outcome nem
se mudou transporte, scheduler ou outro código Rust de produção. As globais
paralela e serial são verificadas novamente após a correção; serial não
substitui o gate paralelo.


Resultados técnicos finais desta FIX (04/10/2026), após corrigir a fixture:

| Gate | Resultado |
|---|---|
| `npm run typecheck` | Exit 0 |
| `npm run build` | Exit 0; IA 60,44 kB / 16,72 kB gzip; main permanece 666,22 kB |
| `node scripts/test-provider-operations.cjs` | PASS; auto silencioso, manual/coalescing, lifecycle, timestamp e regressões anteriores |
| `node scripts/test-provider-operations-dom.cjs` | PASS; toolbar estável entre ticks, sem epoch principal/live region e checks DOM anteriores |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0 |
| `cargo test --manifest-path src-tauri/Cargo.toml operational` | 13 aprovados, 0 falhas |
| Teste exato de regressão TLS | 1 aprovado, 0 falhas; com fixture antiga reproduziu EOF indevido |
| Teste exato de fases HTTP, após correção | 1 aprovado, 0 falhas; 27,77 s de testes |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 528 aprovados, 0 falhas, 2 ignorados; 262,17 s de testes |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | 528 aprovados, 0 falhas, 2 ignorados; verificação serial adicional |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | Exit 0 |
| `git diff --check` | Exit 0 |
| `git diff --check main...HEAD` | Exit 0; verificado novamente no commit da FIX |

Os dois ignorados continuam sendo os gates manuais Codex preexistentes. Main e
doc-tests também terminam sem falhas nas duas globais. Permanecem 15 warnings
unused/dead-code em debug, dois de fixtures na compilação de testes e 41 em
release; nenhum novo warning introduzido. Vite mantém o aviso de chunk main
maior que 500 kB; esse chunk não cresceu. O acréscimo nesta FIX no bundle IA é
aproximadamente 0,84 kB minificado / 0,23 kB gzip, sem dependência nova.

Autoauditoria final: poll automático não ativa feedback manual; clique manual
coalescido não sobrepõe IPC e termina seu feedback; callbacks tardios não escrevem
em hidden/unmount/epoch novo; falha automática preserva a última captura; toolbar
não muda de texto/largura a cada tick e não anuncia status periódico; timestamp
inválido permanece desconhecido; markers privados seguem ausentes do DOM.
A única alteração Rust está em módulo `#[cfg(test)]`. Comando/DTO/boundary,
permissions, policies, accounting, circuit, credentials, A–D e checklist humana
permanecem idênticos ao HEAD auditado. Não houve merge nem gate comercial/real
executado pelo agente. A comprovação visual na WebView e o gate final continuam
pendentes para a Luna e o usuário; LR-8 permanece aberta.


## Fechamento final da LR-8

**Resultado final: PASS completo em 05/10/2026.**

A trilha encerra com:

- LR-8A — quota model + telemetria factual: PASS;
- LR-8B — admission, concurrency e fila: PASS;
- LR-8C — rate accounting, budgets e recovery conservador: PASS;
- LR-8D — backoff, jitter, cooldown e circuit breaker: PASS;
- LR-8E — painel operacional, integração e gate final: PASS.

Candidata final da LR-8E auditada em `8cb9d6f5a980d361df5e736f4b9fb84ea476510b`, seguida do registro de aprovação humana do gate final na mesma branch.

### Evidência do gate final

A–D tiveram evidência humana real no hardware alvo. Em D, o painel mostrou `activeCalls=2/2`, `queueDepth=1/64` e drenagem ordenada da fila até o estado idle.

E–L foram validados por bateria automatizada local determinística, sem providers comerciais, usando Scheduler/Admission/Rate/Resilience/TaskGraph reais, providers controlados, loopback HTTP e SQLite temporário quando necessário. A reauditoria independente confirmou que os testes cruzam as fronteiras corretas e que a instrumentação auxiliar em produção está protegida por `#[cfg(test)]`.

M recebeu aprovação humana final: comportamento geral considerado aceitável e fechamento da LR-8 autorizado.

### Limitações não bloqueantes preservadas

- snapshots operacionais agregados não são globalmente atômicos;
- adapters de produção ainda podem não possuir `TokenUpperBound`;
- token accounting conservador pode permanecer unknown;
- health/cooldown são transitórios e in-memory;
- SQLite síncrono em mutações continua dívida de performance;
- a janela **IA e modelos** ficou longa e será reorganizada em rodada futura de UI/performance, sem solução de navegação congelada antecipadamente;
- preço/custo permanece unknown sem contrato configurado.

Nenhuma dessas limitações invalida as garantias de safety/capacity/routing alcançadas pela LR-8.

**Próxima trilha planejada após o fechamento: LR-8.5 Cognitive Resource Economy & Allocation, preservando o planejamento já existente na `main`.**
