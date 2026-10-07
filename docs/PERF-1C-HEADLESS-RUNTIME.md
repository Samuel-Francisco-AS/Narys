# PERF-1C — Headless Runtime

**Estado:** EM EXECUÇÃO  
**Branch:** `perf-1c-headless-runtime`  
**Base:** `main@cc5bc8a50dcfc66f9c0b178b75df2cb7da05663d`  
**Iniciada em:** 07/10/2026  
**Fase-mãe:** [PERF-1 — Adaptive Presence & Economy Mode](PERF-1-ADAPTIVE-PRESENCE.md)

## Objetivo

Permitir que o Narys Core continue operando **sem WebView/janela persistente**,
com encerramento/reabertura da superfície visual sem cancelar trabalho que a
policy autorize a continuar.

PERF-1C não é "esconder a janela". O estado headless válido exige:

- nenhuma WebView principal viva;
- nenhuma janela invisível usada como keep-alive;
- nenhum renderer/canvas/asset da Presentation;
- Core, Scheduler, workers e tarefas autorizadas permanecem no processo nativo;
- reabertura cria uma nova superfície e reidrata o estado observável;
- shutdown explícito continua encerrando o runtime de forma limpa.

## Estado técnico de partida

Após PERF-1B:

- Economy é a Presentation padrão;
- Presence é lazy/opt-in;
- o Core Rust já é criado no `tauri::Builder.setup/manage`, fora do React;
- a janela `main` ainda é criada declarativamente por `tauri.conf.json`;
- `builder.run(...)` ainda segue o lifecycle padrão do Tauri;
- `WindowController` controla apenas a janela já existente;
- não existe hoje mecanismo de produto para destruir a última WebView e
  posteriormente reconstruí-la;
- `PresentationMode::headless` existe apenas no contrato/harness DEV e ainda
  significa somente "3D desmontado", não runtime realmente sem WebView.

### Bloqueio arquitetural principal descoberto antes da implementação

A conversa **ainda pertence parcialmente ao lifecycle do React/WebView**.

`useConversationController()`:

- mantém `activeTaskId`, streaming/preview e parte do estado transitório no
  React;
- no cleanup do hook, chama `cancelTask(activeTaskId)`.

Além disso, `start_conversation_task`, mocks, Orchestrator e TaskGraph recebem
um `tauri::ipc::Channel<TaskEvent>` originado pela WebView.

A semântica histórica de segurança trata falha do event sink como
`channel_closed`, podendo transformar a tarefa em FAILED/cancelar trabalho.
Isso foi correto antes de existir um recovery path externo, mas impede Headless
real.

Portanto, PERF-1C **não pode** simplesmente interceptar o fechamento da janela.
Ela precisa primeiro separar:

> execução da tarefa

de:

> assinatura de eventos por uma WebView específica.

Esse é o centro técnico da fase.

## Contrato desejado

```text
Narys Native Runtime
├── TaskRegistry / Scheduler / Workers
├── Durable/Persisted state
├── Task Event Broker / observable task state
└── Presentation Host
    ├── 0 WebViews  ← Headless válido
    └── 1+ WebViews quando abertas
```

Uma WebView é subscriber/cliente. Ela não deve ser o proprietário necessário da
execução.

Fechar a Presentation remove a assinatura da UI; não destrói automaticamente a
tarefa quando a policy explicitamente permite continuidade headless.

## Continuidade e policy

A mudança da semântica de `channel_closed` deve ser explícita e limitada.

Não transformar todas as tarefas em "continue escondido" por padrão.

Criar um contrato/policy simples para distinguir pelo menos:

- tarefa **UI-bound**: perda da superfície continua terminal/fail-closed;
- tarefa **headless-safe**: pode continuar sem subscriber visual;
- tarefa que **requer atenção/aprovação**: deve pausar/aguardar ou bloquear a
  transição conforme capability existente.

