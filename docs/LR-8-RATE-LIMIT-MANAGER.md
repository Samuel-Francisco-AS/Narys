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
