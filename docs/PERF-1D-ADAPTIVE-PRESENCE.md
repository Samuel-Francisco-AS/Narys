# PERF-1D — Adaptive Presence

**Estado:** PERF-1D FIX-1 IMPLEMENTADA — aguardando segunda auditoria independente

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
| Presence | Presence, lazy após opt-in persistido | Presence; Economy se attention pendente | Nunca desmonta automaticamente |
| Headless | Core, **zero WebViews** | Economy temporária, sem alterar policy | Controle temporário permanece até Close manual |
| Auto (opt-in) | Economy | Economy | Pode destruir Presentation após blur seguro e delay |

Close Presentation é transitório e não grava policy. Economy/Presence retornam
à preferência na segunda ativação. Headless manual reabre Economy temporária;
não existe leitura da preferência que feche imediatamente a janela de controle.
Attention usa Economy como recuperação. Uma Presence manual já aberta não é
substituída por heurística. Reopen/recovery com attention latched mantém Economy,
inclusive sob policy Presence. Após acknowledgment, uma nova ativação explícita
pode voltar a usar Presence; o acknowledgment sozinho não muda a superfície.

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

Não repetir benchmarks extensos sem finding concreto. A primeira auditoria
independente está registrada abaixo; segunda auditoria focada e gate humano
permanecem pendentes. PERF-1 não está concluída; 1D não é PASS.


## Auditoria independente técnica — 07/10/2026

**Resultado:** arquitetura geral aprovada, porém **PERF-1D-F1 é blocker** antes do gate humano. Não foi identificada necessidade de reabrir o desenho da fase.

### Pontos aprovados

A revisão independente do commit `7b2aa1bc20cc988c21d61d65575fadccb16f38de` confirmou:

- `PresentationController` continua restrito a `economy|presence|detached`;
- policy nativa é separada do estado visual concreto;
- default/migration preservam Economy e Presence existentes;
- Auto trabalha somente Economy ↔ Headless no caminho normal;
- Presence só entra por policy explícita;
- timer Auto é event-driven e único, sem polling adaptativo;
- stale focus/timer/policy tokens são invalidados;
- draft/local work é guard booleano, sem conteúdo;
- reserva de UiBound fecha a corrida de admission do auto-close;
- janelas auxiliares bloqueiam Auto;
- Conversation HeadlessSafe continua independente;
- telemetria é bounded e allowlisted;
- cold Headless cria zero WebViews;
- ciclos nativos preservam Core/TaskId e não duplicam provider call;
- regressões 1A/1B/1C reportadas como verdes;
- probe/harness não entra no binário normal;
- não houve expansão para LR-9, NARYS-TERM ou NARYS-NORM.

As medições e os 15 ciclos são suficientes para a candidata e não exigem nova baseline integral neste momento.

### PERF-1D-F1 — Attention pode perder prioridade de superfície

**Severidade:** blocker de invariável Adaptive/safety.  
**Escopo esperado:** correção pequena e localizada.

`adaptive::reopen()` calcula o alvo aproximadamente por:

~~~text
reason == AttentionRequired
    ? Economy
    : policy.control_surface()
~~~

Isso garante Economy quando a chamada atual possui reason `AttentionRequired`, mas **não garante Economy enquanto uma attention continua latched**.

Cenário reproduzível conceitualmente:

~~~text
policy = Presence
→ usuário fecha Presentation
→ estado Headless
→ request_attention(ApprovalRequired)
→ Economy é reaberta corretamente
→ attention continua pendente
→ segunda ativação explícita do executável
→ reopen(reason = ExplicitActivation)
→ policy.control_surface() = Presence
→ Presence pode substituir Economy com attention ainda pendente
~~~

Há uma variante de corrida durante teardown:

~~~text
close em andamento
→ AttentionRequired grava recovery = AttentionRequired
→ ExplicitActivation chega antes de Destroyed
→ recovery é sobrescrito por ExplicitActivation
→ após destroy, reopen usa Presence se policy = Presence
~~~

O latch `snapshot.attention` permanece, portanto a reabertura deveria continuar em Economy independentemente do último reason não destrutivo.

Isso é relevante porque o banner/acknowledgment de atenção atual está na Economy Shell. Uma Presence aberta com attention latched pode ocultar a única superfície de tratamento visível disponível nesta fase.

### Correção exigida

Enquanto `snapshot.attention.is_some()`, qualquer **reopen/recovery** deve escolher Economy, exceto se existir futuramente uma ação explícita distinta que também trate/acknowledge a attention de forma segura.

Para a PERF-1D atual, a regra deve ser simples:

~~~text
if attention pending:
    target = Economy
else if reason == AttentionRequired:
    target = Economy
else:
    target = policy.control_surface()
~~~

Ou formulação equivalente que preserve a mesma invariável.

