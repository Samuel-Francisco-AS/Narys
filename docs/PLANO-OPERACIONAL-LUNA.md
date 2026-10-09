# Plano operacional — construção estrutural da Luna

> Plano de execução derivado da arquitetura definida em 25/09/2026.
>
> Este é o documento principal para iniciar a refatoração pós-M0. Os trabalhos de avatar/animação e de agente devem avançar em paralelo e se encontrar por contratos, não por dependência temporal.

## 1. Ponto de partida

Implementado hoje:

- Tauri 2 abre a aplicação desktop;
- React/TypeScript compõe a UI;
- Three.js renderiza Luna.glb;
- AvatarViewport/LegacyGlbAdapter carregam o avatar após LR-1;
- Idle e Wave existem embutidos no GLB atual;
- clique/botão acionam Wave;
- retorno ao Idle funciona;
- workaround Mesa software mantém Tauri/WebKitGTK utilizável no hardware atual.

Ainda não implementado:

- conversa funcional;
- Luna Core além do núcleo mínimo de tarefas/eventos da LR-2;
- recuperação semântica/Context Builder;
- providers de IA;
- orquestração;
- rate limiting;
- ferramentas;
- feedback de tarefas reais além do mock da LR-2;
- VRM/VRMA runtime;
- Animation Director completo (prioridades, packs e novos estados);
- proteção final da chave de desbloqueio para segredos reais e políticas de ferramentas futuras.

## 2. Estratégia de execução

Trabalhar em **lanes paralelas**.

~~~text
LANE A — AVATAR / ANIMAÇÃO
Blender → animações → VRM/VRMA → Animation Director
                         │
                         │ AnimationIntent
                         ▼

LANE B — LUNA CORE
Rust → task state → events → memory → tools
                         │
                         │ TaskEvent
                         ▼

LANE C — COGNIÇÃO
Mock → Gemini → segundo provider → scheduler → specialists
                         │
                         ▼

LANE D — SEGURANÇA / OBSERVABILIDADE
CSP → capabilities → secrets → audit → métricas
~~~

Nenhuma lane precisa esperar a “personagem final”.

## 3. Regras de trabalho

- checkpoints pequenos e testáveis;
- nenhuma fase deve misturar refatoração estrutural grande com mudança artística grande;
- manter Legacy GLB enquanto VRM/VRMA não passar pelo gate real;
- evitar adicionar dependências sem necessidade concreta;
- cada milestone deve terminar com build/typecheck e, quando aplicável, teste Tauri;
- registrar decisões que mudarem arquitetura;
- nunca declarar feature pronta só porque existe interface/mock.

## 4. LR-0 — documentação e baseline

**Estado:** iniciado/concluído pela rodada de documentação de 25/09/2026.

Entregáveis:

- ARCHITECTURE-LUNA.md;
- AI-PROVIDERS-ORCHESTRATION.md;
- AVATAR-ANIMATION-RUNTIME.md;
- este plano;
- README apontando para a nova direção.

Gate:

- documentos distinguem implementado × planejado;
- M0 histórico permanece preservado.

## 5. LR-1 — separar o runtime visual sem mudar o comportamento

**Estado:** implementado em 25/09/2026. `AvatarViewport` conecta React ao runtime; `SceneRuntime` cuida de Three.js/WebGL; `AvatarManager` cuida do avatar em cena e do carregamento tardio; `LegacyGlbAdapter` mantém `/models/Luna.glb` e os clipes embutidos; `AnimationRegistry` expõe apenas `idle`/`greeting`; `AnimationDirector` executa as transições atuais. O botão e o raycast solicitam `greeting`. VRM/VRMA, Avatar Packs e o diretor completo continuam fora desta etapa.

**Objetivo:** transformar CharacterScene de protótipo monolítico em componentes internos, mantendo exatamente a Luna atual.

Não introduzir VRM ainda.

### Trabalho

Criar fronteiras equivalentes a:

~~~text
src/avatar/
├─ AvatarViewport.tsx
├─ runtime/
│  ├─ SceneRuntime.ts
│  ├─ AvatarManager.ts
│  ├─ AnimationRegistry.ts
│  ├─ AnimationDirector.ts
│  └─ types.ts
└─ adapters/
   └─ LegacyGlbAdapter.ts
~~~

A forma exata pode variar durante implementação.

### Resultado esperado

- GLB atual continua carregando;
- Idle atual continua tocando;
- Wave continua disponível para debug;
- App.tsx deixa de conhecer detalhes do mixer/clipe;
- CharacterScene deixa de concentrar todas as responsabilidades;
- existe contrato AnimationIntent mesmo que só idle/greeting sejam suportados.

### Gate

- typecheck;
- build;
- Tauri abre;
- Idle visível;
- debug greeting funciona;
- retorno ao Idle;
- sem regressão evidente de WebGL/dispose.

## 6. LR-2 — Luna Core mínimo + stream de eventos

**Estado:** implementado em 26/09/2026. O Rust registra tarefas mock com `TaskId` monotônico, mantém `TaskState` em memória, executa duas etapas com esperas assíncronas, envia `TaskEvent` estruturado por Tauri Channel e aceita cancelamento cooperativo. O registro ativo é removido ao terminar, cancelar ou falhar. O painel React mostra TaskId, estado e eventos recebidos; no navegador sem Tauri ele informa a indisponibilidade do núcleo. A ponte de eventos para `AnimationIntent` mantém `idle` durante trabalho e usa `greeting` provisoriamente na conclusão. Nenhuma LLM, memória ou ferramenta foi introduzida.

**Gate:** `cargo check`, `cargo test`, `npm run typecheck`, `npm run build` e `git diff --check` passaram. Na janela Tauri, o stream chegou em ordem, a conclusão acionou o gesto, o cancelamento limpou a tarefa ativa e uma nova tarefa iniciou depois. Idle e botão manual de aceno permaneceram operantes.

**Lane principal:** B.

**Pode ocorrer em paralelo com:** continuação das animações no Blender.

### Trabalho

No Rust:

- criar módulos base de Luna Core;
- definir TaskId;
- definir TaskState mínimo;
- definir TaskEvent;
- criar comando Tauri simples para iniciar uma tarefa mock;
- usar Channel para enviar progresso ordenado;
- implementar cancelamento básico.

No React:

- painel/timeline mínimo de eventos;
- transformar eventos mock em feedback visível.

No Avatar Runtime:

- mapear alguns TaskEvents para AnimationIntent.

Exemplo:

~~~text
TaskStarted → attentive/working
Waiting → thinking
TaskCompleted → success/idle
TaskFailed → concern/idle
~~~

### Gate

Uma tarefa fake de alguns passos deve:

1. iniciar pelo frontend;
2. executar no Rust;
3. emitir eventos reais;
4. atualizar UI;
5. alterar intent do avatar;
6. terminar/cancelar corretamente.

Nenhuma LLM necessária.

## 7. LR-3 — segurança mínima antes de segredos

