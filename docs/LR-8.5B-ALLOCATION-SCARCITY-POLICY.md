# LR-8.5B — Allocation, Model/Effort Selection & Scarcity Policy

Estado: **IMPLEMENTAÇÃO EM ANDAMENTO — B1 candidata; demais checkpoints não implementados.**

Branch de trabalho: `lr-8.5b-allocation-scarcity-policy`  
Base: `main@a6eec1b63655d860279f606bc55766255903f1fe`

## Objetivo

Transformar os fatos e descriptors criados na LR-8.5A em uma decisão de Auto
determinística, explicável e conservadora:

> entre os recursos e variantes autorizados que realmente conseguem executar a tarefa,
> qual vale consumir agora?

A LR-8.5B introduz **alocação**. Ela não executa ferramentas, não move autoridade de
rate/admission/resilience, não faz handoff depois de output parcial e não integra
Codex/Copilot reais.

A unidade conceitual de decisão continua:

~~~text
resource + access_path + model + effort
~~~

O ResourceAllocator escolhe. O Scheduler executa.

## Baseline observada na main

A implementação atual possui:

- `ResourceCatalog` descritivo, sem singleton ou autoridade de execução;
- `CognitiveResource`, BillingDomain, EconomicFacts e allowances multidimensionais;
- ModelProfile/EffortProfile com `ExecutionFacts`, CognitiveTier e RelativeCostTier;
- bridge read-only para fatos LR-8;
- Scheduler com Fixed/Preferred/Auto;
- Auto legado baseado em posição da policy, prioridade do registry e affinity;
- Scheduler como autoridade de execução, retry/fallback, admission, rate e resilience;
- CognitiveRolePolicy persistindo targets exatos de provider/model/thinking;
- constraint `UNIQUE(role, provider_id)` na persistência;
- Scheduler rejeitando provider_id duplicado na mesma cadeia;
- catálogo de integrações ainda expondo apenas default_model e thinking no nível da
  integração, portanto insuficiente para afirmar catálogo completo por modelo.

Essas limitações devem orientar o desenho; não devem ser "resolvidas" por inferência.

## Princípios

1. **Hard gates antes de score.**
2. **Unknown não recebe interpretação econômica inventada.**
3. **Capability/quality suficiente é requisito, não bônus opcional.**
4. **Mais capacidade que o necessário não é automaticamente melhor.**
5. **Quota incluída também possui custo de oportunidade.**
6. **Recurso pago exige autorização explícita quando houver evidência factual de cobrança.**
7. **Fixed permanece explícito; Preferred permanece ordenado.**
8. **Somente Auto recebe reordenação econômica.**
9. **Scheduler continua autoridade operacional.**
10. **Allocator não executa provider, não reserva quota e não altera ledger.**
11. **Nenhuma tabela por marca, preço comercial ou modelo famoso entra no Core.**
12. **Toda decisão de Auto deve ser explicável por componentes observáveis.**

## Arquitetura alvo

~~~text
CognitiveRolePolicy / Task requirements
                 │
                 ▼
       Candidate / Variant Builder
                 │
     ResourceCatalog + LR-8 facts
                 │
                 ▼
          ResourceAllocator
       ┌─────────┴─────────┐
       │ hard eligibility  │
       │ scarcity/spend    │
       │ deterministic     │
       │ score + rationale │
       └─────────┬─────────┘
                 │
       AllocationPlan / ranked
                 │
                 ▼
              Scheduler
       admission/rate/resilience
       retry/fallback/execution
~~~

O Scheduler não deve aprender regras comerciais. O Allocator não deve aprender como
executar HTTP/provider/agent.

## Decomposição

A implementação deve ser dividida em quatro blocos auditáveis dentro da mesma trilha.
Cada bloco pode receber FIXes antes do próximo.

### B1 — Allocation Contracts & Hard Gates

Criar um módulo de alocação provider-agnostic, preferencialmente sob
`cognitive_resources`.

Conceitos esperados, nomes não prescritivos:

~~~text
AllocationRequest
AllocationPolicy
AllocationProfile
AllocationCandidate
AllocationDecision
AllocationPlan
CandidateExclusion
ScoreBreakdown
QualityFloor
PaidUsePolicy
ReservePolicy
ScarcityState
VariantSelectionMode
~~~

#### Authorization boundary

O Allocator recebe apenas candidatos previamente autorizados pelo Core/policy.

Ele não descobre providers por conta própria e não pode introduzir um recurso que não
esteja autorizado para a unidade de trabalho.

#### Availability

- Known(Unavailable) exclui.
- Known(Available) permite seguir.
- Unknown não é convertido em Available.

Para integração legada de Cognitive Providers, compatibilidade técnica do adapter
(`supports_invocation`) continua sendo evidência operacional separada; o catálogo não
deve falsificar availability para reproduzir esse check.

#### Capabilities

