# PERF-1C — Headless Runtime

**Estado:** AUDITORIA INDEPENDENTE TÉCNICA = PASS — aguardando gate humano final / decisão sobre dívidas
**Branch:** `perf-1c-headless-runtime`  
**Base:** `main@cc5bc8a50dcfc66f9c0b178b75df2cb7da05663d`  
**HEAD remoto verificado antes de editar:** `9d51c0b8a9e1cbbf9002917538aa7fc02ade04dd`
**Iniciada em:** 07/10/2026  
**Fase-mãe:** [PERF-1 — Adaptive Presence & Economy Mode](PERF-1-ADAPTIVE-PRESENCE.md)

O plano original abaixo permanece como referência de requisitos. A arquitetura
implementada, evidências e limites estão na seção de implementação candidata.

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

## Implementação candidata — arquitetura

O estado nativo e as tarefas permanecem no mesmo `App`, `TaskRegistry`,
`ProviderRuntime`, Scheduler, SummaryWorker, SecretStore e SQLite. A WebView
`main` é um host descartável. Nenhum poller operacional JS foi transferido para
Rust. Destruir a WebView encerra React, timers e recursos gráficos junto com ela.

### Policy e fronteira de segurança

`TaskAttachmentPolicy::{UiBound, HeadlessSafe}` pertence ao backend.
`TaskRegistry::register()` continua **UiBound por default**. Somente a
Conversation de produto é registrada explicitamente como HeadlessSafe: seu
contrato atual faz geração de texto/persistência, sem tools ou approvals.
Orchestrator/PlanV1, TaskGraph, continuation resume e diagnósticos permanecem
UiBound. Seus caminhos de `channel_closed`, Scheduler/ProviderError::EventSinkClosed,
retry/fallback fail-closed e testes históricos permanecem presentes.

Fechar toda a Presentation solicita cancelamento cooperativo também dos registros
UiBound, mesmo antes do próximo evento. HeadlessSafe conserva o mesmo controle de
cancelamento/TaskId. A perda de subscriber não altera o orçamento, prioridade,
modelo, thinking, contexto, output, admission, rate ou resilience.

Não existe nova autorização para tools ou ações sensíveis. Estados nativos de
TaskGraph pausado/continuation e os erros de provider que exigem ação continuam
sujeitos aos contratos anteriores. Esta fase não cria approvals/notificações.

### Broker da Conversation

`luna/events.rs` contém o sink nativo e um broker privado da Interaction:

- produtor chama `TaskEventSink`, independente da WebView;
- broker retém **apenas a última tarefa** da Conversation, ativa ou terminal;
- TaskId + session id delimitam a assinatura; nenhuma emissão global;
- existe um subscriber visual substituível da main, sem fan-out irrestrito;
- replay limitado a **128 eventos e 256 KiB serializados**; evento maior que esse
  limite não entra no replay;
- sequência monotônica, estado e evento terminal sobrevivem ao detach;
- replay + instalação do subscriber usam o mesmo lock do produtor;
- cursor inválido, task/session incompatíveis e sequência duplicada são rejeitados;
- falha de Channel remove o subscriber HeadlessSafe e não retorna channel_closed
  ao Scheduler; erros internos do sink continuam fail-closed;
- resposta completa permanece no SQLite, sem exigir streaming infinito em memória.

A abstração é limitada à capability que recebeu continuidade nesta fase. Não
houve reescrita de todos os workers para um framework genérico nem alteração
silenciosa dos Channels UiBound. O caminho de Conversation UiBound continua
exercitado pelos testes históricos e pelo teste explícito de policy.

### Reidratação e CurrentRunSessions

Os comandos exclusivos da capability `main-window` são
`get_current_interaction()` e
`attach_conversation_events(task_id, session_id, after_sequence, channel)`.
Eles observam/assinam; não criam TaskId, provider call ou tarefa.