**Estado:** implementada em 26/09/2026 para o protótipo desktop. CSP restritiva de produção e `devCsp` para Vite, capability `main-window` com cinco comandos declarados no `AppManifest`, validação de `TaskId`, audit estruturado e `SecretStore` Rust com Stronghold foram adicionados. O segredo artificial foi gravado, recuperado após reinício e removido sem passar valor ao React. A chave local do POC ainda precisa de proteção adicional antes de API keys reais; consulte [SECURITY.md](SECURITY.md).

**Gate:** `cargo check`, `cargo test`, `npm run typecheck`, `npm run build` e `git diff --check` passaram. `cargo fmt --check` ficou indisponível porque `rustfmt` não está instalado nesta toolchain. Na janela Tauri, WebGL/Idle/Acenar, tarefa mock/Channel/cancelamento, status de storage e reinício foram validados. No Firefox comum, WebGL/Idle/Acenar e retorno ao repouso funcionaram; o painel desabilitou a tarefa Rust e informou indisponibilidade do SecretStore, sem erro no console inspecionado. Nenhuma credencial real do aplicativo foi usada.

**Auditoria:** PASS em 26/09/2026 para a fundação de segurança desktop. Ficaram registrados como itens futuros não bloqueantes: remover completamente os comandos de diagnóstico da superfície de release e revalidar permissões do material de desbloqueio existente. O gate duro para qualquer credencial real permanece obrigatório antes da LR-6.

**Lane principal:** D.

Deve acontecer antes de cadastrar API keys reais.

### Trabalho

- substituir CSP nulo;
- definir capabilities Tauri explícitas;
- restringir comandos expostos;
- criar boundary types validados;
- introduzir storage de segredos, preferencialmente Stronghold após POC;
- separar configuração não secreta de segredo;
- criar audit log inicial para ações privilegiadas.

### Gate

- nenhuma API key no frontend/localStorage;
- CSP funcional sem quebrar Three.js/assets locais;
- capabilities revisadas;
- secrets acessíveis apenas pelo core.

## 8. LR-4 — persistência, identidade e memória v0

**Estado:** implementada e validada em 26/09/2026. SQLite no Luna Core, importador privado, painel diagnóstico, migration e testes sintéticos foram adicionados. No Tauri dev, a importação produziu uma identidade atual e quatro memórias; a conversa artificial e duas tarefas mock permaneceram após fechar e reabrir. Em um segundo reinício com o JSON privado temporariamente renomeado, os mesmos registros continuaram disponíveis pelo SQLite. O arquivo foi restaurado ao final. Uma tarefa mock cancelada também ficou no histórico como `cancelled`. Consulte [MEMORY-IDENTITY.md](MEMORY-IDENTITY.md).

**Auditoria:** PASS em 26/09/2026. A revisão confirmou separação Rust↔React, migrations versionadas, importação privada transacional/idempotente, versionamento de identidade sem sobrescrita, recuperação determinística de memórias e persistência de conversa/tarefas. Nenhum marcador exclusivo do bootstrap privado foi encontrado no remoto. Pendências não bloqueantes: validar futuramente o `rust-version = 1.77.2` com a toolchain exata e substituir o refresh temporizado do painel de histórico por sinalização explícita após a persistência quando o fluxo deixar de ser diagnóstico.

**Lane principal:** B/D.

### Persistência

Adicionar SQLite local.

Não implementar embeddings ainda.

### Estruturas mínimas

**IdentityProfile**
- version;
- name;
- communication/behavior profile;
- boundaries/principles.

**ConversationSession**
- id;
- timestamps;
- recent messages.

**MemoryRecord**
- type: semantic / episodic / relational / project;
- content estruturado;
- provenance;
- created/updated;
- importance;
- optional expiry.

**TaskRecord**
- task state resumido;
- resultados;
- erros;
- duração;
- uso de providers.

### Gate

Fechar/reabrir app e preservar:

- identidade;
- uma conversa simples;
- registros de memória de teste;
- histórico resumido de tarefa.

## 9. LR-5 — Context Builder + MockProvider

**Estado:** implementada em 26/09/2026. Context Builder usa SQLite LR-4 com limites explícitos; Provider trait object safe (boxed future), Registry mock, Scheduler determinístico, budgets, retry, cooldown, fallback e cancelamento usam Rust. O painel debug recebe chunks/resultados pelo Channel LR-2. Testes usam dados sintéticos. Consulte [COGNITION-RUNTIME.md](COGNITION-RUNTIME.md). Nenhuma API externa ou credencial real foi usada.

**Auditoria:** PASS em 26/09/2026. A revisão confirmou que o Context Builder permanece estruturado e desacoplado de adapters reais, o Scheduler respeita prioridade/capability/cooldown/budget, o mesmo cancelamento do TaskRegistry atravessa provider/retry/fallback e o resultado não expõe identidade ou memória. Nenhum marcador exclusivo do bootstrap privado foi encontrado no remoto.

**Antes da LR-6:** além do gate já existente da chave de desbloqueio Stronghold, endurecer dois pontos da LR-5: (1) propagar falha do Tauri Channel como cancelamento/falha da tarefa cognitiva em vez de descartar o erro de envio, para evitar trabalho/quota continuando sem consumidor; (2) tornar os cenários mock transitórios repetíveis por tarefa — hoje o comportamento `Timeout`/transient usa contador no provider persistente e deixa de falhar na primeira tentativa após a primeira execução do cenário.

**Lane principal:** C.

Antes de gastar cota real, testar contrato completo com MockProvider.

### Trabalho

- Provider trait/interface;
- ProviderCapabilities;
- ProviderRequest;
- ProviderResponse;
- ProviderUsage;
- ProviderError;
- Context Builder;
- Provider Registry;
- configuração enable/disable/priority;
- Scheduler inicial;
- budgets por tarefa.

### MockProvider

Deve simular:

- resposta normal;
- streaming;
- latência;
- 429;
- timeout;
- quota baixa;
- erro fatal.

### Gate

Testes automatizados demonstram:

- retry/cooldown;
- fallback;
- cancelamento;
- orçamento;
- provider desabilitado nunca recebe chamada.

## 10. LR-6 — primeiro provider real: Gemini

**Gate de segurança:** liberado no desktop Fedora após o PRE-LR-6 hardening de 26/09/2026. A chave de desbloqueio Stronghold foi migrada do arquivo local para o credential store do sistema operacional, a migração do vault existente foi validada e o arquivo legado foi removido. Permanecem como limitações conhecidas a ausência de teste com a toolchain Rust 1.77.2 exata, logout/reboot do SO e validação em Windows/macOS; consulte [SECURITY.md](SECURITY.md). Nenhuma API key real foi cadastrada durante o hardening.

**Lane principal:** C.

### Objetivo

Provar o caminho completo:

~~~text
Sam → Luna Core → Context Builder → Scheduler
→ Gemini → TaskResult → Luna Voice provisória → UI
~~~

Nesta fase a Luna Voice pode usar o mesmo provider.

### Requisitos

- API key em storage seguro;
- streaming;
- captura de uso;
- rate limit state;
- Retry-After/429;
- timeout;
- cancelamento;
- quota configurável.

