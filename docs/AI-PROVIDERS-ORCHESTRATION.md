# Provedores, SDKs e orquestração de IA

> Catálogo arquitetural e operacional. Verificado em 25/09/2026.
>
> Limites, modelos e planos mudam com frequência. O runtime não deve codificar cotas comerciais como constantes permanentes; deve permitir configuração e, quando possível, leitura dinâmica de quota/headers.

## 1. Objetivo

A Luna deve usar múltiplos recursos cognitivos sem depender de um único fornecedor. O Luna Core mantém identidade, memória, tarefa, permissões e orçamento; provedores e agentes especialistas executam unidades de trabalho.

A política global prioriza:

1. economia;
2. timing;
3. distribuição.

Essas prioridades são **defaults operacionais**, não autorização para esconder limites de capacidade. Parâmetros que afetem qualidade, latência, custo ou comportamento do modelo devem ser configuráveis e observáveis quando o provider os expuser.

### Política de configuração controlada pelo usuário

O Luna Core deve representar uma política persistida por papel cognitivo (`conversation`, `summary`, `voice`, `worker`, especialistas e futuros papéis), em vez de espalhar constantes de produto dentro dos adapters.

**Atualização UIP-6A:** `conversation` e `summary` agora têm policy persistida por role na migration 003, com provider/modelo/thinking/output/calls editáveis. Somente Gemini está integrado; nenhum agente especialista foi promovido a Cognitive Provider. Consulte [UIP-6-SETTINGS.md](UIP-6-SETTINGS.md).

Registro histórico da UIP-5C (substituído pela UIP-6A): `summary` foi o primeiro papel usado concretamente em um worker de fundo. Seu scheduler é selecionado no composition root e usa temporariamente o provider real disponível, com 1024 tokens de output e uma chamada por sessão. Esta é uma política provisória, sem configuração persistida; provider, modelo, output e thinking por papel permanecem trabalho da UIP-6.

Por papel/provider, a configuração deve poder expressar, quando suportado:

- provider/agente e modelo;
- reasoning/thinking;
- máximo de output ou `provider_max`/sem teto adicional da Luna;
- orçamento de contexto e histórico;
- temperatura/top-p e equivalentes;
- streaming;
- timeouts;
- retries;
- fallback/overflow;
- ferramentas, grounding/web e outras capacidades;
- limites de custo/cota definidos pelo usuário.

Adapters traduzem essa política para a API específica. Eles não devem inventar restrições de produto próprias. Valores não suportados devem ser recusados ou apresentados como indisponíveis; clamps inevitáveis de provider devem ser observáveis, nunca silenciosos.

Uma seleção explícita do usuário tem precedência sobre a heurística do Scheduler. O Scheduler pode advertir e aplicar apenas guardrails de segurança/integridade e limites reais da integração. Se o usuário fixar, por exemplo, um agente Codex compatível como papel `conversation`, o runtime deve respeitar essa escolha enquanto a integração sustentar esse modo.

**Decisão LR-7D (28/09/2026):** não existe uma “LLM principal da Luna”. O Luna Core é a autoridade do sistema; Gemini, Groq e futuros providers ocupam papéis cognitivos substituíveis. LR-7D0 remove hardcodes comerciais das policies de Conversation/Summary e torna primary/fallback configuráveis por provider registrado. LR-7D1 adiciona um papel cognitivo real `orchestrator`/Planner, também configurável, sem transferir ao modelo autoridade sobre permissões ou execução. LR-7D2 evolui para fallback chain + Auto/score + affinity; LR-7D3 fecha com task graph mínimo. Consulte [LR-7D-COGNITIVE-ROLES-ROUTING.md](LR-7D-COGNITIVE-ROLES-ROUTING.md).

## 2. Tipos de integração

### Cognitive Provider

Adapter de inferência geral. Contrato conceitual:

~~~text
capabilities()
health()
estimate_cost()
complete(request)
stream(request)
usage()
~~~

