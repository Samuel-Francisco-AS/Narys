# LR-8.5A — Resource Domains, Access Paths & Cognitive Variants

Estado: **PLANEJAMENTO APROVADO — implementação ainda não iniciada.**

Branch de trabalho: `lr-8.5a-resource-domains-variants`  
Base: `main@3829acca06c8a3a97090b7c921e31a0baf9a8970`

## Objetivo

Criar a linguagem provider-agnostic que permita ao Luna Core representar recursos
cognitivos heterogêneos e a granularidade interna de cada recurso sem alterar ainda
o routing real.

Ao final da LR-8.5A, o runtime deve conseguir descrever com precisão:

- qual recurso cognitivo existe;
- a que classe ele pertence;
- por qual access path ele é usado;
- qual billing domain governa sua quota/custo;
- quais modelos/variantes esse recurso realmente expõe;
- quais níveis de reasoning/effort cada modelo realmente suporta;
- quais capabilities pertencem ao recurso e quais pertencem especificamente à variante;
- quais fatos econômicos/operacionais são known ou unknown e sua provenance;
- quais fatos da LR-8 podem ser associados ao recurso/modelo sem duplicar a autoridade de rate limiting.

A LR-8.5A **descreve**. A LR-8.5B **escolhe**.

Nenhum score econômico, mudança de Auto, fallback, affinity ou decisão de gasto deve
entrar nesta subfase.

## Baseline observada na main

A implementação atual já fornece bases úteis que devem ser preservadas:

- `ProviderRegistry` e `AgentRegistry` são independentes;
- `ProviderConfig` e `AgentConfig` possuem contracts/capabilities próprios;
- `CognitiveTargetPolicy` e `ProviderTarget` já carregam `model` e
  `thinking_level`;
- `ProviderCapabilities` documenta explicitamente que representa a união do que o
  adapter implementa, não uma garantia sobre cada modelo;
- o catálogo atual expõe thinking no nível da integração, portanto não pode ser usado
  como prova de suporte por modelo;
- a LR-8 já possui `Fact<T>`, `Provenance`, `QuotaScope::Provider`,
  `QuotaScope::Model`, `QuotaSnapshot`, `RateSnapshot` e accounting factual;
- a LR-8 continua sendo a autoridade de rate limits, reservas, accounting e bloqueio
  operacional. LR-8.5A apenas consome/normaliza seus fatos.

## Boundary arquitetural

A nova camada deve ser ortogonal a CognitiveProvider e SpecialistAgent.

~~~text
                         Luna Core
                            │
                 Cognitive Resource Catalog
                            │
          ┌─────────────────┴──────────────────┐
          │                                    │
   Cognitive Providers                  Specialist Agents
 ProviderRegistry/Scheduler             AgentRegistry/runtime
          │                                    │
          └───────── descriptors/facts ────────┘
                            │
                  LR-8 telemetry/rate facts
~~~

A implementação preferencial é um módulo de Core separado de `cognition` e
`agents`, por exemplo `src-tauri/src/cognitive_resources/` ou nome equivalente.
O nome final pode mudar se a estrutura existente indicar opção melhor.

Não mover `AgentBackend` para `Provider`, não reutilizar `ProviderConfig` dentro
de `AgentConfig` e não transformar Specialist Agents em Cognitive Providers.

## A1 — Resource Identity & Economic Domains

Criar contratos mínimos, evitando campos especulativos.

Conceitos esperados:

~~~text
CognitiveResource
ResourceId
ResourceClass
ProviderFamily
AccessPath
BillingDomain
BillingKind
AvailabilityFact
~~~

`ResourceClass` deve preservar pelo menos:

- CognitiveProvider;
- SpecialistAgent;
- LocalSupport.

`ProviderFamily` e `AccessPath` não devem obrigar o Core a conhecer marcas por enum
fechada. IDs validados/normalizados são preferíveis, desde que continuem tipados e
bounded.

`BillingDomain` deve possuir identidade própria: recursos da mesma família não
compartilham quota/saldo por inferência.

`BillingKind` pode representar semânticas como:

- IncludedAllowance;
- FreeTier;
- PrepaidCredits;
- MeteredBilling;
- Unknown.

Isso é classificação local/configurada, não tabela comercial.

### Invariantes A1