Como approvals agentivos completos ainda não são capability de produto, a 1C
não deve inventá-los. Para capabilities atuais, provar continuidade com tarefas
que não exigem approval intermediário e documentar a fronteira.

## Event broker / observabilidade

Introduzir uma autoridade nativa apropriada para eventos/estado observável.

Requisitos:

- produtor de TaskEvent não depende diretamente da vida de uma WebView;
- subscriber pode conectar/desconectar;
- ausência temporária de subscriber não perde o estado terminal necessário para
  reidratação;
- sequência/ordem precisa ser determinística ou possuir cursor/sequence;
- eventos privados não devem virar broadcast global sem escopo;
- reabrir a UI consegue descobrir tarefa ativa/terminal e estado suficiente para
  reconstruir a Interaction;
- não duplicar provider call ou reiniciar tarefa ao reidratar;
- cancelamento continua endereçando o mesmo TaskId.

Não é obrigatório persistir todo chunk de streaming para sempre. É obrigatório
que a UI reaberta consiga reconstruir uma visão factual: por exemplo, carregar
mensagens já persistidas, estado da tarefa, resultado terminal e/ou continuar a
partir de um cursor seguro.

Se chunks ainda não persistidos forem impossíveis de recuperar durante o período
sem UI, documentar e resolver a UX sem fabricar texto.

## Conversation ownership

Retirar do cleanup React a autoridade de cancelar automaticamente uma tarefa
**headless-safe** apenas porque a WebView desmontou.

Preservar cancelamento em shutdown real e nas ações explícitas do usuário.

A Conversation deve conseguir, após reabertura:

- localizar/reassociar a sessão corrente quando aplicável;
- identificar TaskId ativo associado à sessão;
- mostrar estado running/completed/failed/cancelled factual;
- carregar histórico persistido;
- continuar recebendo eventos futuros sem criar nova execução.

Não mover toda a UI para Rust. Mover apenas a autoridade necessária para
sobreviver ao lifecycle da UI.

## Lifecycle do processo Tauri

Revisar o lifecycle real do Tauri 2 usado no projeto.

Implementar distinção explícita entre:

1. **Close Presentation / Headless**
   - destrói a WebView/janela principal;
   - mantém processo/Core apenas quando a policy permitir;
   - não mantém janela invisível.

2. **Reopen Presentation**
   - cria uma nova janela/WebView principal;
   - Economy continua default salvo se preferência persistida for Presence;
   - reidrata Interaction/observabilidade do Core.

3. **Quit Narys**
   - encerra processo;
   - cancela/fecha o que deve ser cancelado;
   - executa shutdown dos workers/recursos;
   - não deixa processo órfão.

Não confundir "close" e "quit".

## Recovery / reabertura

PERF-1C precisa de pelo menos **um mecanismo real e utilizável** para reconstruir
a UI depois que nenhuma WebView existe.

Pode ser tray, segunda ativação/single-instance, atalho global, notificação com
ação ou mecanismo equivalente.

A escolha deve considerar o Fedora/GNOME/Wayland real e não pode existir apenas
em teste sintético.

Evitar adicionar múltiplos mecanismos na mesma fase. Implementar o mínimo
confiável.

Se um mecanismo (por exemplo tray) não for utilizável no ambiente real, não
contá-lo como gate aprovado apenas porque compila.

## Janela principal

Centralizar a criação da janela principal em um helper nativo reutilizável,
preservando:

- Economy 1120×720 / mínimo 640×480;
- configuração necessária para Presence;
- CSP/capabilities;
- label `main`;
- aplicação de always-on-top;
- preferência persistida de Presentation.

A configuração declarativa do `tauri.conf.json` pode precisar mudar para que a
janela possa ser destruída e recriada de forma controlada.

Não duplicar configuração em vários locais sem teste de equivalência.

## Headless e Summary/background workers

Validar que SummaryWorker, Scheduler, TaskRegistry, ProviderRuntime,
continuations e outros workers administrados pelo Tauri continuam vivos sem
WebView.

