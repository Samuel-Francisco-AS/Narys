# PERF-1 — Adaptive Presence & Economy Mode

Estado: **PRÓXIMA FASE — pré-condições LR-8 e LR-8.5 concluídas em PASS; executar antes da LR-9.**

## Motivação

A trilha UIP provou a Presence Shell, limitou o render a 30 FPS em foco / 24 FPS em background e suspendeu render quando oculto. Isso reduziu custo sem abandonar a proposta de presença visual da Luna.

Ainda assim, a arquitetura atual continua pagando parte do custo de uma aplicação gráfica mesmo quando o usuário não precisa da personagem 3D. Three.js, WebGL, asset 3D, animações, janela transparente e WebView são recursos de apresentação; não devem ser pré-requisito para o Luna Core continuar planejando, executando tarefas, roteando providers, persistindo estado ou aguardando aprovações.

PERF-1 formaliza uma segunda etapa de performance:

> **presença gráfica passa a ser um recurso alocado conforme a necessidade, não uma condição para a Luna existir.**

A meta não é reduzir a atividade cognitiva nem introduzir latência artificial. CPU e RAM recuperadas da apresentação devem permanecer disponíveis ao Core, aos agentes, às ferramentas e às tarefas do usuário.

## Princípios

1. **Economy não significa Core lento.**
   - nenhum sleep, throttle ou redução artificial de prioridade em Scheduler, Orchestrator, providers, agents ou ferramentas;
   - qualquer ganho de CPU/RAM pode beneficiar o trabalho cognitivo e operacional.

2. **Ocultar não é suficiente.**
   - Economy não pode ser apenas `display: none`, canvas invisível ou janela fora da tela;
   - renderer, loops, animações, listeners e recursos WebGL precisam ter lifecycle explícito.

3. **Presentation é descartável; Core é persistente.**
   - fechar ou desmontar a Presence não encerra conversa, tarefa, fila, memória ou runtime;
   - reabrir a interface reconstrói somente a apresentação necessária.

4. **Nenhum subsistema visual é autoridade de estado.**
   - task state, sessões, memória, credenciais, routing e permissões continuam no Luna Core / runtimes dedicados;
   - UI observa e solicita ações por contratos.

5. **Medir antes e depois.**
   - nenhuma alegação de economia deve ser feita sem baseline reproduzível;
   - otimizações só permanecem se não causarem regressão funcional relevante.

## Modos operacionais

### Presence

Modo visual completo já conhecido:

- Luna 3D;
- CharacterStage / Three.js;
- animações;
- compositor e Conversation UI quando solicitados;
- janela transparente e comportamento de presença.

Presence é o modo de maior custo visual e continua sendo uma experiência de produto válida.

### Economy

Modo de interação de baixo custo:

- sem avatar 3D montado;
- sem renderer Three.js/WebGL ativo;
- sem animation loop;
- sem transparência ou efeitos visuais caros quando não necessários;
- UI DOM/CSS simples para conversa, status, aprovações e configurações essenciais;
- superfícies adicionais carregadas sob demanda.

Economy deve preservar integralmente as capacidades funcionais do Core.

### Headless

Modo sem janela persistente:

- Luna Core permanece ativo;
- Scheduler, Orchestrator, providers, agents, Tool Runtime, SQLite e filas podem continuar operando;
- nenhuma janela WebView precisa permanecer viva apenas para sustentar o runtime;
- interação humana ocorre por abertura sob demanda, atalho, tray, notificação ou superfície equivalente disponível na plataforma.

Headless não significa daemon irrestrito: permissões, approvals, cancelamento, audit e políticas de segurança continuam valendo.

### Adaptive / Auto

Política opcional que escolhe a apresentação conforme contexto:

~~~text
usuário chama a Luna
→ Presence ou Economy conforme preferência

janela permanece sem uso / é recolhida
→ Economy

tarefa autônoma longa sem necessidade visual
→ Headless

aprovação ou atenção do usuário necessária
→ notificação + Economy/Presence sob demanda

interação termina
→ liberar novamente recursos visuais
~~~

A política deve ser configurável. Nenhuma transição automática pode destruir contexto ou esconder uma solicitação de aprovação importante.

## Arquitetura-alvo

~~~text
                         LUNA CORE
       cognition / tasks / memory / security / tools
                              │
                     events + commands
                              │
             ┌────────────────┴────────────────┐
             │                                 │
      Interaction Layer                Presentation Layer
  chat / approvals / settings          disposable surfaces
  notifications / shortcuts                  │
             │                   ┌────────────┼────────────┐
             │                   │            │            │
             │               Presence      Economy      Headless
             │               Three.js      DOM/CSS      no window
             │                   │            │            │
             └───────────────────┴────────────┴────────────┘
