# LR-8.5 — Cognitive Resource Economy & Allocation

Estado: **PLANEJADA — executar somente após LR-8 = PASS completo e antes da LR-9.**

## Motivação

A LR-7 provou roteamento multi-provider, fallback/Auto/affinity e TaskGraph distribuído.
A LR-8 está fechando capacidade operacional: quota factual, admission control, fila,
accounting, budgets, backoff/cooldown, circuit breaker e observabilidade.

Isso ainda não responde uma pergunta de produto mais ampla:

> **qual recurso cognitivo vale consumir agora?**

A Luna não terá apenas "providers gratuitos" e "providers pagos". Ela poderá dispor
simultaneamente de:

- tiers gratuitos de Cognitive Providers;
- franquias incluídas em assinaturas;
- créditos incluídos em benefícios estudantis;
- Specialist Agents com quota própria;
- saldo pré-pago de APIs;
- cobrança medida;
- recursos locais;
- recursos cujo custo, quota ou reset são desconhecidos.

Exemplo concreto de direção arquitetural: Codex pode usar uma franquia vinculada ao
plano ChatGPT; GitHub Copilot pode possuir outra quota vinculada a um benefício
estudantil; Groq/Cloudflare podem fornecer capacidade gratuita; OpenAI API, DeepSeek,
Anthropic ou outros podem usar saldo/cobrança próprios. Esses recursos não são
economicamente equivalentes e também não são necessariamente funcionalmente
intercambiáveis.

A regra-alvo deixa de ser:

~~~text
acabou a quota gratuita
→ usar a API paga
~~~

e passa a ser:

~~~text
recurso atual ficou escasso/indisponível
→ listar recursos compatíveis com a tarefa
→ considerar capacidade, quality floor, quota, reset, urgência,
  afinidade, switching cost e custo monetário
→ preservar recursos escassos quando existir alternativa adequada
→ gastar dinheiro somente quando a policy autorizar
~~~

## Princípio central

**Quota também possui custo de oportunidade, mesmo quando não há cobrança por
chamada.**

Um recurso incluído numa assinatura ou programa estudantil não custa R$ 0 no sentido
operacional: sua franquia pode ser limitada e seu consumo agora pode retirar capacidade
de uma tarefa futura em que ele seria mais valioso.

A LR-8.5 cria uma camada de **economia de recursos cognitivos** acima dos adapters.
Ela não transforma todos os recursos na mesma interface e não move autoridade para
providers ou agentes externos.

## Relação com as abstrações existentes

A arquitetura continua distinguindo:

- **Cognitive Provider** — inferência geral atrás do Scheduler;
- **Specialist Agent** — runtime agentivo próprio, como Codex/Copilot;
- **Local Cognitive Support** — suporte local auxiliar;
- **Tool Runtime** — efeitos e ferramentas sob autoridade do Luna Core.

A LR-8.5 introduz uma visão ortogonal:

~~~text
                         Luna Core
                            │
                    Resource Allocator
                            │
          ┌─────────────────┴─────────────────┐
          │                                   │
  Cognitive Providers                 Specialist Agents
  Gemini / Groq / ...                 Codex / Copilot / ...
          │                                   │
          └──────── economic metadata ────────┘
                            │
                    Resource Economy
~~~

O Resource Allocator não precisa executar a chamada. Ele decide, dentro da policy,
**qual recurso elegível deve receber a próxima unidade de trabalho**.

## Vocabulário conceitual

### CognitiveResource

Identidade econômica/operacional de um recurso que pode realizar trabalho cognitivo.
Não substitui Provider ou AgentBackend.

Campos conceituais:

~~~text
resource_id
resource_class
provider_family
access_path
billing_domain
capabilities
availability
quota_state
reset_state
monetary_cost_state
health
latency_state
task_affinity
switching_cost
reserve_policy
~~~

Campos reais só entram se houver necessidade comprovada. O contrato final pode ser
menor.

