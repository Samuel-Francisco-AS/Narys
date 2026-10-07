# PERF-1A — Baseline & Presentation Lifecycle

**Estado:** EM EXECUÇÃO  
**Branch:** `perf-1a-baseline-lifecycle`  
**Base:** `main@639e02b16395acf6147133c09b1f7a4bf17f19b9`  
**Iniciada em:** 07/10/2026  
**Fase-mãe:** [PERF-1 — Adaptive Presence & Economy Mode](PERF-1-ADAPTIVE-PRESENCE.md)

## 1. Objetivo

PERF-1A estabelece a linha de base mensurável da Presence atual e transforma a
Presentation em um lifecycle explicitamente separável do Narys Core.

A etapa fecha quando for demonstrado que a apresentação pode ser desmontada e
reconstruída sem matar ou duplicar trabalho do Core, perder sessão/conversa,
quebrar cancelamento ou deixar recursos gráficos/processos órfãos.

PERF-1A **não entrega ainda a Economy Shell final**. Ela prepara a fronteira
arquitetural segura para que PERF-1B possa torná-la a interface padrão sem
refazer o Core ou depender do stack 3D.

## 2. Regra de decomposição

PERF-1 possui somente quatro checkpoints formais:

1. PERF-1A — Baseline & Presentation Lifecycle;
2. PERF-1B — Economy Shell;
3. PERF-1C — Headless Runtime;
4. PERF-1D — Adaptive Presence.

PERF-1A pode conter tarefas internas, testes, auditoria e ajustes, mas **não deve
ser fragmentada em subfases formais** como 1A.1/1A.2. Defeitos encontrados
durante implementação ou auditoria podem gerar `FIX-1`, `FIX-2`, etc.,
vinculadas à própria PERF-1A.

## 3. Decisões de produto já fechadas

Estas decisões orientam a arquitetura desde 1A, mesmo que a UI definitiva só
seja implementada em 1B:

- **Economy Shell será a interface padrão da Narys.**
- A interface padrão será 2D, baseada em DOM/CSS, opaca quando isso evitar custo
  de composição e sem efeitos contínuos desnecessários.
- A janela principal deverá ser facilmente redimensionável.
- A navegação esquerda e o painel operacional direito deverão ser recolhíveis,
  restauráveis e redimensionáveis.
- O centro será um workspace para a superfície atual: shell/log, chat, tasks,
  approvals, settings, diagnostics ou outras views.
- O topo terá identidade mínima da Narys (logo em miniatura + nome), sem
  `Narys // perf mode` como elemento permanente.
- A estética fina será ajustada incrementalmente; nenhuma decoração justifica
  blur, transparência real, polling excessivo ou animação contínua sem benefício.
- Three.js, WebGL, GLB, AnimationMixer e loops gráficos deverão ficar
  **desativados por padrão**.
- Presence 3D só poderá ser carregada após opção/configuração explícita do
  usuário. Auto/Adaptive não ganha autorização implícita para ativar 3D.
- Economy não pode reduzir thinking, contexto, output budget, prioridade do
  Scheduler/Orchestrator nem inserir sleeps/throttling cognitivo.

## 4. Estado técnico de partida

No início de PERF-1A:

- `App.tsx` monta `AvatarViewport` incondicionalmente na superfície principal;
- `AvatarViewport` importa Three.js, cria `SceneRuntime`, carrega o avatar,
  cria o `AnimationDirector` e inicia o runtime;
- `SceneRuntime` possui `WebGLRenderer`, canvas, `ResizeObserver`,
  listeners de contexto e `setAnimationLoop`;
- `RenderBudget` limita a cadência para 30 FPS em foco, 24 FPS em background e
  suspende trabalho de frame quando `document.visibilityState === hidden`;
- suspensão de frames **não equivale a desmontagem**: renderer, contexto WebGL,
  asset, Three.js e WebView continuam pertencendo à apresentação viva;
- o teardown existente já chama `setAnimationLoop(null)`, remove listeners,
  desconecta observer, descarta avatar/director/renderer, remove canvas e limpa
  a scene;
- o Narys Core Rust é inicializado pelo `tauri::Builder.setup()` e mantém
  Database, TaskRegistry, providers, Scheduler, SummaryWorker e AgentRegistry
  fora do React;
