# LR-8.5A — Resource Domains, Access Paths & Cognitive Variants

Estado: **IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente e gate.**

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


## Implementação candidata — 05/10/2026

Registro histórico da candidata `35b601daeb82d4697c3328c99a086f4b6f1c966a`.
As decisões de identidade econômica e fatos de modelo/effort abaixo foram
refinadas pelo FIX-1 e FIX-2 ao final deste documento. A seção FIX-2 registra o contrato
vigente depois da correção incremental.

Pré-condições verificadas antes de qualquer edição: branch obrigatória
`lr-8.5a-resource-domains-variants`, fetch + fast-forward sem divergência,
workspace limpo e HEAD remoto/local
`9d906c4ed74ae7c0098cc081d53e5669488e8706`. A base comum com `origin/main`
foi confirmada em `3829acca06c8a3a97090b7c921e31a0baf9a8970`.
Os quatro documentos da trilha e os contratos atuais de cognition, agents,
catalog, policy, registries, telemetry/rate e Scheduler foram lidos antes das
alterações. Esta candidata não declara PASS da LR-8.5A nem conclusão da LR-8.5.

### Contratos e arquivos

Novo módulo público de Core `src-tauri/src/cognitive_resources/`:

| Arquivo | Responsabilidade |
|---|---|
| `mod.rs` | Superfície descritiva e exports; sem execução ou singleton |
| `ids.rs` | ResourceId, ProviderFamily, AccessPath, BillingDomainId, RuntimeId, ModelId, EffortId, QualityLabel e CatalogError |
| `facts.rs` | CatalogFact e CatalogProvenance; bridge explícita de Fact da LR-8 |
| `capabilities.rs` | CognitiveCapability, CapabilitySet e bridges distintas de ProviderCapabilities/AgentCapabilities |
| `types.rs` | CognitiveResource, ResourceIdentity, ResourceClass, ResourceOrigin, BillingDomain/BillingKind, Availability, ModelProfile/ModelFacts, EffortProfile, ExecutionVariant e MonetaryAmount |
| `catalog.rs` | ResourceCatalog, registro validado, lookup, enumeração e snapshots |
| `lr8.rs` | Projeção read-only de ProviderTelemetrySnapshot e RateSnapshot |
| `tests.rs` | Universo sintético obrigatório e regressões de contratos |

Arquivos existentes alterados: `src-tauri/src/lib.rs` (somente declaração do
módulo) e este documento (estado e registro da candidata). Nenhum arquivo de
Scheduler, policy, adapters, AgentBackend, registries, telemetry, rate,
resilience, TaskGraph, frontend ou migrations foi modificado.

### Decisões arquiteturais

- O catálogo possui somente descriptors; não armazena `Arc<dyn Provider>`,
  `AgentBackend`, SecretStore, Database ou referências a managers. A derivação usa
  `CognitiveResource::from_provider_config` e `from_agent_config` sobre as configs
  consultadas nos registries existentes. Family, access path e billing domain
  são fornecidos explicitamente pelo chamador, sem tabelas por marca no Core.
- `ResourceOrigin` distingue IDs locais dos runtimes Provider/Agent e suporte
  Local. Registro exige classe/origem compatíveis. Mesmo ID textual pode existir
  nos dois registries; o catálogo não os funde. Um runtime Provider representa
  um contexto LR-8: registrar o mesmo binding em outro access path é recusado,
  evitando atribuir a mesma quota a dois domínios supostamente independentes.
  Access paths independentes usam bindings independentes.
- BillingDomainId não deriva de Family. Reutilizar explicitamente um domínio
  exige descriptor de domínio idêntico; divergência falha antes do registro.
  BillingKind suporta IncludedAllowance, FreeTier, PrepaidCredits, MeteredBilling
  e Unknown. Kind informado não prova custo marginal, saldo ou quota.
- IDs são bounded e validados também na desserialização. Labels locais aceitam
  ASCII alfanumérico, `-`, `_`, `.` até 64 bytes; ModelId aceita adicionalmente
  `/`, `:`, `@` até 128 bytes. Não há trim, case folding ou alias implícito.
  Formatos mais amplos aceitos pela policy legada não são reescritos: a bridge
  nova recusa um identificador incompatível, sem mudar o runtime legado.