### Gate

- conversa simples funcional;
- estado/memória não mora no Gemini;
- reiniciar app não troca identidade;
- 429 não quebra sessão;
- uso aparece em painel de diagnóstico.

**Atualização LR-6 (26/09/2026):** adapter Interactions API, Stronghold, política outbound mínima, chat SQLite e testes HTTP locais implementados. O gate real permanece pendente de configuração manual da chave e validação Tauri com chamada, cancelamento e reinício; portanto LR-6 ainda não é PASS completo. [Detalhes](GEMINI-PROVIDER.md). LR-7 não foi iniciada.

**Auditoria de implementação (26/09/2026):** arquitetura, privacidade outbound, SecretStore, SSE incremental, cancelamento e persistência local foram aprovados, mas a validação com chave real permanece bloqueada por dois ajustes de protocolo: (1) `interaction.completed` precisa validar `interaction.status` e não tratar `incomplete`, `failed`, `cancelled` ou `requires_action` como sucesso; (2) erros da Interactions API devem usar o campo oficial `error.code` tanto em respostas HTTP quanto em eventos SSE, distinguindo pelo menos `quota_exceeded`, `rate_limit_exceeded`/`too_many_requests`, `authentication`/`permission_denied`, timeout/cancelamento e falhas transitórias. Nenhuma API key real deve ser inserida antes dessa correção.

**LR-6 protocol FIX (26/09/2026):** os dois ajustes da auditoria estão implementados e cobertos por testes locais. Somente `status=completed` com usage válido alcança a resposta final; `error.code` tem prioridade em HTTP e SSE. Estado: **LR-6 implementation complete; protocol audit PASS; real API validation pending**. A validação real está liberada como próxima ação manual, sem ter sido executada nesta rodada. LR-6 ainda não é PASS completo e LR-7 não foi iniciada.

**Auditoria da FIX (26/09/2026): PASS.** A revisão confirmou status terminal fail-closed, mapper compartilhado de `error.code` para HTTP/SSE, distinção entre rate limit e quota, ausência de retry para erros terminais e preservação da regra de não retry/fallback após primeiro chunk. A validação com API key real está liberada; LR-6 só poderá ser fechada após chamada real, streaming/usage, cancelamento e reinício.

**Validação real (26/09/2026): PASS.** A chave Gemini foi cadastrada pelo painel Tauri e permaneceu configurada após fechar/reabrir a aplicação. Uma chamada real concluiu com streaming e usage observado (48 tokens de entrada, 78 de saída, 126 total, thinking 0) e a conversa local persistiu. Em testes adicionais, uma resposta terminou como `provider_incomplete` e permaneceu apenas como prévia não persistida; outra tentativa retornou `unavailable` sem corromper estado; por fim, uma chamada com chunks reais foi cancelada manualmente antes da fase de persistência e terminou como `cancelled`, sem gravar resposta final. Com restart, credencial e conversa anterior foram recuperadas corretamente. **LR-6 = PASS completo.** LR-7 continua não iniciada; a próxima rodada será dedicada a UI/performance antes de avançar providers.

## Interlúdio UIP — interface de presença + performance

**Estado:** **FECHADO em 28/09/2026. UIP-0 → UIP-7 = PASS funcional.** Na UIP-3, always-on-top no Wayland nativo e click-through seguro ficaram adiados; na UIP-4, o resize nativo no GNOME/Wayland ficou como dívida de estabilidade espacial após a rejeição do POC de múltiplas WebViews. Essas limitações não bloqueiam isoladamente o gate. A sequência **UIP-0 → UIP-7** transforma a casca M0-B em uma interface de presença desktop e estabelece orçamento real de renderização antes da LR-7. [Resultado técnico UIP-7](UIP-7-FINAL-PERFORMANCE.md).

Princípios fechados:

- Luna/personagem é o centro; UI é acessória e recolhível;
- janela principal transparente, sem borda e pequena em relação ao monitor;
- modos `always-on-top`, normal e click-through configuráveis;
- reposicionamento por região/comando específico, preservando interação com a personagem;
- compositor e conversa podem desaparecer sem encerrar a sessão;
- cada execução inicia sessão nova; `Nova conversa` cria outra sessão;
- histórico é dividido em sessões e não vira prompt infinito;
- resumo de sessão pode usar LLM assíncrona por papel cognitivo configurável;
- resumo de sessão não equivale a memória persistente;
- UI, Conversation Runtime, Luna Core e Avatar Runtime permanecem independentes;
- renderer usa **30 FPS como teto padrão** e perfis reduzidos quando apropriado;
- gates de performance são curtos/reproduzíveis; endurance virá do uso cotidiano;
- abrir o painel de conversa não aumenta a dimensão de render do CharacterStage;
- futuras reações ao ambiente entram por eventos/BehaviorIntent sem acoplar desktop context ao renderer.

Plano detalhado: [UI-PERFORMANCE-PLAN.md](UI-PERFORMANCE-PLAN.md).

**Sequência obrigatória antes da LR-7:** UIP-0 contratos/baseline → UIP-1 Presence Shell → UIP-2 Render Budget → UIP-3 ergonomia da janela → UIP-4 compositor/conversa → UIP-5 histórico/resumo assíncrono → UIP-6 janelas de configuração → UIP-7 consolidação/performance.

## 11. LR-7 — segundo provider real + distribuição

**Estado final (03/10/2026): LR-7 ENCERRADA — PASS completo.** A trilha evoluiu da fundação multi-provider da LR-7A até o task graph distribuído da LR-7D3. O gate final real criou duas subtarefas independentes, executou trabalho útil em Groq + Cloudflare em paralelo, consolidou um único resultado e persistiu provenance por unidade no SQLite. O Rate Limit Manager completo permanece deliberadamente na LR-8.

**LR-7A-FIX (28/09/2026):** a auditoria apontou que o routing era provider-aware, mas parâmetros de invocação ainda eram únicos por request. A FIX separa a requisição da tarefa dos targets, cada um com model, thinking e timeouts próprios. O Scheduler resolve e valida a configuração do provider selecionado antes de invocá-lo; configuração ausente ou inválida falha fechada. `ProviderRuntime` não possui mais timeouts Gemini. Groq permanece LR-7B; nenhum gate real multi-provider foi executado.

**LR-7B — segundo provider real (28/09/2026): PASS completo.** Groq foi registrado no Registry real como provider `groq`, prioridade 2, com `openai/gpt-oss-20b`, credencial própria no Stronghold, streaming SSE, usage real, reasoning oculto, mapping de 429/Retry-After e diagnóstico isolado `Fixed(groq)`. O gate técnico passou com typecheck/build, cargo check, **89/89 testes**, cargo check release e diff check. No gate humano, a chamada real Groq respondeu por streaming com 126 tokens de entrada, 22 de saída e 148 total; a credencial permaneceu configurada após restart. Conversation e Summary continuaram `Fixed(gemini)`. Um HTTP 429 real do Gemini confirmou que o Scheduler respeita `Fixed` e não desvia silenciosamente para Groq. A distribuição/fallback real Gemini ↔ Groq fica para **LR-7C**.