~~~

O ponto arquitetural central é que Presence, Economy e Headless são **clientes do Core**, não donos do Core.

## Lifecycle visual

Ao sair de Presence, a implementação deve conseguir:

- parar `requestAnimationFrame` / `setAnimationLoop`;
- cancelar timers estritamente visuais;
- desligar observers/listeners do viewport;
- interromper AnimationMixer e referências de clips;
- liberar geometries, materials e textures do asset;
- desmontar o renderer e liberar o contexto WebGL quando apropriado;
- remover referências que impeçam garbage collection;
- impedir carregamento do GLB/Three.js quando o processo inicia diretamente em Economy/Headless;
- evitar manter WebViews auxiliares sem necessidade.

A implementação exata será decidida pela medição da plataforma. Não criar `forceContextLoss` ou hacks equivalentes como requisito sem provar necessidade.

## PERF-1A — Baseline e contratos de lifecycle

**Objetivo:** estabelecer linha de base e provar que Core, Interaction e Presentation podem ter lifecycles independentes antes de remover recursos.

### Trabalho

- medir Presence em foco, background, recolhida e durante tarefa real;
- registrar RSS, CPU, wakeups quando observáveis, WebViews/processos, FPS/frame time e memória WebGL;
- medir latência de operações cognitivas representativas no mesmo ambiente;
- mapear quais módulos montam Three.js, carregam asset e iniciam loops;
- formalizar contratos `PresenceMode` / `PresentationState` ou equivalente;
- provar que sessão/tarefa continuam válidas durante desmontagem da UI;
- definir caminho seguro de reentrada Presence ← Economy/Headless.

### Gate

- baseline reproduzível;
- nenhum estado essencial pertencente exclusivamente ao React/Three.js;
- desmontar/recriar a apresentação não perde TaskId, conversa ou estado de aprovação;
- typecheck/build/testes existentes continuam verdes.

## PERF-1B — Economy Mode

**Objetivo:** entregar uma interface interativa de baixo custo sem avatar.

### Trabalho

- UI compacta e opaca baseada em DOM/CSS;
- chat/composer/status essenciais;
- acesso às configurações sem montar CharacterStage;
- lazy load do stack 3D quando possível;
- renderer/asset/animações não existem enquanto Economy estiver ativo;
- evitar blur, backdrop-filter e animações contínuas sem benefício mensurável;
- preservar atalhos e recuperação da interface.

### Gate

Comparar Economy contra Presence no mesmo cenário:

- RSS menor de forma mensurável;
- CPU idle menor ou igual;
- zero frames 3D produzidos;
- nenhuma perda de streaming, cancelamento, approvals ou settings;
- latência cognitiva não piora por política do modo.

Não fixar antecipadamente um percentual obrigatório de RAM/CPU: o ganho precisa ser factual e será registrado.

## PERF-1C — Headless Runtime

**Objetivo:** permitir que o Luna Core continue operando sem janela WebView persistente.

### Trabalho

- separar lifecycle da janela do lifecycle do Core;
- fechar a última janela sem encerrar tarefa quando a policy permitir;
- reabrir uma superfície por tray/atalho/notificação ou mecanismo equivalente;
- garantir entrega de solicitações de aprovação;
- preservar cancelamento;
- garantir shutdown explícito e limpo;
- não deixar renderer, asset ou janela invisível como dependência disfarçada.

### Gate

- tarefa longa continua após fechamento da apresentação;
- nenhuma WebView permanente é necessária para sustentar o Core;
- reabrir UI reconstrói estado observável corretamente;
- encerramento real do aplicativo continua encerrando workers/processos previstos;
- nenhum processo/renderer órfão após ciclos repetidos.

## PERF-1D — Adaptive Presence

**Objetivo:** tornar Presence uma capacidade alocada sob demanda.

### Trabalho

- policy configurável Presence / Economy / Headless / Auto;
- transições seguras baseadas em intenção do usuário e estado operacional;
- não desmontar UI durante interação ativa;
- sinalizar tarefa que requer atenção antes de permanecer Headless;
- hysteresis/debounce suficiente para evitar alternância frenética;
- telemetria local de transições e razão, sem registrar conteúdo de conversa;
- preferência do usuário tem precedência.

### Gate

Cenário mínimo:

~~~text
Presence
→ usuário inicia tarefa longa
→ UI é recolhida
→ Economy/Headless
→ tarefa continua
→ runtime solicita aprovação
→ usuário é avisado
→ interface reaparece
→ aprovação é tratada
→ tarefa conclui
→ sistema retorna ao modo econômico configurado
~~~