A criação de sessão de produto formaliza **uma sessão selecionada por processo**:
criação/registro são protegidos pelo mesmo lock, e uma segunda criação é rejeitada.
`CurrentRunSessions::selected()` retorna a única sessão ou erro de ambiguidade;
reopen nunca escolhe um elemento arbitrário de múltiplas sessões. Histórico e
retomada explícita continuam no SQLite e substituem a seleção pelo contrato
existente. Início de tarefa, fechamento e retomada validam sessão/work ativo.

O hook React descobre a Interaction, recarrega mensagens e reanexa o TaskId.
Eventos recebidos durante o attach ficam em fila até o snapshot ser aplicado;
callbacks stale, task diferente e sequência repetida são descartados. O preview
histórico não é apresentado como prefixo completo: mostra-se factual
“Resposta em andamento…”, seguido apenas dos chunks novos. Completion recarrega
a resposta persistida; failed/cancelled apresentam o estado terminal factual.
Draft não enviado e escolhas puramente visuais continuam transitórios no React.

O cleanup do hook apenas invalida callbacks. Cancelamento exige ação explícita,
policy UiBound, Quit ou regra terminal existente. A tarefa cuja chamada IPC termina
depois de um unmount também não é cancelada pelo frontend.

### Lifecycle Tauri realmente usado