**LR-7C — distribuição real (28/09/2026): PASS completo.** A Conversa persiste policy `Fixed` ou `Preferred`; em `Preferred`, Gemini é o primário e Groq o fallback explícito com model/thinking próprios. A migration 006 preserva `Fixed` por padrão. O preflight consulta a rota real e não bloqueia envio quando Gemini está em cooldown mas Groq está elegível. Foram observados em provider real: Gemini saudável → Gemini; Gemini HTTP 429 antes do primeiro chunk → Groq na mesma tarefa; Gemini já em cooldown → Groq direto. A UI registra origem/destino/motivo reais, e o grounding de provider/modelo foi corrigido e revalidado. Summary permanece `Fixed(gemini)`; Auto, affinity, task graph, paralelismo e Rate Limit Manager completo continuam fora desta rodada. Veja [LR-7C-DISTRIBUTION.md](LR-7C-DISTRIBUTION.md).

**LR-7D — papéis cognitivos, roteamento configurável e distribuição inteligente.** **LR-7D0 fechada em PASS completo em 29/09/2026**: Gemini deixou de ser estruturalmente privilegiado; Conversation aceita Gemini/Groq como primary em Fixed/Preferred, Summary Fixed aceita ambos, timeouts são independentes por provider e as escolhas persistem após restart. Antes da LR-7D1, a mini-trilha **LR-7D0.5 — Codex Agent Bridge** prepara Codex como backend agentivo separado de CognitiveProvider. **D0.5A fechou em PASS completo e foi integrada à `main` em 29/09/2026**, cobrindo descoberta segura do runtime/autenticação. **D0.5B também fechou em PASS completo e foi integrada à `main` em 29/09/2026**, com ponte efêmera Rust ↔ `codex app-server --stdio`, handshake real e cleanup repetido sem processo órfão. **D0.5C também fechou em PASS técnico e foi integrada à `main` em 29/09/2026**, estabelecendo `AgentBackend`, tipos agentivos próprios e `AgentRegistry` independente dos Cognitive Providers. **D0.5D também fechou em PASS completo após auditoria e gate humano**, com `CodexAgentBackend` real, Planner read-only e `PlanV1`, e foi integrada à `main` pela PR #7 em 29/09/2026. **D0.5E fechou em PASS completo e foi integrada à `main` pela PR #8 em 29/09/2026**, adicionando cancelamento remoto, recovery de lifecycle e eventos factuais. **D0.5F passou auditoria independente e gate humano real e foi integrada à `main` pela PR #9 em 29/09/2026**, encerrando **LR-7D0.5 em PASS completo**. A LR-7D1 está fechada; a LR-7D2 fechou em PASS completo em 01/10/2026. **LR-7D2.5 também foi encerrada em 01/10/2026**: Cloudflare Workers AI passou gate real em Conversation e Orchestrator/PlanV1; Summary automático tornou-se opcional; Mistral ficou tecnicamente integrada, mas operacionalmente bloqueada pela exigência de upgrade da conta Free. A dívida formal é substituir Mistral por OpenAI API paga quando houver orçamento. **LR-7D3 fechou em PASS completo em 03/10/2026**: Task 166, Planner Groq, Workers Groq + Cloudflare, execução paralela, consolidação determinística e provenance persistida. O gate foi validado com Worker output global 8192; 4096 mostrou-se insuficiente para o GLM-4.7-Flash nesse cenário com retry/accounting conservador. **LR-7 está encerrada; a sequência segue para LR-8.** Plano detalhado: [LR-7D-COGNITIVE-ROLES-ROUTING.md](LR-7D-COGNITIVE-ROLES-ROUTING.md).

**Segundo provider escolhido:** Groq (LR-7B).

Essa fase prova que a arquitetura é de verdade multi-provider.

### Trabalho

- implementar segundo adapter;
- Scheduler com score;
- affinity;
- overflow;
- distribuição sequencial;
- paralelismo apenas para subtarefas independentes;
- health/cooldown.

### Cenários de teste

1. Gemini disponível, tarefa simples → uma chamada.
2. Gemini próximo de RPM → tarefa compatível vai ao segundo provider.
3. Gemini 429 → cooldown + fallback.
4. tarefa complexa com duas subtarefas independentes → providers diferentes.
5. contexto grande → affinity evita troca desnecessária.

### Gate

Uma única tarefa pode ser completada usando dois providers diferentes sem perder identidade/estado.

### LR-7D2.5 — provider redundancy + Gemini de-risking

**Estado: ENCERRADA em 01/10/2026 com escopo revisado e dívida operacional registrada.**

Antes do task graph da D3, ampliar a redundância real da camada cognitiva:

- Mistral API direta e Cloudflare Workers AI foram integradas ao contrato genérico;
- Cloudflare passou gate real em Conversation e Orchestrator/PlanV1;
- Mistral permaneceu bloqueada operacionalmente pela exigência de upgrade da conta Free;
- a substituição operacional futura de Mistral por **OpenAI API paga** ficou registrada como dívida;
- se um candidato deixar de atender, substituí-lo por outro provider direto e independente;
- extrair transporte OpenAI-compatible apenas onde houver compatibilidade real;
- manter capabilities, erros, streaming, rate-limit e credenciais específicos por provider;
- validar Fixed/Preferred/Auto com 3+ targets reais;
- garantir que Gemini desabilitado/em cooldown/indisponível não impeça Conversation
  nem os papéis compatíveis quando existirem alternativas autorizadas;
- não antecipar sinais de LR-8, task graph, paralelismo ou ferramentas.

OpenRouter pode ser adicionado futuramente como agregador/fallback, mas não conta
como uma das duas novas rotas principais desta fase.

O gate da D3 deixa de exigir Gemini + Groq nominalmente: deve usar **pelo menos
dois Cognitive Providers independentes, autorizados e elegíveis**. Gemini permanece
suportado, mas não obrigatório.

Plano: [LR-7D2.5-PROVIDER-REDUNDANCY.md](LR-7D2.5-PROVIDER-REDUNDANCY.md).

## 12. LR-8 — Rate Limit Manager completo

**Estado: PASS completo em 05/10/2026.**

Para reduzir correções tardias, a LR-8 foi formalmente decomposta em subfases
sequenciais, cada uma implementada pelo agente e auditada independentemente pela
Luna antes da liberação da seguinte:

1. **LR-8A — modelo de quota + telemetria factual — PASS e integrada à `main` pela PR #14 em 03/10/2026**;
2. **LR-8B — admission control + fila + concurrency — PASS técnico + auditoria + gate humano; integrada à `main` pela PR #15 em 03/10/2026**;
3. **LR-8C — rate accounting + token buckets + budgets — PASS técnico + auditoria; integrada à `main` pela PR #16 em 04/10/2026**;
4. **LR-8D — backoff, jitter, cooldown + circuit breaker — PASS técnico + auditoria; integrada à `main` pela PR #18 em 04/10/2026**;
5. **LR-8E — painel operacional + integração/gate final — PASS técnico + auditoria independente + gate final; integrada à `main` pela PR #21 em 05/10/2026**.

