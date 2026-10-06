# LR-8.5B — Allocation, Model/Effort Selection & Scarcity Policy

Estado: **PLANEJAMENTO APROVADO — implementação ainda não iniciada.**

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