O adapter deve normalizar:

- mensagens/entrada;
- streaming;
- tool calling quando suportado;
- tokens/uso;
- rate-limit metadata;
- erros;
- retry-after;
- custo estimado/real;
- contexto máximo e capacidades.

### Specialist Agent

Runtime agentivo com ciclo de ferramentas próprio. Contrato distinto:

~~~text
capabilities()
start_task()
stream_events()
cancel()
resume()
usage()
~~~

Codex e GitHub Copilot entram inicialmente nesta categoria.

## 3. Candidatos de baixo custo / gratuitos

### Google Gemini API

**Status:** candidato primário para primeiro provider geral.

A API Gemini mantém Free Tier para determinados modelos. Projetos novos podem começar no nível gratuito e os limites variam por modelo/projeto. O runtime deve consultar configuração atual e tratar 429 RESOURCE_EXHAUSTED.

Papel provável:

- conversa geral;
- classificação;
- planejamento leve/médio;
- Luna Voice em modo econômico;
- fallback geral.

Fonte: https://ai.google.dev/gemini-api/docs/billing
Rate limits: https://ai.google.dev/gemini-api/docs/rate-limits

### Groq

**Status:** candidato forte para worker rápido.

A Groq mantém Free Plan com limites por modelo em RPM, RPD, TPM e TPD. Em 25/09/2026, por exemplo, alguns modelos como openai/gpt-oss-120b, openai/gpt-oss-20b e qwen/qwen3.8-27b aparecem com 30 RPM, 1.000 RPD, 8K TPM e 200K TPD no plano gratuito. Esses números são exemplo datado, não configuração fixa do produto.

Os headers incluem informações de limite, restante, reset e Retry-After em 429.

Papel provável:

- workers rápidos;
- análise curta;
- classificação;
- subtarefas paralelas;
- overflow quando outro provider estiver congestionado.

Fonte: https://console.groq.com/docs/rate-limits

### Mistral

**Status:** candidato geral secundário.

O Mistral Studio é habilitado em Free mode por padrão e permite gerar API key sem cartão, sujeito aos limites da conta.

Papel provável:

- segundo provider independente;
- workers médios;
- fallback do Gemini/Groq;
- comparação de qualidade/latência.

Fonte: https://docs.mistral.ai/getting-started/quickstarts/developer/first-api-request

### Cohere

**Status:** candidato experimental/backup.

Trial keys são gratuitas e limitadas. Em 25/09/2026 a documentação informa 1.000 chamadas/mês para trial e 20 req/min em Chat para os modelos listados.

Papel provável:

- experimentos;
- classificação/geração alternativa;
- reserva de baixa frequência.

Não assumir adequação para produção com trial key.

Fonte: https://docs.cohere.com/v1/docs/rate-limits

### OpenRouter Free

**Status:** candidato de fallback/agregação.

Em 25/09/2026 o plano Free anuncia 25+ modelos gratuitos, 4 providers gratuitos e 50 requests/dia.

Papel provável:

- fallback de emergência;
- experimentação com modelos diferentes;
- opção quando providers diretos estiverem sem cota.

Não usar como excuse para duplicar providers sem necessidade; providers diretos continuam preferíveis quando oferecem quota melhor e telemetria mais clara.

Fonte: https://openrouter.ai/pricing/

### Cloudflare Workers AI

**Status:** candidato forte por compatibilidade e cota diária.

Em 25/09/2026 a Cloudflare oferece 10.000 Neurons/dia sem cobrança no Workers AI; alguns modelos específicos exigem plano pago/método de cobrança.

Workers AI oferece endpoint compatível com OpenAI Chat Completions para a maioria dos modelos de texto, permitindo reaproveitar um adapter OpenAI-compatible trocando base URL, token e model.

Papel provável:

- provider geral de baixo custo;
- fallback;
- teste do adapter OpenAI-compatible;
- modelos open-weight.

