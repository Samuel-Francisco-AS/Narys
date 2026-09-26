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
- Luna Core;
- memória persistente;
- identidade estruturada;
- providers de IA;
- orquestração;
- rate limiting;
- ferramentas;
- feedback de tarefa;
- VRM/VRMA runtime;
- Animation Director completo (prioridades, packs e novos estados);
- segurança para segredos/agente.

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

## 24. Próxima ação recomendada

A próxima rodada de implementação estrutural deve ser **LR-1**.

Motivo:

- baixo risco;
- não exige API key;
- não exige animação nova;
- cria fronteira necessária para a lane de avatar;
- reduz acoplamento antes de introduzir Rust Core/IA.

Em paralelo, o trabalho atual de Blender continua normalmente. O Idle manual não precisa estar terminado para LR-1 começar.

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