Capabilities obrigatórias devem ser satisfeitas antes do score.

Para candidatos genéricos controlados pelo Allocator:

- Known(true) satisfaz;
- Known(false) exclui;
- Unknown não satisfaz um requisito obrigatório.

Não copiar capabilities amplas do adapter para ModelProfile por conveniência.

A bridge específica de Cognitive Provider pode continuar usando o check técnico
existente do Scheduler/adapter como autoridade da invocação concreta.

#### Quality floor

Uma tarefa pode declarar `minimum_cognitive_tier` opcional.

Regra de resolução sugerida:

- se há effort selecionado e o effort possui CognitiveTier Known, usar esse tier como
  avaliação específica da execução;
- caso contrário, usar o CognitiveTier Known do modelo;
- se existe quality floor e nenhum tier adequado é conhecido, o candidato não prova
  suficiência e deve falhar fechado;
- nunca inferir tier a partir de model_id ou effort_id;
- nunca converter QualityLabel opaco em tier.

Não implementar ML de qualidade ou previsão probabilística nesta fase.

### B2 — Scarcity, Spend Guard & Deterministic Scoring

#### ScarcityState

Derivar estado somente de facts suficientes.

Estados conceituais:

- Comfortable
- Reduced
- Reserve
- Exhausted
- Unknown

A policy deve conter thresholds locais configuráveis, não limites comerciais.

Para uma dimensão com limit + remaining conhecidos, pode-se derivar fração restante.
Quando limit for desconhecido:

- remaining = 0 pode provar esgotamento daquela dimensão;
- remaining > 0 sem limit não prova percentual de conforto;
- ausência/Unknown permanece Unknown.

Nenhum reset é inventado.

#### Consumo multidimensional

Uma variante pode consumir várias AllowanceDimensions.

Somente dimensões declaradas em `ExecutionFacts.allowance_costs` são consideradas
consumidas.

Para cada dimensão consumida:

- localizar o state correspondente quando existir;
- preservar unit consistency já garantida pela A;
- calcular impacto apenas com facts Known;
- a dimensão mais restritiva pode dominar o estado econômico da variante;
- ausência de state não significa consumo zero nem quota infinita.

Não somar unidades diferentes.

#### LR-8

Provider/model constraints da LR-8 podem alimentar scarcity, respeitando scope e
source.

Não converter automaticamente toda constraint em BillingDomain allowance. São sinais
operacionais paralelos.

O Allocator pode usar capacidade/remaining/reset factual da LR-8 para evitar um
candidato claramente saturado/sem quota, mas RateLimitManager continua sendo a
autoridade que realmente admite ou bloqueia a chamada.

#### ReservePolicy

Reserve é um custo de oportunidade, não um bloqueio universal.

Uma variante em Reserve:

- recebe forte penalidade em Auto;
- ainda pode vencer quando for a única que satisfaz quality/capability;
- ainda pode ser usada por Fixed;
- não é marcada como tecnicamente indisponível.

Exhausted factual para uma dimensão consumida deve tornar o candidato inelegível em
Auto quando não houver outra interpretação operacional legítima.

#### PaidUsePolicy

Semântica mínima:

~~~text
Deny
AllowKnownCostWithinBudget
~~~

Uma evidência explícita de custo monetário/cobrança só pode participar de Auto pago
quando:

- policy autoriza;
- custo da variante é Known;
- moeda/budget são compatíveis;
- custo não excede budget restante conhecido.

Unknown monetary cost nunca é convertido em zero.

BillingKind Unknown não recebe bônus de "grátis".

Não criar auto-purchase, refill ou alteração de plano.

#### AllocationProfile

Perfis mínimos:

- Economy
- Balanced
- Fast

Eles alteram pesos locais, não hard gates.

Sinais permitidos quando Known:

- policy preference;
- registry priority como preferência secundária;
- affinity/context continuity;
- CognitiveTier suficiente;
- RelativeCostTier;
- scarcity;
- monetary cost;
- latency;
- switching/context reconstruction signal.

Economy deve favorecer menor custo/oportunidade quando a capacidade é suficiente.
Fast pode aceitar maior custo para reduzir latência/switching.
Balanced fica entre os dois.

Excesso de CognitiveTier acima do floor não deve receber bônus automático. O objetivo
é a menor capacidade suficiente quando o restante for equivalente.

#### Unknown no score

Unknown não ganha bônus nem penalidade factual.

Exemplo:

~~~text
candidato A: latency known 100ms
candidato B: latency unknown
~~~

não significa automaticamente que B é mais rápido ou mais lento.

O score pode usar somente componentes conhecidos e um baseline determinístico.

#### Determinismo e explicabilidade

Toda decisão deve produzir algo equivalente a:

~~~text
winner
ranked candidates
excluded candidates + reason
score breakdown
profile
quality-floor result
scarcity result
spend-guard result
tie-break
~~~

Tie-break deve ser estável e documentado.

