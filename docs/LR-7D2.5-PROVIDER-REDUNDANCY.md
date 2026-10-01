# LR-7D2.5 — provider redundancy + Gemini de-risking

Estado: **FIX DE HARDENING IMPLEMENTADA NA BRANCH; aguardando gate humano, sem merge na main.**
Posição no roadmap: **LR-7D2 → LR-7D2.5 → LR-7D3**.
Branch de implementação: `lr-7d25-provider-redundancy`.

## Motivação

Os gates reais de LR-7D1/LR-7D2 mostraram que Gemini pode responder com
`503 service_unavailable` e `Retry-After`, enquanto o restante da arquitetura
continua saudável. O Scheduler já trata cooldown/fallback, mas o conjunto real ainda
possui apenas Gemini e Groq. Isso deixa pouco espaço operacional quando um provider
está degradado e torna a redundância menos robusta do que a arquitetura pretende.

A decisão desta mini-trilha é explícita:

> **Gemini continua suportado, mas não é dependência funcional nem gate obrigatório
> da Luna. Nenhum Cognitive Provider individual deve ser necessário para
> Conversation, Summary, Orchestrator ou para o fechamento da LR-7.**

O Luna Core permanece a identidade e a autoridade. Providers são recursos
cognitivos substituíveis.

## Revalidação dos candidatos — 01/10/2026

A escolha foi reavaliada antes do início da implementação com os seguintes critérios:

- acesso real sem cobrança ou método de pagamento obrigatório para o caminho inicial;
- API direta e infraestrutura de inferência independente;
- streaming e uso via HTTP adequados ao contrato `Provider`;
- compatibilidade com Conversation, Summary e Orchestrator/PlanV1;
- modelo configurável e sem dependência comercial dentro do Scheduler;
- erro, timeout, cancelamento, usage e cooldown observáveis sem inventar sinais;
- possibilidade de manter secrets somente no Rust/Stronghold;
- adequação ao princípio de custo inicial R$0 do projeto;
- evitar agregador como uma das rotas principais para não criar um novo ponto único de falha.

### Escolha final 1 — Mistral API direta

**Mantida como primeira nova rota principal.**

Justificativa:

- Mistral Studio oferece Free mode com acesso de API habilitado por padrão e sem
  cartão obrigatório;
- o plano Free anuncia atualmente US$ 10/mês em créditos de API;
- a API expõe `POST /v1/chat/completions`, streaming SSE encerrado por
  `data: [DONE]`, usage e reasoning configurável;
- a infraestrutura é independente de Google, Groq e Cloudflare;
- o formato Chat Completions permite compartilhar somente infraestrutura realmente
  comum com Groq/Cloudflare, sem transformar semântica específica em abstração falsa;
- o provider continua direto: Mistral é responsável pela própria inferência.

**Modelo inicial selecionado:** `mistral-small-2603` — Mistral Small 4.

Razões para o modelo inicial:

- GA e Apache 2.0;
- 256k de contexto;
- unifica instruct, reasoning e coding;
- Chat Completions, function calling e structured output estão documentados;
- `reasoning_effort` é configurável;
- custo nominal baixo ajuda a preservar utilidade do crédito gratuito, sem tornar
  preço uma constante do runtime.

A integração não deve depender do alias `mistral-small-latest` como default
persistido: o ID inicial será explícito e continuará editável pela policy.

**Privacidade operacional:** no Free mode, Mistral informa que input/output pode ser
usado para melhoria/treinamento por padrão. Antes de usar conteúdo real do usuário
no gate humano, deve ser confirmado o opt-out em
`Admin → Privacy → Anonymous improvement data`. Enquanto isso não estiver
confirmado, testes reais devem usar prompts sintéticos/não sensíveis. Zero Data
Retention não é assumido nesta fase.

Fontes oficiais revalidadas:
- https://docs.mistral.ai/getting-started/quickstarts/studio/activate-and-generate-api-key
- https://mistral.ai/pricing/
- https://docs.mistral.ai/models/mistral-small-4-0-26-03
- https://docs.mistral.ai/api
- https://help.mistral.ai/en/articles/698531-why-am-i-hitting-api-rate-limits-and-how-do-i-increase-them
- https://help.mistral.ai/en/articles/455207-can-i-opt-out-of-my-input-or-output-data-being-used-for-training