A trilha mantém como requisitos finais RPM, TPM, RPD/TPD quando factuais,
concurrency, fila, token bucket, parsing de headers, Retry-After, backoff +
jitter, cooldown, circuit breaker, budgets e telemetria. Quota/custo/saúde
desconhecidos permanecem explicitamente desconhecidos; nenhuma regra comercial
mutável é hardcoded no Luna Core.

**LR-8.5A, LR-8.5B e LR-8.5C estão encerradas em PASS técnico em 06/10/2026; LR-8.5 está concluída. Próxima ação: PERF-1 — Adaptive Presence & Economy Mode.** A LR-8E fechou com painel operacional, auditoria independente, gate humano A–D/M e bateria automatizada determinística E–L. O contrato detalhado, limitações residuais e evidências estão em [LR-8-RATE-LIMIT-MANAGER.md](LR-8-RATE-LIMIT-MANAGER.md), [LR-8E-FINAL-GATE.md](LR-8E-FINAL-GATE.md) e [LR-8E-AUTOMATED-GATE-E-L.md](LR-8E-AUTOMATED-GATE-E-L.md).

## 12.5. LR-8.5 — Cognitive Resource Economy & Allocation

**Estado: PASS TÉCNICO — LR-8.5A/B/C concluídas e integradas à `main` em 06/10/2026. PERF-1 concluiu em 07/10; LR-9 está em andamento: A–D PASS/integradas; LR-9E IMPLEMENTAÇÃO CANDIDATA, aguardando auditoria independente da Luna.**

A LR-8 fecha capacidade operacional de cada provider; a LR-8.5 passa a decidir
qual recurso cognitivo vale consumir entre opções heterogêneas.

O runtime deve distinguir:

- Cognitive Providers, Specialist Agents e Local Cognitive Support;
- família do provider de seu caminho de acesso;
- free tier, franquia incluída, créditos estudantis, saldo pré-pago, cobrança
  medida e estado econômico desconhecido;
- rate limit transitório de allowance realmente esgotada;
- quota disponível de capacidade funcionalmente adequada;
- custo monetário de custo de oportunidade de uma quota escassa.

Princípio: **UNKNOWN != FREE, UNKNOWN != PAID e UNKNOWN != ZERO COST.**
Nenhum 429 isolado autoriza escalada para recurso pago, e nenhum adapter decide
sozinho se vale gastar dinheiro.

Decomposição:

1. **LR-8.5A — Resource Domains & Access Paths**;
2. **LR-8.5B — Allocation & Scarcity Policy**;
3. **LR-8.5C — Safe Cross-Resource Handoff**.

A fase deve provar com mocks/fixtures preservação de quota escassa, seleção de
alternativas compatíveis, autorização explícita de gasto e handoff em checkpoint
sem duplicar efeitos. A prova real Codex ↔ Copilot permanece para LR-10/LR-11;
providers pagos reais ficam naturalmente para LR-12.

Plano detalhado: [LR-8.5-COGNITIVE-RESOURCE-ECONOMY.md](LR-8.5-COGNITIVE-RESOURCE-ECONOMY.md).

## 13. LR-9 — Operational Terminal & Cognitive Trace Runtime

**Estado: PASS FINAL — LR-9A/B/C/D/E encerradas, auditadas e integradas à `main`; trilha concluída em 09/10/2026. Próxima fase formal: LR-10 — GitHub Copilot SpecialistAgent.**

LR-9A estabeleceu o Observation Plane com contratos tipados,
OperationalTraceBus process-wide, retenção priority-aware/bounded, live
best-effort, replay com gaps, batching, métricas e coalescing seguro.

LR-9B estabeleceu o Execution Plane com Execution Broker process-wide,
authority separada de provenance, Structured Exec, PTY humana real,
drenagem bounded independente da UI, cancellation/reap e shutdown Headless-safe.

LR-9C materializou a Human Operational Surface: Home virou Terminal lazy,
registry/reattach humano, Channels raw para PTY, Activity bounded/virtualizada,
input/resize coalescidos e continuidade Close/Headless/Reopen.

LR-9D conectou Task lifecycle, Scheduler/providers, TaskGraph/workers, Summary e
Codex planner ao Observation Plane por adapters passivos, com deduplicação,
minimização de conteúdo, raw reasoning fail-closed e zero nova inference/prompt.
O overhead síncrono observado sob burst foi elevado a dívida obrigatória da LR-9E
para profiling e eventual otimização antes do fechamento final da LR-9.

LR-9E consolidou A–D, fechou a dívida de performance do OperationalTraceBus com
profiling + contadores incrementais, suspendeu a assinatura visual de Activity
quando recolhida, endureceu a superfície DEV/release e concluiu gates
concorrentes/nativos debug/release. A auditoria independente aprovou a subfase
sem FIX bloqueante.

**LR-9 — Operational Terminal & Cognitive Trace Runtime está encerrada em PASS.**

Agents continuam sem execution authority e não existe generic shell IPC.
Copilot/Codex executores, approvals/grants e sandbox permanecem para LR-10/11.

[Gate LR-9E e evidências](LR-9E-CONCURRENCY-SECURITY-FINAL-GATE.md).

Fechamentos e evidências:
- [LR-9A — Operational Trace Bus](LR-9A-OPERATIONAL-TRACE-BUS.md)
- [LR-9B — Execution Broker & Real PTY Runtime](LR-9B-EXECUTION-BROKER-PTY.md)
- [LR-9B — evidência nativa](LR-9B-NATIVE-EVIDENCE.json)
- [LR-9C — Terminal Surface & Stream Management](LR-9C-TERMINAL-SURFACE-STREAMS.md)
- [LR-9C — evidência nativa](LR-9C-NATIVE-EVIDENCE.json)
- [LR-9D — Cognitive / Agent Trace Adapters](LR-9D-COGNITIVE-AGENT-TRACE-ADAPTERS.md)
- [LR-9D — gate evidence](LR-9D-GATE-EVIDENCE.json).

O antigo escopo Luna Voice / feedback natural permanece em trilha futura sem
posição fixa. A LR-9 concluída fornece a infraestrutura operacional que
LR-10/Copilot e LR-11/Codex deverão reutilizar.

Objetivo: terminal Linux real para o humano, Execution Broker comum para efeitos
no sistema e observabilidade passiva, bounded e não bloqueante para providers,
workers e SpecialistAgents.

Invariantes: Passive Observability; Execution Independence; Bounded by Design;
Source Fidelity; Terminal is not Authority.

~~~text
Cognitive Plane → decide
Execution Plane → executa via Execution Broker / Exec / PTY
Observation Plane → observa via Operational Trace Bus
~~~

Decomposição:

1. LR-9A — Operational Trace Contracts & Passive Event Bus;
2. LR-9B — Execution Broker & Real PTY Runtime;
3. LR-9C — Terminal Surface & Stream Management;
4. LR-9D — Cognitive / Agent Trace Adapters;
5. LR-9E — Concurrency, Security & Final Gate.