Fontes:
- https://developers.cloudflare.com/workers-ai/platform/pricing/
- https://developers.cloudflare.com/workers-ai/configuration/open-ai-compatibility/

### Hugging Face Inference Providers

**Status:** candidato apenas para experimentação.

Usuários Free recebem atualmente US$ 0,10/mês em créditos de Inference Providers, sujeito a mudança. É pouco para conversa diária, mas útil para testar integração/modelos.

Fonte: https://huggingface.co/docs/inference-providers/pricing

## 4. OpenAI API tradicional

**Status:** provider opcional pago / alta complexidade.

A assinatura ChatGPT não deve ser tratada como crédito da API tradicional. OpenAI API entra como provider próprio, com chave e orçamento financeiro separado.

Política inicial proposta:

- desabilitada por padrão ou marcada como restrita;
- autorização para alta complexidade conforme configuração;
- limite monetário por tarefa/dia;
- nunca escolhida apenas porque outro provider está momentaneamente mais lento se o perfil Econômico proibir gasto.

## 5. OpenAI Codex como agente especialista

**Status:** candidato prioritário para programação/agente de software.

O Codex não deve ser modelado como completion provider comum.

### SDK oficial

O repositório oficial fornece Codex SDK. O SDK TypeScript instala como:

~~~text
@openai/codex-sdk
~~~

Ele envolve o Codex CLI, inicia/resume threads e oferece runStreamed(), que entrega eventos estruturados de progresso, chamadas de ferramentas e mudanças em arquivos.

O SDK Python publicado também explicita login ChatGPT por browser/device-code e reutilização de sessão Codex.

Fontes:
- https://github.com/openai/codex/tree/main/sdk/typescript
- https://github.com/openai/codex/blob/main/sdk/python/docs/getting-started.md

### Autenticação

A integração deve suportar o caminho de **conta ChatGPT/Codex autenticada**, separado da OpenAI API paga.

Não transformar token de sessão ChatGPT em uma falsa OPENAI_API_KEY. Codex autenticado e OpenAI API são produtos/caminhos diferentes.

### Integração recomendada para nosso Rust Core

Como o Luna Core será Rust, há três opções técnicas a validar em protótipo, nesta ordem:

1. **Codex app-server/CLI como subprocesso controlado pelo Rust**, consumindo protocolo/eventos estruturados;
2. pequeno bridge interno usando SDK oficial TypeScript, apenas se ele reduzir significativamente complexidade;
3. SDK Python apenas se houver motivo técnico forte.

Preferência atual: manter o frontend fora do controle de credenciais e processos; a integração deve viver atrás do Rust Core.

O repositório oficial contém app-server/protocolo JSON-RPC usado para interfaces ricas do Codex, o que combina bem com um adapter especialista.

Referência: https://github.com/openai/codex/tree/main/codex-rs/app-server

### Papel na Luna

Usar Codex para:

- analisar repositório;
- diagnosticar testes;
- editar arquivos;
- executar comandos de desenvolvimento autorizados;
- tarefas longas de engenharia.

Não gastar sua franquia em perguntas triviais.

## 6. GitHub Copilot SDK

**Status:** candidato prioritário como segundo agente especialista, aproveitando Copilot Student.

O GitHub mantém SDK oficial para TypeScript, Python, Go, .NET, Java e **Rust**.

Rust:

~~~text
cargo add github-copilot-sdk
~~~

Arquitetura do SDK:

~~~text
Luna Core
   ↓
Copilot SDK
   ↓ JSON-RPC
Copilot CLI server
   ↓
GitHub Copilot
~~~

Fonte: https://github.com/github/copilot-sdk

### Autenticação por conta GitHub

O SDK suporta usuário GitHub conectado e OAuth. O modo padrão pode reutilizar credenciais do Copilot CLI; aplicações também podem passar token OAuth do usuário.

