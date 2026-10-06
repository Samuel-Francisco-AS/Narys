# LR-8.5B — Allocation, Model/Effort Selection & Scarcity Policy

Estado: **PASS TÉCNICO — B1/B2/B3/B4 encerradas após auditoria independente em 06/10/2026.**
A LR-8.5B está aprovada para integração à `main`; a próxima subfase é LR-8.5C.

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

**B1 = PASS TÉCNICO — auditoria independente concluída.**
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


## B1 — Fechamento técnico

Data: **06/10/2026**  
HEAD auditado: `8c977df9eb3217b826e81b1b030341cce4e3958e`

A auditoria independente encerra o checkpoint **B1 — Allocation Contracts & Hard Gates**
em **PASS técnico**.

Foram confirmados:

- contracts provider-agnostic e sem autoridade operacional;
- hard gates de enabled, availability, support, capabilities e quality floor;
- semântica explícita Eligible / Ineligible / Unresolved;
- Unknown preservado sem interpretação otimista;
- capability Runtime e Model separadas;
- tier efetivo effort → model sem soma ou inferência por label;
- universo autorizado coerente por ResourceId;
- runtime Provider/Agent ligado a no máximo um ResourceId;
- clones estruturalmente idênticos permitidos;
- ausência total de scoring, ranking, winner e spend decision;
- Scheduler, Auto, Fixed, Preferred, affinity, fallback, retry, LR-8 e TaskGraph
  behavior-neutral.

Gate reportado: `cognitive_resources` 84 aprovados; suíte Rust completa
626 aprovados, 0 falhas, 2 ignorados. A execução foi realizada no ambiente local;
a auditoria independente revisou o código e diff remoto.

**Próximo checkpoint: B2 — Scarcity, Spend Guard & Deterministic Scoring.**

B2 pode consumir apenas candidatos B1 Eligible para seleção automática. Unresolved
continua sem equivaler a autorização; sua resolução operacional permanece responsabilidade
do builder/bridge da B3 quando houver evidência técnica externa apropriada.

## B2 — Implementação candidata

**B2 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente**.
Esta seção registra somente o motor puro B2; não declara B2 PASS nem LR-8.5B
PASS e não inicia B3/B4/C.

### Pré-condições e arquivos

Branch exclusiva `lr-8.5b-allocation-scarcity-policy`, HEAD inicial local/remoto
`ef575db50ef4d0fcbe1ec57ea05c0ef220048f97`, fetch + fast-forward já atualizado,
workspace limpo e ancestral/base original
`main@a6eec1b63655d860279f606bc55766255903f1fe` confirmados antes de editar.
Os cinco documentos exigidos foram lidos integralmente; allocation, economics,
lr8, catalog, rate, telemetry e Scheduler foram revisados.

| Arquivo em `src-tauri/src/cognitive_resources/` | Responsabilidade B2 |
|---|---|
| `economic_context.rs` (novo) | Captura read-only com identidade, validação de DTOs e ScoringError |
| `scarcity.rs` (novo) | ResolvedExecutionFacts, resolução model/effort, allowances e OperationalPressure |
| `spend.rs` (novo) | SpendAssessment, EconomicEligibility e EconomicExclusion |
| `scoring.rs` (novo) | Request B2, CandidateSignals, pesos, ScoreBreakdown, ranking e AllocationDecision |
| `scoring_tests.rs` (novo) | Gate sintético B2 e auditabilidade |
| `allocation.rs` | Somente getters imutáveis de candidates/requirements/policy; gates B1 intactos |
| `mod.rs` | Declarações, exports e inclusão dos testes B2 |

Este documento é a única alteração fora do módulo. Nenhum arquivo de cognition,
Scheduler, agents, registries, providers, rate/admission/resilience, TaskGraph,
frontend, migration ou persistência foi alterado.

### Contratos e pipeline

`AllocationScoringRequest::new(&b1, selected, contexts)` recebe uma request B1
validada e um subconjunto explicitamente selecionado por `AllocationVariant`,
com `CandidateSignals`. Não expande variantes nem descobre recursos. Executa
`b1.evaluate()` e associa cada resultado por identidade completa, nunca posição
em Vec. Relatório destacado não pode ser injetado com outro descriptor.

Pipeline:

~~~text
universo autorizado B1 imutável
→ seleção explícita de variantes + B1 Eligible obrigatório
→ validação do join e da coerência de BillingDomain
→ resolução descritiva model/effort
→ allowance/LR-8 guards + spend guard
→ somente EconomicEligibility::Eligible recebe ScoreBreakdown
→ rank determinístico + winner em memória
~~~

Unresolved retorna `ScoringError::B1Unresolved`; Ineligible retorna
`B1Ineligible`. Variante externa retorna `CandidateNotInB1Universe`; duplicatas
ou excesso de cardinalidade também invalidam a request. B2 não converte status,
não refaz quality gate e não dá bônus a CognitiveTier excedente. O resultado B1
exato e a resolução de tier são preservados na evidence de cada candidato.

`AllocationDecision` expõe winner opcional, ranked/excluded candidates, policy
(incluindo profile), requirements, pesos, tie-break e evidências por candidato.
`RankedCandidate` contém `ScoreBreakdown` e `order_over_next` tipado;
`ExcludedCandidate` conserva evidências e razões econômicas, sem score. Universo
vazio ou totalmente excluído resulta em winner None. Não existe rationale livre,
prompt, LLM, backend handle, provider call ou autoridade de execução nessa API.

### Join econômico e LR-8 seguro

Os fatos econômicos vêm do próprio `CognitiveResource` emprestado pela B1.
`EconomicContext` tem campos privados e é construído somente por
`capture(descriptor, telemetry_dto, rate_dto)`. A captura recebe DTOs já obtidos,
sem referência a managers, refresh, relógio, HTTP ou IO. Reutiliza a projeção
read-only `project_lr8` da A depois de validar identidade, bounds e duplicatas.

Ambos os DTOs, quando presentes, devem identificar exatamente o RuntimeId
Provider do descriptor; DTO de outro provider, Agent ou Local falha com
`Lr8ResourceMismatch`. Gerações divergentes falham com `Lr8ContextMismatch`.
Fatos detached `Lr8Facts`/`ResourceSnapshot` não são entrada pública dessa bridge:
a projeção A remove provider_id, portanto aceitar uma reassociação arbitrária
não permitiria provar seu binding original. B3 pode reutilizar os DTOs capturados
para montar esse contexto estreito, sem segunda leitura da autoridade.

A request associa contextos por ResourceId e exige igualdade estrutural de todo
CognitiveResource contra B1, incluindo identidade/origin, modelo/effort,
economics, provenances e timestamps. Clones iguais são aceitos. Descriptor
alterado falha com `ConflictingResourceSnapshot`; contexto duplicado ou fora do
subconjunto selecionado falha com `DuplicateEconomicContext` ou
`UnexpectedEconomicContext`. Ausência de contexto não fabrica fatos LR-8.

Resources selecionados com o mesmo BillingDomainId devem possuir EconomicFacts
estruturalmente idênticos, inclusive ordem das listas, provenance e timestamps,
como em `domain_economics()`. Divergência retorna
`ConflictingBillingDomainFacts` antes de qualquer ranking. Sem latest-wins,
merge, soma, timestamp vencedor ou média. B1 FIX-1 e ResourceCatalog permanecem
inalterados. Canonização de saída não reconcilia descriptors contraditórios.

### Resolução model/effort e allowances multidimensionais

Para cognitive_tier, relative_cost, latency_ms e monetary_cost, `ResolvedFact`
conserva CatalogFact/provenance/timestamp e a camada da observação:

- effort selecionado Known → Effort;
- effort ausente/Unknown e modelo Known → Model;
- sem observação Known → Unknown, layer None.

Não há soma, média ou mutação de ExecutionFacts. A resolução de tier coincide
com a B1; tier suficiente é evidence, não componente de score.

Allowances são resolvidas por AllowanceDimensionId em BTreeMap: começa com as
dimensões do modelo; effort substitui a mesma dimensão e acrescenta suas
exclusivas. Amount Unknown do effort também substitui Known do modelo:
relevância específica permanece, sem recuperar magnitude artificialmente.
Unidades diferentes para o mesmo ID em modelo/effort selecionados geram
`EconomicEvidenceConflict`, inclusive quando não existe state que teria feito
A/B1 recusar a inconsistência. Sem conversão. União máxima: 32 dimensões
(16 por ExecutionFacts). Descriptor original continua com as listas separadas.