### Escolha final 2 — Cloudflare Workers AI

**Mantida como segunda nova rota principal.**

Justificativa:

- Workers Free mantém 10.000 Neurons/dia sem cobrança;
- text generation possui limite padrão documentado de 300 requests/min, salvo modelos
  que exigem Workers Paid;
- Workers AI possui endpoint OpenAI-compatible em `/v1/chat/completions`;
- autenticação e execução podem permanecer inteiramente no Rust;
- a infraestrutura é independente de Gemini, Groq e Mistral;
- Cloudflare declara que Customer Content não é usado para treinar modelos ou
  melhorar serviços sem consentimento explícito;
- o catálogo permite trocar modelos sem substituir o provider inteiro.

**Modelo inicial selecionado:** `@cf/zai-org/glm-4.7-flash`.

Razões para o modelo inicial:

- permanece explicitamente disponível no Workers Free;
- é Cloudflare-hosted;
- 131.072 tokens de contexto;
- multilingual, reasoning e function calling;
- indicado para diálogo, instruction-following e multi-turn;
- funciona pelo endpoint Chat Completions OpenAI-compatible;
- oferece diversidade real de família de modelo, evitando que a quarta rota seja
  somente o mesmo GPT-OSS já usado via Groq.

Modelos que exigem Workers Paid, como variantes frontier explicitamente marcadas
como pagas, não entram no default da D2.5.

Fontes oficiais revalidadas:
- https://developers.cloudflare.com/workers-ai/platform/pricing/
- https://developers.cloudflare.com/workers-ai/platform/limits/
- https://developers.cloudflare.com/workers-ai/configuration/open-ai-compatibility/
- https://developers.cloudflare.com/workers-ai/models/glm-4.7-flash/
- https://developers.cloudflare.com/workers-ai/platform/data-usage/
- https://developers.cloudflare.com/changelog/post/2026-07-28-models-require-workers-paid/

## Candidatas substitutas

As substitutas são contingência, não targets adicionais obrigatórios desta fase.

### Substituta prioritária — NVIDIA NIM / build.nvidia.com

Entra se Mistral ou Cloudflare deixar de atender aos requisitos antes do gate final.

Motivos para permanecer como primeira substituta:

- build.nvidia.com mantém vários modelos marcados como `Free Endpoint`;
- existem modelos fortes para chat, coding, planning e agentic reasoning;
- NIM oferece contratos OpenAI-compatible em integrações/modelos suportados;
- é uma infraestrutura de inferência independente das quatro rotas planejadas.

Ressalva: o endpoint hospedado gratuito é tratado pela NVIDIA como API trial e os
limites/condições são menos adequados como fundação permanente do que o Free mode
da Mistral e a alocação diária explícita do Workers AI. Por isso fica como reserva,
não como escolha inicial.

Referências:
- https://build.nvidia.com/search?q=Nemotron
- https://docs.nvidia.com/nim/large-language-models/latest/get-started/prerequisites.html

### Substituta secundária — Cohere

Entra somente se uma segunda alternativa for necessária.

Motivos:

- trial/evaluation key gratuita;
- Chat documentado com 20 requests/min;
- limite mensal de 1.000 chamadas para trial;
- provider direto e independente.

Ressalva: a própria Cohere diferencia trial/evaluation de production keys, o que
torna a rota menos adequada como dependência operacional de longo prazo.

Referência:
- https://docs.cohere.com/v1/docs/rate-limits

### Não contam como substitutas principais nesta fase

- **OpenRouter:** útil futuramente como agregador/fallback, mas não conta como uma
  das duas novas infraestruturas independentes da D2.5.
- serviços cujo acesso gratuito atual seja apenas crédito temporário após ativação
  de cobrança não satisfazem o princípio inicial R$0 desta mini-trilha.

## Objetivo

Adicionar **duas novas Cognitive Providers reais** — Mistral e Cloudflare Workers AI —
e validar a LR-7D2 com um conjunto real maior de targets antes de avançar para o
task graph da D3.

Conjunto alvo do gate final:

```text
Gemini      → Google
Groq        → Groq / GPT-OSS
Mistral     → Mistral / Mistral Small 4
Cloudflare  → Cloudflare Workers AI / GLM-4.7-Flash
```