### ProviderFamily

Família tecnológica/comercial, por exemplo OpenAI, Anthropic, Groq, DeepSeek ou
GitHub. Não define sozinho como o recurso é autenticado ou cobrado.

### AccessPath

Forma específica de acesso a uma família.

Exemplo conceitual:

~~~text
OpenAI
├── ChatGPTPlan / Codex
└── DirectApi
~~~

Duas rotas da mesma família podem possuir autenticação, quota, accounting e billing
completamente diferentes.

### BillingDomain

Domínio econômico isolado. Exemplos genéricos:

- IncludedAllowance;
- FreeTier;
- PrepaidCredits;
- MeteredBilling;
- Unknown.

**Billing domains não compartilham saldo por inferência.** Duas integrações da mesma
família não podem ser tratadas como o mesmo orçamento sem prova factual.

### ScarcityState

Estado derivado apenas de fatos e configuração local, não de tabelas comerciais
hardcoded.

Possíveis conceitos:

- confortável;
- reduzido;
- reserva;
- esgotado;
- reset conhecido/aguardável;
- desconhecido.

Os nomes finais podem mudar. Um percentual só pode ser usado quando numerador e
denominador forem realmente conhecidos.

### ReservePolicy

Policy configurável que permite preservar parte de uma quota para trabalho de maior
valor.

Exemplo:

~~~text
Codex allowance restante ≈ 5%
nova tarefa de código começa em checkpoint seguro

→ não bloquear Codex
→ aumentar custo de oportunidade do Codex no modo Auto
→ considerar Copilot/outro recurso compatível
→ ainda permitir Codex quando sua vantagem justificar ou quando o usuário o fixar
~~~

reserve_floor não é quota comercial nem bloqueio absoluto. É um sinal de alocação
local.

## Invariantes

1. **UNKNOWN != FREE.**
2. **UNKNOWN != PAID.**
3. **UNKNOWN != ZERO COST.**
4. Falta de preço não autoriza gasto.
5. Falta de quota não significa quota infinita nem quota zero.
6. HTTP 429 isolado não autoriza migração para recurso pago.
7. Erro transitório não é prova de allowance esgotada.
8. Uma seleção explícita do usuário continua tendo precedência sobre heurística
   automática, salvo impossibilidade técnica, segurança, permissões ou limites reais.
9. Nenhum adapter decide autonomamente "vale gastar dinheiro".
10. Um recurso só participa de uma seleção se satisfizer as capabilities e o quality
    floor exigidos pela unidade de trabalho.
11. Não há handoff automático depois de output parcial quando isso puder duplicar
    trabalho ou efeitos.
12. Efeitos externos já executados nunca são repetidos apenas porque mudou o recurso
    cognitivo.

## Política de alocação

O Scheduler/Allocator deve considerar apenas sinais disponíveis e normalizados:

- capabilities exigidas;
- quality floor da tarefa;
- quota restante quando factual;
- reset quando factual;
- scarcity/reserve policy;
- custo monetário estimado/observado quando factual;
- limite financeiro autorizado;
- latência;
- health/circuit state;
- task affinity;
- custo de reconstrução de contexto;
- switching cost;
- urgência/prioridade;
- preferência explícita do usuário.

### Custo de oportunidade cognitivo

A decisão não deve minimizar apenas USD/BRL.

Um recurso sem cobrança marginal pode ser preservado se sua quota for rara e um
recurso alternativo abundante conseguir satisfazer a mesma tarefa.

Exemplo ilustrativo:

~~~text
Recursos:
- Codex: quota baixa;
- Copilot: quota confortável;
- Groq: capacidade gratuita disponível;
- API paga: saldo disponível.

Tarefas:
A. ajuste CSS simples
B. implementação Rust média
C. auditoria de concorrência difícil

Alocação possível:
A → recurso geral barato/gratuito
B → Copilot
C → preservar Codex para a tarefa em que seu valor marginal é maior
~~~