Conversation fica com pedidos, respostas, perguntas, approvals, decisões e
relatórios. Terminal concentra shell, comandos, stdout/stderr, subtarefas,
progresso e traces operacionais.

LR-9 não entrega shell irrestrito aos especialistas; cria a fronteira que
LR-10/LR-11 deverão consumir.

Plano: [NARYS-TERMINAL-RUNTIME-TRACK.md](NARYS-TERMINAL-RUNTIME-TRACK.md).

## 14. LR-10 — GitHub Copilot SpecialistAgent

**Estado: PLANEJADA / DOCUMENTAÇÃO E DECOMPOSIÇÃO REGISTRADAS EM 09/10/2026; nenhuma subfase iniciada ou declarada PASS.**

Objetivo: aproveitar a conta Copilot Student e preservar as capacidades
operacionais do Copilot CLI (edição, comandos, testes, ferramentas, sessões e
eventos) **na interface unificada da Narys**. Copilot é `SpecialistAgent` sob
o Rust Core, não um `CognitiveProvider` de conversa trivial nem TUI paralela.

Decisões aprovadas para planejamento:
- POC do SDK **Rust oficial** primeiro; elevar o MSRV atual `1.77.2` para
  **1.94.0** apenas após validar toolchain, Cargo/Tauri, lockfile e regressões;
  manter Edition 2021 da Narys;
- `CopilotAgentAdapter` e supervisor process-wide: runtime sob demanda,
  sem start no boot e sem processo permanente em idle; cancel/stop/reap;
- perfis **Assistido (default)**, **Autônomo isolado** (sandbox comprovado)
  e **YOLO** somente por autorização humana explícita; YOLO não equivale
  a sandbox nem a Autopilot;
- authority agentiva separada de provenance/HumanLocal; o CLI SDK possui
  loop próprio de ferramentas e **não** passa pelo Execution Broker
  automaticamente;
- Narys usa TaskGraph, LR-8.5 quotas/usage, LR-9 traces/passive observation,
  Activity/Conversation, sem raw reasoning e sem leaks de credenciais;
- modelo Auto por padrão; quotas são fatos consultados, e limites do SDK
  podem ser soft, não teto rígido garantido.

**Decomposição:** LR-10A — SDK/Runtime POC; LR-10B — Adapter/Supervisor;
LR-10C — Authority/Approvals/Sandbox/YOLO; LR-10D — TaskGraph/Quota/Trace;
LR-10E — UI e teste real de engenharia; LR-10F — auditoria independente
e gate final de regressão/concorrência/segurança.

**Gate final:** tarefa real em workspace descartável autorizado; eventos,
diff/testes e uso observados; approvals efetivos; cancel e reentrada
comprovados; desligamento do runtime em idle; ausência de bypass no Broker,
duplicação de efeitos ou custos implícitos.

Plano vinculante da fase: [LR-10 — Copilot SpecialistAgent](LR-10-COPILOT-SPECIALIST-AGENT.md).  
Protocolo de entrada: [LR-10A — SDK & Runtime Feasibility POC](LR-10A-FEASIBILITY-POC.md).

## 15. LR-11 — OpenAI Codex SpecialistAgent

### POC

Validar no ambiente real:

- autenticação Codex/ChatGPT existente;
- app-server/CLI;
- thread start/resume;
- streaming de eventos;
- approvals;
- cancelamento;
- working directory;
- sandbox.

### Decisão de adapter

Preferência arquitetural:

1. Rust controla codex app-server/CLI diretamente;
2. usar bridge TypeScript com @openai/codex-sdk se trouxer benefício concreto;
3. Python apenas se houver vantagem clara.

### Política

Codex é especialista em engenharia, não provider geral.

### Gate

Luna consegue:

- abrir tarefa de diagnóstico em diretório permitido;
- mostrar progresso real;
- exigir aprovação para ação sensível;
- cancelar;
- registrar resultado/uso;
- preservar task state fora do Codex.

## 15.5. NX — Narys Cognitive Evolution (pós-LR-11)

**Estado: PLANEJADA / TRILHA EXPERIMENTAL. Execução somente após o fechamento da LR-11.**

A fase pós-LR-11 reserva uma trilha de pesquisa arquitetural para investigar
mecanismos que tornem a Narys estruturalmente diferente de um agente convencional
baseado apenas em mais providers, mais contexto, mais memória ou mais workers.

Tese:

> **Narys transforma recursos cognitivos escassos em capacidade acumulativa.**

A trilha investiga, sem antecipar implementação:

- Cognitive IR;
- Cognitive Metabolism;
- Attention Market;
- Epistemic Ledger e dívida epistemológica;
- Proof-Carrying Actions;
- Skill Foundry;
- Semantic Immune System;
- Shadow Cognition;
- Intent Field;
- Local Reflex Mesh.

A numeração de pesquisa será **NX-0 → NX-8**, separada das LRs de entrega. Cada
bloco deve nascer de hipótese falsificável, baseline e métrica; falhar em provar
ganho é resultado válido e não obriga promoção ao produto.

A propriedade-alvo é:

> **mais experiência útil → menos inteligência externa necessária para obter a mesma capacidade.**

O registro não altera LR-9/LR-10/LR-11, não inicia código e não fixa ainda a
posição relativa de NX contra LR-12/LR-13, NARYS-VOICE, NARYS-NORM ou lanes de
avatar. Essa priorização será feita no checkpoint pós-LR-11.

Plano dedicado:
[NARYS-POST-LR11-COGNITIVE-EVOLUTION.md](NARYS-POST-LR11-COGNITIVE-EVOLUTION.md).

## 16. LR-12 — provider pack adicional

A LR-7D2.5 antecipa a redundância essencial que antes morava parcialmente aqui:
dois providers adicionais e a primeira base OpenAI-compatible, se tecnicamente
adequada. LR-12 fica como expansão posterior, não como correção de dependência.

Depois da arquitetura provada:

- OpenRouter;
- Cohere;
- Hugging Face experimental;
- outros providers diretos que acrescentem capacidade/quota útil;
- OpenAI API paga.

Cada provider só entra se acrescentar capacidade, quota ou fallback útil.

Não transformar quantidade de adapters em objetivo.

## 17. LA-1 — pipeline VRM sem bloquear Core

**Lane A, independente das fases LR.**

Enquanto LR-1/LR-2 avançam, continuar a formação Blender normalmente.

### POC VRM

- instalar VRM Add-on compatível com Blender 3.3.21;
- importar a base VRM;
- salvar .blend fonte;
- validar armature/humanoid;
- exportar VRM 1.0;
- carregar via three-vrm em caminho experimental.

Não substituir o asset de produção ainda.

## 18. LA-2 — primeiro VRMA: Idle

Quando o Idle manual estiver artisticamente aceitável:

- exportar idle.vrma;
- carregar separadamente;
- aplicar ao avatar VRM;
- validar loop;
- validar no navegador;
- validar Tauri;
- medir desempenho básico.