- Cardinalidades locais: 256 recursos, 128 modelos/recurso, 32 efforts/modelo.
  São bounds estruturais, sem significado comercial. Registro é atômico, não
  substitui duplicatas e não oferece referência mutável a descriptors armazenados.
  Enumeração por ID torna snapshots determinísticos; não representa ranking.
- `CatalogFact` mantém Unknown ou Known com valor, timestamp opcional e origem
  tipada. CatalogProvenance adiciona somente IntegrationCatalog e RuntimeContract;
  fatos operacionais reutilizam exatamente `Provenance` da LR-8, encapsulada em
  Operational. Não foi ampliado o contrato de telemetria/front-end da LR-8 para
  acomodar catálogo. Timestamp e metadata numérica usam o bound JSON-safe existente.
- Capabilities do resource são fatos do contrato do runtime. As bridges preservam
  os booleanos próprios de cada família e deixam as capacidades da outra família
  unknown, sem equivaler ToolCalling a ToolUse. Models começam sem capabilities
  herdadas; cada capacidade tem Known(true), Known(false) ou Unknown próprio.
  Ausência no CapabilitySet significa Unknown.
- Enabled lido da config tem provenance RuntimeContract: o registro pode ter
  vindo de defaults do Core ou de configuração humana. Não se afirma origem de
  usuário por inferência. Enabled=true não prova disponibilidade remota;
  enabled=false descreve indisponibilidade por configuração registrada.
- Models e supported_efforts distinguem catálogo Unknown de lista Known vazia.
  `ModelProfile::unknown` aceita nome local explicitamente fornecido, mas não
  inventa capabilities, disponibilidade ou esforços. Não lê/copia os níveis
  globais de `cognition::catalog::Integration.thinking`.
- EffortId aceita low/medium/high/xhigh e labels futuros/provider-specific.
  `from_thinking_level`/`try_thinking_level` são bridges explícitas apenas de
  vocabulário: low, medium e high correspondem exatamente; xhigh e outros
  retornam LegacyEffortNotRepresentable. Isso não prova suporte do backend;
  `ModelProfile::effort` exige declaração explícita naquele modelo.
  `ThinkingLevel` e as policies legadas não foram ampliados.
- `describe_variant` descreve uma tupla resource/access path/billing/model/effort
  solicitada explicitamente. Não enumera, ranqueia ou escolhe candidatos. Rejeita
  effort ausente no catálogo ou suporte Unknown; variantes declaradas indisponíveis
  continuam descritíveis com seus fatos preservados. Effort=None não inventa um
  nível default nem afirma sua disponibilidade.
- ModelFacts comporta contexto, latência, qualidade como label sem ordenação,
  custo por invocação e consumo nas unidades de allowance declaradas pela fonte.
  MonetaryAmount usa moeda explicitamente fornecida e unidades milionésimas;
  não calcula preço por token, conversões, saldo ou autorização de gasto.
  Nesta fase não há fonte de produção de preço/qualidade/saldo; permanecem Unknown.

### Consumo read-only da LR-8

`ResourceCatalog::snapshot(&telemetry, &rate)` recebe DTOs já capturados pelas
respectivas autoridades. O chamador deve capturar rate pela API existente
`RateLimitManager::read_only_snapshots()`. A bridge não chama managers, não
refresha estado vivo, não faz IO e não calcula saldo/reset a partir de usage.

O join usa somente ResourceOrigin::Provider(RuntimeId), nunca Family ou
BillingDomain. TelemetryFacts conserva usage provider-scoped, provider_quotas
separado de model_quotas, retry hint histórico, timestamp e context generation.
Somente modelos explicitamente descritos e com spelling exato recebem fatos
model-scoped. Quota ausente significa Unknown; não existe herança provider → model,
model → provider ou model → outro model.