`AllowanceScarcity` expõe consumo resolvido/camada, state factual opcional,
consumed, state derivado, remaining_percent e ScarcityReason. Só dimensões
explicitamente referenciadas pela variante entram na avaliação. Known amount=0
fica visível como `ExplicitZeroConsumption`, consumed=false, sem percentual,
sem estado confortável inventado e sem influência no agregado. Amount Unknown
conserva relevância; não representa consumo zero.

Algoritmo exato para cada dimensão efetivamente consumida:

1. remaining Known(0) → Exhausted, independentemente de limit/amount Unknown;
2. amount Known > 0 e remaining Known < amount → Exhausted;
3. remaining/limit Known e limit > 0:
   `p = min(100, floor(u128(remaining) * 100 / u128(limit)))`;
4. com ReservePolicy, `p < reserve_below_percent` → Reserve;
   senão `p < reduced_below_percent` → Reduced; senão Comfortable;
5. evidência insuficiente ou ausência de ReservePolicy → Unknown.

A multiplicação ocorre em u128 antes da divisão; saída 0..=100. Thresholds
continuam privados/validados pela B1, `0 <= reserve <= reduced <= 100`.
Igualdade ao threshold não é “below”. Percentual arredondado para zero com
remaining positivo não é Exhausted por si só. Limit Unknown não permite deduzir
percentual de remaining=30. Não há janela, reset, refill ou unlimited inventado.

`ScarcityAssessment` mantém avaliação individual de todas as dimensões e
`ScarcitySummary { known_worst, has_unknown }`. Exhausted domina; depois Reserve,
Reduced e Comfortable. Unknown mantém flag separada e não apaga pior Known nem
acrescenta penalidade. Lista ausente/vazia não afirma execução gratuita/quota
infinita. Requests, tokens, credits, percent e custom units nunca são somados.
Reserve é custo de oportunidade e pode vencer como único candidato suficiente;
Exhausted exclui economicamente do ranking Auto B2.

### Pressão operacional LR-8 e precedência

`OperationalPressure` mantém constraints separadas de AllowanceState. Usa somente
provider constraints e constraints do model_id exato selecionado; nunca sibling
model, family, global RateFacts.saturated, usage acumulado ou retry hint.
Constraint aplicável saturated=true ou effective_remaining=Known(0) → Exhausted
operacional e `Lr8ConstraintSaturated`. É indisponibilidade conservadora nesse
snapshot B2; não é execução, reserva ou substituição da admission LR-8.

RateFacts usam `floor(effective_remaining * 100 / capacity)` quando o par é
Known e capacity > 0, com os mesmos thresholds. É fração do teto conservador
interno da constraint, não percentual de allowance comercial. Capacity isolada
ou remaining Unknown mantém Unknown; não se deduz crédito de consumed/reserved
nem de external.remaining retido. Fontes ExternalFact/LocalPolicy/DailyBudget,
provenance, resets factuais, uncertainty e timestamps/gerações separados ficam
na evidence, sem reavaliar deadlines ou declarar atomicidade entre capturas.

Qualquer constraint RateFacts no mesmo scope exato + dimensão tem precedência
sobre telemetry, inclusive quando effective_remaining está Unknown. Isso evita
ressuscitar headroom conservador pelo último header. Telemetry é fallback só
quando não existe constraint correspondente. Seu limit/remaining factual usa os
mesmos thresholds; remaining zero pode provar saturação. Timing histórico fica
no QuotaSnapshot da evidence; não vira countdown/deadline operacional atual.

Constraints são canonizadas por scope/dimension/source. Não se duplica quota
RateFacts+telemetry. O score usa a maior severidade Known entre allowances e
pressão operacional, sem somar unidades/constraints; todas as evidências ficam
visíveis. Falta de facts continua sem proof de disponibilidade. B3/Scheduler/
RateLimitManager obrigatoriamente revalidarão antes da chamada real.

### Spend guard

`SpendAssessment` separa path pago explícito de custo monetário positivo.
BillingKind MeteredBilling/PrepaidCredits ou monetary_cost efetivo Known > 0
constitui evidência positiva de spend. BillingKind Unknown com custo Unknown
permanece neutro, sem ser classificado como free ou paid.

- Deny → `PaidUseDenied` quando há evidência paga, inclusive custo factual zero
  em path Metered/Prepaid.
- AllowKnownCostWithinBudget → exige custo efetivo Known, currency idêntica ao
  budget e cost <= budget. Ausência → `PaidCostUnknown`; moeda diferente →
  `CurrencyMismatch`; excedente → `PaidBudgetExceeded`. Sem FX.
- PrepaidCredits com unidade monetária requer também monetary_balance Known,
  mesma currency do custo e balance >= cost, inclusive quando cost=0. Ausência,
  moeda diferente ou saldo insuficiente geram razões próprias.
- MeteredBilling não exige saldo pré-pago; saldo Unknown/zero/outra moeda não é
  guard obrigatório desse path.

Budget é teto autorizado para essa decisão/invocação, não saldo de ledger. B2
não debita, acumula spend, compra/refilla créditos nem reserva dinheiro/quota.
Créditos não monetários continuam allowances sem equivalência em dinheiro.

EconomicExclusion é separado de EligibilityReason:
`AllowanceExhausted`, `Lr8ConstraintSaturated`, `PaidUseDenied`, `PaidCostUnknown`,
`CurrencyMismatch`, `PaidBudgetExceeded`, `PrepaidBalanceUnknown`,
`PrepaidBalanceCurrencyMismatch`, `PrepaidBalanceInsufficient`,
`EconomicEvidenceConflict`. Razões múltiplas ficam preservadas em ordem fixa:
conflito/allowance, pressão LR-8, spend. Não existe score de candidato excluído.

429/ProviderError não é input do scorer. RetryHint e last_outcome não entram no
score/guard; nenhuma pressão ou scarcity pode modificar PaidUsePolicy::Deny.
O gate sintético usa last_outcome rate_limited + retry hint e várias pressões
com Deny e confirma exclusão paga em todos os casos.

### CandidateSignals, score e perfis

CandidateSignals possui campos privados/constructor validado e ausência possível:
preference_ordinal 0..=255, registry_priority 0..=32, continuity 0..=100 e
switching_cost 0..=100. Valores são passados explicitamente, sem leitura global,
Scheduler ou registry. Ordinal representa preferência autorizada; priority é
secundária; continuidade não prova qualidade/capability; switching é escala
relativa de reconstrução, nunca dinheiro. B3 mapeará sinais reais e fará clamp
explícito de priority quando apropriado; B2 recusa valores fora dos bounds.

Constantes de policy local do Core (não facts comerciais):

| Peso/componente | Economy | Balanced | Fast |
|---|---:|---:|---:|
| policy_preference | 4 | 6 | 6 |
| registry_preference | 1 | 1 | 1 |
| continuity | 2 | 4 | 12 |
| switching | 2 | 4 | 12 |
| relative_cost | 12 | 6 | 2 |
| Reduced (penalidade) | 600 | 300 | 100 |
| Reserve (penalidade) | 1800 | 900 | 300 |
| monetary_cost | 10 | 5 | 1 |
| latency | 2 | 6 | 15 |

Constantes auxiliares nomeadas: MAX_PREFERENCE_ORDINAL=255, REGISTRY_PRIORITY_CAP=32,
REGISTRY_UTILITY_CAP=3, LATENCY_BUCKET_MS=100, MAX_LATENCY_BUCKET=100 e
MONETARY_NORMALIZATION=100. Para fatos/sinais conhecidos:

~~~text
preference = (MAX_PREFERENCE_ORDINAL - ordinal) * policy_weight
registry   = floor((32 - priority) * 3 / 32) * registry_weight
continuity = continuity_signal * continuity_weight
switching  = -switching_cost * switching_weight
relative   = -relative_cost_tier * relative_weight
scarcity   = 0 (Comfortable/sem Known), -Reduced_weight ou -Reserve_weight
monetary   = -floor(cost_micros * 100 / budget_micros) * monetary_weight
latency    = (100 - 2 * min(floor(latency_ms / 100), 100)) * latency_weight
total      = preference + registry + continuity + switching
             + relative + scarcity + monetary + latency
~~~

Unknown/None sempre produz componente 0, sem valor médio/default comercial.
Known(0) RelativeCostTier não afirma dinheiro zero. Não existe cognitive-tier
bonus nem overprovision penalty. Registry tem utilidade máxima 3, inferior a
um passo de ordinal em todos os profiles. Economy enfatiza reserva e custo;
Balanced mantém Reserve mais forte que pequena continuidade; Fast permite
vantagem factual grande de latência/continuidade superar Reserve.