Isto é comportamento de Auto/Allocator. Uma policy Fixed explícita não deve ser
silenciosamente reescrita.

## Escalada para dinheiro

A LR-8.5 deve separar pelo menos os seguintes conceitos quando a integração conseguir
observá-los:

- RateLimited;
- AllowanceExhausted;
- CreditBalanceExhausted;
- SpendLimitExceeded;
- PaymentRequired;
- falha transitória;
- estado econômico desconhecido.

Os nomes finais pertencem ao contrato Rust; esta lista registra a semântica.

Fluxo-alvo:

~~~text
recurso preferido indisponível/escasso
        │
        ▼
existem recursos compatíveis já incluídos/gratuitos?
        │
   sim ─┴─► considerar esses recursos primeiro segundo policy
        │
       não
        ▼
há recurso pago compatível?
        │
   não ─┴─► pausar / aguardar / reportar
        │
       sim
        ▼
policy autoriza gasto?
        │
   não ─┴─► pausar / aguardar / pedir decisão quando apropriado
        │
       sim
        ▼
custo/saldo/limites são suficientemente conhecidos?
        │
   não ─┴─► fail closed para auto-spend
        │
       sim
        ▼
executar dentro do budget
~~~

A LR-8.5 não deve definir preços comerciais no Core. Pricing mutável pertence a
configuração/fonte factual específica e deve carregar provenance/validade.

## Handoff entre recursos

A troca segura ocorre preferencialmente em:

- início de uma nova subtarefa;
- fronteira de nó do TaskGraph;
- checkpoint persistido;
- após conclusão de uma etapa;
- antes de uma nova chamada sem output parcial.

Exemplo desejado:

~~~text
etapa 1 concluída com Codex
→ TaskState persistido
→ Codex entra em reserva
→ etapa 2 ainda não iniciou
→ Allocator seleciona Copilot
→ Copilot recebe somente o contexto/handoff necessário
~~~

Evitar:

~~~text
Codex executou metade de uma operação com efeitos
→ quota mudou
→ reiniciar a mesma operação inteira em outra API
~~~

O Shared Cognitive State continua sendo a autoridade de continuidade. Nenhum provider
ou Specialist Agent é dono exclusivo do estado da tarefa.

## Subfases

### LR-8.5A — Resource Domains & Access Paths

Objetivo: criar a linguagem provider-agnostic de recurso econômico sem mudar ainda
o routing real.

Entregas:

- contrato mínimo CognitiveResource ou equivalente;
- classes CognitiveProvider, SpecialistAgent e LocalSupport preservadas;
- ProviderFamily, AccessPath e BillingDomain;
- quota/saldo/custo/reset conhecidos ou desconhecidos explicitamente;
- provenance de sinais econômicos;
- distinção entre quota incluída, free tier, prepaid e metered sem regras por marca
  no Core;
- snapshots/telemetria seguros;
- nenhuma tabela comercial hardcoded.

Gate: representar corretamente cenários sintéticos com múltiplos recursos da mesma
família e billing domains independentes.

### LR-8.5B — Allocation & Scarcity Policy

Objetivo: tornar o Auto consciente de escassez e custo de oportunidade sem autorizar
gasto silencioso.

Entregas:

- scarcity/reserve policy configurável;
- quality floor/capability gate antes do score econômico;
- score que considere quota, reset, custo, affinity e switching cost;
- perfis Econômico/Balanceado/Rápido alinhados à arquitetura existente;
- autorização explícita de uso pago;
- budgets monetários quando houver fonte factual;
- distinção entre rate limit, allowance exhaustion e billing failures;
- nenhuma escalada 429 → paid implícita.

Gate: simulações mostram preservação de recurso escasso quando existe alternativa
adequada e uso do recurso escasso quando sua capability/qualidade é necessária.

### LR-8.5C — Safe Cross-Resource Handoff

Objetivo: provar continuidade entre recursos heterogêneos.

