# PERF-1D — Adaptive Presence

**Estado:** PERF-1D IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente

Branch: `perf-1d-adaptive-presence`. Base: `main@e998bcee7f586effad1d8c8245d14f3fa4c020ef`.
Workspace inicialmente limpo e HEAD conferido antes de editar. A/B/C permanecem
PASS. Esta é a quarta e última implementação formal; PERF-1 não está encerrada.
Não há PR/merge nem nova subfase.

## Objetivo e fronteira

Uma autoridade nativa de policy/orquestração reutiliza as capacidades auditadas:
Presence lazy/opt-in, Economy DOM/CSS e PresentationHost descartável da 1C.
`PresentationController` permanece **somente economy/presence/detached** e não
recebe policy, timer, detector de idle ou scheduler. Headless é ausência real de
WebViews; `detached` continua sendo apenas o harness visual DEV da 1A.

`adaptive.rs::AdaptivePresentationManager` contém policy persistida, estado do
host, geração da janela (`epoch`), revisão da superfície, foco, guard, decisão
pendente, transição em andamento, atenção, recuperação pendente e ring local.
SQLite é autoridade da preferência; React é autoridade de seu draft transitório;
Core permanece autoridade de tarefas, sessões, providers e cancelamento.

## Semântica das quatro policies

| Policy persistida | Cold start | Segunda ativação explícita | Background |
| --- | --- | --- | --- |
| Economy (default) | Economy | Economy | Nunca fecha automaticamente |
| Presence | Presence, lazy após opt-in persistido | Presence | Nunca desmonta automaticamente |
| Headless | Core, **zero WebViews** | Economy temporária, sem alterar policy | Controle temporário permanece até Close manual |
| Auto (opt-in) | Economy | Economy | Pode destruir Presentation após blur seguro e delay |

Close Presentation é transitório e não grava policy. Economy/Presence retornam
à preferência na segunda ativação. Headless manual reabre Economy temporária;
não existe leitura da preferência que feche imediatamente a janela de controle.
Attention usa Economy como recuperação. Uma Presence manual já aberta não é
substituída por heurística. A ativação explícita posterior usa a preferência.

**Auto NUNCA seleciona Presence**, nem por foco, resposta de provider, atenção,
atividade ou reopen. Escolher Auto enquanto Presence está montada publica
Economy, desmonta 3D pelo lifecycle existente e exige acknowledgment do frontend
antes de permitir auto-close. Somente escolher Presence explicitamente autoriza
3D no caminho de produto. Os controles visuais antigos permanecem DEV.

## Migration e persistência

Migration **018 / schema 18** adiciona `presentation_policy`, com CHECK para as
quatro opções. Copia `presentation_mode` economy/presence sem transformar opt-in
existente em Auto. Instalação nova permanece Economy. `presentation_mode`
continua estritamente economy/presence; não virou enum de policy. Writes de
policy atualizam os dois campos em uma única instrução SQLite e não sobrescrevem
layout, settings gerais ou cognição. O modo retornado à UI é a superfície atual
do manager, inclusive Economy temporária/attention, e não um opt-in legado que
possa ativar 3D indevidamente nessa recuperação.

A migration tolera fixtures antigas que rebaixam user_version mantendo tabelas:
se a coluna já existe, preserva a preferência. Upgrade real schema 17 e defaults
estão cobertos separadamente. Versão futura continua rejeitada. Identifiers,
paths, vault, namespace e `rust-version` não mudaram.

## Regra determinística, triggers e hysteresis