Monetary só é comparável depois do guard, com custo Known na currency autorizada
e <= budget. Sem base comparável, componente neutro. Budget zero + cost zero
produz percentual 0, sem divisão por zero. Ratios usam u128. Latency é utility
centrada [-100,100], permitindo Unknown neutro entre observações rápidas e
lentas, sem afirmar que Unknown é a mais rápida/lenta. Clamp impede valores
numéricos extremos de dominar/overflow; não é latência comercial default.

Componentes usam i64 e operações saturating explícitas. Bounds e pesos impedem
saturação para inputs válidos: totais possíveis dentro de Economy [-6260,1423],
Balanced [-3930,2533], Fast [-3610,4233]. `component_sum()` usa saturating_add;
os testes também somam com checked_add e verificam igualdade exata nos limites.
ScoreBreakdown preserva evidence de cada influência, incluindo monetary
comparability/fração, scarcity/Unknown e camada de cada fato resolvido.

Cenário sintético documentado: pequena continuity=5 não supera Reserve em
Economy/Balanced. Em Fast, A Reserve + latency=100ms + continuity=100 + switching=0
obtém 2370; B Comfortable + latency=10000ms + continuity=0 + switching=100 obtém
-2700. A vence. Exhausted nunca chega ao score, mesmo com essas vantagens.

### Determinismo e desempate

Maior total primeiro; empate usa menor preference_ordinal explícito (None após
ordinais conhecidos), depois identidade canônica AllocationVariant, na ordem
resource_id/access_path/billing_domain_id/model_id/effort. A ordem lexical é
somente identidade estável, não hierarquia de modelos/efforts por nome.

TieBreakReason distingue NoEconomicCandidate, OnlyEconomicCandidate, HigherTotal,
PolicyOrdinal e CanonicalVariant; cada posição também informa order_over_next.
Inputs/exclusions são canonizados por identidade. Contextos e dimensões usam
BTreeMap; maps, ordem de registro/Vec, ponteiros ou HashMap iteration não decidem
winner. Mudar o ordinal explícito pode mudar a decisão legitimamente.

### Gate sintético e resultados técnicos

89 testes B2 cobrem os requisitos A–AC e ScoreBreakdown: least sufficient/floor
B1, ausência de bônus por tier, Reserve/Comfortable, candidato único suficiente,
Exhausted/consumo por chamada, Unknown, múltiplas dimensões, esforço/substituição/
unidade/zero, paid deny/allow/Unknown/currency/budget, saldo Prepaid e independência
Metered, Billing Unknown, domínio compartilhado e provenances, affinity/perfis,
latência Unknown/extrema, escopo LR-8 exato/provider/global saturation, precedência
sem dupla contagem, ausência de autorização por 429, rejeição B1 Unresolved/
Ineligible, igualdade dos inputs sem debit/IO, determinismo por repetição e ordens
incidentais, bounds de request/signals/constraints, 256 candidatos e união de
32 dimensões, serialização sanitizada e soma checked nos máximos.

Resultados finais no código do commit
`3793898c4a96f328cd855ad8488178f332bda751` (06/10/2026):

| Gate | Resultado |
|---|---|
| `/home/sam/.cargo/bin/rustfmt --edition 2021 --check src-tauri/src/cognitive_resources/*.rs` | Exit 0; módulo inteiro, sem reformatação de legado |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0; 15 warnings preexistentes |
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources::scoring_tests` | Exit 0; 89 aprovados, 0 falhas, 0 ignorados; 0,01 s |
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources` | Exit 0; 173 aprovados, 0 falhas; 0,11 s |
| Regressões B1 / LR-8.5A no módulo e na global | 46 / 38 aprovados; todos os testes anteriores preservados |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=4` | Exit 0; 715 aprovados, 0 falhas, 2 ignorados; 252,89 s; main/doc-tests sem falhas |
| cognition / agents na suíte global | 370 / 110 aprovados; dois manuais agents ignorados |
| smart routing / TaskGraph runtime na global | 10 / 13 aprovados |
| LR-8 na global | rate 69, telemetry 33, admission 18, resilience 76, operational 8 e LR-8E 14; todos aprovados |
| `git diff --check`, diff desde HEAD inicial e `git diff --cached --check` | Exit 0 |

Os dois ignorados são os gates Codex manuais preexistentes
`real_app_server_handshake` e `manual_final_codex_agent_bridge_gate`. Nenhum
provider/credencial comercial real foi usado. As regressões operacionais foram
executadas na suíte global, sem rodadas redundantes ou mudança de timeout/fixture.
Warnings: 15 da biblioteca e dois de fixtures de teste (identity/memory), todos
preexistentes; nenhum warning novo, nenhum suprimido. Os warnings da biblioteca
continuam unused/dead-code em agents, cognition, persistence e security, fora
do módulo B2. Não se aplicou rustfmt global nem se alterou frontend.

A primeira compilação dirigida encontrou um nome inválido de capability somente
na fixture nova (`TextStream` em vez de `Streaming`); corrigido antes dos gates
finais. A rodada inicial de 85 testes passou; após os refinamentos documentados
(Timing histórico, bounds de thresholds e registry secundário) a rodada final
de 89 passou. Nenhuma falha foi dispensada. A comparação literal com o HEAD
inicial confirmou allocation.rs byte a byte idêntico após remover apenas os
três getters novos. Todos os boundaries de runtime ficaram fora do diff.

### Limitações e dívidas para B3/B4

- B3: builder real autorizado, variant expansion, obtenção/prova operacional,
  reconstrução explícita de eligibility quando legítima, mapeamento dos sinais
  reais e bridge Auto. Fixed/Preferred e Auto real permanecem inalterados.
- B3/Scheduler/LR-8: revalidar contexto, freshness e gates antes de transporte;
  snapshot B2 não reserva, admite, autoriza transporte ou garante execução.
- B4: persistência/desserialização agregada, configuração/UI, defaults duráveis,
  migration quando aprovada e gate integrado. Pesos locais requerem calibração
  futura; não existe tabela por marca/preço nem source selection/reconciliação.
- Nenhum ledger monetário, reserva de dinheiro/quota, provider/SpecialistAgent
  real ou integração Codex/Copilot foi antecipado. Política de freshness/refresh
  não foi criada. Pressão B2 não interpreta todos os gates de health/persistence
  do runtime e não substitui seu enforcement.
- Handoff, partial output, fallback seguro e reexecução permanecem nas fronteiras
  B3/C. B2 não recebe estado de output parcial nem altera a proteção existente.

**B2 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente**.

## B2 FIX-1 — Explicit Preference Dominance

A auditoria independente do HEAD
`986e17476c3f4c8557d18e88d533e24952bba296` identificou uma inconsistência:
`None` indicava ausência de prova de preferência, mas recebia score 0 enquanto
ordinais explícitos maiores que zero recebiam score negativo. Assim, com os
demais componentes idênticos, a ausência vencia a preferência autorizada.
O desempate por PolicyOrdinal não era alcançado nesses casos.

Fórmula anterior:

~~~text
Some(ordinal) -> -ordinal * policy_weight
None          -> 0
~~~

Fórmula corrigida, com MAX_PREFERENCE_ORDINAL=255 e os mesmos pesos locais:

~~~text
Some(ordinal) -> (MAX_PREFERENCE_ORDINAL - ordinal) * policy_weight
None          -> 0
~~~

`None` permanece neutro, com evidence ausente e sem bônus ou penalidade.
Some(0) recebe 255 vezes o peso; Some(254), uma vez; Some(255), zero.
Em igualdade dos demais componentes, a ordem é Some(0), Some(1), ...,
Some(255), None. A última relação usa o desempate PolicyOrdinal existente,
sem alterar sua implementação nem usar MAX+1.

Cada candidato com ordinal explícito recebe exatamente 255 vezes o peso a
mais que na fórmula anterior. Para quaisquer dois ordinais explícitos a e b,
a diferença absoluta continua `abs(a - b) * policy_weight`. Decisões nas quais
todos os candidatos possuem ordinal explícito conservam as diferenças de
score, a ordem e os winners; a FIX altera a relação com ordinal ausente.

| Profile | Faixa de preference | Faixa anterior do total | Faixa corrigida do total |
|---|---:|---:|---:|
| Economy | [0,1020] | [-7280,403] | [-6260,1423] |
| Balanced | [0,1530] | [-5460,1003] | [-3930,2533] |
| Fast | [0,1530] | [-5140,2703] | [-3610,4233] |

As faixas são atingíveis por inputs válidos. As operações continuam inteiras
e saturating, com somas checked nos testes; os limites não causam overflow.
Os demais componentes, pesos, guards, exclusões e boundaries permanecem iguais.
A única alteração de produção está no componente policy_preference de
`src-tauri/src/cognitive_resources/scoring.rs`.

Sete novos gates em `scoring_tests.rs`, cada um exercitando os três profiles:

- `b2_fix1_none_vs_ordinal_one_explicit_preference_wins`: preferência explícita
  vence ausência; reproduz a falha no código auditado antes da correção.
- `b2_fix1_none_vs_maximum_ordinal_uses_policy_tie_break`: ambos têm componente
  e total zero, mas Some(255) vence None por PolicyOrdinal, mesmo com identidade
  canônica desfavorável.
- `b2_fix1_full_preference_order_with_identical_evidence`: valida a sequência
  Some(0), Some(1), Some(100), Some(254), Some(255), None.
- `b2_fix1_all_explicit_ordinal_distances_are_preserved_each_profile`: verifica
  todos os 256 ordinais e todos os pares, incluindo 0–1 e 10–20.
- `b2_fix1_absent_preference_is_neutral_with_other_signals`: componente None=0
  e evidence None, independentemente de outros sinais presentes.
- `b2_fix1_explicit_only_decision_preserves_old_totals_differences_and_winner`:
  compara com os totais anteriores em decisão com sinais heterogêneos.
- `b2_fix1_updated_score_extrema_checked_sum_without_overflow`: atinge os
  limites mínimo/máximo de cada profile e verifica total, component_sum e
  checked_add, incluindo a faixa positiva máxima de preferência.

Gates anteriores de preferência e ScoreBreakdown foram ajustados para o novo
baseline. Winners de decisões exclusivamente com ordinal explícito são
preservados. O caso existente None versus Some(0) mantém o winner e agora registra
HigherTotal; o caso Some(255) versus None registra PolicyOrdinal.

Validação da FIX em 06/10/2026:

| Gate | Resultado |
|---|---|
| Regressão None vs Some(1), antes da correção | Falha esperada: None venceu em Economy; reproduzido com a fórmula auditada |
| rustfmt dos dois arquivos Rust alterados e `--check` de todo `cognitive_resources` | Exit 0; nenhum legado fora do escopo reformatado |
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources::scoring_tests` | Exit 0; 96 aprovados, 0 falhas, 0 ignorados; 0,05 s |
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources` | Exit 0; 180 aprovados, 0 falhas; 0,12 s |
| B1/FIX-1 e LR-8.5A no módulo e na global | 46 e 38 aprovados, respectivamente |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=4` | Exit 0; 722 aprovados, 0 falhas, 2 ignorados; 247,50 s; main/doc-tests sem falhas |
| cognition / agents na global | 370 / 110 aprovados |
| smart routing / TaskGraph runtime na global | 10 / 13 aprovados |
| LR-8 na global | rate 69, telemetry 33, admission 18, resilience 76, operational 8, LR-8E 14; todos aprovados |
| `git diff --check` e `git diff --cached --check` | Exit 0 |