As requisições usam a assinatura Copilot do usuário autenticado. Não é necessária uma API key de modelo OpenAI/Anthropic para o modo normal do Copilot.

Fonte: https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/authenticate

### Quota e telemetria

O SDK expõe:

- token counts por chamada;
- uso de janela de contexto;
- métricas acumuladas;
- custo em AI credits;
- preços por modelo;
- quota da conta via account.getQuota.

Isso é extremamente útil para o Scheduler da Luna: ele pode considerar quota real antes de despachar trabalho.

Fonte: https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/usage-and-billing

### Copilot Student

Em 25/09/2026, GitHub documenta que **Copilot Student e Copilot Free usam modelos por auto model selection**. Portanto, não projetar a integração dependendo de escolher arbitrariamente um modelo específico no Student.

Fonte: https://docs.github.com/en/copilot/reference/ai-models/supported-models

### Papel na Luna

- agente especialista de programação;
- alternativa/complemento ao Codex;
- uso da franquia já disponível no Student;
- worker agentivo quando sua quota estiver saudável.

## 7. Adapter OpenAI-compatible

Criar uma abstração para providers que implementam protocolo compatível com OpenAI, mas **não assumir compatibilidade perfeita**.

Campos/configuração mínimos:

- provider id;
- base URL;
- API key reference;
- model;
- capabilities;
- rate limits conhecidos;
- timeout;
- retry policy;
- streaming;
- tool support;
- responses/chat-completions dialect.

Cloudflare Workers AI será um bom primeiro teste dessa abstração.

## 8. Scheduler: política de distribuição

Uma resposta do Gemini não obriga a próxima etapa a ir novamente para Gemini.

Exemplo:

~~~text
Etapa 1 — entender pedido       → Gemini
Etapa 2 — resumir arquivo A     → Groq
Etapa 3 — analisar log B        → Mistral
Etapa 4 — editar repositório    → Codex
Etapa 5 — consolidar resultado  → Luna Voice
~~~

A troca só acontece quando o ganho superar custo de contexto/latência.

### Sinais considerados

- suitability;
- quota_remaining;
- rpm/tpm headroom;
- queue_depth;
- estimated_latency;
- estimated_cost;
- context_transfer_cost;
- failure/cooldown state;
- task affinity;
- user policy.

## 9. Rate limiting

Manter estado por provider/model quando necessário:

~~~text
ProviderQuotaState
- rpm_limit / remaining / reset
- tpm_limit / remaining / reset
- rpd/tpd
- concurrent_limit
- cooldown_until
- recent_latency
- recent_errors
- cost_today
~~~

Mecanismos:

- token bucket;
- fila;
- concurrency semaphore;
- Retry-After;
- exponential backoff + jitter;
- circuit breaker/cooldown;
- orçamento por tarefa.

## 10. Orçamento por complexidade

Configuração inicial conceitual, não números congelados:

- **trivial**: zero ou uma chamada;
- **simples**: uma chamada preferencialmente;
- **média**: poucas chamadas, distribuição somente se houver ganho;
- **complexa**: task graph + workers;
- **agentiva longa**: orçamento explícito, progress events e cancelamento.

O runtime deve parar quando o valor marginal de uma nova chamada não justificar tempo/custo.

## 11. Luna Voice

A saída final e feedback natural devem preservar uma identidade única mesmo com workers diferentes.

A Luna Voice recebe um pacote resumido:

~~~text
UserRequest
TaskResult
RelevantIdentity
RelevantMemory
RecentConversation
PresentationConstraints
~~~

Workers internos podem responder em formato estruturado. A Luna Voice não deve receber automaticamente todos os logs/tokens brutos.

## 12. Ordem de integração recomendada