O objetivo não é manter quatro providers ativos para sempre. O objetivo é provar
que nenhuma marca individual é estruturalmente obrigatória e que o Core consegue
operar sobre múltiplas infraestruturas reais autorizadas.

## Princípios preservados

- nenhum provider recebe status de "LLM principal";
- todos os providers usam o mesmo contrato `Provider`;
- policy, identidade, memória, permissões, budgets e lifecycle continuam no Luna Core;
- credenciais permanecem separadas no SecretStore/Stronghold;
- a UI nunca recebe o valor de secrets salvos;
- `Fixed`, `Preferred` e `Auto` continuam sendo os modos da D2;
- Auto só escolhe entre targets explicitamente autorizados pelo usuário;
- não adicionar sinais fictícios de quota/latência/custo antes da LR-8;
- falha/cooldown de um provider não deve degradar a sessão inteira quando outro
  target autorizado e saudável puder executar o mesmo papel;
- nenhuma dependência comercial entra em validações semânticas ou no Scheduler;
- D2.5 não executa task graph, paralelismo ou ferramentas de D3/LR-13;
- reasoning interno de provider não deve ser persistido, retransmitido ao usuário
  nem confundido com resposta final;
- compatibilidade OpenAI de protocolo não significa compatibilidade semântica total.

## Estratégia de implementação — uma etapa

Por preferência de execução e pelo estado maduro da fundação após a D2, a LR-7D2.5
será implementada em **uma única branch/PR**, com checkpoints internos e commits
de FIX quando necessário. Os checkpoints abaixo não são fases independentes e não
exigem merge intermediário.

### Checkpoint 0 — baseline + transporte comum mínimo

Antes de adicionar providers:

- registrar baseline da suíte atual da D2;
- localizar duplicação comprovada entre adapters;
- extrair somente primitivas realmente comuns, por exemplo:
  - parsing bounded de `Retry-After`;
  - classificação comum de falha de rede onde a semântica coincidir;
  - espera cancelável;
  - framing/parsing SSE de Chat Completions somente quando o formato realmente coincidir;
- preservar payload, autenticação, modelo, erros específicos e particularidades de
  streaming dentro do adapter de cada provider;
- provar por testes que Gemini/Groq mantêm o comportamento da D2.

Para suportar Cloudflare sem novo hardcode frágil, o catálogo deve poder representar
**uma ou mais credenciais requeridas por integração**. Gemini, Groq e Mistral usam
uma chave; Cloudflare requer token + Account ID. O batch de presença da D2 deve
continuar abrindo o Stronghold uma única vez por operação e validar o conjunto
necessário sem expor valores.

Não criar migration SQLite só para acomodar Account ID se o SecretStore tipado
resolver o requisito de forma simples e segura.

### Checkpoint 1 — integrar Mistral e Cloudflare

#### Mistral

- adicionar `MistralApiKey` ao SecretStore;
- registrar `mistral` no catálogo e ProviderRegistry;
- default inicial `mistral-small-2603`;
- model continua configurável por target;
- integrar timeout handle existente;
- suportar streaming, usage, cancelamento e erros normalizados;
- mapear `reasoning_effort` somente onde o modelo suportar;
- descartar thinking chunks do fluxo público/persistido e emitir apenas resposta final;
- integrar Fixed/Preferred/Auto sem regra especial no Scheduler;
- validar Conversation real;
- validar Summary e Orchestrator/PlanV1 fail-closed.

#### Cloudflare Workers AI

- adicionar material de credencial tipado para API token e Account ID;
- registrar `cloudflare` no catálogo e ProviderRegistry;
- default inicial `@cf/zai-org/glm-4.7-flash`;
- model continua configurável por target;
- construir base URL no Rust sem colocar token/Account ID em logs ou frontend;
- usar `/v1/chat/completions` como contrato inicial;
- suportar streaming, usage, cancelamento e erros normalizados;
- classificar corretamente erros conhecidos de quota/capacidade/pagamento sem
  inventar telemetria de LR-8;
- integrar Fixed/Preferred/Auto;
- validar Conversation, Summary e Orchestrator quando compatíveis.

A camada comum não deve obrigar Mistral, Groq e Cloudflare a terem o mesmo conjunto
de parâmetros. Campos incompatíveis são omitidos ou recusados de forma explícita.