Warnings comparados com os logs finais da B2 anterior: os mesmos 15 da biblioteca
e dois de fixtures de teste, sem warning novo ou suprimido. Os dois ignorados
continuam sendo `real_app_server_handshake` e
`manual_final_codex_agent_bridge_gate`, que exigem integração Codex manual.
As regressões operacionais foram executadas na suíte global.

O diff de produção foi comparado literalmente com o HEAD auditado: removendo
apenas a substituição do componente policy_preference, `scoring.rs` permanece
idêntico. Nenhum outro arquivo de produção mudou. Scheduler, Auto real, Fixed,
Preferred, affinity real, fallback, retry, admission, resilience, RateLimitManager,
TaskGraph e registries permanecem intocados. As dívidas de B3/B4 acima não mudam.

**B2 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente**.


## B2 — Fechamento técnico

Data: **06/10/2026**  
HEAD auditado: `54ce655575a00ab5a3f4b1497044a3bcd6bba333`

A auditoria independente encerra o checkpoint **B2 — Scarcity, Spend Guard & Deterministic Scoring**
em **PASS técnico**.

Foram confirmados:

- B1 Eligible obrigatório antes de qualquer ranking;
- join B1/B2 por identidade completa, sem reinterpretação de descriptors;
- BillingDomain compartilhado com economics divergentes falha fechado;
- resolução effort → model sem soma de facts;
- allowance multidimensional por dimension ID, sem conversão/soma de unidades;
- scarcity Comfortable/Reduced/Reserve/Exhausted/Unknown;
- Reserve como custo de oportunidade e Exhausted como exclusão econômica;
- LR-8 read-only, provider/model scoped e sem dupla contagem com telemetry;
- spend guard fail-closed para Metered/Prepaid e custos monetários positivos;
- Unknown preservado como neutro, nunca convertido em free/paid/fast/slow;
- Economy/Balanced/Fast com pesos locais explícitos;
- ausência de bônus por CognitiveTier excedente;
- score inteiro, bounded, auditável e determinístico;
- preferência explícita domina ausência após B2 FIX-1;
- nenhum efeito sobre Scheduler, Auto real, Fixed, Preferred, retry/fallback,
  admission/resilience, TaskGraph ou registries.

Gate reportado: B2 96 aprovados; `cognitive_resources` 180 aprovados;
suíte Rust completa 722 aprovados, 0 falhas, 2 ignorados. A execução ocorreu no
ambiente local; a auditoria independente revisou código e diffs remotos.

**Próximo checkpoint: B3 — Variant Expansion & Provider Auto Bridge.**

B3 é o primeiro checkpoint autorizado a alterar o comportamento real de Auto.
Fixed e Preferred devem permanecer semanticamente inalterados. Scheduler continua
autoridade de execução/admission/rate/resilience e não deve absorver regras
comerciais do allocator.

## B3 — Implementação candidata

**B3 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente**.
Somente B3: sem PASS antecipado, B4/C, migration, UI, nova policy persistida,
pricing remoto ou integração de SpecialistAgent/Codex/Copilot.

### Pré-condições e arquitetura

Branch exclusiva `lr-8.5b-allocation-scarcity-policy`; HEAD inicial local/remoto
`b915a963ce92141daf1bd7c5e9c327a21918a89d`. Fetch + fast-forward já atualizado,
workspace limpo e ancestral original
`main@a6eec1b63655d860279f606bc55766255903f1fe` confirmados antes das edições.
Os quatro documentos solicitados foram lidos integralmente; cognitive_resources,
Scheduler, policy/types/registry, TaskGraph, Luna runtime, smart routing e o
composition root foram revisados.

`cognitive_resources/provider_bridge.rs` contém `ProviderAutoAllocator`,
`OperationalVariantProof`, `ResolvedProviderCandidate`, `AutoRoutePlan`,
`AutoRouteEntry`, exclusões e erros tipados. A bridge não executa provider,
HTTP, reservation, admission, refresh de quota ou mutação de catálogo.

~~~text
ProviderTaskRequest.targets (CognitiveRolePolicy)
→ enumeração bounded das variantes autorizadas
→ B1 + prova operacional exata, sem reescrever facts
→ entrada interna sealed para o mesmo núcleo econômico B2
→ ranking global B2
→ primeira/melhor variante de cada provider
→ AutoRoutePlan congelado
→ Scheduler: resilience / budget / rate / admission / execução / retry / fallback
~~~

A fonte do universo é exclusivamente `targets`. O Registry só fornece config e
adapter para IDs já autorizados; não acrescenta candidatos. O catálogo só fornece
descriptors desses IDs. ResourceId deve corresponder ao provider_id, classe deve
ser CognitiveProvider e origin deve apontar ao mesmo Provider Runtime. Enabled e
capabilities Known contraditórios com o Registry falham fechado; não há merge
permissivo. IDs duplicados/inválidos, ausência de descriptor e incoerência não
produzem uma cadeia parcialmente inventada.

### Catálogo de produção e default temporário

Scheduler possui um allocator com catálogo provider-only, inicializado uma vez a
partir de ProviderConfig, usando `CognitiveResource::from_provider_config`:

- ResourceId, ProviderFamily e BillingDomainId recebem a identidade local do provider;
- access_path é `provider_runtime`; origin é `Provider(provider_id)`;
- enabled e capabilities descrevem somente o contrato registrado;
- models, availability remota e todos os economics permanecem Unknown.

