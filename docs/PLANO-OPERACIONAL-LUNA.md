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

**Estado:** em andamento desde 26/09/2026 após **LR-6 = PASS completo**. **UIP-0, UIP-1, UIP-2, UIP-3 e UIP-4 estão fechadas**; a próxima etapa é **UIP-5 — histórico + resumo assíncrono**. Na UIP-3, always-on-top no Wayland nativo e click-through seguro ficaram adiados; na UIP-4, o resize nativo no GNOME/Wayland ficou como dívida de estabilidade espacial após a rejeição do POC de múltiplas WebViews. Essas limitações não bloqueiam a trilha. A sequência **UIP-0 → UIP-7** transforma a casca M0-B em uma interface de presença desktop e estabelece orçamento real de renderização antes da LR-7.

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

**Candidatos preferidos:** Groq ou Mistral.

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

## 12. LR-8 — Rate Limit Manager completo

Pode começar parcialmente antes, mas fecha aqui.

### Requisitos

- RPM;
- TPM;
- RPD/TPD;
- concurrency;
- token bucket;
- fila;
- header parsing;
- retry-after;
- exponential backoff + jitter;
- cooldown;
- circuit breaker;
- daily/task budget;
- telemetry.

### Painel

Mostrar por provider:

- enabled;
- health;
- queue;
- requests;
- tokens;
- quota known/unknown;
- cooldown;
- custo;
- uso recente.

## 13. LR-9 — Luna Voice e feedback natural

Separar worker de apresentação.

### Trabalho

- TaskResult estruturado;
- output model policy;
- fallback;
- Identity + RelevantMemory;
- progress templates locais;
- LLM apenas para feedback complexo quando necessário.

### Regra

Eventos simples não gastam LLM.

Exemplo:

~~~text
ToolStarted(test_runner)
→ "Estou executando os testes agora."
~~~

pode ser template local.

## 14. LR-10 — GitHub Copilot SpecialistAgent

Aproveitar Copilot Student.

### POC primeiro

- validar SDK Rust;
- login/autenticação;
- listar capabilities/model behavior;
- ler account quota;
- iniciar sessão;
- stream de eventos;
- cancelamento.

### Integração

Criar CopilotAgentAdapter atrás do Luna Core.

Nunca chamar diretamente do React.

### Política inicial

- tarefas de desenvolvimento;
- respeitar auto model selection do Student;
- considerar quota real no Scheduler;
- não usar para conversa trivial.

### Gate

Luna despacha uma tarefa de código controlada ao Copilot, recebe eventos, mostra progresso e registra consumo.

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

## 16. LR-12 — provider pack adicional

Depois da arquitetura provada:

- Cloudflare Workers AI;
- adapter OpenAI-compatible;
- OpenRouter;
- Cohere;
- Hugging Face experimental;
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

## 24. Próxima ação recomendada

Com **LR-6 = PASS completo**, **UIP-0 → UIP-4 fechadas** e **UIP-5A/UIP-5B = PASS funcional**, a etapa corrente é **UIP-5C — resumo assíncrono + título de sessão**, dentro de **UIP-5 — histórico + resumo assíncrono**, conforme [UI-PERFORMANCE-PLAN.md](UI-PERFORMANCE-PLAN.md). LR-7 permanece deliberadamente aguardando o fechamento da trilha UIP. A dívida de estabilidade espacial da UIP-4 no Wayland fica para UIP-7 ou investigação nativa dedicada. O trabalho de Blender segue independente; o offset dos brincos no GLB atual está documentado como dívida do pipeline de exportação, sem evidência de defeito no runtime Three.js.

## 25. Definição da primeira grande entrega funcional

A primeira versão da “Luna estrutural” estará demonstrada quando houver:

- Luna Core Rust;
- TaskEvent via Channel;
- Identity persistente;
- Memory v0 persistente;
- Gemini + segundo provider;
- Scheduler econômico;
- rate limit/fallback;
- feedback em andamento;
- Luna Voice;
- Avatar Runtime desacoplado;
- pelo menos Idle plugável;
- Animation Director;
- secrets protegidos.

Codex, Copilot e grande catálogo de animações podem ser adicionados incrementalmente depois sem refazer essa base.

## PRE-LR-6 hardening — fechamento dos gates

O hardening introduz propagação terminal de falha do Channel (`channel_closed`), mocks transitórios por tentativa de cada tarefa e chave Stronghold no credential store do SO, com migração verificada do arquivo legado. O release deixa de registrar handlers diagnósticos LR-3/LR-4/LR-5; permissões declarativas residuais da capability estática seguem documentadas. **Estado: concluído no Fedora em 26/09/2026.** `cargo check`, `cargo test`, `cargo check --release`, typecheck/build frontend e migração/reabertura real passaram. O Secret Service disponibilizou a chave após reinício do app; `luna-lr3.unlock` não reapareceu. A toolchain Rust 1.77.2 exata e um reboot/logout do SO não foram testados. Esse hardening posteriormente liberou e sustentou a LR-6, que foi implementada, auditada e validada com API real em 26/09/2026. O próximo trabalho estrutural é a trilha UIP antes da LR-7.

**Auditoria:** PASS em 26/09/2026. A revisão confirmou que falhas do event sink impedem execução/retry/fallback subsequentes e resultam em `channel_closed`; os mocks transitórios usam `attempt` por execução e permanecem repetíveis com o mesmo runtime; cooldown continua compartilhado entre tarefas; o SecretStore usa `UnlockKeyStore` interno com credential store do SO e não possui fallback plaintext. A migração só remove o legado após validação da chave recuperada e do snapshot. O gate técnico para iniciar LR-6 no desktop Fedora está liberado.