1. UNKNOWN != FREE.
2. UNKNOWN != PAID.
3. Dois access paths da mesma família podem possuir billing domains independentes.
4. O Core não contém preço, plano comercial ou limite de assinatura hardcoded.
5. Identificadores possuem limites de tamanho/formato e falham fechado.
6. Snapshots não expõem credenciais, account IDs sensíveis ou material de autenticação.

## A2 — Model & Effort Profiles

Adicionar granularidade intrarrecurso sem substituir os tipos de policy atuais.

Conceitos esperados:

~~~text
ModelProfile
ModelAvailability
CognitiveCapability / CapabilitySet
EffortProfile
ExecutionVariant
~~~

Uma variante selecionável deve poder ser identificada conceitualmente por:

~~~text
resource + access_path + model + effort
~~~

### ModelProfile

Deve permitir representar, quando disponível:

- model id;
- availability factual/configurada;
- capabilities específicas do modelo;
- quality/capability metadata normalizada;
- níveis de effort suportados;
- contexto/latência/custo/quota como known ou unknown;
- provenance dos fatos.

Não copiar automaticamente `ProviderCapabilities` do adapter para todos os modelos
como se fosse capacidade factual de cada variante.

### EffortProfile

O contrato deve comportar níveis como `low`, `medium`, `high`, `xhigh` e
possíveis valores futuros/provider-specific.

**Não ampliar automaticamente o enum legado `ThinkingLevel` nesta fase.**

`ThinkingLevel { Low, Medium, High }` continua válido nas policies existentes.
A nova camada deve possuir representação mais geral e bridges explícitas quando um
backend concreto puder mapear uma variante para o contrato legado.

Um effort não suportado:

- não é inventado;
- não é aproximado silenciosamente;
- não deve aparecer como candidato disponível.

## A3 — Facts & LR-8 Bridge

Não criar outro Rate Limit Manager.

A nova camada deve consumir fatos existentes da LR-8 por referência/snapshot ou bridge
estreita.

Fatos relevantes já disponíveis:

- quota provider/model;
- remaining;
- reset;
- provenance;
- usage/accounting;
- estado operacional quando apropriado.

A associação entre modelo no Resource Catalog e `QuotaScope::Model` deve ser explícita
e sem herança automática para outros modelos.

Specialist Agents e LocalSupport podem permanecer economicamente `Unknown` até suas
integrações futuras publicarem fatos reais.

### Provenance

A LR-8 já define provenance operacional. A LR-8.5A pode reutilizar esse vocabulário
quando semanticamente correto ou adicionar uma provenance própria estreita para fatos
de catálogo/configuração, sem falsificar origem.

Qualquer metadata de capacidade, qualidade ou custo relativo deve indicar origem,
por exemplo:

- integration/catalog fact;
- user configuration;
- local runtime observation;
- external/provider fact;
- unknown.

Os nomes finais pertencem ao contrato Rust.

## Capability bridge

Provider e Agent capabilities permanecem tipos distintos nas suas respectivas
fronteiras.

A nova camada pode introduzir uma descrição comum de capacidades, por exemplo
`CognitiveCapability`/`CapabilitySet`, com conversões explícitas:

~~~text
ProviderCapabilities ──► resource/model descriptor
AgentCapabilities    ──► resource/model descriptor
~~~

Essa descrição comum existe para comparação futura na LR-8.5B; ela não unifica os
runtimes nem move autoridade.

## A4 — Synthetic Gate

A LR-8.5A fecha somente com testes determinísticos sem consumo de provider comercial.

Fixture mínima obrigatória:

~~~text
Family A
├── AccessPath included
│   └── BillingDomain allowance-A
│       ├── Model cheap
│       │   ├── low
│       │   └── medium
│       └── Model strong
│           ├── medium
│           ├── high
│           └── xhigh
└── AccessPath direct-api
    └── BillingDomain prepaid-B
        └── Model api-model

Family B
└── AccessPath free-api
    └── BillingDomain free-C
        └── Model alt
~~~

O gate deve provar:

1. múltiplos recursos heterogêneos coexistem;
2. dois access paths da mesma família conservam billing domains independentes;
3. múltiplos modelos pertencem ao mesmo recurso;
4. effort support é model-specific;
5. modelo indisponível permanece representável e não é mascarado como disponível;
6. capabilities podem diferir por modelo;
7. known e unknown permanecem distintos;
8. provenance sobrevive ao snapshot;
9. quota de `QuotaScope::Model` não vaza para outro modelo;
10. resource/model descriptors não carregam segredo;
11. Provider e Agent registries continuam independentes;
12. nenhuma tabela comercial ou regra por marca é necessária;
13. nenhum teste de A decide "qual candidato é melhor".

## Não objetivos

A LR-8.5A não deve:

- alterar `auto_score()`;
- alterar ordem de targets;
- mudar fallback/retry/affinity;
- introduzir reserve/scarcity scoring;
- decidir entre modelos;
- autorizar gasto;
- integrar Copilot completo;
- ampliar o Codex para execução operacional;
- integrar APIs pagas novas;
- consultar preços online durante runtime;
- inferir qualidade por nome comercial;
- persistir cartão ou dados de cobrança;
- substituir LR-8 admission/rate/resilience;
- transformar todos os recursos em Provider.

## Superfície esperada

A implementação deve buscar uma API pequena e testável. Exemplo conceitual, não
prescritivo:

~~~text
ResourceCatalog
  register_resource(...)
  resource(...)
  snapshot(...)

CognitiveResourceDescriptor
  id
  class
  family
  access_path
  billing_domain
  availability
  capabilities
  models

ModelProfile
  model_id
  availability
  capabilities
  supported_efforts
  facts/provenance
~~~

Evitar service locator global, estado mutável desnecessário e duplicação dos registries
existentes.

## Persistência

Persistência nova só deve ser criada se houver necessidade concreta nesta subfase.

Preferência inicial:

- catálogo estrutural derivado das integrações/registries;
- fatos runtime/snapshot mantidos pelas autoridades existentes;
- configuração explícita persistida apenas quando já houver UX/contract justificando.

Não criar migration apenas para "guardar tudo" preventivamente.

## Arquivos provavelmente envolvidos

Lista indicativa; o implementador deve confirmar pelo código:

- `src-tauri/src/lib.rs`;
- novo módulo de cognitive resources;
- `src-tauri/src/cognition/catalog.rs`;
- `src-tauri/src/cognition/types.rs` somente se bridge mínimo for necessário;
- `src-tauri/src/agents/types.rs` somente se bridge mínimo for necessário;
- `src-tauri/src/cognition/telemetry.rs` / `rate.rs` somente para API read-only
  estreita, sem mover autoridade;
- testes dedicados da LR-8.5A.

Evitar mudanças em Scheduler/TaskGraph/Resilience salvo import/integração estritamente
necessária e sem alteração comportamental.

## Estratégia de implementação

Ordem sugerida:

1. introduzir tipos IDs/enums/facts mínimos e validação;
2. implementar descriptors de resource/billing/access path;
3. implementar ModelProfile/EffortProfile;
4. criar capability bridge sem unir registries;
5. expor bridge read-only para fatos LR-8;
6. compor ResourceCatalog/snapshot;
7. criar fixtures sintéticas e gate;
8. rodar testes existentes completos relevantes;
9. revisar diff procurando alteração acidental de routing.

## Critério de fechamento

LR-8.5A recebe PASS quando:

- o catálogo representa Resource → AccessPath/BillingDomain → Model → Effort;
- Provider, Agent e LocalSupport cabem no contrato sem serem transformados no mesmo runtime;
- known/unknown/provenance são preservados;
- fatos LR-8 são consumidos sem duplicar sua autoridade;
- capacidades model-specific podem divergir das capabilities amplas do adapter;
- effort support é model-specific;
- billing domains independentes permanecem independentes;
- não existe seleção econômica nova;
- testes sintéticos cobrem o gate;
- suíte anterior não regride;
- auditoria independente confirma que nenhum boundary LR-7/LR-8 foi violado.

## Auditoria pós-implementação

A reauditoria deve procurar especialmente:

- brand logic escondida em enums/matches;
- preço/quota hardcoded;
- herança indevida provider → model;
- `Unknown` convertido em zero/free/unavailable;
- thinking global aplicado a todos os modelos;
- AgentConfig/ProviderConfig unificados por conveniência;
- ResourceCatalog tomando autoridade de Scheduler ou RateLimitManager;
- mudanças silenciosas em Auto/fallback;
- snapshots vazando segredo;
- migrations sem necessidade comprovada.