Nenhum deles deve depender de timers JS da Economy Shell.

O polling operacional da 1B simplesmente deixa de existir quando a UI fecha;
isso é correto.

## Attention

Enquanto não existir sistema completo de approvals:

- tarefa que precise de input humano não deve avançar silenciosamente além do
  contrato atual;
- estado de atenção pendente deve ser observável ao reabrir;
- se houver notification/recovery mechanism disponível, pode sinalizar atenção
  factual sem conteúdo privado desnecessário.

Não implementar Voice/Adaptive/browser automation.

## Testes obrigatórios

Cobrir de forma determinística:

1. Core continua vivo com zero WebViews;
2. fechar Presentation não chama shutdown do Core;
3. tarefa headless-safe continua após subscriber/WebView desaparecer;
4. perda de subscriber não produz `channel_closed` para tarefa headless-safe;
5. tarefa UI-bound mantém fail-closed quando essa policy for usada;
6. cancelamento pelo mesmo TaskId funciona headless;
7. resultado terminal fica recuperável;
8. nova UI reidrata sem iniciar segunda tarefa/provider call;
9. criação → destruição → criação repetida da main não acumula windows/WebViews;
10. Quit encerra tarefas/workers segundo policy;
11. Economy/Presence continuam funcionando após reabertura;
12. nenhuma API DEV necessária no build de produção.

Preservar baterias PERF-1A/1B.

## Gate humano mínimo

No ambiente Tauri real:

```text
iniciar tarefa real
→ fechar Presentation
→ confirmar ausência de janela/WebView
→ tarefa continua
→ reabrir UI pelo mecanismo real
→ mesma sessão/TaskId/resultado observável
→ cancelar outra tarefa em headless ou após reopen
→ Quit
→ confirmar processo encerrado
```

Também confirmar que fechar a UI sem tarefa não deixa processo preso
indefinidamente contra a intenção do usuário; o comportamento deve ser explícito
na UX/policy.

## Performance

PERF-1C deve medir o estado realmente headless.

Registrar:

- RSS/CPU do processo nativo sem WebView;
- quantidade de WebViews/processos WebKit = 0 durante headless;
- comparação Economy vs Headless;
- custo de reabertura;
- ausência de renderer/canvas/GLB;
- nenhuma redução de policy cognitiva para obter economia.

Não exigir percentual arbitrário.

## Não objetivos

PERF-1C não deve:

- implementar PERF-1D/Auto;
- ativar Presence automaticamente;
- implementar NARYS-TERM/PTy;
- adicionar SpecialistAgents;
- adicionar novos providers;
- criar approval system completo;
- resolver toda a UX da Home;
- fazer NARYS-NORM;
- criar daemon de sistema/serviço de boot;
- suportar Android;
- manter janela invisível como "headless";
- alterar budgets cognitivos para reduzir CPU/RAM.

## Dívidas herdadas da 1B

As dívidas documentadas no fechamento da PERF-1B permanecem não bloqueantes.
Somente tratar uma delas na 1C se ela bloquear diretamente lifecycle sem WebView.

## Gate de PASS

PERF-1C só fecha quando:

1. o Core pode existir com **zero WebViews**;
2. pelo menos uma tarefa real/headless-safe continua sem subscriber visual;
3. UI pode ser recriada por mecanismo real;
4. reabertura reconstrói estado sem duplicar execução;
5. cancelamento continua correto;
6. Quit real encerra o processo e recursos previstos;
7. não há janela oculta/renderer como keep-alive;
8. ciclos repetidos não deixam WebViews/processos órfãos;
9. Economy e Presence não regrediram;
10. testes e medições estão registrados;
11. 1D não foi antecipada.

## Próxima etapa após PASS

> **PERF-1D — Adaptive Presence**

A 1D poderá então decidir quando usar Economy/Headless com base em policy e
histerese. Ela não deve começar antes de 1C provar que Headless é um estado real,
recuperável e seguro.