Não foram criadas tabelas de preço, latência, qualidade, billing kind, quotas,
modelos ou efforts. Um erro na inicialização de allocation é guardado e bloqueia
Auto; não impede construir/executar Fixed ou Preferred. Catálogo enriquecido,
AllocationPolicy e quality floor podem ser injetados por constructor nos testes.

Produção usa explicitamente: **Balanced / VariantSelectionMode::Auto /
PaidUsePolicy::Deny / reserve=None**, sem quality floor configurado nesta B3.
`AllocationPolicy::default()` público da B1/B2 não mudou. Thresholds sintéticos
ficam nas fixtures; defaults persistíveis e controles pertencem à B4.

### OperationalVariantProof e Unknown

A prova tem constructor privado e nasce somente após registro, enabled, runtime
capabilities suficientes e `supports_invocation` aceitando o target/mode exatos.
Ela conserva identidade completa, ProviderTarget (incluindo timeouts), InvocationMode
exato e capabilities do runtime. Sua reutilização para modelo, effort, timeout ou
mode diferentes retorna InvalidOperationalProof. Não contém backend handles;
não é serializada e seu Debug omite o mode/schema.

`ResolvedProviderCandidate` tem campos/constructor privados. Conserva a evidence
B1 original, a prova e CandidateSignals; só aceita B1 sem contradições e com
**todas** as reasons restantes cobertas. A whitelist operacional permite:

- ModelSupportUnknown e EffortSupportUnknown da invocação exata;
- AvailabilityUnknown em Resource/Model e em Effort quando selecionado;
- ResourceEnabledUnknown, porque registro enabled foi comprovado separadamente;
- CapabilityUnknown apenas no scope Runtime/camada Resource, quando o contrato
  registrado prova explicitamente a capability exigida.

Nunca cobre ResourceDisabled, ModelNotSupported, EffortNotSupported,
AvailabilityUnavailable, CapabilityUnsupported, CognitiveTierBelowFloor,
CognitiveTierUnknown com floor obrigatório ou CapabilityUnknown model-specific.
Known(false)/Unavailable não são reescritos; catálogo Known sem modelo/effort
pedido continua rejeitando essa variante mesmo se o adapter a aceitar.

Models Unknown não vira Known([policy_model]); supported_efforts Unknown também
não é preenchido artificialmente. Sem ModelProfile descrito, uma ExecutionFacts
Unknown transitória alimenta a resolução B2: tier, RelativeCostTier, monetary
cost e latency Unknown, sem consumos/allowances inventados. Com modelo descrito e
effort Unknown, fatos realmente descritos do modelo seguem a resolução B2 já
aprovada; não há facts falsos de effort. Quality floor Unknown não é promovido.

A entrada interna `AllocationScoringRequest::from_provider_candidates` só aceita
os valores sealed da bridge. Reutiliza o join econômico, spend/scarcity guards,
pesos, score, sorting e desempate B2. A API pública B2 continua exigindo B1
Eligible e recusando Unresolved/Ineligible; B1 e seu relatório não foram alterados.
As evidence Unknown originais permanecem Unknown, inclusive na avaliação interna.

### Variant expansion e collapse

A variante explícita sempre entra na avaliação. Em Auto, listas Known de modelos
são enumeradas, com no-effort e efforts explicitamente descritos. Todas as
invocações representáveis são validadas pelo adapter no InvocationMode real.
Não há inferência por nome, lista remota ou modelo hardcoded.

`EffortId::try_thinking_level()` aceita apenas low/medium/high. xhigh, ultra,
reasoning-4 e outros não representáveis geram `EffortBridgeUnsupported`, sem
converter para High, panic ou invalidar as outras variantes válidas. Isso é uma
limitação da bridge atual, não um erro dos facts do catálogo.

A expansão é canonizada por AllocationVariant e limitada a
MAX_ALLOCATION_CANDIDATES=256, incluindo variantes não representáveis na contagem.
Excesso retorna ExpansionOverflow antes de consultar adapters/scorer; não trunca.
Depois do ranking global B2, a primeira entrada de cada provider é mantida.
Assim há no máximo um ProviderTarget por provider, mantendo UNIQUE(role,provider_id),
validação do Scheduler e fallback somente entre providers. O target vencedor
preserva provider_id/timeouts e substitui somente model/thinking_level.

### CandidateSignals e affinity

Todos os modelos/efforts de um provider recebem seu ordinal original da policy
(0..MAX_TARGETS-1). Priority usa explicitamente `min(priority,32)`, sem rejeitar
u16 maior. Required ProviderCapabilities são mapeadas para CapabilityScope::Runtime;
não são copiadas para ModelProfile. `supports_invocation` valida a compatibilidade
concreta de modelo/effort/mode.

`context_switch_signal(bytes) = min(100, ceil(bytes/1024))`, com divisão antes da
adição para evitar overflow. Zero produz ausência de continuidade/switching;
sem affinity autorizada conhecida, ambos são neutros. Para affinity conhecida,
a variante daquele provider recebe continuidade positiva; os demais recebem
switching cost positivo na mesma escala. São sinais relativos locais, não preço,
quota, tokens, capacidade, qualidade ou economia de cache comprovada.

Affinity permanece no mesmo armazenamento bounded, session/runtime scoped e
atualizada após sucesso, inclusive Fixed/Preferred. Restart limpa affinity.
Não foi criado banco/ledger de continuidade. Pesos B2 podem mudar o winner por
profile e contexto; as expectativas numéricas D2 dos testes foram atualizadas
para B2, inclusive o menor sinal positivo de contexto. `auto_score()` e suas
constantes exclusivas foram removidos; não há soma/reordenação Auto posterior.

### Scheduler, TaskGraph, eventos e LR-8

`resolve_provider_chain` é a única engine de ordering. `run` a chama uma vez antes
da primeira tentativa; `ranked_provider_ids` a usa através da projeção read-only
`ranked_provider_targets`. A assinatura de ranking recebe InvocationMode explícito,
com callers atualizados, sem assumir streaming para structured output.
TaskGraph usa os targets escolhidos nessa mesma engine, conservando a variante
B2 ao fixar cada Worker. O pin Fixed posterior é a fronteira de execução já
existente, não uma segunda decisão Auto. A projeção de IDs permanece disponível
para os callers/testes que só precisam de IDs.

Fixed/Preferred conservam também a distinção operacional anterior: a projeção
consultiva ignora runtimes desabilitados ou sem capabilities exigidas, enquanto
execução preserva suas recusas/validação exata. Essa finalidade não cria outro
ordering Auto; ambas usam o mesmo plano B3 nesse modo.

O plano contém targets/scores/exclusões bounded, sem prompt, input, histórico,
affinity key, credenciais, account IDs remotos, headers, handles ou ScoreBreakdown.
SchedulerEvent::Selected e TaskEventKind::ProviderSelected agora usam Option<i64>.
Auto emite `auto_allocator` e o total B2 da variante planejada; Fixed conserva
`fixed`, Preferred conserva `preferred_order`, ambos score=None. TypeScript
continua number|null e adiciona auto_allocator; strings antigas permanecem no
union apenas para compatibilidade. Os bridges Luna/TaskGraph repassam o i64.

Cada planejamento captura telemetry.snapshots() e **rate.read_only_snapshots()**
uma única vez por autoridade. Não usa o reader mutável rate.snapshots().
`EconomicContext::capture_provider_models` aplica as mesmas validações de DTO,
identity, generation, bounds e duplicatas da captura B2, projetando também o
scope exato de modelos operacionalmente comprovados sem ModelProfile. Isso não
altera membership do catálogo. Provider scope aplica; sibling model não aplica.
Capturas não prometem transação global; generation divergente falha fechado.

Rate/resilience/admission continuam sendo revalidados nos gates existentes antes
de chamada real. Uma recusa depois do plano é respeitada, sem re-score. Retries
usam o mesmo target/score; fallback consome somente o próximo provider do plano.
429 não adiciona provider, não relaxa PaidUsePolicy e não habilita path pago.
Partial output continua terminal para retry/fallback; cancellation, output budget,
PendingSchedulerAttempt/call budget, event ordering, usage e RAII foram preservados.
Fixed/Preferred bypassam integralmente allocation/economics/expansion, inclusive
com facts paid/exhausted/expensive ou catálogo de allocation inválido.

### Gates sintéticos e integrados