Não registrar prompts, secrets ou material privado nos traces.

### B3 — Variant Expansion & Provider Auto Bridge

Esse é o bloco que altera comportamento real do Auto.

#### Regra fundamental

**Fixed e Preferred não são reordenados pelo ResourceAllocator.**

- Fixed continua usando a variante explicitamente configurada.
- Preferred continua respeitando a ordem do usuário.
- Auto pode usar ResourceAllocator.

#### Não quebrar UNIQUE(role, provider_id)

A persistência atual e o Scheduler operam com no máximo um target por provider.

Não remover essa invariância apenas para encaixar variantes.

Em vez disso:

1. a policy autoriza os providers atuais;
2. um target pode manter sua variante explícita como fallback/Fixed;
3. quando VariantSelectionMode = Auto, o Candidate Builder pode expandir somente
   ModelProfile/EffortProfile realmente conhecidos no ResourceCatalog;
4. o Allocator compara variantes completas;
5. antes de entregar a cadeia ao Scheduler, mantém **no máximo uma variante escolhida
   por provider**;
6. Scheduler recebe novamente provider IDs únicos.

Assim o Scheduler não precisa entender fallback entre dois modelos do mesmo provider.

Uma falha depois do início da chamada segue as regras atuais. Troca de variante após
output parcial pertence à LR-8.5C, não a B.

#### Model catalog Unknown

Se o catálogo de variantes do provider for Unknown:

- não inventar alternativas;
- a variante explicitamente configurada na policy pode continuar sendo o candidato
  legado, desde que passe os checks técnicos existentes;
- seus fatos econômicos desconhecidos permanecem Unknown.

#### ThinkingLevel legado

Quando um target existente possui Low/Medium/High, a bridge para EffortId pode ser
usada como identidade da variante pedida.

Isso não prova catálogo/suporte por modelo; o adapter continua validando a invocação.

xhigh e outros efforts só podem entrar no Candidate Builder quando forem
explicitamente publicados por ModelProfile.

#### Scheduler integration

Preferência arquitetural:

- construir `AllocationPlan` antes da execução;
- Scheduler consome a ordem/variante/score já decididos para Auto;
- Scheduler continua aplicando health, admission, rate, retry e fallback;
- não aplicar o affinity antigo novamente se ele já entrou no score do Allocator;
- `ranked_provider_ids` e `run` devem compartilhar a mesma regra de ordering.

Não duplicar dois sistemas de Auto concorrentes.

Eventos devem distinguir decisão econômica do Auto legado, com rationale sanitizada.

### B4 — Policy Persistence, Settings Surface & Final Gate

A configuração econômica deve permanecer separada da identidade/facts dos recursos.

Preferência: policy própria por CognitiveRole, em vez de encher
`CognitiveRolePolicy` de detalhes econômicos sem relação com routing.

Contrato persistível mínimo sugerido:

~~~text
role
allocation_profile
variant_selection_mode
minimum_cognitive_tier (optional)
paid_use_policy
max_paid_currency (optional)
max_paid_micros (optional)
reduced_below_percent
reserve_below_percent
~~~

Os nomes/schema finais devem ser auditados antes da migration.

Regras:

- defaults migration-safe;
- Fixed/Preferred permanecem semanticamente iguais;
- paid use deve defaultar conservadoramente;
- thresholds são policy local, não quota comercial;
- Percent deve usar bounds já existentes;
- moeda e budget devem formar par válido;
- persistência corrupta falha fechada.

Uma UI mínima pode expor profile e paid authorization se necessária para tornar a
policy realmente configurável. Não ampliar a UI para um editor completo de billing
domains nesta fase.

## Relação com o Auto legado

O score atual é:

- posição da policy como componente primário;
- prioridade do registry como componente secundário;
- affinity como bônus dependente de contexto.

A LR-8.5B deve preservar esses sinais, mas movê-los para um score explicável junto aos
novos componentes.

A posição da policy continua sendo preferência do usuário, não uma verdade econômica.

Affinity continua sendo custo de continuidade, não proof de qualidade.

Nenhuma preferência por marca é permitida.

## Compatibilidade e rollout

A implementação deve ser incremental.

B1/B2 devem ser módulos puros e testáveis antes de qualquer mudança do Scheduler.

Somente depois dos gates B1/B2 o B3 altera Auto.

Se facts econômicos forem insuficientes, o Allocator deve permanecer determinístico e
não inventar valores. A presença de poucos facts não deve tornar o runtime
inoperável.

O desenho deve distinguir:

- candidato inelegível por hard gate;
- candidato elegível com facts econômicos Unknown;
- candidato explicitamente pago sem autorização;
- candidato tecnicamente bloqueado depois pelo LR-8/resilience.

## Synthetic Gate obrigatório

Nenhum teste precisa gastar quota real.

O gate deve provar pelo menos:

### 1. Least sufficient capacity