### Checkpoint 2 — gate multi-provider + fechamento

O gate final deve demonstrar:

1. Gemini desabilitado, em cooldown ou indisponível não impede Conversation quando
   existem outros targets autorizados;
2. Fixed funciona individualmente com os novos providers;
3. Preferred respeita ordem escolhida com 3+ targets reais e executa fallback real;
4. Auto escolhe somente entre providers autorizados e saudáveis;
5. affinity mantém exatamente a semântica definida na D2;
6. pelo menos um novo provider produz `PlanV1` válido no Orchestrator;
7. pelo menos um novo provider produz Summary válido;
8. parser/validação de PlanV1 e Summary continuam fail-closed;
9. restart preserva policy, ordem, model e estado seguro das credenciais;
10. cancelamento mantém terminal único e cleanup;
11. indisponibilidade de um provider não perde identidade, sessão, histórico ou TaskState;
12. Mistral só recebe conteúdo humano real após confirmação do opt-out de training;
13. nenhum secret aparece em SQLite, frontend, evento, log, erro público ou documentação;
14. Scheduler continua sem `match`/regra comercial para Gemini, Groq, Mistral ou Cloudflare;
15. LR-7D3/LR-8/task graph/tools continuam fora de escopo.

## FIXes

Finding de auditoria ou gate humano que viole um invariante da D2.5 gera commit
`FIX` na mesma branch. Depois de cada FIX relevante:

- repetir testes localizados;
- repetir a suíte técnica completa;
- repetir somente os gates humanos afetados;
- atualizar este documento com a causa, correção e evidência.

Uma FIX não deve ser usada para antecipar LR-7D3 ou LR-8.

## Plano de contingência — máximo três etapas

A execução padrão permanece **uma etapa**.

Somente se a implementação provar que o blast radius ficou grande demais para
auditar com segurança, a branch poderá ser dividida em no máximo três entregas:

1. **D2.5A — common transport + Mistral**;
2. **D2.5B — Cloudflare Workers AI**;
3. **D2.5C — redundância multi-provider + hardening/gate final**.

Essa decomposição é contingência, não o plano executado nesta branch. A fase
segue como uma única LR-7D2.5, com checkpoints e commits internos coerentes;
ela não muda escopo nem critério de fechamento.

## Relação com LR-7D3

A D3 deve exigir que uma tarefa real use **pelo menos dois Cognitive Providers
independentes, autorizados e elegíveis**, registre qual provider executou cada
subtarefa e consolide um único resultado.

Gemini pode participar, mas **não é obrigatório**.

## Relação com LR-8

D2.5 não antecipa o Rate Limit Manager completo.

Ela usa apenas os sinais já disponíveis ou diretamente necessários para erro/fallback:

- configured/enabled;
- capability;
- policy/ordem;
- cooldown/health já conhecido;
- score atual;
- affinity atual;
- `Retry-After`/erro factual quando o provider o expõe.

RPM/TPM/RPD/TPD dinâmicos, queue, token bucket, circuit breaker completo,
telemetria de latência/custo e decisões baseadas em quota real continuam na LR-8.

## Gates técnicos

Antes de fechamento, no mínimo:

- `npm run typecheck`;
- `npm run build`;
- `cargo check --manifest-path src-tauri/Cargo.toml`;
- `cargo test --manifest-path src-tauri/Cargo.toml`;
- `cargo check --release --manifest-path src-tauri/Cargo.toml`;
- `git diff --check`;
- rustfmt nos arquivos Rust tocados;
- auditoria independente do diff final;
- gate humano real com credenciais e rede para Mistral e Cloudflare.

Warnings preexistentes não devem ser apresentados como regressões da D2.5, mas
qualquer warning novo causado pela fase deve ser investigado.

## Critério de fechamento

LR-7D2.5 só pode ser declarada PASS completo quando:

- Mistral e Cloudflare Workers AI estiverem integrados e validados como rotas cognitivas reais,
  ou uma candidata tiver sido substituída formalmente conforme os critérios acima;
- Gemini puder ser retirado do conjunto ativo sem tornar a Luna incapaz de conversar
  e planejar nos papéis suportados;
- o Scheduler operar com 3+ targets reais sem regra por marca;
- pelo menos três infraestruturas de inferência independentes permanecerem
  operacionalmente utilizáveis;