RateFacts conserva constraints de scope Provider separadas das constraints por
Model, suas sources ExternalFact/LocalPolicy/DailyBudget, provenance, external
facts, capacity/effective_remaining opcionais, consumed/reserved, uncertainty,
reset e saturação. Facts retidos por rate não substituem os últimos fatos da
telemetry. Capturas mantêm timestamps separados e não prometem atomicidade global.
Gerações divergentes e snapshots/scopes telemetry duplicados são rejeitados.
Um novo snapshot após rotação lê somente o estado atualizado das autoridades;
o catálogo não tem cache de fatos operacionais.

Agent e Local não recebem fatos LR-8 de Providers, mesmo se tiverem a mesma
family/ID nominal. Nenhuma quota, reset, preço ou saldo foi criado para eles.
Snapshots excluem payloads/raw outcomes, credentials, headers, URLs autenticadas,
account IDs remotos, material Stronghold, pagamentos e backend handles. Os IDs
são labels locais públicos fornecidos pelo Core; validação sintática não é um
redator de segredos e não autoriza usar account/token como label.

### Gate sintético e validação

A fixture usa somente Family A/B e caminhos included/direct-api/free-api, com
allowance-A/prepaid-B/free-C. Cheap declara low/medium; Strong declara
medium/high/xhigh; API-Model e Alternative têm metadata Unknown. Há ainda modelo
Unavailable, SpecialistAgent e LocalSupport, capabilities divergentes, fatos
Known(0) e Unknown, várias provenances e quotas/constraints LR-8 reais sintéticas.

Testes cobrem coexistência, independência de domínio/path/binding, múltiplos
modelos, esforços específicos, bridge legada sem aproximação, disponibilidades
Unavailable/Unknown, capabilities sem herança, facts/provenance/timestamps,
quota exata por modelo (inclusive diferenças de case), ausência de vazamento para
outro access path, snapshots sem mutação das autoridades, sources externas versus
policies/budget locais, usage parcial provider-scoped, rotação e geração,
validação de IDs/fatos/cardinalidades, registro atômico e snapshots sem segredos.
O teste de segurança usa Provider com execute que falha se chamado e backend
Agent com markers privados. Nenhum teste LR-8.5A decide qual candidato é melhor.
Não há inferência comercial, conta real ou teste manual Codex executado.

Uma rodada dirigida inicial teve 12 sucessos e uma falha: o teste novo de rotação
esperava remoção dos buckets externos. A LR-8 mantém os índices de buckets e
substitui sua evidência por Unknown. A asserção foi corrigida para exigir
capacity/effective_remaining/provenance ausentes e QuotaSnapshot Unknown;
nenhuma alteração na autoridade de rate foi feita para acomodar o teste.

Resultados técnicos finais no código candidato:

| Gate | Resultado |
|---|---|
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | Exit 1 por diferenças preexistentes nos mesmos 44 arquivos detectados antes da implementação; nenhum arquivo novo com diff de formatação |
| `/home/sam/.cargo/bin/rustfmt --edition 2021 --check src-tauri/src/cognitive_resources/*.rs` | Exit 0; todos os arquivos novos formatados |
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources` | 16 aprovados, 0 falhas; validação dirigida e inclusão na suíte final |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=4` | Exit 0; 558 aprovados, 0 falhas, 2 ignorados; 260,91 s de testes; main e doc-tests sem falhas |
| cognition na suíte completa | 370 aprovados, 0 falhas |
| agents na suíte completa | 110 aprovados, 0 falhas, 2 ignorados |
| LR-8 na suíte completa | rate_tests 69; telemetry_tests 33; admission_tests 18; resilience_tests 76; operational_tests 8; gate LR-8E 14; todos aprovados |
| TaskGraph runtime na suíte completa | 13 aprovados, 0 falhas |
| `git diff --check` e `git diff --cached --check` | Exit 0 |
| Typecheck/frontend | Não aplicável: nenhum contrato/comando/frontend exposto foi alterado |

A suíte global usou quatro threads para limitar a contenção de Stronghold já
registrada nos gates anteriores; nenhum timeout ou teste foi alterado. Os dois
ignorados são `real_app_server_handshake` e
`manual_final_codex_agent_bridge_gate`, manuais Codex preexistentes. Compilação
manteve 15 warnings de biblioteca e dois de fixtures de testes preexistentes;
nenhum warning novo no módulo. A execução completa final não apresentou falhas.

