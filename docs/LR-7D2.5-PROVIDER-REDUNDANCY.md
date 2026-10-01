# LR-7D2.5 — provider redundancy + Gemini de-risking

Estado: **PLANEJADA. Não iniciar antes de LR-7D2 = PASS completo e integrada.**
Posição no roadmap: **LR-7D2 → LR-7D2.5 → LR-7D3**.

## Motivação

Os gates reais de LR-7D1/LR-7D2 mostraram que Gemini pode responder com
`503 service_unavailable` e `Retry-After`, enquanto o restante da arquitetura
continua saudável. O Scheduler já sabe tratar cooldown/fallback, mas o conjunto
real ainda possui apenas Gemini e Groq. Isso deixa pouco espaço operacional quando
um provider está degradado e torna a redundância menos robusta do que a arquitetura
pretende.

A decisão desta mini-trilha é explícita:

> **Gemini continua suportado, mas não é dependência funcional nem gate obrigatório
> da Luna. Nenhum Cognitive Provider individual deve ser necessário para
> Conversation, Summary, Orchestrator ou para o fechamento da LR-7.**

O Luna Core permanece a identidade e a autoridade. Providers são recursos
cognitivos substituíveis.

## Objetivo

Adicionar **pelo menos dois Cognitive Providers reais adicionais**, preferindo
infraestruturas independentes, e validar a LR-7D2 com um conjunto real maior de
targets antes de avançar para o task graph da D3.

Candidatos iniciais:

1. **Mistral API direta** — provider independente para conversa, planejamento e
   workers gerais.
2. **Cloudflare Workers AI** — segunda infraestrutura independente e primeiro
   candidato forte para exercitar uma base reutilizável OpenAI-compatible.

A escolha será revalidada no início da implementação quanto a acesso real,
compatibilidade e possibilidade de uso sem custo obrigatório. Se um candidato
deixar de atender ao projeto, ele deve ser substituído por outro provider direto
e independente antes de declarar a mini-trilha concluída.

OpenRouter permanece útil como agregador/fallback futuro, mas **não conta como uma
das duas novas rotas principais de redundância nesta fase**, para não trocar uma
dependência de provider por um novo ponto único de falha intermediário.

## Princípios

- nenhum provider recebe status de "LLM principal";
- Gemini, Groq e os novos providers usam o mesmo contrato de Cognitive Provider;
- policy, identidade, memória, permissões, budgets e lifecycle continuam no Luna Core;
- credenciais permanecem separadas no SecretStore/Stronghold;
- a UI nunca recebe secrets;
- `Fixed`, `Preferred` e `Auto` continuam sendo os modos da D2;
- Auto só escolhe entre targets explicitamente autorizados pelo usuário;
- não adicionar sinais fictícios de quota/latência/custo antes da LR-8;
- falha/cooldown de um provider não deve degradar a sessão inteira quando outro
  target autorizado e saudável puder executar o mesmo papel;
- nenhuma dependência comercial deve aparecer em validações semânticas, migrations
  ou gates;
- D2.5 não executa task graph, paralelismo ou ferramentas de D3/LR-13.

## Transporte reutilizável

A mini-trilha deve avaliar uma base comum para APIs OpenAI-compatible, sem assumir
que todos os providers são semanticamente idênticos.

A camada comum pode reaproveitar:

- HTTP/SSE;
- request/stream lifecycle;
- cancelamento;
- usage básico;
- timeout;
- framing comum de Chat Completions quando realmente compatível.

Cada integração continua responsável por:

- endpoint/base URL;
- autenticação;
- modelos/defaults;
- capabilities;
- parâmetros suportados;
- parsing de erro;
- rate-limit/retry metadata;
- particularidades de streaming;
- grounding/tool support quando existir;
- normalização factual de usage.

Compatibilidade de protocolo não autoriza copiar silenciosamente semântica de
Groq, Mistral ou Cloudflare entre si.

## Subfases propostas

### LR-7D2.5A — base extensível de adapters