Se falhar, manter Legacy GLB e diagnosticar sem bloquear LR.

## 19. LA-3 — Animation Director em produção

Quando LR-1 e o POC VRM/VRMA estiverem sólidos:

- manifest do Avatar Pack;
- animation intents;
- priorities;
- fades;
- fallback;
- debug panel;
- remover dependência do botão Acenar no fluxo normal.

O botão pode continuar no painel de debug.

## 20. LA-4 — crescimento incremental do vocabulário corporal

Cada animação nova deve ser uma pequena entrega.

Exemplos:

- greeting;
- thinking;
- focus-input;
- acknowledge;
- success;
- concern;
- amused;
- stretch;
- speaking overlays.

Adicionar animação não pode exigir alteração no Luna Core.

## 21. LR-13 — ferramentas reais

Só depois de identidade, memória, segurança, task state e feedback estarem funcionais.

Começar por baixo risco:

- leitura de arquivos;
- listagem de diretórios;
- informações do sistema.

Depois:

- criação/edição de arquivos em escopo autorizado;
- execução de comandos allowlisted;
- Git;
- integrações externas.

Toda ferramenta:

- capability;
- permission;
- input schema;
- timeout;
- cancellation;
- audit;
- sensitivity level.

## 22. LR-14 — avaliação Economia / Timing / Distribuição

Criar suíte de cenários.

### Economia

Medir:

- chamadas/tarefa;
- tokens/tarefa;
- custo;
- cache hits;
- calls evitadas;
- provider quota consumption.

### Timing

Medir:

- time-to-first-feedback;
- time-to-first-token;
- task completion latency;
- queue delay;
- provider delay.

### Distribuição

Medir:

- uso por provider;
- concentração;
- fallback;
- fairness;
- affinity savings;
- 429 evitados/ocorridos.

Não otimizar cegamente distribuição se isso piorar economia ou timing.

## 23. Android e mensageiros

Permanecem posteriores ao núcleo desktop, mas a arquitetura não deve bloqueá-los.

Antes de Android:

- Luna Core com abstrações portáveis;
- storage funcionando;
- provider registry;
- avatar pack;
- performance gate.

Mensageiros serão canais da Luna, não Luna separadas.

### Decisão de configuração cognitiva — 27/09/2026

O gate humano da UIP-5A revelou que o chat ainda carregava `max_output_tokens=512` e `thinking_level=low` como limites internos herdados da LR-6. A partir desta decisão, **parâmetros que afetem capacidade, qualidade, latência, custo ou comportamento cognitivo não podem permanecer invisíveis quando forem configuráveis pela integração**.

Até a UIP-6, ajustes como elevar temporariamente o output budget são aceitáveis como defaults de protótipo claramente documentados. Na UIP-6, a janela IA/modelos deve permitir política persistida por papel cognitivo, incluindo provider/agente, modelo, reasoning/thinking, teto de saída ou máximo do provider, contexto, timeouts, retries/fallback, streaming, custo/cota e parâmetros específicos suportados. Escolhas explícitas do usuário têm precedência sobre heurísticas automáticas, salvo limites reais de segurança, permissão ou capacidade da integração.

## 23.5. NARYS-NORM — Identity & Namespace Normalization

**Estado: PLANEJADA / SEM POSIÇÃO FIXA NO ROADMAP.**

Em 07/10/2026 o projeto adotou **Narys** como nome do produto/ecossistema.
**Luna** continua sendo a agente persistente/identidade que opera dentro desse
sistema. A decisão de branding não autoriza uma substituição textual global:
parte das ocorrências de `Luna` é semanticamente correta e deve permanecer,
enquanto `Assistente-3D` e outros identificadores antigos podem estar presos a
compatibilidade, dados locais, credenciais, bundle IDs, paths ou histórico.

A trilha **NARYS-NORM** existe para revisar o código e normalizar a identidade
técnica de forma deliberada, com migração quando necessário. Ela é
**transversal e não preemptiva**: pode começar no próximo checkpoint estável,
depois da LR-11 ou em uma janela posterior. Não é pré-requisito para concluir
LR-11 e não deve interromper trabalho funcional apenas por existir.

Objetivos:

- inventariar ocorrências de nomes antigos em código, UI, documentação, build,
  package metadata, Tauri, storage, credential store, banco, logs, telemetry,
  testes, paths e automações;
- classificar cada ocorrência como identidade pública Narys, identidade da
  agente Luna, compatibilidade/legado intencional ou dívida removível;
- migrar somente identificadores cuja troca seja segura ou possua caminho de
  compatibilidade explícito;
- impedir perda de SQLite, memória, conversas, continuations, settings, segredos
  ou outras informações por mudança de app identifier/path;
- manter histórico Git e documentos históricos factualmente preservados;
- terminar com uma superfície coerente: Narys como sistema/produto e Luna como
  agente, sem aliases acidentais ou mistura semântica.

O plano detalhado, riscos e gates estão em
[NARYS-NORMALIZATION-TRACK.md](NARYS-NORMALIZATION-TRACK.md).

## 23.6. NARYS-TERM — promovida para LR-9

**Estado:** PROMOVIDA em 08/10/2026.

A trilha registrada durante PERF-1B passou a ocupar formalmente LR-9 e foi
ampliada com Execution Broker, Operational Trace Bus, passive observability,
stream management e preparação explícita para LR-10/LR-11.

Plano mestre:
[NARYS-TERMINAL-RUNTIME-TRACK.md](NARYS-TERMINAL-RUNTIME-TRACK.md).

## 23.7. NARYS-VOICE — Unified Voice & Natural Feedback

**Estado:** TRILHA FUTURA / SEM POSIÇÃO FIXA.

O antigo escopo LR-9 foi preservado sem fase numerada. Não é pré-requisito para
LR-10/LR-11 e não deve fabricar traces para o Terminal.

Plano:
[NARYS-VOICE-FUTURE-TRACK.md](NARYS-VOICE-FUTURE-TRACK.md).

## 24. Próxima ação recomendada

Com **LR-6 = PASS completo**, **UIP-0 → UIP-7 = PASS funcional**, **LR-7 = PASS
completo**, **LR-8 = PASS completo**, **LR-8.5A/B/C = PASS técnico** e
**PERF-1A/B/C/D = PASS**, a trilha **PERF-1 — Adaptive Presence & Economy Mode**
está encerrada.

O fechamento em 07/10/2026 consolida:

- Economy como default 2D;
- Presence como opt-in explícito;
- Headless real sem WebView;
- Adaptive Auto restrito a Economy ↔ Headless;
- continuidade do Core/Conversation sem dependência da Presentation;
- Attention com recuperação segura em Economy;
- dívidas não bloqueantes preservadas documentalmente.

A PERF-1 está integrada à `main`. Em 08/10/2026, NARYS-TERM foi promovida e ampliada para a próxima fase funcional: **LR-9 — Operational Terminal & Cognitive Trace Runtime**.

NARYS-NORM permanece transversal. O antigo escopo Luna Voice foi preservado em NARYS-VOICE, sem posição fixa.

