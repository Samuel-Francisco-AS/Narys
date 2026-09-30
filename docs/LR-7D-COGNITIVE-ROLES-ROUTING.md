# LR-7D — papéis cognitivos, roteamento configurável e distribuição inteligente

Estado: **LR-7D0 — PASS completo e integrada à `main` em 29/09/2026. LR-7D1 — candidata à auditoria/gate humano; D2/D3 planejadas.**
Execução prevista: **LR-7D0 pelo Codex; auditoria independente pela Luna; gate humano pelo usuário.**

### D0.5 — fronteira futura de agentes

A mini-trilha D0.5 começa após o fechamento da D0 e mantém Codex separado dos
Cognitive Providers. **D0.5A foi fechada em PASS completo em 29/09/2026** e detecta
somente o runtime e o estado seguro de autenticação. **D0.5B também foi fechada em PASS e integrada em 29/09/2026**, adicionando a ponte efêmera Rust ↔ `codex app-server --stdio`; **D0.5C também foi integrada em PASS em 29/09/2026**, estabelecendo `AgentBackend` e `AgentRegistry`; D0.5D fechou em PASS completo após auditoria e gate humano, adicionando o primeiro `CodexAgentBackend` real com Planner read-only + `PlanV1`, e foi integrada à `main` pela PR #7 em 29/09/2026. D0.5E também fechou em PASS completo e foi integrada à `main` pela PR #8 em 29/09/2026, adicionando cancelamento remoto, recovery de lifecycle e eventos factuais. D0.5F passou auditoria independente e gate humano real e foi integrada à `main` pela PR #9 em 29/09/2026, encerrando a mini-trilha **LR-7D0.5 em PASS completo**. O próximo checkpoint é **LR-7D1 — Orchestrator/Planner configurável**. Consulte [LR-7D0.5 — Codex Agent Bridge](LR-7D05-CODEX-AGENT-BRIDGE.md).

## Princípio central

A Luna não possui uma "LLM principal" fixa.

O **Luna Core (Rust)** mantém identidade, sessão, memória, permissões, budgets, TaskState e autoridade sobre execução. Cognitive Providers como Gemini e Groq são recursos substituíveis usados por papéis cognitivos configuráveis.

Uma futura LLM escolhida como **Orchestrator/Planner** pode propor decomposição, seleção de workers e consolidação, mas não recebe autoridade para ignorar permissões, budgets, capabilities ou decisões do Core.

## Estado herdado da LR-7C

Já existe:
- Gemini e Groq como Cognitive Providers reais;
- `Fixed` e `Preferred`;
- fallback antes do primeiro chunk;
- cooldown compartilhado e overflow;
- model/thinking por target;
- observabilidade `from → to → reason`;
- configuração persistida para Conversation/Summary;
- gate real Gemini saudável → Gemini;
- gate real Gemini 429 → Groq na mesma tarefa;
- gate real Gemini em cooldown → Groq direto.

Dívida intencional deixada pela LR-7C:
- policy ainda valida primário como Gemini;
- fallback de Conversation ainda valida Groq;
- Summary ainda é efetivamente Gemini;
- UI ainda contém textos/opções Gemini → Groq;
- runtime conhece timeouts por provider de forma parcialmente estática;
- não existe papel cognitivo Orchestrator;
- não existe fallback chain;
- `Auto`, score, affinity e task graph ainda não são produto.

---

## LR-7D0 — providers genéricos por papel + remoção dos hardcodes

### Implementação candidata

- A persistência valida apenas a estrutura da policy. O boundary de settings e o início da conversa validam o provider contra o catálogo, Registry, capabilities e estado da credencial. Provider desconhecido, indisponível ou sem credencial falha fechado.
- Conversation monta targets por `provider_id`, cada qual com model, thinking e timeouts próprios. `Fixed` aceita Gemini ou Groq; `Preferred` aceita as duas ordens com targets diferentes. O Scheduler continua responsável por cooldown, retry, fallback anterior ao primeiro chunk e cancelamento, sem regras por marca.
- Summary permanece `Fixed`, com Gemini ou Groq configurável. O worker consulta a policy escolhida para disponibilidade e resolve timeouts pelo provider antes da chamada; claim, recovery, validação de JSON e persistência continuam sob o Core.
- `get_ai_settings` oferece metadados seguros derivados dos providers registrados. A UI usa esses dados para as opções, capabilities, thinking e disponibilidade. Credenciais seguem separadas no Stronghold e só o indicador `configured` é exposto.
- A migration 007 adiciona `provider_timeout_settings`, copia o timeout Gemini existente e semeia Groq com os defaults já usados pelo adapter (45 s request, 15 s idle). A tabela Gemini antiga é preservada para upgrade não destrutivo.
- A mudança não ativa novos providers, Orchestrator, fallback chain, Auto, task graph nem o Rate Limit Manager da LR-8. A aprovação depende da auditoria independente e do gate humano.

