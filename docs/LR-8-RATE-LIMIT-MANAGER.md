# LR-8 — Rate Limit Manager completo

Estado: **PLANEJADA / liberada após LR-7 = PASS completo em 03/10/2026.**
Pré-requisito: `main@71c9a650ffc92459811309d7749593d2244b9d46` ou posterior, contendo o fechamento da LR-7D3.
Próxima subfase: **LR-8A — modelo de quota + telemetria factual**.

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

## LR-8A — implementação candidata (03/10/2026)

**IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**
Branch: `lr-8a-rate-model-telemetry`, base
`977caacb76f0b867432cbaab8e54b469e6e9ee26`.
**LR-8B continua bloqueada** até auditoria/PASS da 8A. Nenhum merge ou gate
humano de providers comerciais é declarado por esta implementação.

### Arquitetura e ownership

`cognition/telemetry.rs` contém a autoridade única `TelemetryStore`, pertencente
ao `Scheduler` do `ProviderRuntime` compartilhado no composition root.
Conversation, Summary, Orchestrator e Workers continuam usando esse mesmo
Scheduler; não possuem ledgers de telemetria próprios. Os schedulers mock de
DEV e os fixtures têm stores isolados, como já tinham runtimes isolados.

O store usa um único `Mutex<BTreeMap<provider_id, State>>`, inicializado somente
com IDs registrados e machine-safe (ASCII alfanumérico, `_`, `-`, até 64 bytes).
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
`ProviderUsage`, `SchedulerUsage` e `ProviderResponse`. O default serve aos
providers locais/fixtures, cuja invocação é `execute`; adapters remotos devem
sobrescrever a fronteira de envio. Todos os quatro de produção a sobrescrevem.
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
   observadas no SSE; Groq non-streaming observa o usage da resposta parseada.
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
| Gemini Interactions | início real de envio | input/output/total e thought opcional no terminal válido | Retry-After opcional | unknown | unknown |
| Groq streaming | início real de envio | input/output/total quando usage válido | Retry-After opcional | unknown | unknown |
| Groq JsonSchema non-streaming | início real de envio | input/output/total quando resposta/usage válidos | Retry-After opcional | unknown | unknown |
| Cloudflare Workers AI | início real de envio | input/output/total opcional; ausência preservada | Retry-After opcional | unknown | unknown |
| Mistral | início real de envio | input/output/total opcional; ausência preservada | Retry-After opcional | unknown | unknown |

Não foram adicionados headers específicos de quota/rate-limit por marca.
A matriz descreve os adapters inspecionados, não afirma que o serviço comercial
nunca forneça outros headers. A descoberta/normalização de quotas comerciais
continua não implementada. `observe_quota` aceita fatos normalizados e testados,
mas nenhuma fonte de configuração de quota/UI é ligada na 8A.

O único header observado é o `Retry-After` já suportado operacionalmente.
Sua sintaxe é `delay-seconds` inteiro não negativo ou HTTP-date, conforme
[RFC 9110, §10.2.3](https://www.rfc-editor.org/rfc/rfc9110.html#name-retry-after),
consultada em 03/10/2026. `factual_retry_after_at` tem relógio controlável;
metadados ausentes, negativos, fracionários, ambíguos/duplicados, datas passadas,
overflow ou duração acima do bound local existente de sete dias produzem unknown.
Não expõe o header bruto e não persiste headers. O parser operacional anterior
continua intacto, inclusive clamp, fallback de 429, 503/Unavailable e cooldown;
um clamp operacional não é apresentado como fato do provider.

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

Gates sobre o código final:

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

Rust emitiu 15 warnings no check debug e 41 no release. Quatro correspondem aos
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
- Agregação por provider, não por conta/modelo/janela; amostras são fatos
  históricos, não saldo disponível nem estimativa de cobrança remota.
- Adapters mantêm seus contratos atuais de parsing: uso numérico não retornado
  por um parser que falhou não é extraído de corpo bruto ou deduzido do output.
- Nenhum header comercial, configuração de quota ou preço está conectado ainda.
- Nenhuma alteração de routing/score/affinity, backoff, policy de cooldown,
  limites de tarefa, identidade/memória/avatar, ferramentas ou executores.
- Admission/fila/concurrency enforcement: 8B; buckets/budgets/janelas/restart
  safety: 8C; unificação cooldown/health/circuit breaker: 8D; painel: 8E.
- Auditoria deve conferir especialmente a fronteira do send nos quatro adapters,
  a extensão aditiva do trait e a distinção entre somas reportadas, reporters,
  requests iniciadas e accounting conservador D3.

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