~~~text
cheap/medium: tier 2, custo baixo
strong/high: tier 4, custo alto
floor = 2
→ cheap/medium
~~~

Com floor = 4:

~~~text
→ strong/high
~~~

Sem inferência pelos nomes.

### 2. Scarce included allowance

~~~text
resource A: tier suficiente, allowance em Reserve
resource B: tier suficiente, allowance Comfortable
profile Economy/Balanced
→ B
~~~

Se só A satisfizer quality floor:

~~~text
→ A ainda é elegível e vence
~~~

Reserve não é bloqueio absoluto.

### 3. Exhausted

Uma dimensão Known remaining=0 consumida pela variante impede sua seleção Auto quando
há alternativa elegível.

LR-8 continua responsável por bloquear a chamada real.

### 4. Paid deny

Recurso explicitamente paid, mesmo tecnicamente melhor:

~~~text
PaidUsePolicy::Deny
→ não selecionado
~~~

### 5. Paid allow + known budget

Paid permitido + custo Known dentro do budget pode participar e vencer quando
justificado.

Custo Unknown ou moeda incompatível falha fechado para auto-spend.

### 6. Unknown economics

Unknown não é tratado como free, paid, zero, unlimited, exhausted, fast ou slow.

Candidato pode permanecer elegível quando nenhum hard gate depende do fato Unknown,
mas não ganha vantagem econômica fictícia.

### 7. Affinity versus scarcity

Pequena affinity não deve necessariamente vencer uma penalidade forte de Reserve.

Em Fast ou com grande switching/context cost, continuidade pode vencer quando a
policy documentada justificar.

### 8. Multiple variants same resource

O allocator compara múltiplos modelos/efforts do mesmo resource e escolhe uma variante.

O bridge para Scheduler reduz isso para uma variante por provider.

### 9. Fixed/Preferred preserved

Fixed não sofre reordenação econômica.
Preferred mantém exatamente a ordem explícita.

### 10. Unsupported/unavailable

Modelo Unavailable, effort não suportado ou capability Known(false) não participa.

Quality floor com tier insuficiente/Unknown deve falhar fechado.

### 11. Multidimensional scarcity

Uma variante que consome duas allowances deve considerar ambas sem somar unidades.
A dimensão mais restritiva pode dominar o custo de oportunidade.

### 12. Deterministic tie

Mesmas evidências → mesmo vencedor independentemente da ordem de HashMap/registry.

### 13. No 429 → paid leap

RateLimited isolado nunca autoriza recurso pago.

### 14. No partial-output handoff

A alteração de ranking não modifica a proteção atual contra fallback depois de output
parcial.

## Não objetivos

A LR-8.5B não deve:

- integrar Codex/Copilot reais;
- executar SpecialistAgent via Scheduler;
- implementar handoff/checkpoint;
- refazer TaskGraph;
- comprar créditos;
- alterar planos;
- consultar pricing web em runtime;
- hardcodar preços comerciais;
- atribuir tier por nome de modelo;
- inferir ordem low/medium/high/xhigh pelo texto;
- somar currencies/unidades incompatíveis;
- transformar scarcity em rate limiter;
- substituir affinity por memória semântica nova;
- repetir efeitos externos;
- remover a independência ProviderRegistry/AgentRegistry.

## Arquivos provavelmente envolvidos

Indicativo:

- `src-tauri/src/cognitive_resources/allocation.rs` ou submódulo equivalente;
- `src-tauri/src/cognitive_resources/scarcity.rs`;
- `src-tauri/src/cognitive_resources/*_tests.rs`;
- `src-tauri/src/cognition/scheduler.rs` somente a partir do B3;
- `src-tauri/src/cognition/types.rs` para transportar plano/contexto se necessário;
- `src-tauri/src/cognition/policy.rs` somente se a ponte exigir;
- persistence/migration para AllocationPolicy somente no B4;
- settings command/UI somente no B4 se necessário.

Evitar tocar adapters individuais para ensinar economia.

## Ordem recomendada de implementação

1. B1 — contracts + hard gates, sem Scheduler.
2. auditoria/FIXes de eligibility e quality floor.
3. B2 — scarcity/spend/scoring + explainability, ainda sem Scheduler.
4. auditoria/FIXes de determinismo, Unknown e paid guard.
5. B3 — variant expansion + bridge Auto do Scheduler.
6. regressões LR-7D2/LR-8 e testes de fallback/partial output.
7. B4 — persistência/configuração mínima e gate integrado.
8. auditoria final da LR-8.5B.

Não implementar B1–B4 num único patch gigante.

## Critério de fechamento

LR-8.5B recebe PASS quando:

- há ResourceAllocator provider-agnostic e testável;
- hard capability/quality gates precedem economia;
- scarcity é derivada somente de facts suficientes;
- Reserve preserva custo de oportunidade sem se tornar bloqueio universal;
- paid Auto é fail-closed e exige autorização;
- Economy/Balanced/Fast são determinísticos e explicáveis;
- múltiplas variantes do mesmo resource podem ser comparadas;
- o bridge entrega no máximo uma variante por provider ao Scheduler;
- Fixed e Preferred permanecem semanticamente inalterados;
- Auto real usa o Allocator sem duplicar o antigo score;
- LR-8 continua autoridade operacional;
- nenhum handoff pós-output foi antecipado;
- gates sintéticos e regressões completas passam;
- auditoria independente confirma ausência de preço/marca/inferência escondida.

## Próxima fase

Depois do PASS da B:

**LR-8.5C — Safe Cross-Resource / Cross-Variant Handoff**

A C utilizará a decisão da B em fronteiras seguras e cuidará de continuidade,
idempotência, cancelamento e proteção contra efeitos duplicados.


## B1 — Implementação candidata

**B1 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente.**
A LR-8.5B permanece em andamento; este registro não declara PASS da B1 nem
PASS da trilha, e não inicia B2/B3/B4.

### Pré-condições e boundary

Implementação exclusiva na branch `lr-8.5b-allocation-scarcity-policy`, após
confirmar HEAD local/remoto `bd808cfa1b2094b7fc75ccb24587928c1c6cbc6b`, fetch,
fast-forward (já atualizado), workspace limpo e base da trilha
`main@a6eec1b63655d860279f606bc55766255903f1fe`. Os cinco documentos exigidos
foram lidos, e os contratos de cognitive_resources, policy, scheduler e types
foram revisados antes das alterações.

A API é pura, síncrona, determinística e sem relógio implícito. Não possui
Database, SecretStore, HTTP, filesystem, backend handles, Provider/AgentBackend
ou manager mutável. Timestamps existentes são evidência recebida explicitamente
em CatalogFact, sem decisão de freshness ou refresh.

`AllocationRequest` recebe somente o universo **já autorizado pelo Core/policy**.
A API não prova a autorização do chamador e não enumera ResourceCatalog,
ProviderRegistry ou AgentRegistry. Universo vazio produz relatório vazio.
A construção desse universo e a prova operacional pertencem à futura B3.
`Eligible` descreve suficiência dos gates de catálogo B1, nunca permissão de
execução ou gasto, nem admission LR-8.

### Contratos adicionados

- `AllocationRequest`, `AllocationPolicy`, `AllocationCandidate` e
  `AllocationVariant` (identidade completa da variante).
- `CandidateRequirements`, `CapabilityRequirement` e `CapabilityScope`.
- `CandidateEligibility`, `EligibilityReport`, `EligibilityStatus` e
  `EligibilityReason`, com `CapabilityEvidence`, `AvailabilityEvidence`,
  `EffectiveTier` e `EvidenceLayer`.
- `QualityFloor` reutiliza `CognitiveTier`; `PaidBudget` reutiliza
  `MonetaryAmount`, cujo `micros` representa o teto autorizado neste contrato.
- `AllocationProfile::{Economy, Balanced, Fast}` e
  `VariantSelectionMode::{Explicit, Auto}` são apenas configuração.
- `PaidUsePolicy::{Deny, AllowKnownCostWithinBudget { budget }}`: default Deny;
  budget obrigatório dentro da variante Allow impede pares moeda/budget ausentes
  e combinações Deny-com-budget. Moeda possui três letras ASCII maiúsculas,
  micros é JSON-safe, zero é permitido como teto zero. Sem FX ou spend decision.
- `ReservePolicy` valida `0 <= reserve <= reduced <= 100`, com campos privados;
  `ScarcityState::{Comfortable, Reduced, Reserve, Exhausted, Unknown}` não é
  derivado nem aplicado aos gates. Nenhum threshold comercial/default é inventado.
- `AllocationError` oferece erros tipados e sanitizados, sem eco de payload.

O candidato contém uma referência imutável a `CognitiveResource` validado,
`ModelId` e `Option<EffortId>`. Sua projeção de identidade inclui `resource_id`,
`access_path`, `billing_domain_id`, `model_id` e `effort`. Classe, enabled,
capabilities, availability, ModelFacts e EffortFacts vêm dos descriptors LR-8.5A
já associados ao recurso, sem ModelProfile destacado ou cópia de todo o resource.
Isso evita inconsistência entre identidade e evidência. `ExecutionVariant` não
é usado como entrada porque `describe_variant` rejeita suporte de effort Unknown
antes que B1 possa explicá-lo. Nenhum contrato LR-8.5A foi alterado.

### Hard gates e explainability

- `Eligible`: todos os gates exigidos comprovados.
- `Ineligible`: existe contradição factual obrigatória, mesmo que outros fatos
  estejam Unknown; as demais reasons não são descartadas.
- `Unresolved`: nenhuma contradição, mas falta prova obrigatória. Não participa
  como se estivesse aprovado; requer evidência adicional na futura B3.