### Auditoria independente da Luna — 29/09/2026

**PASS da auditoria independente. O gate humano foi concluído posteriormente com sucesso.**

A revisão remota confirmou:
- durante a auditoria, `main` permaneceu em `a427586c4c5027cdf6e4f92b230a6de44f6035f9` e a candidata ficou isolada em `lr-7d0-provider-neutral-routing`; após os gates, a PR #3 foi integrada por squash;
- a policy persistida valida estrutura sem assumir Gemini/Groq;
- catálogo/Registry/settings fazem a validação concreta de provider, capability, thinking e credencial;
- Conversation constrói targets/timeouts por `provider_id` e cobre as duas ordens de Preferred;
- Summary permanece Fixed, mas aceita Gemini ou Groq;
- a UI de roteamento deriva providers do backend e remove Gemini/Groq da semântica genérica;
- migration 007 copia os timeouts Gemini existentes, cria defaults Groq equivalentes aos defaults já usados pelo adapter e preserva policies no upgrade v6 → v7;
- testes cobrem reopen e isolamento de timeouts;
- permissions/capabilities do novo comando de timeout seguem restritas à janela de IA;
- LR-7D1/D2/D3 e LR-8 não foram antecipadas.

Os gates `typecheck/build/cargo check/cargo test 96/96/release/diff check` foram executados pelo Codex e reportados como PASS; a auditoria da Luna foi uma revisão independente do código remoto, não uma segunda execução local desses comandos.

Observações não bloqueantes:
1. o catálogo de integração atual associa cada provider a um `SecretKey`; isso atende Gemini/Groq, mas futuros providers OAuth/local poderão exigir abstração de autenticação mais ampla;
2. Gemini não anuncia `defaultModel` no catálogo. A configuração persistida atual é preservada e a troca Gemini ↔ Groq funciona, mas após reinício com Gemini totalmente fora dos targets ativos a UI pode exigir que o usuário informe novamente o model ao reativá-lo.

O gate humano descrito abaixo foi executado em 29/09/2026 e aprovado.

### Objetivo

Eliminar Gemini como provider estruturalmente privilegiado.

Conversation e Summary devem poder escolher pela interface qualquer **Cognitive Provider registrado e compatível**, atualmente Gemini ou Groq, sem alterar código.

### Requisitos

1. **Policy genérica**
   - remover validações `provider_id == "gemini"`;
   - remover validações `fallback_provider_id == "groq"`;
   - `Fixed` aceita um provider registrado/compatível;
   - `Preferred` aceita primary + fallback diferentes e compatíveis;
   - fallback não pode ser o mesmo target do primary;
   - nenhuma policy deve assumir nomes comerciais para decidir semântica.

2. **Conversation genérica**
   - deve funcionar em pelo menos:
     - Fixed(Gemini);
     - Fixed(Groq);
     - Preferred(Gemini → Groq);
     - Preferred(Groq → Gemini);
   - model/thinking/timeouts continuam específicos de cada target;
   - retry/fallback/cancelamento mantêm as regras da LR-7C.

3. **Summary genérico**
   - remover a amarra Gemini;
   - permitir Fixed(Gemini) ou Fixed(Groq);
   - Preferred para Summary só deve ser habilitado se a implementação puder preservar o contrato de output/claim/recovery sem regressão; caso contrário pode permanecer Fixed nesta subfase, mas o provider Fixed deve ser configurável;
   - mudança de provider não pode alterar mensagens da conversa nem violar o worker oportunista.