Não é necessário criar prioridade genérica complexa nem framework de events.

### Testes obrigatórios da FIX-1

Adicionar pelo menos:

1. policy Presence + Headless + Attention → Economy;
2. enquanto Attention permanece pendente, ExplicitActivation → continua Economy;
3. close/recovery com AttentionRequired seguido por ExplicitActivation antes de Destroyed → recovery final Economy;
4. acknowledgment limpa attention;
5. após acknowledgment, nova ExplicitActivation sob policy Presence pode selecionar Presence normalmente;
6. Auto continua incapaz de selecionar Presence;
7. nenhuma nova provider call/TaskId;
8. baterias 1A/1B/1C/1D permanecem verdes.

O probe nativo pode reutilizar o fluxo atual; não é necessário repetir 15 ciclos ou 30 s reais se a alteração ficar estritamente nessa prioridade e os testes determinísticos + um gate nativo curto cobrirem o caminho.

### Dívidas não bloqueantes observadas

Não promovidas a FIX nesta auditoria:

- manual/transient Headless não reserva `ui_suspended` como o auto-close faz; hoje não existe caminho de produto sem WebView que admita nova task UiBound, mas a invariável deve ser revisitada quando surgirem callers nativos/agents;
- attention ainda não possui caller de produto nem approval framework;
- partial native window failures continuam dívida herdada;
- MSRV/DBus/packaging/endurance/cross-platform permanecem as dívidas já registradas.

### Decisão

`PERF-1D = FIX-1 REQUIRED`.

Não executar gate humano nem fechar PERF-1 antes da correção e de uma segunda auditoria focada.


## PERF-1D FIX-1 — Attention priority

**Estado de execução:** PERF-1D FIX-1 IMPLEMENTADA — aguardando segunda auditoria independente

Base sincronizada por fast-forward na mesma branch:
`95ca734d131e8df914f494b059511d8914b4296c`. Workspace limpo e HEAD conferidos
antes da alteração. Não há nova branch, subfase ou expansão de arquitetura.

### Causa e correção

A decisão antiga usava somente o reason da chamada. Uma attention ainda latched
podia perder sua superfície Economy quando uma ativação explícita posterior
usava policy Presence, inclusive quando substituía o reason de recovery durante
close. O latch permanecia, mas o único banner/acknowledgment disponível ficava
oculto pela superfície Presence.

`Machine::reopen_target(reason)` é uma consulta pequena e sem efeitos colaterais,
chamada pelo mesmo `adaptive::reopen()` após o guard de close em andamento:

```text
attention.is_some() OU reason == AttentionRequired → Economy
caso contrário → policy.control_surface()
```

Recovery continua podendo guardar o último reason. Após Destroyed, o reopen
consulta o latch atual, portanto `ExplicitActivation` não supera attention
pendente. Nenhuma fila, prioridade genérica, state machine ou timer foi criado.
Policy Presence permanece persistida; somente a superfície de recuperação fica
Economy enquanto houver attention pendente.

`acknowledge_presentation_attention` permanece inalterado: limpa somente o latch
e publica o snapshot. Não escolhe Presence, não cria tarefa e não aprova execução
ou ferramenta. Uma **nova** ExplicitActivation depois do acknowledgment pode
selecionar Presence. Auto segue restrito a Economy/Headless; Economy, Headless,
Close != Quit, guards e todas as policies cognitivas permanecem inalterados.

### Cobertura determinística

Três testes em `adaptive_tests.rs` acrescentam:

- Presence + Headless + AttentionRequired → Economy, com as três razões
  allowlisted; novo ExplicitActivation/UserPolicy continua Economy e conserva
  o latch e a policy Presence;
- close marcado em andamento, recovery AttentionRequired, sobrescrita por
  ExplicitActivation e conclusão de Destroyed: o target de recovery continua
  Economy e a attention permanece latched;
- latch limpo: a superfície atual não muda pelo acknowledgment modelado, mas
  novo ExplicitActivation pode selecionar Presence; AttentionRequired continua
  selecionando Economy mesmo sem latch;
- Auto, Economy e controle temporário Headless sempre escolhem Economy em
  reopen, com/sem attention, sem efeitos colaterais na preferência ou no estado.

A corrida é provada deterministicamente na Machine e na função de target usada
em produção. O probe existente não pausa o callback nativo de Destroyed; não foi
introduzido um hook de teardown apenas para controlar essa ordem. O gate real
abaixo cobre ativação sobre Economy já aberta e novo reopen explícito depois de
Close com attention ainda latched. Não se apresenta essa segunda sequência como
uma injeção da corrida antes de Destroyed.

### Gate nativo curto

```bash
cargo build --release --features perf1d-probe --manifest-path src-tauri/Cargo.toml
python3 scripts/perf1d-native-probe.py --attention-priority --output /tmp/narys-perf1d-fix1.json
```