- os gates técnico, auditoria independente e gate humano passarem;
- D3 ainda não tiver sido antecipada.

Somente depois disso começa **LR-7D3 — task graph mínimo + subtarefas independentes**.

## Estado da implementação nesta branch

Implementação candidata ao gate humano, não PASS final. O runtime agora registra
quatro providers (`gemini`, `groq`, `mistral` e `cloudflare`) no mesmo catálogo e
no mesmo Scheduler, sem regra comercial por marca. Mistral usa a API Chat
Completions oficial com `mistral-small-2603`; Cloudflare Workers AI usa o endpoint
OpenAI-compatible por Account ID com `@cf/zai-org/glm-4.7-flash`.

O catálogo representa uma lista tipada de credenciais por integração. O
SecretStore adiciona `MistralApiKey`, `CloudflareApiToken` e
`CloudflareAccountId`; a presença do Cloudflare só é configurada quando ambas
estão presentes e continua sendo consultada em um único batch serializado. A UI
aceita/substitui/remove os valores sem devolver valores persistidos ao frontend.

As integrações implementam streaming SSE, `[DONE]`, usage opcional, cancelamento,
timeouts de request/idle, limites de modelo, classificação de HTTP e
`Retry-After`. Nenhuma capability além de `text_stream` é anunciada para
Cloudflare; reasoning não é emitido como resposta nem persistido. O Cloudflare
não inventa métricas de Neurons, quota ou custo.

O gate de privacidade da Mistral permanece humano: no Free mode, probes devem usar
conteúdo sintético até a confirmação explícita do opt-out de
training/improvement. Esta branch não executou chamadas externas e não afirma
Zero Data Retention.

### FIX pós-auditoria independente

A FIX removeu `include_reasoning` do payload Mistral, corrigiu
`max_completion_tokens` para `max_tokens`, preservando `stream`,
`stream_options.include_usage` e `reasoning_effort`. O parser Mistral agora aceita
conteúdo typed com `thinking` e `text`, descarta integralmente reasoning, emite
somente text, aceita a transição para string e falha fechado para typed chunks
malformados. Os testes cobrem reasoning-only, fechamento de thinking com primeiro
text, continuação string, usage, `[DONE]`, payload completo e ausência do texto
privado no output público. Os níveis Low/Medium/High continuam anunciados porque
essa semântica agora é suportada pelo adapter; reasoning nunca entra em
`ProviderChunk`, `ProviderResponse`, histórico ou SQLite.

Cloudflare não envia mais `include_reasoning` nem `reasoning_effort`. Erros HTTP
leem no máximo 64 KiB, sem logging do corpo, e os códigos conhecidos são
classificados bounded/fail-closed: 3036 como `QuotaExceeded`, 3040 como
`Unavailable` elegível a retry/fallback, e 5035 como `QuotaExceeded`; 401/403
autenticação permanecem `Authentication` quando não há código conhecido.
Cloudflare agora lê token e Account ID por uma única operação `with_client(false,
...)`, com um lock, uma abertura e uma snapshot. Set e delete usam uma única
operação `with_client(true, ...)`, sem persistência parcial se a operação falhar.
Nenhum valor é retornado por status/presence ou serializado.

As primitivas realmente comuns de transporte foram extraídas para
`cognition/transport.rs`: `Retry-After` bounded, classificação de erro de rede e
espera cancelável. Payloads, autenticação, regras de modelo, semântica de erro e
reasoning continuam específicas dos adapters. Scheduler, Gemini e Groq não
receberam regra comercial nem mudança semântica intencional.

Resultados reais dos gates desta FIX: `npm run typecheck` PASS, `npm run build`
PASS, `cargo check --manifest-path src-tauri/Cargo.toml` PASS, `cargo test
--manifest-path src-tauri/Cargo.toml` PASS (268 testes, 0 falhas, 2 ignorados),
`cargo check --release --manifest-path src-tauri/Cargo.toml` PASS e `git diff
--check` PASS. O build mantém somente o warning preexistente de chunk JavaScript
grande; os warnings Rust de dead code/imports não são regressões desta FIX.
Nenhuma chamada externa real foi feita pela suíte; o gate humano de
Mistral/Cloudflare permanece separado. D3/LR-8 continuam fora de escopo.