4. **Settings/Registry como fonte**
   - dropdowns devem ser derivados dos providers retornados pelo backend, não de opções hardcoded no React;
   - expor metadados úteis por provider, no mínimo: id, display name, configured, capabilities relevantes e thinking suportado;
   - provider não configurado deve aparecer como indisponível/explicado ou ser rejeitado de forma clara;
   - modelo continua configurável por target; não inventar model discovery se a API não fornecer isso;
   - a seção hoje chamada “Timeouts globais do Gemini” deve virar configuração **por provider**, no mínimo Gemini e Groq, com persistência/restart e sem compartilhar valores silenciosamente.

5. **Runtime config genérica**
   - evitar funções que recebam parâmetros chamados `gemini_timeouts`/`groq_timeouts` como contrato permanente;
   - resolver configuração de invocação pelo `provider_id`;
   - resolver timeouts/configuração técnica do provider por um mecanismo genérico e extensível;
   - não copiar model/thinking/timeout de um provider para outro;
   - provider desconhecido, desabilitado, sem credencial ou incompatível deve falhar fechado e com erro sanitizado.

6. **UI**
   - remover textos como “Fixed · somente Gemini” e “Gemini → Groq” como regra estrutural;
   - mostrar Primary e Fallback por nome real escolhido;
   - warnings de retry/budget devem usar nomes dinâmicos;
   - preservar credenciais separadas e nunca devolver secrets ao frontend.

7. **Migração**
   - preservar configurações existentes da LR-7C;
   - nenhum upgrade pode trocar provider do usuário silenciosamente;
   - database schema/version deve ser testado em upgrade e reopen.

### Fora de escopo da LR-7D0

- novo provider externo;
- papel Orchestrator em uso real;
- fallback chain com 3+ targets;
- `Auto` na UI;
- score;
- affinity;
- task graph;
- paralelismo;
- Rate Limit Manager LR-8;
- ferramentas/agentes especialistas;
- Luna Voice.

### Gate técnico

- typecheck;
- build;
- cargo check;
- cargo test;
- cargo check --release;
- diff check;
- migrations/reopen;
- testes específicos para primary/fallback invertidos;
- nenhum teste pode depender de chamada externa.

### Gate humano — PASS em 29/09/2026

Validação manual com o aplicativo Tauri e credenciais reais:

- a interface exibiu timeouts independentes para Gemini e Groq;
- Conversation permitiu alternar o provider primário entre Gemini e Groq;
- o fallback pôde ser invertido entre Groq → Gemini e Gemini → Groq apenas por configuração;
- as escolhas persistiram após restart;
- Conversation continuou funcional após a troca de provider;
- Summary foi configurado para Groq em modo Fixed e gerou título/resumo válido, persistido corretamente no histórico;
- o histórico permaneceu íntegro;
- Gemini apresentou HTTP 429/cooldown durante parte da validação; isso foi tratado como condição externa do provider e não como regressão da LR-7D0.

Com isso, o gate de arquitetura foi satisfeito: trocar Gemini ↔ Groq como provider de Conversation e escolher Groq para Summary exigiu somente configuração pela interface.

**LR-7D0 = PASS completo.**

### Gate humano

Com Gemini e Groq configurados:

1. Conversation Fixed(Gemini) → Gemini responde.
2. Conversation Fixed(Groq) → Groq responde.
3. Preferred(Gemini → Groq) → Gemini saudável continua preferido.
4. Preferred(Groq → Gemini) → Groq saudável continua preferido.
5. Summary Fixed(Gemini) continua funcionando.
6. Summary Fixed(Groq) gera título/resumo válido sem corromper sessão.
7. restart preserva todas as escolhas.
8. UI não contém conceito implícito de “Gemini principal”.

### Gate de arquitetura

Trocar Gemini ↔ Groq como primary/fallback em Conversation e como provider Fixed de Summary exige **somente configuração**, sem alteração de código.

---

## LR-7D1 — papel cognitivo Orchestrator/Planner

### Objetivo

Adicionar uma policy persistida `orchestrator` configurável pela interface.

O usuário escolhe provider, model, thinking, output/input budget, timeout/retry e routing compatível.

### Regra de autoridade