**Estado LR-5 (26/09/2026):** o contrato `Provider` Rust, Registry, Context Builder e Scheduler inicial estão implementados com dois IDs mock locais. Há seleção por enabled/capability/prioridade, retry básico, cooldown por `retry_after`, fallback, budget de chamadas/output e uso artificial. Chunks passam por Tauri Channel. Consulte [runtime cognitivo LR-5](COGNITION-RUNTIME.md). Nenhum provider real, segredo ou quota comercial foi integrado. A etapa 2 abaixo depende primeiro do gate da chave de desbloqueio Stronghold em [SECURITY.md](SECURITY.md).

**Extensão LR-6 (26/09/2026):** `GeminiProvider` implementa Interactions API em um Scheduler separado com somente `gemini`; não há fallback mock para chat real. `ProviderRequest` mantém o `ContextBundle` local, mas `MinimalOutboundContext` só autoriza nome e idioma da identidade e a mensagem atual. `store:false` é explícito. SSE fornece texto e usage real; 429 usa o cooldown do Scheduler. Naquele checkpoint, o gate com a API real ainda aguardava chave manual; a LR-6 foi depois fechada em PASS. Contrato e limites: [GEMINI-PROVIDER.md](GEMINI-PROVIDER.md).

1. MockProvider local para testar Scheduler sem gastar cota.
2. Gemini como primeiro provider geral.
3. Groq como segundo provider real isolado na LR-7B; distribuição/fallback Gemini ↔ Groq é provada na LR-7C.
4. Rate Limit Manager completo.
5. OpenAI-compatible adapter + Cloudflare Workers AI.
6. OpenRouter/Cohere/Hugging Face como experimentos/fallback.
7. GitHub Copilot SDK como SpecialistAgent.
8. Codex adapter como SpecialistAgent.
9. OpenAI API paga somente depois de orçamento/limites estarem consolidados.

Essa ordem pode mudar por bloqueio técnico, mas a primeira prova de arquitetura precisa usar pelo menos **dois providers independentes** para evitar uma abstração falsa.

**Atualização UIP-6C (28/09/2026):** `conversation` é foreground; `summary` é background oportunista e não inicia provider enquanto há conversa em andamento. O Scheduler mantém cooldown compartilhado para 429 e 503/Unavailable com Retry-After. Summary já iniciado não é interrompido; preemption e Rate Limit Manager completo seguem planejados para LR-8. UIP-6C permanece CANDIDATA ao gate humano.

**Atualização LR-7A (28/09/2026):** LR-7 foi iniciada com a fundação multi-provider local. O contrato usa `ProviderTimeouts` e seleção explícita `Fixed` (somente o escolhido), `Preferred` (primeiro o escolhido, com fallback permitido) ou `Auto` (prioridade entre elegíveis). A policy persistida existente continua `Fixed("gemini")`; a UI não expõe novos modos. `RateLimited` e `Unavailable` com Retry-After registram cooldown e podem seguir para outro provider elegível antes de qualquer chunk, dentro do orçamento. Depois do primeiro chunk não há retry nem fallback. Cancelamento e falha de Channel encerram a tarefa; erros terminais não disparam fallback. Groq fica para LR-7B; nenhum gate multi-provider real está em PASS e LR-8 segue separada.

**LR-7A-FIX (28/09/2026):** o routing era provider-aware, mas parâmetros de invocação ainda eram únicos por request. `ProviderTaskRequest` agora carrega targets com configuração individual; ao selecionar um target, o Scheduler constrói um `ProviderRequest` contendo apenas a configuração daquele provider. Target sem configuração única e válida falha fechado, sem reutilizar model, thinking ou timeouts de outro. A configuração Gemini persistida permanece `Fixed("gemini")` e seu handle de timeouts pertence ao estado Gemini específico. Groq permanece LR-7B; nenhum gate real multi-provider foi executado. LR-8 segue separada.