O check global de formatação também foi executado antes das alterações e já
falhava na base. A comparação das listas de arquivos com diferenças confirmou
os mesmos 44 arquivos antes/depois, sem arquivo novo. Não se aplicou formatação
global para evitar refactor cosmético fora do escopo. A primeira tentativa de
chamar `rustfmt` sem caminho não o encontrou no PATH; a ferramenta instalada em
`/home/sam/.cargo/bin/rustfmt` foi usada para formatar e verificar o módulo.
Essa limitação de PATH foi resolvida e não impediu os gates do código novo.

### Limitações e dívidas adiadas

- Sem migration, persistência nova, comando Tauri, UI ou catálogo global gerenciado.
  A superfície é derivável sob demanda de configs/registries e facts capturados;
  não há catálogo factual por modelo de produção disponível para preencher campos
  desconhecidos. A integração global atual não é fonte suficiente de effort support.
- Provenance/timestamps descrevem origem e momento; não criam política de validade
  temporal, refresh remoto ou afirmação de saldo atual. Captura não é admissão.
- Metadata de custo por invocação e quota units depende de fonte explícita; não
  existe pricing web ou schema comercial automático.
- LR-8 permanece autoridade de constraints, reservations, accounting, cooldown,
  resilience e bloqueios. Preservam-se seus limites aceitos, inclusive snapshots
  não atômicos entre autoridades e token bounds ausentes nos adapters atuais.
- LR-8.5B: elegibilidade, quality floor, comparação/ranking, selection de resource/
  model/effort, scarcity/reserve policy, spend authorization e budgets monetários.
- LR-8.5C: checkpoints, handoff e continuidade entre recursos/variantes, com
  idempotência e proteção contra duplicação de efeitos.
- Adapters paid novos e quotas agentivas reais continuam nas integrações futuras;
  nenhum Codex/Copilot completo foi antecipado.

**Auto/fallback/retry/affinity/routing/admission/resilience/TaskGraph não tiveram
comportamento alterado.** O diff de produção existente é somente a declaração
aditiva do módulo em lib.rs. A candidata aguarda auditoria independente e gate;
esta auto-revisão e os testes não substituem esse fechamento.

## FIX-1 — Contratos cognitivos/econômicos e allowance genérico

Registro histórico da correção em `4458c8e45f1d95e8ab31af42231896cd52a2b707`.
A unicidade por unidade e o consumo único descritos nesta seção foram refinados
pela FIX-2 abaixo, inclusive a limitação de múltiplas janelas da mesma unidade.

Estado mantido: **IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente e gate.**
Correção incremental sobre `35b601daeb82d4697c3328c99a086f4b6f1c966a`,
exclusivamente em `lr-8.5a-resource-domains-variants`.

### Problemas identificados pela auditoria

1. `EffortProfile` descrevia ID/disponibilidade, mas não capacidade/economia.
   `QualityLabel` opaco e fatos somente no modelo não permitiam avaliações
   futuras sem interpretar nomes comerciais ou low/medium/high/xhigh.
2. Saldo monetário e fatos operacionais LR-8 não representavam allowance
   não monetário genérico de Agents/Local, com limit/remaining/reset independentes.
3. `BillingDomain` incluía kind/balance dentro de `ResourceIdentity`; o registro
   exigia igualdade de observações para compartilhar o domínio. Capturas
   diferentes podiam impedir a reutilização de uma identidade estável.

### Contrato de modelo/effort adotado

`ModelFacts` mantém `quality: CatalogFact<QualityLabel>` e
`context_tokens: CatalogFact<u64>`, e compõe `execution: ExecutionFacts`.
`EffortProfile` mantém ID/disponibilidade e recebe `facts: ExecutionFacts`.
A composição compartilhada contém:

| Campo | Representação |
|---|---|
| `cognitive_tier` | `CatalogFact<CognitiveTier>` |
| `relative_cost` | `CatalogFact<RelativeCostTier>` |
| `latency_ms` | `CatalogFact<u64>` |
| `monetary_cost` | `CatalogFact<MonetaryAmount>` |
| `allowance_cost` | `CatalogFact<AllowanceConsumption>` com unidade e quantidade explícitas |

Todos os campos começam Unknown. Cada Known preserva provenance/timestamp
próprios. Latência/custos descrevem uma invocação segundo a fonte explícita;
não se calculam preço por token ou consumo a partir do nome do effort.
O antigo `quota_cost` sem unidade foi refinado para `allowance_cost` tipado.

`CognitiveTier` é uma escala ordinal normalizada 0..=255, provider-agnostic;
valores maiores representam capacidade maior segundo a avaliação explicitamente
configurada/catalogada. `RelativeCostTier` é uma escala ordinal 0..=255 de
consumo relativo crescente. Ambas são ordenáveis, bounded e validadas inclusive
na desserialização; não representam razões, ranking automático ou garantia de
qualidade. Tier relativo zero não afirma custo monetário/allowance zero.
Nenhum modelo ou effort recebe tier por padrão. A atribuição e a comparabilidade
das avaliações dependem da fonte, que fica preservada no fato.

`ExecutionVariant` agora preserva `model_facts` e `effort_facts` separadamente.
Não há herança, soma, override ou resolução de conflitos entre os dois perfis;
effort Unknown continua Unknown mesmo com modelo conhecido. Effort=None mantém
`effort_facts=None`, sem inventar semântica para o default do backend.
EffortId continua aberto e sem ordenação por capacidade/custo. `ThinkingLevel`
permanece Low/Medium/High; a bridge existente continua recusando xhigh e valores
não representáveis, sem aproximação. Não há algoritmo de quality floor.

### Allowance econômico genérico

`EconomicFacts`, associado ao descriptor do recurso fora de sua identidade,
contém `billing_kind: CatalogFact<BillingKind>`,
`monetary_balance: CatalogFact<MonetaryAmount>` e `allowances: Vec<AllowanceState>`.
Cada `AllowanceState` declara:

- `unit: AllowanceUnit`: Requests, Tokens, Credits, Percent ou Custom com
  `AllowanceUnitId` validado/bounded;
- `limit` e `remaining`: `CatalogFact<u64>` independentes;
- `reset`: `CatalogFact<Timing>` independente, reutilizando somente o DTO LR-8
  DelayMs/UnixMs, sem manager, QuotaScope ou instrução de refill.

Não há inferência de percentual, denominação monetária de credits ou conversão
entre unidades. Percent, quando explicitamente declarado, usa pontos percentuais
inteiros 0..=100; limit Unknown não vira 100. Unidades restantes usam o bound
JSON-safe `MAX_FACT_VALUE`, assim como valores temporais/timestamps. Quando
limit e remaining são conhecidos, remaining acima de limit falha na validação.
São aceitas até 16 dimensões por descriptor, uma observação por unidade;
duplicatas e excesso de cardinalidade falham antes de alterar o catálogo.
Lista ausente/vazia significa dimensões não reportadas, sem saldo implícito.

O contrato serve às três classes. Um SpecialistAgent pode preencher credits,
limit 100, remaining 5 e reset conhecido diretamente no descriptor, sem
ProviderRegistry, provider HTTP, RateLimitManager ou migration. O catálogo não
classifica exhausted/unlimited, não bloqueia invocação nem autoriza gasto.
Fatos LR-8 continuam separados na projeção existente, sem conversão automática
para allowance genérico e sem herança provider → model. Nenhuma integração real
Codex/Copilot ou fonte de quota/preço de produção foi adicionada.

### Estabilidade do BillingDomain

`BillingDomain` contém somente `BillingDomainId`. Kind/saldo/allowances são
observações em `CognitiveResource.economics`, fora de `ResourceIdentity`.
Dois descriptors explicitamente vinculados ao mesmo ID podem ser registrados
mesmo com valores, timestamps ou provenances diferentes; snapshots conservam
essas evidências separadas por recurso. IDs distintos continuam independentes.