- identificar duplicação real entre adapters atuais;
- extrair somente transporte/contratos que sejam de fato comuns;
- manter erro/capability/configuração específicos por provider;
- garantir que adicionar um provider novo não exija alterar regras comerciais no Scheduler.

Gate: testes locais provam que a abstração comum não muda comportamento de
Gemini/Groq nem mistura configuração/credenciais.

### LR-7D2.5B — Mistral como Cognitive Provider real

- registro no ProviderRegistry;
- credencial própria no Stronghold;
- configuração de model/thinking/capabilities conforme suporte real;
- streaming, usage, timeout, cancelamento e erros normalizados;
- integração com Fixed/Preferred/Auto;
- Conversation real;
- Summary/Orchestrator somente se o contrato de saída puder ser validado
  fail-closed como nos providers existentes.

Gate: chamada real + streaming/usage/cancelamento + restart/persistência, sem
fallback oculto.

### LR-7D2.5C — Cloudflare Workers AI como Cognitive Provider real

- credencial/configuração próprias;
- primeiro uso real da base OpenAI-compatible, se a compatibilidade se confirmar;
- model configurável sem hardcode de um único modelo;
- streaming/usage/cancelamento/erros;
- Fixed/Preferred/Auto;
- nenhuma credencial ou regra Cloudflare no frontend.

Gate: chamada real e integração ao Scheduler com os mesmos invariantes de Core.

### LR-7D2.5D — gate de redundância multi-provider

Validar um conjunto real alvo de quatro providers:

```text
Gemini
Groq
Mistral
Cloudflare Workers AI
```

Se um dos dois candidatos novos for inviável, substituí-lo por outro provider
direto antes do gate.

O gate deve demonstrar:

1. Gemini desabilitado, em cooldown ou artificialmente indisponível não impede
   Conversation quando existem outros targets autorizados;
2. Auto escolhe somente entre providers autorizados e saudáveis, sem hardcode
   comercial;
3. Preferred respeita a ordem escolhida com 3+ targets reais;
4. pelo menos um novo provider produz `PlanV1` válido no Orchestrator ou falha
   fechado sem corromper a tarefa;
5. pelo menos um novo provider executa Summary válido ou é explicitamente marcado
   como incompatível para esse papel por capability;
6. restart preserva policy, ordem, model/configuração e estado seguro das
   credenciais; affinity continua com a semântica definida na D2;
7. cancelamento e terminal único continuam corretos;
8. nenhuma indisponibilidade do Gemini exige mudança de código ou reconfiguração
   estrutural da aplicação;
9. falha de um provider não perde identidade, sessão, histórico ou TaskState.

## Mudança do gate da LR-7D3

O gate antigo citava Gemini + Groq nominalmente. Isso deixa de ser correto.

A D3 deve exigir que uma tarefa real use **pelo menos dois Cognitive Providers
independentes, autorizados e elegíveis**, registre qual provider executou cada
subtarefa e consolide um único resultado.

Gemini pode participar, mas **não é obrigatório**.

## Relação com LR-8

D2.5 não antecipa o Rate Limit Manager completo.

Ela usa apenas os sinais já disponíveis na D2:

- configured/enabled;
- capability;
- policy/ordem;
- cooldown/health já conhecido;
- score atual;
- affinity atual.

RPM/TPM/RPD/TPD, queue, token bucket, circuit breaker completo, telemetria de
latência/custo e decisões baseadas em quota real continuam na LR-8.

## Critério de fechamento

LR-7D2.5 só pode ser declarada PASS completo quando:

- duas novas rotas cognitivas reais foram integradas e validadas;
- Gemini pode ser retirado do conjunto ativo sem tornar a Luna incapaz de
  conversar/planejar dentro dos papéis suportados;
- o Scheduler opera com 3+ targets reais e sem regra por marca;
- pelo menos três infraestruturas de inferência independentes permanecem
  operacionalmente utilizáveis;
- os gates técnico, auditoria independente e gate humano passarem;
- D3 ainda não tiver sido antecipada.

Somente depois disso começa **LR-7D3 — task graph mínimo + subtarefas
independentes**.