- Enabled Known(false) exclui; Unknown gera Unresolved. Enabled não reescreve
  availability e não afirma disponibilidade remota.
- Availability Known(Unavailable) exclui em resource, model ou effort selecionado;
  Known(Available) satisfaz aquela camada; Unknown conserva Unresolved.
- Catálogo de modelos Known sem o modelo pedido exclui com ModelNotSupported;
  Unknown produz ModelSupportUnknown. O modelo não é inventado.
- Effort pedido ausente em lista Known (inclusive vazia) exclui com
  EffortNotSupported; catálogo Unknown produz EffortSupportUnknown.
  Availability de effort não pedido é None, sem default inventado. Se suporte
  não foi provado, a evidence de availability permanece Unknown e a reason é
  de suporte, sem fabricar disponibilidade ou multiplicar razões redundantes.

**Capabilities:** requisito declara scope Runtime ou Model, sem mappings por
provider. Runtime avalia somente o fato do resource. Model exige Known(true)
no ModelProfile; resource Known(true) nunca supre model Unknown. Resource
Known(false) veta uma capability model-specific mesmo com modelo Known(true).
Resource Unknown com model Known(true) satisfaz apenas o scope Model; exigir
ambas as provas usa dois requisitos distintos, um por scope. Model Known(false)
não define uma característica estrutural exigida somente no Runtime.
Known(false) gera CapabilityUnsupported; ausência/Unknown gera CapabilityUnknown.
Não há equivalência silenciosa ToolCalling/ToolUse nem herança de adapter.

**Quality floor:** tier Known do effort selecionado tem precedência descritiva;
caso contrário usa tier Known do modelo; caso contrário Unknown. Effort Known
inferior não é mascarado por modelo superior. Não há soma, inferência por nome,
QualityLabel, provider, preço ou marca. Com piso, Known abaixo exclui,
Known suficiente satisfaz e Unknown gera Unresolved/CognitiveTierUnknown.
Sem piso, tier Unknown não acrescenta uma reason. O relatório registra o fato
resolvido e sua camada/provenance sem reescrever os fatos originais.

Economia não participa da eligibility: custo monetário, allowances, saldo,
latência, RelativeCostTier, profile, reserve e paid policy não favorecem nem
excluem. Descriptors estruturalmente inválidos são recusados no construtor como
InvalidCandidate, reutilizando a validação LR-8.5A (inclusive consistência de
unidades), antes da avaliação; isso não deriva uma decisão econômica.

### Bounds, ordering e serialização

Requisitos são privados, limitados a 20 pares capability/scope, sem duplicatas
exatas e canonizados pela ordem dos enums. Runtime e Model para a mesma capability
são requisitos distintos. Piso reutiliza o bound 0..=255 da LR-8.5A.
Requests aceitam até 256 variantes completas, rejeitam tuplas duplicadas e mantêm
a ordem do chamador, sem seleção, sort econômico ou winner. O descriptor mantém
os bounds e invariantes existentes de modelos, efforts, fatos e allowances.
Policies são válidas por construção: não há desserialização agregada permissiva
que possa contornar os construtores; persistência/desserialização ficam para B4.

Reasons seguem ordem fixa: enabled, resource availability, model support e
availability, effort support e availability, capabilities canonizadas (resource
antes de model quando ambos falham), quality. Somente BTreeSet interno e ordering
explícito; nenhuma iteração HashMap/HashSet define o output.

Relatórios possuem collections privadas produzidas exclusivamente de inputs
bounded. Serialização inclui somente identidade pública local da variante,
configuração validada, facts/provenance dos hard gates e razões enum; exclui
metadata opaca de qualidade, economics do descriptor, outros modelos/efforts,
origem/runtime binding, secrets, remote account IDs, backend handles e texto de
tarefa/prompt. IDs continuam labels públicos fornecidos pelo Core: validação
sintática não é redator de segredos nem autorização para usá-los como labels.

### Arquivos e gate sintético

- Criados: `src-tauri/src/cognitive_resources/allocation.rs` e
  `src-tauri/src/cognitive_resources/allocation_tests.rs`.
- Alterados: `src-tauri/src/cognitive_resources/mod.rs` (módulo, export e testes)
  e este documento (estado e registro B1).
- Nenhum arquivo de Scheduler, cognition policy/types, registries, providers,
  agents, LR-8, TaskGraph, frontend ou migration foi modificado.

33 testes B1 determinísticos, sem backend ou provider real, cobrem A–R:
capability true/false/Unknown; tiers suficiente/insuficiente/Unknown/sem piso;
tier effort específico, fallback descritivo e precedência de esforço inferior;
unavailable resource/model/effort; availability Unknown em cada camada;
disabled/enabled Unknown; effort ausente/lista vazia/catálogo Unknown;
modelo ausente/catálogo Unknown; múltiplas falhas com ordem estável e precedência;
economia Unknown/conhecida sem efeito; independência resource/model e scopes;
serialização sanitizada com provenance e identidade. Também cobrem bounds,
duplicatas, policies válidas por construção, todas as classes de recurso,
labels opacos sem tier inferido, request vazia e ausência de expansão/ranking.