`provider_bridge_tests.rs` contém 41 gates B3, com adapters controlados, clocks
injetados, LR-8/Scheduler reais e HTTP exclusivamente loopback no gate Paid Deny.
Matriz dos requisitos A–AE:

| Gates | Evidência |
|---|---|
| A/B/L | Fixed/Preferred com paid+exhausted+expensive; invocação/timeouts explícitos e distinção ranking/execução preservados; erro de catálogo não bloqueia modos explícitos |
| C/D/E/F | Só B2, registry-only ausente, modelo/effort Unknown com prova exata, Known absent/Unavailable e quality floor invioláveis |
| G/H/I/J | Model/effort expansion Economy, múltiplos providers/variantes e collapse global, timeouts, exclusões xhigh/ultra/reasoning-4, overflow sem truncar |
| K/M/N | Paid Deny antes de HTTP/reservation/admission, Reserve reordena e continua utilizável quando único, Exhausted exclui |
| O/P/Q/AC | LR-8 provider/exact/sibling com catálogo Unknown, reset só projetado em cópia, ledger/DTO/catalog invariantes e geração divergente tipada |
| R/S/T | Signals bounded/clamp/ordinal, continuidade por sucesso real, zero/ausência neutros, restart sem affinity, Balanced/Fast versus Reserve |
| U/V/W | Engine compartilhada por ranking/run/targets TaskGraph, auto_allocator com total B2 e serialização real de score negativo |
| X/Y/Z/AA/AB | Plano/variante/score congelados, retries antes de fallback, 429 sem salto pago, chunk terminal, cancellation e call budget |
| AD/AE | Determinismo com registry/catalog em ordens diferentes, JSON plan/event sem markers privados e Debug de proof sem schema |

Gates adicionais verificam impossibilidade de reutilizar a prova para sibling,
outro effort, timeout/mode diferente, nenhuma fabricação de Known e recusa da
mesma request Unresolved pela API pública B2. As regressões antigas conservam
Fixed/Preferred e os gates operacionais; somente expectativas do Auto D2 ou de
seleção esgotada foram adaptadas à decisão B3 autorizada.

Validação final em 2026-10-06 (contagens por módulo dentro da suíte completa;
grupos sobrepostos não devem ser somados):

| Gate | Resultado técnico |
|---|---|
| B3, execução dirigida `provider_bridge` | 41 aprovados, 0 falhas |
| B2 | 96 aprovados, 0 falhas |
| B1/FIX-1 | 46 aprovados, 0 falhas |
| LR-8.5A (base + FIX-1/2/3) | 38 aprovados, 0 falhas |
| `cognitive_resources`, execução dirigida | 221 aprovados, 0 falhas |
| Smart routing / Scheduler unitários | 10 / 2 aprovados |
| Rate / admission (módulos principais + integração) | 70 / 20 aprovados |
| Resilience / telemetry | 76 / 33 aprovados |
| LR-8E | 14 aprovados |
| TaskGraph runtime | 13 aprovados |
| Conversation preflight | 12 aprovados |
| Orchestrator / summary | 17 / 12 aprovados |
| Suíte Rust completa, `--test-threads=4` | 762 aprovados, 0 falhas, 2 ignorados; 272,92s |
| `cargo check` | concluído sem erros |
| Frontend `npm run typecheck` | concluído sem erros |
| `test-provider-operations.cjs` / `test-provider-operations-dom.cjs` | ambos concluídos sem falhas |
| `git diff --check` e staged diff | sem erros |
| rustfmt dos 16 arquivos Rust criados/alterados | sem diferenças |

Comandos Rust dirigidos:

~~~bash
cargo test --manifest-path src-tauri/Cargo.toml provider_bridge
cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=4
cargo check --manifest-path src-tauri/Cargo.toml
~~~

A execução completa inclui todas as regressões listadas acima, testes HTTP
loopback dos adapters, segurança/persistence e demais módulos.

Os dois ignorados são `real_app_server_handshake` (requer app-server local) e
`manual_final_codex_agent_bridge_gate` (requer Codex autenticado e quota). Não
pertencem à B3 e não foram habilitados para consumir inferência externa.

Avisos: 12 warnings de biblioteca e 1 warning da fixture de testes, sem
supressão. Incluem a projeção `ranked_provider_ids` sem consumidor de produção
agora que TaskGraph precisa dos targets completos; os demais são warnings já
existentes. A checagem global de rustfmt ainda encontra 41 arquivos com drift
anterior; cada um foi comparado com `b915a963ce92141daf1bd7c5e9c327a21918a89d`
usando o mesmo formatter. Nenhum drift novo foi introduzido; esses arquivos fora
do escopo não foram reformatados.

Durante desenvolvimento foram corrigidas uma comparação que incluía timestamps
observacionais e a expectativa numérica de Conversation (1542 = 1524 + 2 + 4×4).
A distinção anterior de ranking/execução de Fixed/Preferred recebeu regressão
explícita antes da validação final. Não houve relaxamento de deadlines, retirada
de gates ou alteração dos contratos públicos B1/B2 para obter os resultados.

### Dívidas B4/C e limitações

B4: persistência/configuração por role, UI, defaults duráveis, configuração de
budget/Reserve/quality floor e calibração futura dos pesos/sinal de contexto.
Economics e catálogo de modelos/efforts de produção continuam Unknown enquanto
não houver fonte real. Sem refresh econômico remoto, tabela comercial, pricing
web, ledger monetário, purchase/refill ou configuração de budgets nesta B3.

C/trilhas posteriores: same-provider fallback entre variantes, handoff seguro,
replay/checkpoint e continuidade após output; SpecialistAgent allocation e
execução real de Codex/Copilot permanecem fora do escopo. Preservam-se as
limitações LR-8: snapshots não globalmente atômicos, TokenUpperBound possivelmente
ausente, accounting Unknown/conservador, health transitório, SQLite síncrono em
mutações e pressuposto de uma instância ativa. Prova operacional não garante
availability remota nem substitui qualquer autoridade operacional.

**B3 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente**.


## B3 — Fechamento técnico

Data: **06/10/2026**  
HEAD auditado: `cb5c3262c9c716f3a1bcfd87532ab9c96b219bb0`

A auditoria independente encerra o checkpoint **B3 — Variant Expansion & Provider Auto Bridge**
em **PASS técnico**.

Foram confirmados:

- universo Auto derivado exclusivamente dos targets autorizados;
- OperationalVariantProof limitado à invocação exata e sem fabricação de facts;
- catálogo Unknown preservado como Unknown;
- contradições Known continuam invioláveis;
- expansão de modelos/efforts bounded e representável pelo contract atual;
- no máximo uma variante por provider entregue ao Scheduler;
- Fixed e Preferred fora do allocator econômico;
- Auto real passa por B1+B2;
- score legado do Scheduler retirado da autoridade do Auto;
- `ranked_provider_ids`, TaskGraph e `run` compartilham a mesma engine;
- AutoRoutePlan congelado como cadeia autorizada completa, não winner único;
- retries/fallback consomem apenas a cadeia congelada;
- 429/503/timeout podem avançar para o próximo provider seguro;
- recurso pago previamente excluído não pode ser introduzido por falha no meio da execução;
- partial output continua impedindo retry/fallback inseguro;
- LR-8 continua autoridade operacional no momento da chamada;
- eventos Auto usam `auto_allocator` e score B2 `i64`.

Gate reportado: B3 41 aprovados; `cognitive_resources` 221 aprovados;
suíte Rust completa 762 aprovados, 0 falhas, 2 ignorados. A execução ocorreu no
ambiente local; a auditoria independente revisou código, testes e diffs remotos.

**Próximo checkpoint: B4 — Policy Persistence, Settings Surface & Final Gate.**

B4 deve tornar a policy econômica durável/configurável sem alterar as fronteiras
B1/B2/B3, sem introduzir pricing remoto e sem antecipar LR-8.5C.


## B4 — Implementação candidata

Data: **06/10/2026**
Branch: `lr-8.5b-allocation-scarcity-policy`
HEAD remoto de entrada confirmado: `1c752d29236d5ef4ea92c24d49721106ef01529d`
Commit de implementação validado: `d8554e04f15b472e7bfdc5db0ce09a962a8cf04b`
Base da trilha: `main@a6eec1b63655d860279f606bc55766255903f1fe`

Esta candidata implementa exclusivamente **Policy Persistence, Settings Surface &
Final Gate**. B1/B2/B3 mantêm seus contratos. A auditoria independente deverá
avaliar a B4 antes do fechamento e merge da trilha.

### Migration 013 e contrato persistido