Um `WindowEvent::Focused(false)` da **incarnação atual** agenda exatamente um
sleep nativo de **30 s** (`AUTO_HEADLESS_DELAY`). Duplicar blur não duplica timer.
`Focused(true)` cancela o handle e invalida seu token. O Adaptive não adiciona interval ou polling
JavaScript; o poller operacional preexistente da Economy permanece na UI e morre
com a WebView. Não há monitor global de teclado/mouse, score, ML ou chamada cognitiva.
APIs conferidas na crate instalada Tauri 2.11.6 e na
[documentação oficial de WindowEvent](https://docs.rs/tauri/2.11.6/tauri/enum.WindowEvent.html).

O timeout só propõe auto-close quando **todas** são verdadeiras:

1. policy Auto e host Economy;
2. main existe e é a única WebView window;
3. foco ausente há pelo menos 30 s;
4. nenhuma task UiBound ativa;
5. nenhuma atenção pendente;
6. UI acknowledged nesta epoch/revisão e sem guard;
7. nenhum close/reopen em andamento e runtime não está saindo;
8. token ainda pertence à decisão atual.

Depois do timeout, um handshake confirma o guard **atual** de React e repete os
fatos nativos. Só então o mesmo PresentationHost da 1C termina o WebProcess
Linux e destrói a WebView. Não se chama hide/minimize para simular Headless.
Se o timeout encontrar um blocker, a decisão é abandonada. Remover blocker
não reinicia automaticamente o timer: exige um novo foco → blur e delay inteiro.
Essa escolha conservadora evita polling e timers de retry nesta primeira versão.
Após qualquer reopen é necessário perder foco novamente e aguardar o delay
completo. Foco de compositor físico permanece no gate humano; o probe chama o
mesmo handler com foco controlado, sem inventar uma variante de WindowEvent.

## Proteção de trabalho local e UiBound

`useAdaptiveGuard` reporta somente booleano em vazio → não vazio / não vazio →
vazio, mudança de superfície ou incarnation. Nenhum texto/keystroke é enviado.
O guard cobre draft com qualquer comprimento (inclusive espaços), confirmação de
retomada que depende desse draft e a fase de Interaction ocupada sem TaskId
(preflight/envio/seleção de sessão). Conversation com TaskId HeadlessSafe em
execução não bloqueia Auto.

O acknowledgment de close lê a referência síncrona atual da Interaction;
`document.body.inert` bloqueia novas edições durante a checagem final nativa.
Rejeição, erro ou invalidação libera input; sucesso destrói a própria WebView.
Uma UI que não carregar/assinar/acknowledge não fornece autorização de teardown:
permanece fail-closed. Gerações antigas não liberam o guard de uma UI nova.

Settings/qualquer janela auxiliar bloqueiam por contagem nativa. Não existe nova
edição crítica na main além dos estados descritos. Preview de splitter é um gesto
visual descartável; o layout só é gravado no commit existente.

`TaskRegistry::has_ui_bound_work()` consulta fatos. `suspend_ui_if_safe()` reserva
a retirada da UI sob o mesmo lock de admission: não se pode admitir uma task
UiBound entre a checagem e o close. Nenhum lock atravessa operação nativa ou
await. Reopen libera essa reserva. HeadlessSafe pode continuar/registrar;
UiBound permanece default e mantém fail-closed da 1C. Não há reclassificação de
Orchestrator, TaskGraph, diagnostics ou continuations.

## Transições, preferência e shutdown

Mutations de host/policy são serializadas no event loop Tauri, inclusive comandos
assíncronos, single-instance e callbacks de timer. Locks protegem apenas estado
curto; nenhuma operação longa fica sob mutex. `transitioning`/`closing` continuam
ativos até a destruição factual, em vez de concluir no enqueue de `destroy()`.
Reopen durante close fica pendente e usa o mesmo helper depois de Destroyed.
Incarnation nos callbacks de cada janela rejeita foco/destruição stale.

Mudança de policy invalida token/timer antes da nova decisão. Persistência
precede close em Headless manual. Falha imediata do host restaura a preferência
anterior ou retorna erro explícito de rollback. A UI recarrega a preferência ao
receber erro, sem afirmar sucesso. Ack de Economy precisa corresponder à revisão
atual; a convergência visual incompleta bloqueia Auto.

ExplicitActivation cancela auto-close pendente e marca foco/intenção nova.
Quit invalida timer, handshake e recuperação antes do shutdown cooperativo já
existente; o host não cria outra janela depois de Quit. Close nunca solicita
shutdown de HeadlessSafe. As mesmas instâncias Core/TaskRegistry/ProviderRuntime
permanecem vivas, e os comandos de reattach continuam read-only em relação a
execução: não criam TaskId nem provider call.

## Attention contract

Razões allowlisted: `approval_required`, `task_failed`, `user_input_required`.
`request_attention(AppHandle, AttentionReason)` pode ser chamado por um caller
nativo fora do event loop; despacha `require_attention` no event loop. Sem
prompt, conteúdo, secret ou payload arbitrário. Snapshot expõe somente a razão.

Headless + atenção chama o mesmo `PresentationHost::reopen` usado por
single-instance, com reason `AttentionRequired` e superfície **Economy**.
A razão fica latched, bloqueando auto-close até reconhecimento explícito na UI;
não tem expiry silenciosa. Reconhecer atenção não aprova ferramenta/execução.

**Não há caller de aprovação em produção**, pois essa capability ainda não
existe. Nesta candidata, os sinais de atenção são gates sintéticos do contrato
nativo, usando as mesmas functions de produção. Nenhum approval framework ou
approval falso foi criado. A futura integração de tools/agents deve solicitar
atenção pela API e gerenciar seu próprio estado/autorização. Erros de Conversation
continuam recuperáveis pela 1C; não foram acoplados ao manager só para criar um
caller artificial.

## Telemetria local

Ring **64** em memória; registro `{from,to,reason,policy,timestamp}` com estados,
policy e reasons enum: startup, user_policy, explicit_activation,
auto_background_timeout, attention_required, manual_close, quit. Timestamp UTC
em milissegundos. Não existe string livre ou conteúdo de conversa nesse registro.
Snapshot read-only contém guard/foco, epoch/revisão, transição, atenção,
`pendingToken`, `timerActive` e histórico bounded. Não há SQLite logger, sampler
permanente ou broadcast de TaskEvents privados. Os novos comandos de guard/ack
de close são concedidos somente à main; settings pode mudar policy/ler snapshot.

## Probes e validação

`scripts/perf1d-native-probe.py` usa feature explícita `perf1d-probe`, frontend
embutido e WebViews Tauri/Wry reais. Reutiliza a infraestrutura isolada da 1C
(Stronghold synthetic key, SQLite, Groq adapter HTTP/SSE loopback, DBus próprio),
mas possui driver de cenários separado. Os hooks adicionais ficam em
`perf1d_probe.rs` e não são compilados no produto normal. Só o probe possui poller
de arquivo/clock controlado; o delay do produto não recebe env override.

A primeira transição atravessa os 30 s reais. As demais adiantam a deadline da
mesma decisão; não substituem host/guards/React. Attention ApprovalRequired e
UserInputRequired são injetadas sem fingir execução/aprovação de tools. UiBound
sintética testa admission factual. Conversation usa runtime/provider adapter
reais com endpoint local, sem API comercial ou credencial do usuário.

Reprodução:

```bash
npm run typecheck
npm run build
node scripts/test-presentation-lifecycle.cjs
node scripts/test-economy-shell.cjs
node scripts/test-headless-interaction.cjs
node scripts/test-adaptive-presentation.cjs
node scripts/test-adaptive-policy-hydration.cjs
node scripts/test-provider-operations.cjs
node scripts/test-provider-operations-dom.cjs
node scripts/test-allocation-settings.cjs
cargo check --manifest-path src-tauri/Cargo.toml
cargo check --release --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=2
cargo build --release --features perf1d-probe --manifest-path src-tauri/Cargo.toml
python3 scripts/perf1d-native-probe.py --output /tmp/narys-perf1d-native.json
python3 scripts/perf1c-native-probe.py --seconds 1 --output /tmp/narys-perf1d-regression-1c.json
cargo build --release --manifest-path src-tauri/Cargo.toml
```

As medições/probes nativos devem rodar sem compilação/testes concorrentes.
Os probes WebKit de 1A/1B continuam aplicáveis. Evidência local estruturada:
[PERF-1D — observed evidence](PERF-1D-OBSERVED-EVIDENCE.json).

## Ciclos e performance observados

Coleta Linux/Tauri/Wry com software rendering, DBus/data isolados e SSE HTTP
local. Nenhuma compilação/suíte concorrente durante a medição. Foco foi
controlado pelo handler nativo; foco físico do compositor ainda depende do gate
humano. Não é benchmark de provider comercial nem validação cross-platform.

**15 ciclos completos** Auto Economy → Headless → Economy, alternando attention
e single-instance. Em cada Headless: zero WebViews, zero WebKitWebProcess,
nenhum pending token/timer/transição. Em cada retorno: uma main/um renderer,
mesmos ponteiros TaskRegistry/ProviderRuntime, nenhuma chamada de provider nova.
O helper WebKitNetworkProcess persistente permitido pela 1C permaneceu único;
não é WebKitWebProcess de renderização. Quit encerrou o processo (exit 0) sem
helpers órfãos. Cold start Headless separado iniciou sem construir WebView.

| Medida | Observado |
| --- | --- |
| Primeiro auto-close, delay real | **30.293,08 ms** |
| Reopen attention até Economy hidratada/guard ack (10 amostras) | mediana **1.086,105 ms**, 926,30–1.235,29 ms |
| Reopen explicit activation (12 amostras, inclusive Presence manual/cold control) | mediana **894,32 ms**, 872,00–1.004,04 ms |
| Auto-close com deadline controlada (19 amostras; não latência do delay real) | 257,81–297,75 ms |
| Ring após ciclos / maior snapshot do driver / limite | **45 / 52 / 64**; overflow 200 registros → 64 em teste |
| Provider calls no driver completo | **2**: uma Conversation concluída e outra cancelada por Quit; zero duplicação por transição |

RSS da árvore (`native + helpers`), em estados separados depois de aquecimento:

| Estado | Antes dos ciclos | Depois dos ciclos | Native antes → depois |
| --- | --- | --- | --- |
| Economy | **555,80 MiB** | **558,41 MiB** | 192,82 → 194,89 MiB |
| Headless | **292,20 MiB** | **243,34 MiB** | 236,08 → 186,85 MiB |

Não foi observado crescimento grosseiro na coleta curta. A oscilação/alocação
do native após Presence e tarefas impede interpretar a queda como ganho causal
do manager. RSS não prova ausência absoluta de leak; endurance prolongado segue
dívida. Não se estabeleceu percentual arbitrário. CPU idle extensa não foi
repetida porque os runtimes A/B/C não foram reconstruídos; Auto acrescenta um
sleep cancelável por blur, sem polling contínuo ou timers duplicados. Os pollers
de arquivo do harness e SSE heartbeat pertencem apenas à coleta isolada.

## Matriz dos testes mínimos

| Requisitos | Prova |
| --- | --- |
| 1–3: migration Economy/Presence/default | `adaptive_tests::migration_preserves_legacy_opt_in_new_default_and_reopen` + suites históricas SQLite |
| 4–7, 21: estabilidade manual, Auto Economy, nenhum opt-in implícito | testes da Machine, hydration stale, grafo de produção e probe nativo Presence/Auto |
| 8–10: blur, focus cancel e policy/token stale | clock `Instant` controlado nos testes; timer real e expiry hook no probe |
| 11–14: UiBound, HeadlessSafe, draft e settings | TaskRegistry/reserva atômica; hook real boolean/event-driven; blockers na WebView nativa |
| 15–20, 22: teardown factual, attention, ativação explícita e preferência temporária | probe Tauri/Wry, plugin DBus single-instance, cold start Headless e reattach |
| 23: Quit | invalidação determinística + Quit com tarefa ativa e Quit com timer pendente no probe |
| 24–25: ring bounded/privacidade | 200 registros no teste nativo; schema estrito de cada registro; ausência de conteúdo no guard e snapshot |
| 26–28: ciclos, subscriber, mesma tarefa e calls | ciclos nativos; broker conserva no máximo um subscriber (`Option<Channel>`), detach/reattach da 1C; contadores HTTP e ponteiros de instância |
| 29: regressão A/B/C | três scripts oficiais, probes WebKit lifecycle/boot/Shell, probe nativo da 1C e suíte Rust integral |
| 30: produto sem harness | grafo/stripping frontend; handlers cfg de probe; build normal release e inspeção do binário |

## Resultados da validação final

- `npm run typecheck` e `npm run build`: exit 0.
- Os oito scripts Node da reprodução acima: exit 0, incluindo lifecycle,
  Economy, Headless Interaction, guards/hydration Adaptive, provider operations,
  DOM e allocation settings. Nenhum teste anterior foi removido. A assertion
  antiga de que settings não oferece Auto/Headless foi substituída pelo novo
  contrato das quatro policies, mantendo a rejeição dessas policies no
  `PresentationController` concreto.
- `cargo check` debug/release: exit 0. `cargo test -- --test-threads=2`:
  **988 passed, 0 failed, 2 ignored**, 360,56 s. Os dois ignores são anteriores
  e exigem Codex app-server/autenticação local; nenhum ignore foi adicionado.
  As cinco novas provas Rust e todas as assertions de migration passaram.
- Probe WebKit 1A: 20 ciclos e teardown/falha parcial/retry, além dos boots
  Economy/detached. Probe WebKit 1B production: boot Economy sem contexto/frame
  3D, preferências restauradas e override DEV ignorado. Todos retornaram pass.
- Probe nativo 1D: cenário completo + 15 ciclos + cold start Headless, pass.
  Probe nativo original 1C: pass; Conversation e Summary sem WebView,
  reattach/cancel e Quit corretos, quatro calls previstas do fixture, exit 0 e
  nenhum helper órfão. A amostragem idle de 1 s dessa regressão não fundamenta
  uma comparação de CPU.
- `cargo build --release --features perf1d-probe` e build release normal sem
  features de probe: exit 0. O binário normal não contém os markers de
  ambiente/clock do harness; o frontend de produção também passou o stripping
  DEV. SHA-256 normal:
  `1ada1db72ae262be498025b570af2be83d7e2b3e9e13e4c8a19ef5d23355538d`.

Toolchain da coleta: Rust/Cargo **1.98.1**, Fedora Linux. `rust-version` declarado
permanece **1.77.2**; sua inconsistência prévia não foi corrigida nesta fase.
Build frontend mantém o warning do chunk lazy 3D >500 kB. Checks Rust mantêm
warnings de dead code (inclusive hook de attention sem caller atual); não foram
mascarados nem tratados por mudanças fora do escopo.

## Findings de execução registrados

- A primeira suíte Rust integral terminou com 982 testes passando e seis
  assertions históricas de schema falhando (`18 != 17`). Foram atualizadas
  somente essas expectativas de versão; as verificações de preservação de
  policies, ledger, checkpoints, histórico e rollback permaneceram intactas.
  Nenhuma policy cognitiva mudou e nenhum ignore foi adicionado.

- O primeiro gate de Auto chegou a zero WebViews com Conversation running, mas
  falhou no gate zero WebKitWebProcess: uma settings anteriormente destruída
  conservava seu renderer e já não estava no mapa para o close da main alcançá-lo.
  Foi necessário fatorar `PresentationHost::destroy_window` e usar o mesmo
  terminate-web-process → destroy também no CloseRequested independente das
  duas janelas de settings. O probe fecha settings pelo evento real e exige um
  único renderer antes de continuar. Essa dívida foi tocada porque bloqueava
  diretamente o Headless/Leak Gate do Adaptive; não é redesign ou reconstrução
  da 1C. A coleta interrompida permanece rejeitada como prova de performance.

- O primeiro smoke instrumentado bloqueou o auto-close com draft real, mas
  excedeu o timeout ao limpar esse draft: a injeção JS de draft/clear redeclarava
  uma `const` global no mesmo contexto. O harness passou a usar IIFEs. A coleta
  interrompida não é evidência de ciclos/RSS; o guard não foi relaxado.
- Durante implementação, o compilador rejeitou `AppHandle::webviews()` na
  configuração estável usada. O código usa `webview_windows()` real, suficiente
  para todas as superfícies existentes (main e settings; não existem child
  WebViews nesta configuração). Nenhuma API/WindowEvent inventada entrou.
- A revisão acrescentou proteção a respostas atrasadas de Presence na hidratação
  e a confirmações de guard concorrentes. Testes reproduzem ambos os casos;
  nenhuma resposta stale pode selecionar 3D depois de Auto ou liberar input de
  uma checagem mais nova.

## Dívidas preservadas e gate humano restante

Home/refinamento visual, overlay DEV sobre ×, endurance prolongado, MSRV declarado
1.77.2 vs resolução Linux >=1.87, packaging DBus, amplo cross-platform, NARYS-TERM
e NARYS-NORM permanecem fora desta entrega. O teardown independente de settings precisou liberar seu renderer porque o gate
Auto revelou retenção após o fechamento auxiliar. A corrida close/reopen precisou
de proteção porque Auto introduz decisões concorrentes; foi tratada somente na
fronteira do host, sem reconstruir a 1C.

Depois da auditoria independente, o gate humano é curto:

- A: escolher Auto, perder foco, observar Headless após 30 s e abrir Narys de
  novo; Economy retorna.
- B: escolher Presence, mudar de janela/esperar; Presence permanece.
- C: escolher Auto de novo; Economy aparece e nenhuma Luna 3D surge sozinha.

Não repetir benchmarks extensos sem finding concreto. Auditoria independente e
esse gate ainda não foram executados. PERF-1 não está concluída; 1D não é PASS.