### Resultados técnicos

| Gate | Resultado |
|---|---|
| `/home/sam/.cargo/bin/rustfmt --edition 2021 --check src-tauri/src/cognitive_resources/*.rs` | Exit 0; módulo inteiro |
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources` | Exit 0; 71 aprovados, 0 falhas: 38 LR-8.5A + 33 B1 |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=4` | Exit 0; 613 aprovados, 0 falhas, 2 ignorados; 249,84 s; main/doc-tests sem falhas |
| cognition / agents na suíte completa | 370 / 110 aprovados; agents com 2 manuais ignorados |
| LR-8 na suíte completa | rate 69, telemetry 33, admission 18, resilience 76, operational 8 e LR-8E 14; todos aprovados |
| smart routing / TaskGraph runtime na suíte completa | 10 / 13 aprovados; demais testes de TaskGraph, Scheduler e roles também passaram |
| `git diff --check` e `git diff --cached --check` | Exit 0 no diff final |

Os dois ignorados são os gates Codex manuais preexistentes
`real_app_server_handshake` e `manual_final_codex_agent_bridge_gate`.
Mantidos 15 warnings da biblioteca e dois de fixtures de testes, todos fora do
módulo novo; nenhum warning suprimido ou novo. Não houve falha de teste nos
gates finais. A primeira compilação dirigida detectou lifetime ausente no helper
novo de testes; corrigido para vincular o borrow somente ao resource antes da
execução dirigida final e da suíte completa.

A global usa quatro threads, como os gates finais LR-8.5A, para limitar contenção
Stronghold; nenhum timeout/fixture legado foi alterado. Não foi aplicado rustfmt
global: os arquivos legados com drift preexistente ficaram intocados. Não há
necessidade de gate frontend, release ou comercial/real para esta superfície
pura, sem alteração de DTO/comando/runtime exposto.

### Dívidas deliberadamente adiadas

- **B2:** derivação de scarcity, spend guard/cálculo de budget restante,
  reserve operacional, pesos, scoring, comparação econômica e winner selection.
- **B3:** builder autorizado, catálogo/expansão real de variantes, combinação com
  supports_invocation/prova operacional, bridge Auto e plano para Scheduler.
  Unresolved não pode ser silenciosamente convertido em Eligible.
- **B4:** persistência/desserialização agregada, migration/configuração e UI mínima,
  defaults duráveis e gate integrado final.
- Freshness/calibração/reconciliação de fontes e integrações comerciais reais não
  são resolvidas por estes gates. Handoff continua LR-8.5C.

**B1 filtra e explica. B2 pontua. B3 altera o Auto real.** Não existe scoring,
rank, winner, alteração em Scheduler/auto_score/Fixed/Preferred/Auto/affinity,
fallback/retry/admission/resilience/RateLimitManager/TaskGraph ou nos registries.

## B1 FIX-1 — Authorized Universe Coherence

**B1 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente.**
Correção incremental sobre o HEAD auditado
`e5244c92bef19e13cadc13f8e3e90b1ffcf10882`, exclusivamente na branch
`lr-8.5b-allocation-scarcity-policy`. Branch/HEAD local e remoto foram confirmados,
fetch + fast-forward não encontraram divergência, e o workspace estava limpo
antes das alterações. Sem declaração de PASS, merge ou início de B2/B3/B4.

### Problema e invariâncias

A auditoria independente aprovou os contratos/hard gates, mas identificou que
validar cada candidato isoladamente, seguido apenas por cardinalidade e tuplas
duplicadas, permitia snapshots contraditórios do mesmo ResourceId na request.
Também permitia que um runtime não local fosse vinculado a resources distintos,
enfraquecendo a invariância LR-8.5A de único credential/quota context por runtime.

`AllocationRequest::new()` agora reduz os candidatos por ResourceId usando
BTreeMap local e compara **todo CognitiveResource** por igualdade estrutural.
Todos os candidatos do mesmo ResourceId devem conservar identity completa,
access path, billing domain, family, class, origin, enabled, availability,
capabilities, models/efforts, economics e provenance/timestamps idênticos.
Descriptors clonados iguais são aceitos; pointer identity não é exigida.
Qualquer divergência retorna `AllocationError::ConflictingResourceSnapshot`.
Não há merge de facts, reconciliação, latest-wins ou escolha de fonte.