A LLM Orchestrator **não executa ferramentas diretamente por autoridade própria**. Ela produz plano/decisões estruturadas; o Luna Core valida:
- schema;
- capabilities;
- dependências;
- budgets;
- permissões;
- cancelamento;
- política do usuário.

Nenhum papel decorativo: `orchestrator` só entra na UI quando existir um caminho real de runtime que o utilize.

### Implementação candidata

A migration 008 amplia a policy persistida para incluir `orchestrator`, preservando
Conversation/Summary e semeando Gemini com defaults conservadores. O papel expõe
provider, model, thinking, output budget, input budget de planejamento, timeout por provider e
retry; permanece `Fixed` nesta fase. A UI deriva os providers do catálogo do
backend e nunca recebe segredos.

O comando `start_orchestrator_planning` registra uma tarefa e carrega/valida a policy no momento da
operação, limita o objetivo, envia apenas identidade técnica mínima (sem
histórico ou memória), chama o Scheduler e retorna o provider efetivamente usado
junto do `PlanV1` validado. O plano é apenas exibido: nenhuma ferramenta ou passo
é executado.

Gemini e Groq continuam anunciando somente `text_stream()`. A saída usa
instrução textual estrita e aceita exclusivamente JSON cru; fences, texto extra,
reparo heurístico e reasoning oculto não são aceitos. `PlanV1::parse`/`validate`
continua sendo a autoridade única para limites, IDs, dependências, ciclos,
capabilities, riscos e `needsUserInput`; erros falham fechados e sanitizados.

O gate humano deve selecionar Gemini, salvar e executar o objetivo controlado,
verificar provider e plano, reiniciar para confirmar persistência, trocar apenas
o Orchestrator para Groq e repetir. Fallback chain, Auto, score, affinity e
task graph permanecem explicitamente fora desta entrega.

### Gate

Trocar Orchestrator entre Gemini e Groq pela UI muda a próxima operação de planejamento real sem alterar identidade, permissões ou estado da Luna.

---

## LR-7D2 — fallback chain + Auto/score + affinity

### Objetivo

Evoluir de `primary + fallback` para rota ordenada e seleção automática controlada.

Exemplo:

```text
Conversation
mode = Preferred
1. Groq / model A
2. Gemini / model B
3. futuro provider / model C
```

### Modos

- `Fixed`: exatamente um target;
- `Preferred`: cadeia ordenada definida pelo usuário;
- `Auto`: Scheduler escolhe apenas entre targets explicitamente permitidos pelo usuário.

### Score inicial

Sem antecipar LR-8 completo, considerar apenas sinais realmente disponíveis:
- capability/suitability;
- enabled/configured;
- cooldown/health;
- prioridade/policy do usuário;
- estimated context-transfer cost;
- affinity da tarefa/sessão.

RPM/TPM/RPD/TPD, token bucket, queue e circuit breaker completo ficam na LR-8.

### Affinity

Evitar troca de provider quando o custo de transferir contexto/continuidade superar o ganho esperado, salvo falha/cooldown/policy explícita.

### Gate

Uma mesma configuração permite trocar a ordem dos providers e o Scheduler respeita a preferência/affinity sem hardcodes comerciais.

---

## LR-7D3 — task graph mínimo + subtarefas independentes

### Objetivo

Permitir que uma tarefa complexa seja decomposta em unidades independentes, sem construir ainda o sistema multiagente completo.

### Regras

- somente paralelizar subtarefas declaradamente independentes;
- uma única TaskId raiz;
- subtarefas possuem IDs/estado/eventos reais;
- cancelamento raiz propaga;
- eventos exibidos ao usuário correspondem a trabalho real;
- nenhum diálogo fictício entre agentes;
- resultados estruturados retornam ao Core para consolidação;
- budgets globais e por subtarefa são respeitados.

### Gate final LR-7

Uma tarefa real:
1. cria pelo menos duas subtarefas independentes;
2. usa Gemini e Groq em trabalho útil;
3. registra exatamente qual provider fez cada unidade;
4. consolida um único resultado;
5. preserva identidade, sessão, estado e cancelamento.

Após esse gate, **LR-7 pode ser declarada encerrada** e LR-8 assume o Rate Limit Manager completo.