`013_cognitive_allocation_policy.sql` cria `cognitive_role_allocation_policies`,
separada de `cognitive_role_policies`. A migration e `PRAGMA user_version = 13`
compartilham uma transação com rollback por RAII. Versões futuras `>13` falham.

| Coluna | Contrato SQLite |
|---|---|
| `role` | PK não nula, quatro roles, FK para routing com `ON DELETE CASCADE` |
| `allocation_profile` | `economy`, `balanced`, `fast` |
| `variant_selection_mode` | `explicit`, `auto` |
| `minimum_cognitive_tier` | NULL ou inteiro `0..255` |
| `paid_use_policy` | `deny`, `allow_known_cost_within_budget` |
| `max_paid_currency` / `max_paid_micros` | Deny exige ambos NULL; Allow exige ambos presentes, moeda exatamente três bytes ASCII A–Z e inteiro `0..9007199254740991` |
| `reduced_below_percent` / `reserve_below_percent` | Ambos NULL ou inteiros `0 <= reserve <= reduced <= 100` |
| `updated_at` | Texto não nulo atualizado pelo save |

Os CHECKs incluem presença explícita e `typeof`, evitando tanto pares parciais
quanto o bypass de CHECK por resultado NULL. Não há JSON econômico opaco, FX,
observações privadas, preços, account IDs ou secrets na tabela.

Fresh DB → v13 e upgrade v12 → v13 produzem as mesmas quatro linhas:
Conversation, Summary, Orchestrator e Worker com **Balanced / Auto variant /
Paid Deny / sem floor / sem reserve**. Currency, micros e thresholds começam
NULL. Esses defaults são exatamente a policy temporária de produção aprovada na
B3. O teste do upgrade usa routing Auto com dois targets e compara os snapshots
runtime com a fixture B3; os gates sintéticos de ranking preservam esse contrato.

### DTO e snapshot runtime validado

`cognition/allocation_policy.rs` introduz `CognitiveRoleAllocationPolicy`, DTO de
configuração local separado de `CognitiveRolePolicy`. O DTO é desserializável;
`MonetaryAmount`, `ReservePolicy`, `PaidUsePolicy`, `AllocationPolicy` e
`AllocationRuntimePolicy` não ganham Deserialize de aggregates.

`to_runtime()` usa exclusivamente `CognitiveTier::new`, `MonetaryAmount::new` e
`ReservePolicy::new` para criar `AllocationRuntimePolicy`, com campos privados:
`AllocationPolicy` e `Option<QualityFloor>`. O snapshot é provider-agnostic e
válido por construção. Enum desconhecido, row ausente, pares incoerentes e valores
inválidos falham com códigos sanitizados, sem ecoar budget/currency/payload.

`load_role_runtime_policy(s)` captura routing, targets e allocation na mesma
SQLite read transaction. TaskGraph captura Orchestrator e Worker em uma única
transação de preflight. Uma transação já aberta pelo caller é respeitada. Após o
preflight, o snapshot viaja na tarefa; não há consulta global durante ranking.

### Save composto e API Settings

`get_ai_settings` mantém o contrato de routing e acrescenta `allocationPolicies`
com as quatro configurações locais. Sua leitura composta também usa uma única
read transaction. Não expõe catálogo, ScoreBreakdown ou observations econômicas.

`update_cognitive_role_settings` valida roles iguais, routing, allocation e
credentials/compatibility atuais. Routing, targets, allocation e limpeza de
pending summaries quando Summary está disabled são escritos na **mesma write
transaction**. Qualquer falha faz rollback de tudo. A resposta contém os dois
DTOs relidos no banco antes do commit. SummaryWorker continua recebendo kick.

O command de routing anterior permanece compatível e conserva a limpeza
transacional de Summary disabled. `summary_input_max_bytes = 0` continua
significando desligamento; salvar economics não reativa o resumo.

### Policy por tarefa e wiring dos quatro papéis

`ProviderTaskRequest.allocation_policy` transporta `Option<AllocationRuntimePolicy>`.
Auto exige Some válido. None falha antes de ProviderSelected, fallback,
reservation, admission ou provider call. O preflight também rejeita row ausente
ou corrompida; não há default silencioso de produção.

Fixed/Preferred ignoram allocation no Scheduler e não carregam a row econômica
no preflight. Continuam executando com row econômica ausente/corrompida ou até
com tabela econômica indisponível na fixture. O editor exige o conjunto completo
para exibir configuração, mas explicit execution não depende dele.

- **Conversation:** o preflight real carrega o par; `chat_budget_and_request`
  repassa o snapshot para Scheduler. Gates adicionais atravessam `start_conversation`
  e comprovam corrupção fail-closed e save durante preflight já capturado.
- **Summary:** cada execução claimed captura routing/allocation juntos. Auto
  inválido é deferido como configuração inválida antes de provider call;
  Fixed/Preferred continuam independentes e disabled permanece disabled.
- **Orchestrator:** preflight carrega o par e os builders de planning/TaskGraph
  recebem a allocation do próprio papel.
- **Worker/TaskGraph:** a allocation de Worker capturada junto com Orchestrator
  chega ao ranking inicial através de `rank_worker_targets`. Após atribuição, a
  unidade executa Fixed com allocation None; não recebe novo score econômico.

`ranked_provider_ids`, `ranked_provider_targets` e `run` continuam compartilhando
`resolve_provider_chain`. A engine recebe só o snapshot runtime e não lê SQLite,
role ou Settings. `cognitive_resources` continua sem SQLite.

`ProviderAutoAllocator` possui **somente ResourceCatalog**. `plan` recebe a policy
da tarefa e deriva o floor dessa policy. A antiga policy B3 existe apenas como
helper cfg(test) para fixtures de regressão; não é autoridade de produção.
O catálogo de produção continua minimalista, com economics/tiers/latency/
allowances/modelos adicionais Unknown.

`routing_mode` escolhe o modo de ordenação entre providers. Separadamente,
`variant_selection_mode = explicit` limita cada provider à invocação configurada;
`auto` permite a expansão B3 de variantes conhecidas, representáveis e suportadas.
Nenhum modo introduz providers fora dos targets autorizados ou discovery remoto.

### Painel IA e precisão monetária

Cada RoleForm mantém um único botão Salvar e inclui a seção compacta **Auto
econômico**: perfil, seleção de variante, mínimo cognitivo opcional, reserva local
e paid use. Fixed/Preferred desativam visualmente a seção preservando os valores.
Voltar para Auto reapresenta a configuração anterior. A UI atualiza seu estado
com a resposta do backend; reload retorna os valores efetivamente persistidos.

O teto decimal é por decisão/invocação. O parser string-based com BigInt aceita
no máximo seis casas, rejeita expoentes, negativos, overflow e moeda fora de três
letras ASCII maiúsculas; micros é convertido para Number somente após validar o
limite JSON-safe. O formatador permite roundtrip exato inclusive no máximo.
Não há multiplicação floating-point, arredondamento de autorização, FX, saldo,
depósito, orçamento mensal, preço de plano ou ledger acumulado.

Reserva exige os dois thresholds locais em `0 <= reserve <= reduced <= 100`;
desabilitada persiste ambos NULL. Floor aceita sem mínimo ou `0..255` e avisa que
Unknown não é promovido, podendo deixar Auto sem candidatos no catálogo atual.
A UI explica que facts desconhecidos não são presumidos gratuitos, pagos,
rápidos ou abundantes. Não há fields de preço/provider ou editor BillingDomain.

### Gate integrado final e congelamento

O gate `b4_final_integrated_persisted_deny_zero_paid_http_then_allow_new_task`
atravessa SQLite → runtime snapshot → builder Conversation/ProviderTaskRequest →
expansão B3 → B1 → B2 → AutoRoutePlan → Scheduler → adapter sintético com HTTP
loopback. Economy/Deny exclui A pago/rápido com **zero HTTP, reservation e admission**;
B suficiente executa e o evento usa `auto_allocator`. Persistir Allow com teto
suficiente permite A na nova tarefa; o snapshot antigo mantém Deny. Nenhuma API
comercial é chamada.

Os gates também comprovam Economy→Fast mudando o winner somente na nova tarefa;
profiles/paid independentes por role; Explicit→Auto na expansão; floor eliminando
tier insuficiente e Unknown; cost Unknown, currency mismatch, exceeded/zero
budget, zero known cost e Prepaid; reserve configurada e ausência de thresholds.