Plano encerrado:
[PERF-1-ADAPTIVE-PRESENCE.md](PERF-1-ADAPTIVE-PRESENCE.md).

Último checkpoint:
[PERF-1D-ADAPTIVE-PRESENCE.md](PERF-1D-ADAPTIVE-PRESENCE.md).

## 25. Definição da primeira grande entrega funcional

A primeira versão da “Luna estrutural” estará demonstrada quando houver:

- Luna Core Rust;
- TaskEvent via Channel;
- Identity persistente;
- Memory v0 persistente;
- múltiplos Cognitive Providers independentes, sem provider individual obrigatório;
- Scheduler econômico;
- rate limit/fallback;
- feedback em andamento;
- terminal operacional/trace runtime bounded;
- Execution Broker preparado para SpecialistAgents;
- Avatar Runtime desacoplado;
- pelo menos Idle plugável;
- Animation Director;
- secrets protegidos.

Codex, Copilot e grande catálogo de animações podem ser adicionados incrementalmente depois sem refazer essa base.

## PRE-LR-6 hardening — fechamento dos gates

O hardening introduz propagação terminal de falha do Channel (`channel_closed`), mocks transitórios por tentativa de cada tarefa e chave Stronghold no credential store do SO, com migração verificada do arquivo legado. O release deixa de registrar handlers diagnósticos LR-3/LR-4/LR-5; permissões declarativas residuais da capability estática seguem documentadas. **Estado: concluído no Fedora em 26/09/2026.** `cargo check`, `cargo test`, `cargo check --release`, typecheck/build frontend e migração/reabertura real passaram. O Secret Service disponibilizou a chave após reinício do app; `luna-lr3.unlock` não reapareceu. A toolchain Rust 1.77.2 exata e um reboot/logout do SO não foram testados. Esse hardening posteriormente liberou e sustentou a LR-6, que foi implementada, auditada e validada com API real em 26/09/2026. O próximo trabalho estrutural é a trilha UIP antes da LR-7.

**Auditoria:** PASS em 26/09/2026. A revisão confirmou que falhas do event sink impedem execução/retry/fallback subsequentes e resultam em `channel_closed`; os mocks transitórios usam `attempt` por execução e permanecem repetíveis com o mesmo runtime; cooldown continua compartilhado entre tarefas; o SecretStore usa `UnlockKeyStore` interno com credential store do SO e não possui fallback plaintext. A migração só remove o legado após validação da chave recuperada e do snapshot. O gate técnico para iniciar LR-6 no desktop Fedora está liberado.

**Atualização UIP-6C (28/09/2026):** UIP-6A/UIP-6B/UIP-6C e UIP-6 estão fechadas em PASS funcional. O Scheduler compartilha cooldown de Retry-After de 429 e 503 entre conversa e resumo. Summary é oportunista e aguarda a conclusão das conversas foreground antes de iniciar nova chamada. A dívida residual de Summary consumir disponibilidade Gemini antes da primeira mensagem manual permanece para LR-7/LR-8 ou rodada dedicada de estabilidade; LR-7 e LR-8 não começaram.

**Atualização UIP-7 (28/09/2026):** segundo gate curto de performance e lifecycle concluído e aprovado por Sam. [UIP-7 = PASS funcional / FECHADA](UIP-7-FINAL-PERFORMANCE.md); **UIP-0 → UIP-7 encerradas**. A próxima etapa funcional é **LR-7 — segundo provider real + distribuição**. Groq e Mistral seguem candidatos; a escolha será feita na nova sessão dedicada à LR-7.


**Atualização LR-7B (28/09/2026):** PASS completo. Groq foi integrado como segundo provider real no Registry, com Stronghold, streaming SSE, usage e diagnóstico `Fixed(groq)`. Conversation/Summary permaneceram `Fixed(gemini)` nesta etapa; a distribuição real foi fechada posteriormente na LR-7C.
## LR-7D1 — PASS completo

Fechada em 30/09/2026 após auditoria independente e gate humano. O Orchestrator
persistido alterna Gemini/Groq por configuração, registra `TaskId` antes do
preflight bloqueante, usa Scheduler/TaskRegistry, valida `PlanV1` de forma
fail-closed e não executa ferramentas ou passos. Persistência, planejamento real
Groq, responsividade e cancelamento em `running` com Groq/Gemini foram
validados. No gate final, Gemini respondeu HTTP 503 `service_unavailable` com
`Retry-After` de 30 s; o tratamento de erro/cooldown foi correto, mas essa
rodada não é registrada como sucesso Gemini → `PlanV1`. D2/D3, LR-8 e novas
permissões do Codex permaneciam adiadas naquele checkpoint. O estado posterior
e o fechamento da LR-7 estão registrados na seção de próxima ação acima.

## LR-7D2 — registro histórico da implementação candidata

A branch `lr-7d2-smart-routing`, criada da main com PR #10 integrada, entrega
migration 009 / schema 9, lista ordenada de targets, Preferred chain e Auto com
score determinístico e affinity de Conversation por sessão (256 entradas em
memória, limpa no restart). Os três papéis usam configuração individual por
target; a UI edita a ordem e os eventos mostram motivo/score factual. Luna Core
continua autoridade; Orchestrator propõe somente PlanV1 e Summary preserva o
histórico. Nenhuma ferramenta, task graph, paralelismo ou LR-8 foi antecipada.

Naquele checkpoint, a próxima ação era auditoria independente e gate humano
Gemini/Groq descritos em [LR-7D2-SMART-ROUTING.md](LR-7D2-SMART-ROUTING.md).
A D2 foi posteriormente fechada, seguida por D2.5 e D3. Blender,
identidade/memória e fronteira Codex permaneceram independentes.

## Atualização PERF-1C — 07/10/2026

**PERF-1A/1B continuam PASS**, com as dívidas da 1B preservadas. A Conversation
passa a usar broker nativo e policy explícita HeadlessSafe; tarefas UiBound
conservam fail-closed. Presentation pode ser destruída e recriada por segunda
ativação, sem recriar Core ou iniciar outra tarefa. Quit é ação distinta.

Estado: **PERF-1C PASS — concluída e integrada à main**.
[Arquitetura, testes, medições e dívidas](PERF-1C-HEADLESS-RUNTIME.md).

## Atualização PERF-1D — 07/10/2026

A quarta e última implementação formal da PERF-1 cria autoridade nativa de policy
separada da superfície React. Economy permanece default; Presence exige escolha
manual; Headless persistido admite Economy temporária por ativação explícita;
Auto opera somente Economy/Headless com 30 s de hysteresis, guards e attention
estruturada. Nenhuma policy cognitiva ou capability LR-9/10/11 foi antecipada.
[Implementação, evidências e gates](PERF-1D-ADAPTIVE-PRESENCE.md).
Estado: **PERF-1D = PASS** após FIX-1, segunda auditoria e gate humano. **PERF-1 = PASS completo** em 07/10/2026. TERM/NORM continuam trilhas futuras separadas.