**LR-7B fechada (28/09/2026): PASS técnico + humano.** Groq entrou como Cognitive Provider real de prioridade 2 usando `openai/gpt-oss-20b`. O adapter possui configuração/timeout/credencial próprios e fronteira outbound explícita. O diagnóstico real `Fixed(groq)` concluiu por streaming com usage observado de 126 tokens de entrada, 22 de saída e 148 total; a credencial persistiu após restart. Conversation/Summary permaneceram `Fixed(gemini)`, e um 429 real do Gemini confirmou que o Scheduler não faz fallback oculto quando a policy é Fixed. Distribuição/overflow real fica para LR-7C. Quotas atuais não são hardcoded; LR-8 continua responsável por rate-limit manager/telemetria avançada.


**LR-7C fechada (28/09/2026): PASS técnico + humano.** `conversation` pode persistir `Preferred(gemini)` com target Groq explícito; cada target mantém model/thinking/timeouts próprios. A migration preserva `Fixed` até opt-in do usuário. O frontend consulta o estado de roteamento e o fallback observável registra `from → to → reason`. Em provider real foram validados Gemini saudável → Gemini, Gemini 429 → Groq na mesma tarefa e Gemini já em cooldown → Groq direto. Provider/modelo atuais são grounded explicitamente nos adapters para evitar inferência incorreta pelo histórico. `summary` continua `Fixed(gemini)`. [Detalhes](LR-7C-DISTRIBUTION.md).

**D0.5A — PASS completo em 29/09/2026; Codex fora dos Cognitive Providers:** a interface de IA e modelos
consulta o executável Codex e o estado de autenticação reportado por
`codex login status`. **D0.5B também está integrada em PASS**, com ponte efêmera Rust ↔ `codex app-server --stdio` para handshake diagnóstico sem inferência. Conversation/Summary permanecem inalterados; `AgentBackend` e registry genérico começam apenas em D0.5C. O contrato e a
fronteira de segurança estão em [LR-7D0.5](LR-7D05-CODEX-AGENT-BRIDGE.md).

**D0.5C — PASS técnico e integrada à `main` em 29/09/2026:** `AgentBackend`, seus tipos próprios e
`AgentRegistry` agora existem como fundação genérica, independentes de
`CognitiveProvider`/`ProviderRegistry`. Nenhum backend real, registro de
produção ou UI foi adicionado. **D0.5D fechou em PASS completo após auditoria e gate humano**, com
`CodexAgentBackend` real restrito a Planner read-only + `PlanV1`, integrada à `main` pela PR #7 em 29/09/2026. O próximo checkpoint é D0.5E — cancelamento, recovery e eventos reais.

**D0.5D — PASS completo e integrada à `main` pela PR #7:** o `CodexAgentBackend` de produção propõe `PlanV1` via thread efêmera do app-server, com outputSchema e validação determinística pelo Luna Core. A thread usa cwd temporário fora do projeto, sandbox read-only, `approvalPolicy=never`, nenhuma environment, ferramentas dinâmicas ou workspace roots, ShellTool desabilitado, web desabilitada e MCPs efetivos desabilitados após `config/read`. Somente `planning` e `structured_output` são capabilities do backend. O diagnóstico em IA e modelos exibe o plano validado, sem executar seus passos. Cancelamento remoto e progresso real pertencem à D0.5E; Conversation, Summary e Scheduler permanecem independentes.


**D0.5E — PASS completo e integrada à `main` pela PR #8 em 29/09/2026:** o contrato genérico de AgentBackend mantém cancelamento por AtomicBool e agora possui eventos semânticos de fatos reais. Codex usa interrupção remota única via `turn/interrupt`, espera bounded, recovery de recursos por chamada efêmera e inspeção fail-closed das notifications já observadas durante cancelamento. O gate manual real `manual_isolated_cancel_lifecycle` passou; após o teste, `git status --short` e `pgrep -af 'codex app-server --stdio'` ficaram vazios. Não há execução de PlanV1, estado global de sessão, retry de turno ou ligação ao CognitiveProvider/Scheduler/TaskRegistry. O próximo checkpoint é D0.5F.