O gate de save durante fallback conserva a cadeia [A,B,C]: A 429 → B 503 → C.
Ordem, modelos, scores, autorização paga, floor e thresholds permanecem
congelados. Provider recém-autorizado e paid previamente excluído não entram na
tarefa; não há reload, expansão adicional ou re-score. A alteração persistida
vale somente para a próxima tarefa. Os eventos públicos não recebem economics.

### Validação e regressões

B4 dedicada: **54 testes Rust aprovados**, incluindo 37 persistence/atomicity,
15 runtime/gate/settings/architecture e 2 preflight real de Conversation.
Frontend B4: **85 verificações aprovadas**, cobrindo precisão/bounds, currency,
thresholds, floor, preservação e renderização React estática da seção econômica.
`npm run typecheck`, helpers de operações e DOM de operações também aprovados.

As contagens abaixo foram extraídas da execução integral final; grupos se
sobrepõem, e os 54 testes B4 também foram executados como filtro dedicado.

| Gate/grupo | Aprovados |
|---|---:|
| B4 dedicada | 54 |
| B3 | 41 |
| B2 | 96 |
| B1/FIX-1 | 46 |
| LR-8.5A | 38 |
| Todos cognitive_resources | 221 |
| Persistence legado | 32 |
| Smart routing | 10 |
| Scheduler (módulo) | 2 |
| Rate | 70 |
| Admission | 20 |
| Resilience | 76 |
| Telemetry | 33 |
| LR-8E | 14 |
| TaskGraph runtime | 13 |
| Conversation preflight | 14 |
| Orchestrator | 17 |
| Summary | 12 |
| Suíte Rust completa | **816**, 0 falhas, 2 ignorados |

Comandos executados: `cargo test --manifest-path src-tauri/Cargo.toml b4_ -- --test-threads=4` e
`cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=4`;
`cargo check --manifest-path src-tauri/Cargo.toml`; `npm run typecheck`;
`node scripts/test-allocation-settings.cjs`,
`node scripts/test-provider-operations.cjs` e
`node scripts/test-provider-operations-dom.cjs`.

A primeira execução integral encontrou dois asserts de fixtures: uma contagem
antiga de mensagens alterada indevidamente ao atualizar asserts de versão e um
assert de migration ainda esperando v12. Ambos foram corrigidos sem alterar
comportamento de produção; a suíte integral foi repetida e passou.

Rustfmt executado nos arquivos Rust alterados. Drift legado fora desses
arquivos não foi reformatado; diffs amplos apenas de formatação foram revertidos
nos módulos de registro/persistence e builders com alteração pontual. `git diff --check` aprovado. Os warnings de base
permanecem sem supressão: 12 de biblioteca e 1 da fixture de testes, sem warnings
novos; inclui `ranked_provider_ids` sem consumidor de produção. Os dois gates
externos ignorados continuam exigindo app-server/Codex autenticado e quota.

### Arquivos desta candidata

Criados (8):

~~~text
scripts/test-allocation-settings.cjs
src-tauri/migrations/013_cognitive_allocation_policy.sql
src-tauri/permissions/autogenerated/update_cognitive_role_settings.toml
src-tauri/src/cognition/allocation_policy.rs
src-tauri/src/cognition/allocation_policy/runtime_tests.rs
src-tauri/src/cognition/allocation_policy/tests.rs
src/settings/AllocationPolicyEditor.tsx
src/settings/allocationPolicyDraft.ts
~~~

Alterados (32):

~~~text
docs/LR-8.5B-ALLOCATION-SCARCITY-POLICY.md
src-tauri/build.rs
src-tauri/capabilities/settings-ai.json
src-tauri/src/cognition/admission_tests.rs
src-tauri/src/cognition/fix5_tests.rs
src-tauri/src/cognition/gemini.rs
src-tauri/src/cognition/groq.rs
src-tauri/src/cognition/groq_commands.rs
src-tauri/src/cognition/lr8e_gate_tests.rs
src-tauri/src/cognition/mod.rs
src-tauri/src/cognition/orchestrator.rs
src-tauri/src/cognition/policy.rs
src-tauri/src/cognition/rate_tests.rs
src-tauri/src/cognition/resilience_tests.rs
src-tauri/src/cognition/scheduler.rs
src-tauri/src/cognition/settings.rs
src-tauri/src/cognition/summary.rs
src-tauri/src/cognition/task_graph_runtime.rs
src-tauri/src/cognition/task_graph_worker.rs
src-tauri/src/cognition/telemetry_tests.rs
src-tauri/src/cognition/tests.rs
src-tauri/src/cognition/types.rs
src-tauri/src/cognitive_resources/allocation.rs
src-tauri/src/cognitive_resources/provider_bridge.rs
src-tauri/src/cognitive_resources/provider_bridge_tests.rs
src-tauri/src/lib.rs
src-tauri/src/luna/conversation_preflight_tests.rs
src-tauri/src/luna/runtime.rs
src-tauri/src/persistence/migrations.rs
src-tauri/src/persistence/mod.rs
src-tauri/src/persistence/tests.rs
src/settings/AiSettingsApp.tsx
~~~

### Limitações e dívidas LR-8.5C

Policy configurável não cria fatos econômicos. O catálogo de produção continua
Unknown; ativar floor pode eliminar todos os candidatos. Não há inferência de
tier/preço/latency, refresh, fonte econômica nova ou alteração de pesos B2.

Permanece a race operacional aprovada: capacidade LR-8 pode estar disponível no
plano e a reservation real falhar depois de mudança concorrente. B4 não converte
`RateCapacityExceeded` em replan econômico; melhoria de liveness fica para trilha
futura. Snapshots de telemetria/rate não são globalmente atômicos. Demais
limitações de accounting/health/instância LR-8 e congelamento B3 permanecem.

LR-8.5C e trilhas posteriores continuam responsáveis por eventual handoff seguro,
fallback de segunda variante no mesmo provider, replay/idempotency/checkpoint de
efeitos externos e continuidade após output. Nada disso foi implementado aqui.
SpecialistAgent allocation, Codex/Copilot execution, novas APIs comerciais,
price discovery/web fetch, compras/refill, balance polling, invoices, ledger
monetário e FX permanecem fora do escopo.

**B4 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente**.


## Fechamento LR-8.5B

Data: **06/10/2026**  
HEAD final auditado antes do fechamento: `20153cc0879b13294c5c1b3cabbd93507cd2e039`

A auditoria independente encerra **LR-8.5B — Allocation, Model/Effort Selection &
Scarcity Policy** em **PASS técnico**.

Estado consolidado:

- B1 — Allocation Contracts & Hard Gates: **PASS**;
- B2 — Scarcity, Spend Guard & Deterministic Scoring: **PASS**;
- B3 — Variant Expansion & Provider Auto Bridge: **PASS**;
- B4 — Policy Persistence, Settings Surface & Final Gate: **PASS**.

A trilha entrega um Auto provider-agnostic que exige hard eligibility antes do score,
preserva Unknown sem interpretação otimista, considera scarcity/custo de oportunidade,
aplica spend guard fail-closed, compara variantes conhecidas, congela uma cadeia
autorizada para execução e mantém Fixed/Preferred semanticamente explícitos.

A B4 torna a policy econômica persistente por papel sem mover SQLite para o Scheduler.
Conversation, Summary, Orchestrator e Worker recebem snapshots imutáveis por tarefa;
alterações de Settings valem somente para novas tarefas. O save composto de routing +
allocation é transacional, e a migration 013 preserva exatamente os defaults aprovados
na B3.

O gate final reportado passou com **816 testes Rust, 0 falhas e 2 ignorados**, além
dos gates frontend/typecheck. O gate integrado comprovou SQLite → snapshot → B3/B1/B2
→ AutoRoutePlan → Scheduler → provider sintético, incluindo Paid Deny com zero HTTP /
reservation / admission para o recurso excluído, nova tarefa com Paid Allow e cadeia
congelada preservando fallback 429 → 503 → próximo provider.

Limitações deliberadamente preservadas para fases posteriores:

- catálogo de produção ainda pode permanecer Unknown;
- facts econômicos reais não são inventados por configuração;
- race entre planejamento LR-8 e reservation operacional não dispara replan econômico;
- não existe fallback para segunda variante do mesmo provider;
- não existe handoff/replay depois de output parcial;
- SpecialistAgent allocation e execução Codex/Copilot permanecem fora da LR-8.5B.

Nenhuma parte da LR-8.5C foi antecipada.

**Próxima subfase liberada: LR-8.5C — Safe Cross-Resource / Cross-Variant Handoff.**