A consulta estreita `ResourceCatalog::domain_economics` retorna fatos apenas
quando todas as observações daquele domínio são idênticas, incluindo provenance
e timestamps. Divergência retorna `ConflictingEconomicFacts`; domínio ausente
retorna `BillingDomainNotFound`. Não há merge, latest-wins, soma ou escolha
silenciosa de fonte. A identidade não depende do sucesso dessa consulta.
O catálogo continua sendo uma descrição capturada, sem ledger vivo ou API de
mutação de fatos registrados. Uma nova captura pode construir outro catálogo.

### Arquivos e regressões

- Novos: `src-tauri/src/cognitive_resources/economics.rs` e `fix1_tests.rs`.
- Alterados no módulo: `catalog.rs`, `ids.rs`, `mod.rs`, `types.rs`, `tests.rs`.
- Documentação: `docs/LR-8.5A-RESOURCE-DOMAINS-VARIANTS.md`.

Os 16 testes anteriores foram preservados, adaptando apenas localização de
kind/saldo, composição de ModelFacts e inclusão de facts no EffortProfile.
A antiga rejeição de registro por fatos do mesmo domínio foi substituída pelo
gate de identidade estável e consulta econômica fail-closed.
Nove testes novos cobrem:

1. Medium/high/xhigh do mesmo modelo com tiers 2/3/4 e consumo 1/2/4,
   distintos no snapshot/variante, com provenance preservada.
2. Mesmo ID high com capacidade/custo/latência diferentes em recursos distintos.
3. Modelos do mesmo recurso com tiers explícitos e provenances diferentes.
4. Fatos Unknown do effort sem herança do modelo; xhigh sem bridge aproximada.
5. Allowance credits de SpecialistAgent, 100/5/reset conhecido, sem autoridades LR-8.
6. Limit/remaining/reset Unknown em todas as classes e campos parcialmente
   conhecidos sem derivação de percentual ou disponibilidade.
7. Identidade compartilhada com capturas divergentes, consulta fail-closed e
   independência de outro billing domain.
8. Evidência idêntica compartilhada e coexistência de dinheiro/unidades
   não monetárias sem conversão.
9. Bounds de tiers/fatos/timing/cardinalidade, unidades duplicadas e atomicidade.

Nenhum teste escolhe um candidato ou interpreta semanticamente nomes.

### Validação do FIX-1

| Gate | Resultado |
|---|---|
| `/home/sam/.cargo/bin/rustfmt --edition 2021 --check src-tauri/src/cognitive_resources/*.rs` | Exit 0; módulo inteiro, incluindo arquivos tocados/novos |
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources` | Exit 0; 25 aprovados, 0 falhas: 16 regressões anteriores e 9 testes FIX-1 |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=4` | Exit 0; 567 aprovados, 0 falhas, 2 ignorados; 254,50 s de testes; main e doc-tests sem falhas |
| cognition / agents na suíte completa | 370 / 110 aprovados; agents com 2 manuais ignorados |
| Gates LR-8 na suíte completa | rate_tests 69, telemetry_tests 33, admission_tests 18, resilience_tests 76, operational_tests 8 e LR-8E 14; todos aprovados |
| TaskGraph runtime na suíte completa | 13 aprovados, 0 falhas |
| `git diff --check` no diff incremental | Exit 0 |
| Typecheck/frontend | Não aplicável; nenhum contrato exposto ao frontend alterado |

Os dois ignorados continuam sendo os gates manuais Codex
`real_app_server_handshake` e `manual_final_codex_agent_bridge_gate`.
Foram mantidos os 15 warnings de biblioteca e dois de testes preexistentes;
nenhum warning novo do FIX-1 e nenhuma falha. O check de rustfmt foi limitado ao
módulo: o check global tem as diferenças preexistentes em 44 arquivos registradas
na candidata original; esses arquivos não foram reformatados. Os gates LR-8 e
as suítes de cognition/agents foram executados pela suíte completa, sem rodada
redundante ou alteração de testes desses runtimes.

### Limitações e fronteiras preservadas