O novo modo reaproveita Tauri/Wry, WebViews, single-instance, Core e o fixture
HTTP/SSE local reais. Uma única Conversation começa em Economy; com ela ativa,
Presence é escolhida e carregada explicitamente, depois fechada. Attention
ApprovalRequired reabre Economy; segunda ativação com latch pendente mantém
Economy, zero canvas/GLB e banner visível. Outro Close/reopen explícito preserva
Economy e o latch. O acknowledgment real via IPC limpa attention sem mudar a
superfície/revisão; só a ativação posterior permite carregar Presence novamente.

Durante todas essas transições, o driver verifica mesmo TaskId/session,
TaskRegistry/ProviderRuntime, uma única tarefa ativa e uma única provider call.
O histórico terminal permanece vazio até Quit, que persiste somente o TaskId
original cancelado. Close final mantém a tarefa HeadlessSafe ativa; Quit
cancela/encerra o runtime. O cenário antigo do
probe foi preservado no branch default. Não há 15 ciclos nem delay real de 30 s
nesse gate, que é funcional e não uma nova medição de performance.

### Resultados e findings

O gate curto passou com uma única provider call, TaskId 1 preservado,
`approval_required` latched durante ativação/recovery Economy, acknowledgment
sem mudança de superfície/revisão e ativação posterior Presence ready com um
canvas e `/models/Luna.glb` HTTP 200. Quit exit 0, somente TaskId 1 cancelado no
histórico terminal e nenhum helper órfão.

A regressão nativa `--smoke` também passou: Auto Economy ↔ Headless sem Presence,
guards de draft/UiBound/settings, Presence manual estável, Headless manual,
single-instance, continuidade de Core/TaskId e Quit. As duas provider calls
nesse cenário pertencem às duas Conversations previstas pelo gate antigo;
no cenário FIX-1 houve somente uma. Zero ciclos de endurance e todos os
deadlines de Auto foram controlados, sem esperar 30 s reais.

Validações concluídas: `npm run typecheck`, `npm run build`, os cinco scripts
obrigatórios de presentation/economy/headless/adaptive/hydration e os três
scripts oficiais de provider operations, provider DOM e allocation settings.
Build release com `perf1d-probe`, `cargo check` e `cargo check --release` também
passaram. `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=2`
concluiu com **991 passed, 0 failed, 2 ignored preexistentes**, em 397,76 s nos
testes unitários; binário e doc-tests também passaram. Os ignores existentes
exigem app-server Codex local/autenticado e gate manual; nenhum ignore foi
adicionado e nenhum teste foi removido. O build frontend e os checks Rust sem
features de probe passaram; os hooks continuam isolados na feature existente.

Os scripts Node executados foram:

- `test-presentation-lifecycle.cjs`;
- `test-economy-shell.cjs`;
- `test-headless-interaction.cjs`;
- `test-adaptive-presentation.cjs`;
- `test-adaptive-policy-hydration.cjs`;
- `test-provider-operations.cjs`;
- `test-provider-operations-dom.cjs`;
- `test-allocation-settings.cjs`.

Não houve repetição da bateria longa de performance: a alteração é somente
prioridade do target, comprovada pelos testes determinísticos e WebViews reais.
Nenhum novo finding de produto foi observado nesses gates. A auditoria original
permanece registrada como evidência histórica; este resultado não declara PASS
nem encerra PERF-1.

Evidências estruturadas, sem conteúdo de conversa, em
[PERF-1D-FIX-1-OBSERVED-EVIDENCE.json](PERF-1D-FIX-1-OBSERVED-EVIDENCE.json).

Dois findings foram restritos ao novo harness, sem alterar o produto:

- A primeira coleta foi interrompida porque a assertion esperava a tarefa
  running em `task_records`, que guarda somente histórico terminal. O gate
  passou a exigir tabela vazia enquanto running, uma única tarefa ativa e
  somente o TaskId original cancelado depois de Quit.
- A segunda coleta já percorreu corretamente as superfícies, mas esperava uma
  entrada GLB em ResourceTiming. WebKit omitiu esse fetch do protocolo nativo,
  mesmo com Presence ready e canvas. O gate exige agora o callback nativo de
  recurso `/models/Luna.glb` status 200 após acknowledgment/ativação, além de
  ready/canvas. Enquanto attention pendente, continua exigindo zero requests
  AvatarViewport/GLB pelo mesmo callback. As coletas interrompidas foram
  rejeitadas como evidência do gate concluído.

As dívidas da auditoria (ui_suspended manual/transient, partial native failures,
MSRV, approvals, Home/DEV, packaging/endurance/cross-platform e TERM/NORM/LR-9)
não foram tratadas. O gate humano continua aguardando segunda auditoria focada.