Entregas:

- checkpoints/handoff estruturado via Shared Cognitive State;
- provenance por unidade;
- troca somente em fronteiras seguras;
- idempotência/anti-duplicação de efeitos;
- cancelamento preservado;
- política de pausa quando auto-spend não for autorizado;
- cenários sintéticos allowance → alternativa incluída → paid reserve.

Gate mínimo desta fase usa mocks/fixtures e contratos já existentes. **Não puxar a
implementação completa de Copilot ou Codex para LR-8.5.**

O gate real Codex ↔ Copilot fica condicionado às integrações posteriores de
LR-10/LR-11.

## Relação com o roadmap

Sequência-alvo:

~~~text
LR-8D
→ LR-8E
→ LR-8 PASS
→ LR-8.5 Cognitive Resource Economy & Allocation
→ LR-9 Voice / feedback natural
→ LR-10 GitHub Copilot SpecialistAgent
→ LR-11 OpenAI Codex SpecialistAgent
→ LR-12 provider pack adicional / paid providers
~~~

### LR-10 e LR-11

Essas fases conectam quotas reais e lifecycle dos Specialist Agents ao contrato
econômico criado na LR-8.5. A LR-8.5 não antecipa SDK, login, approvals, sandbox ou
session lifecycle desses agentes.

### LR-12

LR-12 deixa de ser apenas "mais adapters" e passa a ser também o primeiro local
natural para validar providers pagos reais contra a economia de recursos.

Candidatos atuais incluem:

- DeepSeek API direta;
- OpenAI API paga;
- Anthropic;
- outros providers que acrescentem capacidade, quota ou fallback útil.

Valores mínimos de recarga, preços e tabelas comerciais **não fazem parte do
contrato** e não devem ser hardcoded; são fatos mutáveis.

## Relação com LR-8

A LR-8 continua com seu escopo atual e **não recebe LR-8F**.

LR-8 responde:

> "posso chamar este recurso agora sem violar capacidade/quota/budget operacional?"

LR-8.5 responde:

> "entre os recursos que podem executar esta unidade, qual é racional consumir agora
> e tenho autorização para gastar?"

Assim:

- LR-8 permanece responsável por rate limits, fila, accounting, cooldown, circuit
  breaker e telemetria operacional;
- LR-8.5 consome esses fatos e acrescenta semântica de escassez, billing domain,
  custo de oportunidade e spend policy;
- adapters continuam traduzindo fatos específicos;
- Luna Core continua sendo a autoridade de policy.

## Não objetivos

A LR-8.5 não deve:

- integrar DeepSeek/OpenAI/Anthropic apenas para provar a abstração;
- completar Copilot ou Codex;
- armazenar cartão ou dados de pagamento;
- comprar créditos automaticamente;
- alterar assinatura/plano de terceiros;
- hardcodar preços/tabelas de planos;
- assumir que duas quotas da mesma empresa são intercambiáveis;
- transformar todos os recursos em Provider;
- permitir auto-spend com custo desconhecido;
- reexecutar efeitos externos para facilitar handoff.

## Critério de fechamento

LR-8.5 fecha quando o runtime consegue demonstrar, sem dinheiro real:

1. múltiplos recursos heterogêneos e billing domains independentes;
2. quota conhecida, unknown e scarcity sem precisão falsa;
3. preservação de quota escassa;
4. seleção de alternativa compatível incluída/gratuita;
5. escalada paga somente quando policy e budget autorizam;
6. recusa conservadora de auto-spend em estado econômico desconhecido;
7. handoff em checkpoint sem duplicar efeitos;
8. preferência explícita do usuário preservada;
9. integração limpa com fatos produzidos pela LR-8;
10. caminho claro para LR-10/LR-11/LR-12 sem antecipá-las.

Depois disso, providers e Specialist Agents reais podem entrar como novos recursos
sem cada adapter reinventar sua própria interpretação de "vale gastar dinheiro?".