As escalas ordinais não possuem calibração de produção fornecida pelo Core.
Allowance usa quantidades inteiras e uma dimensão por unidade; múltiplas janelas
da mesma unidade podem usar IDs custom explicitamente distintos. Não há seleção
de fonte, freshness policy, refresh remoto ou persistência nova. Sem migration.
ProviderCapabilities/AgentCapabilities, ProviderRegistry/AgentRegistry e os
backends continuam independentes; o FIX-1 não altera seus arquivos ou execução.
LR-8 permanece autoridade operacional de Providers.

Quality-floor selection, ResourceAllocator, scarcity scoring, reserve policy,
spend authorization e routing econômico permanecem em LR-8.5B; handoff e
continuidade em LR-8.5C. Scheduler, Auto/auto_score, fallback, retry, ordering,
affinity, admission, resilience e TaskGraph permanecem behavior-neutral:
nenhum arquivo desses runtimes foi alterado pelo FIX-1. Não houve merge para main
nem declaração de PASS ou conclusão da LR-8.5.

## FIX-2 — Dimensões de allowance e consumo multidimensional

Estado: **IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente e gate.**
Correção incremental sobre `4458c8e45f1d95e8ab31af42231896cd52a2b707`,
exclusivamente em `lr-8.5a-resource-domains-variants`. Antes das alterações,
branch/HEAD local e remoto foram conferidos, a branch foi sincronizada por
fast-forward (já atualizada) e o workspace estava limpo.

### DimensionId separado da unidade

A auditoria identificou que a unicidade por `AllowanceUnit` impedia múltiplas
janelas da mesma unidade. `AllowanceDimensionId` agora identifica explicitamente
uma dimensão no billing domain, com spelling exato, até 64 bytes ASCII
alfanuméricos/`-`/`_`/`.`; vazio, caracteres inválidos e excesso de tamanho são
recusados também na desserialização. O Core não interpreta janelas a partir do ID.

`AllowanceState` contém `id: AllowanceDimensionId`, `unit: AllowanceUnit`,
`limit: CatalogFact<u64>`, `remaining: CatalogFact<u64>` e
`reset: CatalogFact<Timing>`. `EconomicFacts.allowances` exige unicidade por ID,
permitindo rolling-5h e weekly com Percent, ou requests-per-minute e
requests-per-day com Requests, no mesmo domínio. ID duplicado falha com
`DuplicateAllowanceDimension`, mesmo se a unidade/observação for diferente.
Registro continua validado atomicamente.

Requests, Tokens, Credits, Percent e Custom foram preservados. Custom representa
uma unidade realmente customizada; não identifica janelas. Percent continua
explicitamente limitado a pontos percentuais inteiros 0..=100, sem derivação.
Limit/remaining/reset continuam independentemente Unknown ou Known com
provenance/timestamp. `BillingDomain` permanece somente uma identidade estável;
observações econômicas e consulta consolidada fail-closed mantêm o contrato FIX-1.

### Consumo por múltiplas dimensões

`ExecutionFacts.allowance_costs: Vec<AllowanceConsumption>` substitui o fato
único. Cada entrada contém `dimension_id: AllowanceDimensionId`,
`unit: AllowanceUnit` e `amount: CatalogFact<u64>`; a quantidade possui sua própria
provenance/timestamp e pode ser Unknown independentemente das outras entradas.
O mesmo ID repetido dentro de um ExecutionFacts é recusado, mesmo com unidades
ou quantidades diferentes. Tanto states quanto consumos são bounded a 16 entradas;
quantidades e timestamps mantêm os bounds numéricos existentes.

Uma invocação pode descrever rolling-5h → 2 Percent e weekly → 1 Percent ao mesmo
tempo. Os fatos de modelo e effort preservam suas listas separadamente no
snapshot e em ExecutionVariant, sem soma, conversão, herança ou override.
Uma dimensão ausente da lista é consumo não reportado, nunca zero; lista vazia
também não afirma invocação gratuita. Uma dimensão declarada com amount Unknown
preserva essa incerteza explicitamente. Somente Known(0) afirma quantidade zero.
O catálogo não cria consumos com base no estado de allowance, nem cria estados
a partir de consumos. Fatos de consumo podem existir sem saldo/janela conhecidos;
não há cruzamento automático com evidência LR-8 ou interpretação de IDs.