- entretanto a aplicação ainda nasce com a janela principal prevista no
  `tauri.conf.json`, e a independência completa de WebView pertence à PERF-1C.

Essa base torna plausível separar lifecycle sem reescrever o renderer.

## 5. Trabalho da PERF-1A

As atividades abaixo são uma única implementação/checkpoint. A ordem pode ser
ajustada durante execução sem criar novas subfases.

### Baseline reproduzível

Coletar no mesmo ambiente e build, com duração de amostra registrada:

- Presence visível em foco e idle;
- Presence visível sem foco;
- Presence oculta/minimizada quando a plataforma permitir distinção confiável;
- Presence durante uma operação cognitiva representativa.

Registrar quando disponível de forma confiável:

- RSS/memória agregada dos processos relevantes;
- CPU idle e durante operação;
- processos e WebViews associados à aplicação;
- FPS/frame time e callbacks processados/skipped;
- renderer/canvas/contexto WebGL/asset ativo;
- wakeups/context switches somente se a coleta for reproduzível;
- latência de uma ou mais operações do Core para comparação futura;
- hardware, SO, sessão gráfica, build/ref e comandos usados na medição.

Não fixar meta percentual antecipada. O baseline deve permitir comparação factual
com PERF-1B/1C.

### Ownership e fronteiras

Produzir mapa de ownership entre:

- Narys Core/Rust;
- conversation/task/session state;
- React Interaction Layer;
- WindowController;
- Presence/Avatar runtime;
- settings e janelas auxiliares.

Classificar explicitamente qualquer estado cuja perda ao desmontar React possa:

- encerrar ou duplicar tarefa;
- perder `TaskId`;
- perder sessão/conversa corrente;
- perder estado de cancelamento;
- esconder approval pendente;
- trocar routing/policy/configuração;
- reconstruir estado persistente de forma divergente.

Estado essencial deve permanecer no Core/persistência ou ser reidratável por
contrato; Presentation não pode virar autoridade apenas para facilitar a PERF.

### Contrato de Presentation

Introduzir um contrato mínimo, com nomes equivalentes aos abaixo se o código
pedir outra nomenclatura:

```text
PresentationMode = economy | presence | headless
PresentationState = modo + lifecycle observável
```

Na 1A o contrato existe para separar responsabilidades. Não é necessário
implementar a Economy Shell final nem Headless completo.

As regras mínimas são:

- Core não conhece detalhes do renderer;
- Presence é uma superfície montável/desmontável;
- Interaction observa e solicita ações por contratos;
- teardown de Presentation não representa shutdown do Core;
- reentrada da Presentation reconstrói apenas o estado visual/observável;
- nenhuma transição dispara execução cognitiva duplicada.

### Fronteira real da Presence

Encapsular a superfície 3D atual de forma que sua montagem seja explicitamente
condicional.

Preservar o comportamento aprovado da UIP. Não refatorar Three.js por estética.

O trabalho deve aproveitar, revisar e testar os lifecycles já existentes em:

- `AvatarViewport`;
- `SceneRuntime`;
- `AvatarManager`;
- `AnimationDirector`;
- `RenderBudget`.

### Teardown e reentrada

Provar ciclos controlados de:

```text
Presence
→ desmontar Presentation 3D
→ estado Core continua válido
→ remontar Presence
→ UI reidrata o estado observável correto
```

Verificar:

- animation loop parado;
- timers estritamente visuais cancelados;
- listeners removidos;
- `ResizeObserver` desconectado;
- mixer/actions descartados;
- avatar removido e assets liberados pelo adapter;
- renderer descartado;
- canvas removido;
- referências antigas não recebem eventos depois do teardown;
- uma nova Presence consegue criar renderer/asset/animação normalmente.

Não adicionar `forceContextLoss` ou hacks de driver como requisito sem evidência
de que `renderer.dispose()` e o lifecycle normal sejam insuficientes.

### Continuidade do Core

Durante teardown/reentrada, validar pelo menos:

- sessão/conversa permanece identificável;
- tarefa iniciada antes da desmontagem não reinicia nem duplica;
- cancelamento continua chegando à tarefa correta;
- estado necessário para approval/atenção humana permanece recuperável;
- provider/routing/policies permanecem no Core/persistência;
- teardown da Presentation não encerra Scheduler/workers por efeito colateral.

Quando uma parte ainda não possuir um approval real implementado, testar o
contrato observável equivalente existente e documentar a limitação sem fabricar
capability.

### Preparação da fronteira de carregamento

PERF-1A deve deixar o código pronto para que PERF-1B possa iniciar em Economy sem
carregar Presence.

A etapa pode introduzir root/surface selection e lazy boundary necessários, mas
**não deve construir a UI visual final da 1B**.

Critério arquitetural desejado:

```text
bootstrap
→ Core/runtime
→ escolher Presentation
   ├─ Economy: não alcançar import/asset/renderer 3D
   ├─ Presence: carregar stack 3D sob demanda
   └─ Headless: tratado plenamente em PERF-1C
```

Se a separação total do bundle 3D depender de trabalho visual da 1B, a 1A deve
ao menos provar a fronteira de import e documentar precisamente o que resta.

### Medição após a separação

Reexecutar cenários relevantes para garantir que a refatoração de lifecycle:

- não piorou deliberadamente latência do Core;
- não introduziu CPU idle maior sem justificativa;
- não criou crescimento grosseiro de memória após ciclos repetidos;
- não deixou múltiplos renderers/canvas/processos órfãos.

A grande redução de recursos é esperada principalmente em 1B/1C; 1A não deve
falsificar ganho apenas por esconder a janela.

## 6. Evidências esperadas

O fechamento deve registrar:

- comandos/metodologia de baseline;
- números medidos antes e depois da refatoração de lifecycle;
- mapa de ownership;
- arquivos/contratos alterados;
- testes automatizados adicionados;
- resultado dos ciclos teardown/reentry;
- resultado de typecheck/build/testes Rust/frontend;
- limitações reais da plataforma que afetem a medição.

Métricas impossíveis ou não confiáveis no ambiente devem ser marcadas como
`N/A` com justificativa; não inventar proxy silencioso.

## 7. Gate de PASS

PERF-1A só pode ser marcada PASS quando:

1. baseline reproduzível estiver registrada;
2. Presence atual continuar funcional;
3. Presentation possuir fronteira/lifecycle explícito;
4. desmontar e recriar Presence não perder TaskId, conversa ou estado necessário
   para continuidade/atenção humana;
5. teardown não encerrar nem duplicar trabalho do Core;
6. loop/listeners/observer/renderer/avatar antigos forem encerrados pelo caminho
   de lifecycle previsto;
7. ciclos repetidos não mostrarem vazamento grosseiro ou múltiplos renderers
   sobreviventes;
8. a arquitetura estiver pronta para Economy ser default sem exigir que o Core
   importe ou possua o stack 3D;
9. typecheck/build/testes existentes e novos estiverem verdes;
10. nenhuma feature de 1B/1C/1D tiver sido antecipada apenas para declarar PASS.

## 8. Fora de escopo

PERF-1A não deve:

- construir o layout final da Economy Shell;
- redesenhar sidebar/painel operacional;
- tornar Headless completo;
- implementar política Adaptive;
- alterar providers, Scheduler ou ResourceAllocator;
- reduzir parâmetros cognitivos para economizar recursos;
- reorganizar NARYS-NORM;
- refazer a arte/modelo 3D;
- resolver dívidas Wayland sem relação causal com o lifecycle;
- adicionar voz, browser automation ou novos agents.

## 9. Política de fixes

Falha encontrada pela auditoria permanece dentro da PERF-1A.

Formato esperado:

- implementação candidata;
- auditoria independente;
- `FIX-1`, `FIX-2`, ... somente quando necessário;
- nova auditoria/gate;
- PASS e fechamento documental.

Não criar novos checkpoints formais para absorver fixes.

## 10. Próxima etapa após PASS

Somente após PERF-1A PASS:

> **PERF-1B — Economy Shell**

PERF-1B transforma a interface 2D leve em default, implementa o layout
redimensionável/recolhível acordado e prova que o caminho padrão não monta nem
carrega o stack 3D.