Inspecionados no Cargo.lock e código instalado: **tauri 2.11.6,
tauri-runtime-wry 2.11.4, wry 0.55.1**. Fontes:
[RunEvent 2.11.6](https://docs.rs/tauri/2.11.6/tauri/enum.RunEvent.html),
[WebviewWindow 2.11.6](https://docs.rs/tauri/2.11.6/tauri/webview/struct.WebviewWindow.html).

A main permanece como configuração autoritativa em `tauri.conf.json`, label
`main`, com `create:false`. `presentation::reopen()` usa
`WebviewWindowBuilder::from_config` no event loop, tanto no setup quanto na
segunda ativação. Não se duplica a configuração Economy 1120×720, mínimo 640×480,
transparência/decorations/resize/CSP. Always-on-top é reaplicado nativamente;
preferência Economy/Presence e layout são hidratados pelo frontend existente.
Core e workers não são reconstruídos.

Close Presentation remove o subscriber, cancela UiBound e chama **destroy()**
nas WebView windows de Presentation, inclusive settings auxiliares. Não usa
hide, opacity, off-screen ou janela keep-alive. O X nativo da main passa por
CloseRequested e solicita o mesmo fechamento. Settings continuam fecháveis
separadamente. A ausência só é considerada concluída após o evento nativo e
`app.get_webview_window("main") == None` / mapa vazio.

`App::run` recebe `ExitRequested`. Somente saída automática `code:None` é
prevenida, inclusive durante a espera cooperativa de Quit. Saída programática `Some(code)`
não é impedida. Não se transforma o processo em imortal.

Economy e Presence oferecem **Fechar interface** e **Sair da Narys**. O controle
X da Economy informa que Core continua e que abrir Narys novamente retorna.
Quit fecha admission de registros novos, cancela os controles ativos, sinaliza
SummaryWorker, aguarda tarefas/guards de persistência e SummaryWorker cooperativamente até 5 s e chama `app.exit(0)`. Summary
interrompido por shutdown volta a pending; recovery existente continua aplicável.
O limite de espera evita processo preso por worker que não responde. Não há
promessa de concluir toda persistência se um worker exceder esse prazo.

### Um mecanismo de reopen

Foi adicionado somente **tauri-plugin-single-instance = 2.3.7**, oficial, como
primeiro plugin. Sua segunda ativação agenda o helper no event loop. Se main já
existe, apenas pede foco; caso contrário cria uma main nova. Assim abrir o mesmo
executável/launcher Narys volta à UI mesmo com zero WebViews.

No Linux o plugin usa um serviço DBus da sessão e independe de tray GNOME,
AppIndicator ou global shortcut Wayland. A versão escolhida declara Rust 1.77.2;
não foi feita atualização global de Tauri/dependências. O lock inclui suas
dependências transitivas DBus: zbus 5.19.0 declara Rust 1.87, enquanto o
manifesto do produto declara 1.77.2. A validação desta candidata usa Rust 1.98.1
(Fedora); não atesta a toolchain mínima do manifesto. Essa incompatibilidade de
declaração é registrada explicitamente, sem alegar suporte 1.77.2. A dívida de
auditoria de MSRV da 1B permanece.
Referências: [plugin oficial](https://v2.tauri.app/plugin/single-instance/),
[API 2.3.7](https://docs.rs/tauri-plugin-single-instance/2.3.7/tauri_plugin_single_instance/).
A fonte instalada foi conferida; a documentação latest pode exigir toolchain
mais recente. Não há daemon, serviço de boot, atalho ou tray adicionais.

### Finding de WebKitGTK

O Drop de WebviewWrapper no tauri-runtime-wry 2.11.4 mantém o WebContext durante
o processo no Linux, citando [tauri #14626](https://github.com/tauri-apps/tauri/issues/14626).
WebKit pode conservar o **WebKitNetworkProcess** mesmo sem WebViews. Isso não é
uma janela invisível, renderer ou subscriber. O probe registra separadamente
WebView, WebKitWebProcess e WebKitNetworkProcess e verifica que ciclos não
acumulam processos. Não se mata um helper WebKit por PID como “otimização”.

O smoke com `destroy()` isolado mostrou renderers retidos: mapa Tauri vazio,
mas WebKitWebProcess continuava e RSS crescia a cada reopen. Esse resultado
foi rejeitado. No Linux, Close agora usa `with_webview` (callback no UI thread)
e [WebView::terminate_web_process](https://webkitgtk.org/reference/webkit2gtk/stable/method.WebView.terminate_web_process.html),
API pública desde WebKit 2.34, **antes** de enfileirar destroy. A dependência
Linux `webkit2gtk = 2.0.2` é a mesma binding já presente pelo Wry, com v2_38;
nenhuma biblioteca WebKit foi atualizada. O gate exige zero WebKitWebProcess
após cada close e zero WebViews. O network helper compartilhado continua
separado na evidência. O probe também registra requests reais do protocolo
de assets: WebKit não fornece todos eles em Resource Timing para tauri://,
então Presence é verificada por GLB HTTP 200 no protocolo + phase ready + canvas,
e Economy por ausência de requests GLB/chunk 3D + zero canvas.

### Probe e limites de evidência

`scripts/perf1c-native-probe.py` inicia um binário release com feature explícita
`perf1c-probe`, dados/identidade/credenciais sintéticos isolados e DBus próprio.
A feature é excluída do build normal. A fixture usa **Groq adapter HTTP/SSE real,
Conversation, preflight, SecretStore/Stronghold, Scheduler e SQLite reais** com
endpoint loopback; não envia chamadas comerciais nem usa a credencial do usuário.
As WebViews são Tauri/Wry reais com o frontend de produção embutido.

O driver envia pela UI React real, destrói Presentation, observa tarefa sem
WebViews, abre o mesmo executável (plugin real), verifica reattach factual,
conclui sem UI, carrega o resultado persistido, alterna Economy/Presence em
novas WebViews, repete ciclos, cancela pelo mesmo TaskId, executa SummaryWorker
sem UI e solicita Quit com outra tarefa ativa. Contadores de provider e
endereços das instâncias nativas detectam reinício/duplicação do Core.

A instrumentação usa um poller de arquivo **somente no build de probe**, sem
necessidade no produto. A permissão de relatório DOM é adicionada dinamicamente
somente nessa feature e só para main. Nenhum comando de diagnóstico do probe é
registrado no build normal. As medições têm esse pequeno overhead nativo em
ambos os estados. Foco físico, composição/driver, uso humano e provider
comercial não são atestados pela fixture.

O enum visual DEV da 1A foi renomeado de `headless` para `detached`. Mantém a
cobertura de teardown sem chamar uma WebView viva de Headless. Headless real
não é PresentationMode; preferências de produto continuam economy|presence.

### Reprodução

```bash
npm run typecheck
npm run build
node scripts/test-presentation-lifecycle.cjs
node scripts/test-economy-shell.cjs
node scripts/test-headless-interaction.cjs
node scripts/test-provider-operations.cjs
node scripts/test-provider-operations-dom.cjs
node scripts/test-allocation-settings.cjs
cargo check --manifest-path src-tauri/Cargo.toml
cargo check --release --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=2
cargo build --release --features perf1c-probe --manifest-path src-tauri/Cargo.toml
python3 scripts/perf1c-native-probe.py --output /tmp/narys-perf1c-native.json
cargo build --release --manifest-path src-tauri/Cargo.toml
```

Compilação/testes e medições são etapas separadas. Em hardware com pouca RAM,
`CARGO_BUILD_JOBS=2` limita compiladores; isso não altera Scheduler de produto.

### Gates humanos restantes

- auditoria independente do diff, policy, capabilities, replay e evidências;
- Conversation com provider comercial autorizado: fechar, abrir pelo launcher,
  mesma session/TaskId, completion e cancelamento, sem alterar policies;
- uso físico do launcher no Fedora/GNOME/Wayland, transparência/Idle/Wave em
  Presence após reopen e clareza dos controles Fechar interface/Sair;
- endurance mais longo, foco/oclusão, plataformas/DPI diferentes;
- encerramento com integrações agentivas externas, quando estiverem em uso.

As dívidas da 1B permanecem. NARYS-TERM, NARYS-NORM e PERF-1D não foram iniciadas.
Esta entrega não declara PASS.

### Finding do build direto por Cargo

O primeiro smoke nativo não carregou React: `generate_context!` usa modo dev
quando tauri/custom-protocol não está habilitado, inclusive em `cargo build
--release`. O manifesto anterior não habilitava essa feature; sem Vite em
localhost:5173 a main existia, mas não era prova da UI de produção. Essa amostra
foi descartada, sem receber gate ou métrica válida.

O manifesto agora define `custom-protocol = ["tauri/custom-protocol"]` como
feature default, permitindo que os comandos Cargo exigidos embutam `dist`.
O [CLI Tauri 2.11.5](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.5/crates/tauri-cli/src/interface/rust.rs)
remove default features que ativam esse protocolo em `dev_options` e habilita
o protocolo em build. Assim `npm run tauri dev` conserva Vite/HMR. A verificação
nativa exige DOM e reattach observados, além de janela presente; a existência
de processos/WebViews, sozinha, não atesta frontend carregado. As evidências
históricas da 1B não são reutilizadas como comparador da 1C.


### Cobertura e validação final

| Cobertura solicitada | Evidência |
| --- | --- |
| 1–3: detach/quebra de subscriber, HeadlessSafe e fail-closed UiBound | testes do broker, Conversation real/preflight e suites históricas de event sink |
| 4–6: mesmo TaskId cancelável, terminal recuperável e subscriber novo | TaskRegistry, broker bounded/scoped, reattach + SQLite e cancelamento nativo |
| 7–9: nenhuma provider call/TaskId duplicada; sessão correta | fixture de runtime, hook React real e probe Tauri com contagem HTTP/CurrentRunSessions |
| 10–14: destroy, Core vivo, exatamente uma main, ciclos e Quit | probe nativo: oito fechamentos, sete reaberturas, zero WebViews/WebKitWebProcess em todos os fechamentos, saída 0 e nenhum helper vivo restante |
| 15–17: Economy sem 3D, Presence após reopen, regressões 1A/1B | grafo estático, boot contracts, WebKit lifecycle/Shell, assets nativos e Presence phase ready |

Validação de produto concluída com os comandos exigidos:

- `npm run typecheck` e `npm run build`: exit 0; chunk 3D permanece opt-in;
- seis scripts Node da lista de reprodução: exit 0; lifecycle com 20 ciclos e
  seis falhas parciais/retry; allocation settings com 85 checks;
- WebKit production Shell limpo e layout restaurado: exit 0;
- WebKit DEV lifecycle (20 ciclos + erro/retry) e boot contracts Economy/Detached:
  exit 0;
- `cargo check` debug e release: exit 0;
- `cargo test -- --test-threads=2` final após teardown Linux: **983 aprovados,
  0 falhas, 2 ignored**, 512,82 s; main e doctests com zero testes/falhas;
- baseline Core da 1A permanece incluída nessa suite (cinco chamadas e dez turns);
- `cargo build --release` de produto: exit 0; build explícito de probe: exit 0;
- sintaxe Python e `git diff --check`: sem erros.

Os dois ignored são os gates Codex autenticado/handshake preexistentes; nenhum
foi acrescentado para a 1C. Rust apresenta 16 warnings debug/39 release no build
normal (incluindo a policy usada pelos testes), e Vite mantém o warning do chunk
3D >500 KiB. Não foram removidos testes nem reduzidas policies para validar.

O smoke estrutural já percorreu o gate nativo inteiro. Suas amostras de 1 s com
outros testes em execução não são medições finais de performance. O driver de
probe foi corrigido para serializar registros JSONL concorrentes de assets/IPC
e ler o acknowledgment antes de verificar morte do processo: Quit rápido pode
já ter terminado legitimamente. Continuam obrigatórias a saída 0, a persistência
da tarefa cancelada e a ausência de helpers vivos; não se ignora falha de Quit.


### Medições nativas observadas — 07/10/2026

Dados brutos: [PERF-1C-OBSERVED-EVIDENCE.json](PERF-1C-OBSERVED-EVIDENCE.json).
A execução final completa retornou **exit 0**, com todas as assertions de
continuidade, cancelamento, Summary, assets, ciclos e Quit satisfeitas. Isso é
resultado do probe; **não é PASS da fase**, que aguarda auditoria independente.

Mesmo binário release de probe, mesmo processo/Core, frontend de produção
embutido, Fedora/GNOME/Wayland e `LIBGL_ALWAYS_SOFTWARE=1`. Sem compiladores,
Vite ou suite Rust em execução durante as quatro amostras. Ordem: Economy →
Headless → Headless → Economy; 10 s de aquecimento + 30 s de amostragem em cada
estado. Não houve churn de processos nas amostras. RSS soma `/proc` de Narys e
descendentes (não é PSS, portanto pode contar páginas compartilhadas mais de
uma vez). CPU é percentual de um núcleo, integrado por ticks dos mesmos PIDs.

| Estado/amostra | RSS agregado mín–máx (MiB) | CPU idle (% de um núcleo) | WebViews | WebKitWebProcess | WebKitNetworkProcess |
| --- | ---: | ---: | ---: | ---: | ---: |
| Economy 1 | 528,30–532,62 | 6,467 | 1 | 1 | 1 |
| Headless 1 | 224,01–224,01 | 0,267 | 0 | 0 | 1 |
| Headless 2 | 224,01–224,01 | 0,267 | 0 | 0 | 1 |
| Economy 2 | 529,16–533,42 | 6,400 | 1 | 1 | 1 |

RSS final médio da Economy: 530,44 MiB; Headless: 224,01 MiB, **57,77% menor**.
CPU médio Economy: 6,4335%; Headless: 0,267%, **95,85% menor** nesta fixture.
Não se generaliza essa diferença para workload de provider ativo ou outros
GPUs: são amostras idle de uma máquina, com renderização software, foco físico
não atestado e pequeno overhead do poller exclusivo de probe nos dois estados.
Não se reutiliza baseline histórica da 1B como comparador.

Árvore Headless em **cada um dos oito fechamentos**: Narys + um
WebKitNetworkProcess; **zero WebKitWebProcess e zero WebViews**. Economy:
Narys + um NetworkProcess + um WebProcess. Nenhuma janela invisível/keep-alive.
Economy não solicitou GLB/chunk 3D; Presence reaberta solicitou GLB status 200,
criou um canvas e atingiu ready (load com Idle/Wave validado pelo runtime).
Em Headless não existe DOM, canvas, Three.js ou renderer executando.

As sete reaberturas por segunda ativação levaram **0,979–1,551 s**, mediana
**1,041 s**, do lançamento do segundo executável ao bootstrap/observação da
UI. Esse custo inclui DBus, criação da nova WebView e cliente IPC; não promete
GLB/Presence totalmente pronta nesse mesmo instante. TaskRegistry e
ProviderRuntime conservaram seus endereços/instâncias. A primeira tarefa
conservou sessão/TaskId e exatamente uma chamada; as quatro chamadas totais
foram resposta concluída, resposta cancelada, Summary e resposta cancelada por
Quit. Não houve chamadas duplicadas por reattach.

Resultado terminal e resumo foram confirmados no SQLite. Quit com tarefa
ativa persistiu cancelled, terminou com exit 0 e não deixou helpers vivos.
O arquivo de evidência registra SHA-256 do binário instrumentado e do build
normal; o build normal não contém as variáveis/endpoints de ativação do probe.

**PERF-1C IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente**.


## Auditoria independente técnica — 07/10/2026

**Resultado:** **PASS técnico. Nenhuma FIX de arquitetura Headless é exigida.**

A auditoria independente revisou o commit
`09b2052864119670f541ae2d0b442370cac6da48`, o diff contra o plano inicial,
o broker, lifecycle Tauri, Conversation, SummaryWorker, capabilities, probe nativo
e evidências registradas.

### Broker e policy

O desacoplamento principal está correto:

- `TaskAttachmentPolicy::UiBound` permanece default;
- somente Conversation de produto entra explicitamente como
  `HeadlessSafe`;
- `TaskEventSink::UiBound` continua propagando `channel_closed`;
- `TaskEventSink::HeadlessSafe` retém estado/eventos no broker e remove somente
  o subscriber visual quando o Channel falha;
- broker é escopado por TaskId + session id;
- sequência monotônica rejeita duplicação/out-of-order;
- replay é bounded por 128 eventos / 256 KiB;
- gap de replay é sinalizado por `replay_complete=false`;
- terminal permanece observável mesmo quando evento grande não cabe no replay;
- subscriber novo não cria tarefa/provider call.

A perda da WebView não virou autorização global para execução oculta:
Orchestrator, TaskGraph, diagnósticos e demais registros continuam UiBound.

### Conversation e reidratação

A autoridade de cancelamento foi corretamente retirada do cleanup React para a
capability HeadlessSafe.

O fluxo de recovery:

1. obtém a única sessão selecionada do processo;
2. carrega SQLite;
3. observa o TaskId já existente;
4. anexa novo subscriber;
5. não chama `start_conversation_task`;
6. descarta callbacks stale/task errado/sequência duplicada;
7. recarrega resposta persistida no terminal.

O tratamento de replay truncado é conservador: não apresenta chunks antigos como
prefixo completo; mostra estado factual e só concatena eventos futuros.

`CurrentRunSessions::selected()` falha em ambiguidade, evitando escolher sessão
arbitrária.

### Lifecycle Headless

`tauri.conf.json` usa `create:false` e `presentation::reopen()` cria a main
a partir da configuração autoritativa.

Close Presentation:

- desanexa subscriber;
- cancela somente registros UiBound;
- destrói todas as WebViews de Presentation;
- no Linux termina explicitamente o WebProcess antes de `destroy()`;
- não usa hide/minimize/off-screen como keep-alive.

Reopen:

- usa o plugin single-instance como único mecanismo;
- cria/foca exatamente uma main;
- preserva instâncias do Core/TaskRegistry/ProviderRuntime;
- mantém Economy/Presence por preferência persistida.

Quit:

- bloqueia novos registros;
- solicita cancelamento;
- sinaliza SummaryWorker;
- aguarda cooperativamente com deadline de 5 s;
- usa saída programática para não ser retida pelo keep-alive.

A distinção Close / Reopen / Quit está materializada, não apenas documentada.

### Evidência nativa

O probe usa WebViews Tauri/Wry reais e frontend de produção embutido. A fixture
de provider é loopback, mas atravessa adapter Groq, preflight, SecretStore,
Scheduler, SQLite e Conversation reais.

A auditoria considera a evidência suficiente para o gate técnico:

- oito estados fechados com zero WebViews e zero WebKitWebProcess;
- sete reopens por segunda ativação;
- TaskRegistry/ProviderRuntime preservados;
- mesma session/TaskId;
- uma única provider call para a tarefa reanexada;
- completion sem UI;
- cancelamento do mesmo TaskId;
- SummaryWorker concluindo sem UI;
- Quit com tarefa ativa persistindo cancelled e exit 0;
- nenhum helper da árvore observada sobrevivendo ao Quit;
- Economy e Presence reabertas sem regressão detectada.

A permanência de um `WebKitNetworkProcess` compartilhado não equivale a uma
WebView oculta e não invalida o gate Headless. O requisito relevante é ausência
de janela/WebView/renderer/WebProcess de renderização.

### Performance

As duas rodadas Economy e duas Headless são comparáveis dentro do mesmo binário
release/probe e mesmo processo/Core.

A evidência registra aproximadamente:

- Economy: 530,44 MiB RSS médio / 6,4335% de um core;
- Headless: 224,01 MiB RSS / 0,267% de um core;
- zero WebViews/WebKitWebProcess no estado Headless;
- redução observada de ~57,77% de RSS e ~95,85% do CPU idle nesta fixture.

Esses valores permanecem específicos desta máquina, software rendering e janela
de medição; não são promessa universal.

### Dívida de MSRV / dependency resolution

O manifesto do produto ainda declara `rust-version = "1.77.2"`.

A introdução de `tauri-plugin-single-instance = 2.3.7` resolveu no lock Linux
`zbus 5.19.0`, cuja crate declara MSRV Rust 1.87. Portanto o MSRV efetivo da
árvore Linux atual é maior que o manifesto.

Isso **não invalida a implementação Headless validada no ambiente atual
Rust 1.98.1**, mas é uma inconsistência real de build contract.

Tratamento recomendado, fora do gate arquitetural da 1C:

- decidir se Narys ainda promete Rust 1.77.2;
- se sim, controlar/pinar uma resolução compatível e validar a toolchain;
- se não, atualizar o `rust-version` para o mínimo efetivamente suportado e
  registrar a mudança.

Não mascarar o problema apenas porque o toolchain local é mais novo.

### Riscos/dívidas restantes

Permanecem como gate humano ou dívida, não como falha técnica comprovada:

1. provider comercial autorizado em Conversation Headless;
2. launcher físico GNOME/Wayland em vez do segundo binário controlado pelo probe;
3. Presence real após reopen com inspeção humana de transparência/Idle/Wave;
4. endurance mais longo;
5. corrida rara close ↔ segunda ativação;
6. comportamento de falha parcial ao destruir múltiplas janelas;
7. MSRV efetivo da árvore Linux;
8. packaging sandboxado futuro (Flatpak/Snap) precisará declarar acesso DBus para
   single-instance.

### Decisão

`PERF-1C = PASS TÉCNICO / AGUARDANDO GATE HUMANO FINAL OU DECISÃO EXPLÍCITA DE
CONVERTER OS GATES RESTANTES EM DÍVIDA`.

Não avançar automaticamente para PERF-1D sem fechamento documental da 1C.