Depois de validar todos os snapshots, o conjunto reduzido de resources preserva
a mesma regra de ResourceCatalog: um `ResourceOrigin::Provider(runtime_id)`
não pode pertencer a ResourceIds distintos, assim como um
`ResourceOrigin::Agent(runtime_id)`. Violação retorna
`AllocationError::ConflictingRuntimeBinding`. Provider e Agent continuam
namespaces separados, inclusive com o mesmo label nominal de runtime.
`ResourceOrigin::Local` permanece isento da unicidade. ResourceCatalog não foi
alterado; nenhum registry é consultado ou enumerado.

BillingDomains compartilhados entre resources distintos não impõem igualdade
econômica nesta FIX. Evidências econômicas divergentes continuam válidas nesses
resources; `domain_economics()` mantém seu contrato LR-8.5A fail-closed, intocado.
A futura B2 decidirá como consumir essas evidências.

### Ordem de validação e boundary

Ordem fixa, com fases completas antes de avançar:

1. cardinalidade da request;
2. coerência integral dos descriptors por ResourceId;
3. unicidade de bindings não locais no conjunto reduzido;
4. duplicate AllocationVariant.

Um conflito de snapshot tem precedência mesmo quando uma variante duplicada ou
um binding conflitante aparece antes na ordem de candidatos. Binding conflitante
precede DuplicateCandidate; excesso de cardinalidade precede todos os conflitos.
BTreeMap/BTreeSet internos são bounded pela cardinalidade já validada; a ordem
de candidatos no relatório continua sendo a ordem recebida. Erros são enums
sem payload e não ecoam IDs, fatos, prompts ou texto de tarefa.

Eligibility, enabled, availability, model/effort support, capability scopes,
quality floor e seus relatórios permanecem exatamente como no HEAD auditado.
Sem scoring, ranking, winner, spend decision ou integração operacional.
Scheduler/auto_score/Fixed/Preferred/Auto/affinity/fallback/retry/admission,
resilience, RateLimitManager, TaskGraph, registries, UI e migrations intocados.

### Testes e arquivos

13 testes sintéticos adicionais em `allocation_tests.rs`, sem IO/clock/backend:

- vários modelos/efforts derivados de um único resource;
- descriptors clonados iguais, inclusive com binding Provider repetido;
- mesmo ResourceId com path/domain/family/class/origin diferentes;
- mesma identity com enabled/availability/capabilities/model/effort/economics,
  provenance ou timestamp divergentes;
- mesmo runtime Provider ou Agent em ResourceIds distintos;
- resources Local distintos e runtimes diferentes válidos;
- namespaces Provider/Agent independentes;
- precedência global de snapshot sobre binding/duplicate, bounds sobre conflitos
  e binding sobre duplicate; erros Display/JSON sem payload;
- BillingDomain compartilhado com evidências econômicas divergentes aceitas,
  sem mutação de descriptors ou inferência econômica.

Os 33 testes B1 e os 38 testes LR-8.5A anteriores foram preservados. Diff de
produção restrito ao import, dois erros novos e validação de universo no
construtor de request; nenhum código de avaliação dos gates foi modificado.
Arquivos alterados: `src-tauri/src/cognitive_resources/allocation.rs`,
`src-tauri/src/cognitive_resources/allocation_tests.rs` e este documento.

### Resultados

| Gate | Resultado |
|---|---|
| `/home/sam/.cargo/bin/rustfmt --edition 2021 --check src-tauri/src/cognitive_resources/*.rs` | Exit 0; módulo inteiro |
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources` | Exit 0; 84 aprovados, 0 falhas: 38 LR-8.5A + 33 B1 + 13 FIX-1 |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=4` | Exit 0; 626 aprovados, 0 falhas, 2 ignorados; 257,86 s; main/doc-tests sem falhas |
| cognition / agents na suíte completa | 370 / 110 aprovados; agents com 2 manuais ignorados |
| LR-8 na suíte completa | rate 69, telemetry 33, admission 18, resilience 76, operational 8 e LR-8E 14; todos aprovados |
| smart routing / TaskGraph runtime na suíte completa | 10 / 13 aprovados; demais testes de TaskGraph e Scheduler também passaram |
| `git diff --check` e `git diff --cached --check` | Exit 0 no diff incremental final |

Os dois ignorados continuam sendo os gates manuais Codex preexistentes
`real_app_server_handshake` e `manual_final_codex_agent_bridge_gate`.
Mantidos 15 warnings da biblioteca e dois de fixtures, todos preexistentes,
sem warning novo ou supressão. Nenhuma falha de compilação/teste nesta FIX.
A suíte completa mantém quatro threads, como o gate B1 auditado, sem alteração
de timeout ou fixture legado. Rustfmt foi aplicado apenas aos dois arquivos
Rust da FIX e verificado no módulo; nenhum arquivo legado fora do escopo foi
reformatado. A comparação com o HEAD auditado confirmou os 33 corpos de testes
B1 e o código de avaliação de eligibility intactos, além de ResourceCatalog e
todos os boundaries operacionais preservados.

Estado mantido: **B1 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente**.
Esta FIX não declara PASS nem libera B2.