Sem perda de estado, execução duplicada ou aumento artificial da latência cognitiva.

## Métricas e cenários

A comparação deve usar cargas equivalentes.

### Visuais

- FPS / frame time;
- renderer ativo/inativo;
- canvas/drawing buffer;
- contexto WebGL;
- asset carregado;
- quantidade de loops/timers visuais;
- número de WebViews/processos de apresentação.

### Sistema

- RSS;
- CPU idle e CPU durante interação;
- wakeups/context switches quando a plataforma permitir coleta confiável;
- crescimento de memória após ciclos Presence ↔ Economy;
- tempo de reconstrução da Presence.

### Core

- tempo até primeiro evento/feedback;
- time-to-first-token quando aplicável;
- latência de planejamento;
- latência de execução de worker;
- queue delay;
- cancelamento;
- tempo de resposta de approvals.

**Regra:** PERF-1 pode melhorar essas métricas por liberar recursos, mas não pode piorá-las deliberadamente para economizar UI.

### Reorganização futura de IA e modelos

A janela **IA e modelos** acumulou, ao longo de LR-7/LR-8, providers, credenciais,
políticas por papel, diagnósticos, agentes, TaskGraph, telemetria, admission, rate
accounting e resilience. O gate humano da LR-8E mostrou que uma superfície única
muito longa já prejudica navegação e observação de estados transitórios.

Uma rodada futura de UI/performance deve portanto **reorganizar essa superfície
para reduzir comprimento, custo visual e distância entre informação e ação**.

A forma exata fica deliberadamente em aberto. Sub-abas, navegação lateral,
seções montadas sob demanda, superfícies auxiliares ou outra arquitetura podem
ser avaliadas quando essa rodada começar. A decisão deve considerar ergonomia,
performance, frequência de uso, capabilities e o estado real das funcionalidades
naquele momento; este registro não congela previamente uma solução.

A reorganização deve preservar os contratos já conquistados: UI como cliente do
Core, segredos fora do React, permissões por superfície, estado operacional
read-only quando aplicável e ausência de trabalho visual desnecessário.

## Relação com UIP

PERF-1 **não reabre nem invalida UIP-0 → UIP-7**.

UIP respondeu:

> como manter a Luna 3D utilizável com um orçamento visual controlado?

PERF-1 responde:

> quando a presença 3D não é necessária, podemos deixar de pagar por ela inteiramente sem desligar a Luna?

As medições de UIP permanecem baseline histórica. PERF-1 cria uma nova comparação entre modos de apresentação.

## Relação com o roadmap

Sequência planejada:

~~~text
LR-8 PASS
→ LR-8.5 Cognitive Resource Economy & Allocation
→ PERF-1 Adaptive Presence & Economy Mode
→ LR-9 Luna Voice / feedback natural
→ LR-10 Copilot SpecialistAgent
→ LR-11 Codex SpecialistAgent
→ expansão de ferramentas e operação
~~~

PERF-1 fica nessa posição por duas razões:

1. **não interferir na estabilização cognitiva da LR-8/LR-8.5**;
2. **recuperar orçamento de CPU/RAM antes de aumentar a carga permanente com voz, agentes, ferramentas, navegador e integrações externas.**

Ela é portanto um **checkpoint de eficiência antes da fase operacional pesada**.

## Não objetivos

PERF-1 não deve:

- reduzir qualidade, contexto, thinking ou output budget de LLMs apenas por estar em Economy;
- aumentar deliberadamente timeouts/latência do Core;
- alterar políticas econômicas/rate limit da LR-8/LR-8.5;
- introduzir novo provider;
- implementar voz;
- implementar browser automation;
- completar click-through/always-on-top se isso não for necessário ao lifecycle;
- refazer a arte/modelo da Luna;
- exigir Android;
- converter o projeto inteiro para outro toolkit gráfico.

## Critério de fechamento

PERF-1 fecha quando:

1. Presence, Economy e Headless possuem lifecycles definidos;
2. Economy não monta o stack 3D;
3. Headless sustenta Core sem janela persistente;
4. Adaptive alterna modos sem perder estado;
5. approvals/cancelamento continuam seguros;
6. ciclos repetidos não revelam crescimento grosseiro de memória;
7. ganhos de CPU/RAM são registrados com baseline reproduzível;
8. nenhuma regressão deliberada de latência cognitiva é introduzida;
9. a arquitetura preserva Presentation como cliente descartável do Core;
10. a próxima expansão operacional pode consumir o orçamento recuperado sem depender da Presence.