### Arquivos e gate adicional

- Novo: `src-tauri/src/cognitive_resources/fix2_tests.rs`.
- Alterados: `economics.rs`, `ids.rs`, `mod.rs`, `tests.rs`, `fix1_tests.rs`
  no mesmo módulo, e este documento.

Os 25 testes anteriores foram adaptados somente para IDs explícitos, lista de
consumos e provenance da quantidade. O teste antigo de duplicatas agora exige
ID repetido; não se perdeu cobertura de valores/fatos/bridges/identidades.
Seis testes adicionais cobrem:

1. SpecialistAgent com duas janelas Percent, 100/60 e 100/20, resets distintos,
   snapshot/provenance preservados e consulta ao mesmo domínio, sem LR-8.
2. Duas dimensões Requests com IDs distintos; duplicatas falham atomicamente,
   inclusive quando a unidade diverge.
3. Consumos multidimensionais conhecidos em modelo/effort, preservados
   independentemente no snapshot e ExecutionVariant, com provenance/timestamp.
4. Consumo Unknown explícito e dimensão sem consumo reportado, sem criação de
   zeros, herança ou dimensões automáticas.
5. Duplicatas/bounds de consumo nos dois perfis, incluindo quantidade Percent,
   timestamps e cardinalidade, com rejeição atômica.
6. Validação/desserialização bounded de IDs e preservação das cinco unidades;
   Percent inválido é recusado também no estado econômico.

### Validação da FIX-2

| Gate | Resultado |
|---|---|
| `/home/sam/.cargo/bin/rustfmt --edition 2021 --check src-tauri/src/cognitive_resources/*.rs` | Exit 0; módulo inteiro formatado/verificado |
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources` | Exit 0; 31 aprovados, 0 falhas: 25 anteriores e 6 novos |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=4` | Exit 0; 573 aprovados, 0 falhas, 2 ignorados; 240,89 s de testes; main e doc-tests sem falhas |
| cognition / agents na suíte completa | 370 / 110 aprovados; agents com 2 manuais ignorados |
| Regressões LR-8 na suíte completa | rate_tests 69, telemetry_tests 33, admission_tests 18, resilience_tests 76, operational_tests 8 e LR-8E 14; todos aprovados |
| TaskGraph runtime na suíte completa | 13 aprovados, 0 falhas |
| `git diff --check` no diff incremental | Exit 0 |
| Typecheck/frontend | Não aplicável; nenhum contrato exposto ao frontend alterado |

Os dois ignorados continuam sendo os testes manuais Codex
`real_app_server_handshake` e `manual_final_codex_agent_bridge_gate`.
A compilação manteve os 15 warnings de biblioteca e dois de testes preexistentes,
sem warning novo desta correção. Não houve falha de teste. Os gates LR-8 foram
executados na suíte completa, sem repetir rodadas dirigidas já cobertas.
Os 44 arquivos com diferenças preexistentes no check global de formatação
continuam fora do diff; foi usado somente rustfmt do módulo, sem refactor legado.

### Fronteiras e limitações

Sem migration, integração real, ledger, depletion, accounting ou seleção.
ProviderRegistry, AgentRegistry e autoridade LR-8 não foram modificados.
SpecialistAgent continua publicando allowance sem se tornar Provider.
ThinkingLevel não foi ampliado e a bridge de xhigh continua fail-closed.
Model/effort economics e identidade estável do domínio permanecem separados.
As quantidades continuam inteiras; não foi introduzida política de freshness,
calibração de produção ou reconciliação de fontes.

Scheduler, Auto/auto_score, fallback, retry, affinity, admission, resilience e
TaskGraph permanecem behavior-neutral; nenhum arquivo desses runtimes foi tocado.
Scarcity, ResourceAllocator, quality-floor selection, spend authorization e
handoff continuam adiados para LR-8.5B/C. Não há declaração de PASS ou conclusão
da LR-8.5, nem merge para main.
