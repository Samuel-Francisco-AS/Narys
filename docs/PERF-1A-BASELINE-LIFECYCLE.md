# PERF-1A — Baseline & Presentation Lifecycle

**Estado:** PERF-1A IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente

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

## 11. Registro da implementação candidata — 07/10/2026

Esta entrega continua sendo **uma única PERF-1A**. O default de produção segue
Presence; Economy/Headless são somente contratos sem superfície 3D no harness
DEV. Nenhuma janela é fechada por esses contratos e não há Headless Runtime,
política Adaptive, layout Economy, migração de storage ou alteração cognitiva.

### Arquitetura efetivamente implementada

`App` permanece o host de Interaction. `useConversationController`, compositor,
painel, diagnósticos já montados, listeners de settings e `WindowController` não
estão abaixo da seleção de Presentation. Somente `PresenceSurface`, dentro de
`character-stage`, é montada condicionalmente. Sair de Presence executa cleanup
React; não usa ocultação CSS. Reentrar usa uma nova `generation`/React key.

`PresentationController` possui apenas `mode` (`presence | economy | headless`),
`phase` (`loading | ready | error | detached`) e `generation`. Não importa Tauri,
Three.js ou clientes de tarefa. Selecionar o mesmo modo é idempotente; recriar
Presence é explícito; reports de gerações antigas ou modos desmontados são
ignorados. O estado é observado por `useSyncExternalStore` e pelos atributos
`data-presentation-mode`/`data-presentation-phase` do host.

`PresenceSurface` é a fronteira de `React.lazy(import('../avatar/AvatarViewport'))`.
O bootstrap geral não conhece Three.js. Há fallback de loading, error boundary
e retry explícito, com nova instância de lazy e nova geração. Erros de criação
WebGL, carregamento GLB e clipes ausentes reportam `ready=false`, desmontam o
viewport e oferecem retry. Falhas de contexto/render já reportavam esse sinal.
A configuração comum de FPS foi extraída para `presentation/renderConfig.ts`;
os valores aprovados 30/24 e os settings persistidos permanecem os mesmos.

O harness DEV adiciona os controles `presence`, `economy`, `headless` e
`Recriar Presence` ao diagnóstico existente. Também expõe
`window.__narysPerf1A.{setMode,recreate,snapshot}` e aceita o parâmetro DEV
`?presentation=economy`/`headless`. O snapshot não contém mensagens, drafts,
credenciais ou reasoning; apresenta IDs, contagem, streaming e rota observada.
Esses controles/parâmetros não alteram settings e a API DEV é eliminada no
bundle de produção. Os nomes Economy/Headless não prometem a UI da 1B ou a
independência de WebView da 1C.

### Ownership confirmado no código

| Estado/recurso | Dono real e limite da prova |
| --- | --- |
| TaskId e tarefas ativas | `luna/runtime.rs::TaskRegistry`, sequência monotônica recuperada do histórico/checkpoints; `ActiveTask` e cancelamento cooperativo Rust. Seleção visual não chama register/start/cancel. |
| Histórico de tarefas, checkpoints e continuations | SQLite / módulos `persistence`; recuperação ocorre em `lib.rs::setup`, não ao criar Presence. |
| Sessões e mensagens persistidas | SQLite; `CurrentRunSessions` Rust autoriza as sessões desta execução. Criar/retomar sessão é comando explícito. |
| Sessão corrente, TaskId observado, draft, prévia, streaming, busy/generation | `useConversationController` no host Interaction `App`. Mantidos durante detach/reentry 3D. O cleanup desse hook ainda cancela tarefa quando o próprio host é desmontado: independência de fechamento/reload da WebView continua sendo trabalho da 1C. |
| Channel e cancelamento | Cliente de tarefa/closure do controller na Interaction; Rust executa e persiste. Fechar Channel pode abortar trabalho conforme hardening existente. O Channel não está no viewport. |
| Routing/policies, targets, budgets e timeouts | SQLite + Scheduler/ResourceAllocator Rust; snapshot por operação no preflight. Presentation não grava nem modifica policies. |
| Settings gerais e credenciais | SQLite / Stronghold Rust e janelas de settings existentes; `App` só observa FPS/always-on-top. Nenhum identifier/path/vault mudou. |
| Approval/atenção humana | Não há approval completo de ferramentas em Conversation. O equivalente verificado é controle de tarefa ativa/cancelamento, resultado persistido, rota e eventos. Confirmação de retomada do histórico é UX local, não approval agentivo. Sem capability fabricada. |
| WindowController | Host Interaction; janela, drag, layouts UIP e listener de posição. Não é recriado nas transições 3D. `presence/composer/conversation` aqui são layouts UIP, distintos dos modos de Presentation. |
| Estado visual | Status/ready/intent no host; root, mixer/actions, raycast, renderer/canvas, observer, cadência e diagnóstico pertencem à instância descartável do avatar. Não possuem autoridade sobre tarefas/sessões. |
| Core e workers | `tauri::Builder.setup/manage`: Database, TaskRegistry, ProviderRuntime/Scheduler, SummaryWorker, AgentRegistry, CurrentRunSessions. Não recebem modo visual nem conhecem Three.js. |

Não foi necessário mover estado para Rust. A fronteira preserva o estado
transitório essencial na Interaction já existente; não afirma que toda a
Interaction possa ser destruída sem cancelar trabalho. Os painéis DEV também
têm cancelamento em seus próprios cleanups; as transições não os desmontam,
mas fechar/trocar um painel segue a semântica diagnóstica anterior.

### Revisão do teardown

O caminho normal existente foi preservado:

1. `AvatarViewport` chama `runtime.stop()` → `setAnimationLoop(null)`;
2. limpa `runtimeRef`, remove pointer listener e descarta/limpa `directorRef`;
3. `AnimationDirector.dispose()` faz `stopAllAction()` e `uncacheRoot()`;
4. `AvatarManager.dispose()` remove root e libera o adapter; carregamentos tardios
   são descartados sem inserir root nem reportar ready/error na instância morta;
5. `SceneRuntime.dispose()` para loop, remove listeners de foco/visibilidade e
   contexto, limpa timer DEV, desconecta ResizeObserver, faz renderer.dispose(),
   remove canvas e limpa scene.

Foi corrigida uma lacuna factual em `LegacyGlbAdapter`: o GLB tem **1 skin**, e o
adapter não descartava `Skeleton`/bone texture nem fechava ImageBitmaps. A revisão
de `three/src/objects/Skeleton.js` e `GLTFLoader.js` instalados confirmou esses
recursos. Agora skeletons e bitmaps compartilhados dentro do asset são deduplicados
antes de dispose/close, além do descarte existente de geometries/materials/textures.
Não foi acrescentado `forceContextLoss`, polling de lifecycle ou workaround de
driver. `renderer.dispose()` não prova destruição imediata do objeto/contexto
WebGL no driver: a coleta final de contextos/heap continua sob WebKit/GC/SO.

### Arquivos da entrega

- `src/presentation/PresentationController.ts`, `PresenceSurface.tsx`,
  `renderConfig.ts`: contrato, superfície e configuração comum;
- `src/App.tsx`: seleção e harness DEV, mantendo Interaction montada;
- `src/avatar/AvatarViewport.tsx`: reports de falha para o lifecycle;
- `src/avatar/runtime/RenderBudget.ts`: reexport do contrato de configuração;
- `src/avatar/adapters/LegacyGlbAdapter.ts`: skeleton/ImageBitmap teardown;
- `scripts/test-presentation-lifecycle.cjs`: testes determinísticos sem novo framework;
- `scripts/perf1a-process-baseline.py`, `perf1a-webkit-probe.py` e fixtures JS:
  medição / integração opcionais, usando somente dependências já instaladas;
- `src-tauri/src/luna/conversation_preflight_tests.rs`: workload de referência
  do Core com provider fixture, sem mudança de runtime de produção;
- este documento e `PERF-1A-OBSERVED-EVIDENCE.json`: resultados auditáveis.

### Metodologia reproduzível

Ambiente observado: Fedora Workstation 44, x86_64, GNOME/Wayland,
Intel Core i7-3770 (4 cores / 8 CPUs lógicas), 7.819 MiB de RAM e 7.818 MiB
swap; Node 24.18.0, Cargo 1.98.1, WebKitGTK 2.54.0. O launcher Linux já estabelece
`LIBGL_ALWAYS_SOFTWARE=1` se ausente; o probe usa o mesmo default. O nome
reportado pelo contexto é `WebKit WebGL`, não identificação factual de GPU.
Nenhuma dependência de sistema foi instalada.

Referência anterior: `838a42e4d62846a2ff6795b29a0fac9b66b5392f`, confirmada no
HEAD e em `git ls-remote origin refs/heads/perf-1a-baseline-lifecycle` antes de
editar. O baseline inicial foi coletado antes da mudança de código. Uma repetição
da referência usa `git archive` em diretório temporário, sem branch/worktree ou
alteração de histórico. Registrar ref, horário, carga concorrente e foco em cada
amostra é obrigatório; dados DEV não equivalem a um benchmark release.

Comandos para repetir no checkout correspondente:

```bash
npm run typecheck
npm run build
npm run dev
# Em outro terminal: frontend real em uma única WebView GTK, sem Core Rust.
python3 scripts/perf1a-webkit-probe.py > /tmp/perf1a-graphics.jsonl
# Integração: React real + WebGL/GLB reais + IPC sintético, sem provider comercial.
python3 scripts/perf1a-webkit-probe.py --lifecycle > /tmp/perf1a-cycles.jsonl
python3 scripts/perf1a-webkit-probe.py --boot-contract --url 'http://127.0.0.1:5173/?presentation=economy'
python3 scripts/perf1a-webkit-probe.py --boot-contract --url 'http://127.0.0.1:5173/?presentation=headless'
node scripts/test-presentation-lifecycle.cjs
```

Para repetir a referência original sem modificar o checkout atual:

```bash
mkdir -p /tmp/perf1a-before-source
git archive 838a42e4d62846a2ff6795b29a0fac9b66b5392f | tar -x -C /tmp/perf1a-before-source
ln -s "$PWD/node_modules" /tmp/perf1a-before-source/node_modules
node node_modules/vite/bin/vite.js /tmp/perf1a-before-source --host 127.0.0.1 --port 5174
# O probe pertence à candidata, mas nesta coleta carrega o frontend anterior.
python3 scripts/perf1a-webkit-probe.py --url http://127.0.0.1:5174/
```

O probe espera 10 s e amostra `/proc` por aproximadamente 30 s, junto com os
reports `[UIP-0]` existentes em intervalos de 5 s. `elapsedMs`, foco e visibilidade
são dados observados; `targetFps` é teto configurado, não FPS medido. O snapshot
final confirma canvas, GLB e duração de carregamento. GLB carregado não significa
que exista render constante quando o WebKit não entrega RAF.

Para medir **Tauri/Core e sua árvore real**, usar:

```bash
# Desktop DEV, com frontend servido pelo Vite. Diretório de dados isolado opcional:
XDG_DATA_HOME=/tmp/perf1a-isolated-data cargo run --manifest-path src-tauri/Cargo.toml
# Obter PID exato em ps -eo pid,ppid,comm e informar somente o processo do app:
python3 scripts/perf1a-process-baseline.py --pid <PID_APP> --scenario presence-focus-confirmado --seconds 30
```

O sampler registra PID/PPID, início do processo, CPU ticks e RSS, sem commandline,
mensagens ou segredos. CPU = delta de ticks / CLOCK_TICKS / duração × 100: **100%
é um core lógico**, não a máquina inteira. RSS é soma das árvores por segundo e
pode contar páginas compartilhadas mais de uma vez; não é PSS nem memória GPU.
Mudança de identidade/processo é reportada; consumo entre amostras de processos
que nascem/morrem rapidamente pode faltar. Não misturar WebViews de outros apps.

Gate humano: repetir amostras **sem compilações/testes concorrentes**, fechar
janelas auxiliares, aquecer 10 s, manter tamanho/settings/asset/driver iguais,
confirmar foco/desfoco e minimizar/restaurar fisicamente. Copiar os reports UIP-0
da WebView Tauri junto da amostra de processos. Se Wayland não emitir visibility
hidden ao minimizar, registrar o sinal real; não simular o evento para produzir
uma medição de compositor. Para atividade cognitiva, usar a mesma mensagem e
policy existentes, capturar TaskId, provider_selected, primeiro chunk, terminal,
cancelamento e histórico antes/depois de ao menos 20 transições DEV. A amostra
de CPU deve cobrir o intervalo real de trabalho, sem alterar budgets/thinking.

Referência automatizada do Core (não é TTFT de API comercial):

```bash
cargo test --manifest-path src-tauri/Cargo.toml perf1a_core_baseline_conversation_fixture -- --nocapture --test-threads=1
```

Esse teste mede registro e tempo até terminal + persistência/fechamento do
Channel em 5 conversas independentes, usando runtime de conversa, preflight,
Scheduler, SQLite e provider fixture. Valida uma seleção/chamada por conversa,
mesmo TaskId em seus eventos e dez mensagens persistidas. O runtime Core de
produção não foi alterado; esse custo inclui o SecretStore sintético do fixture,
não um provider real nem RTT Tauri.

### Baseline e resultados realmente observados

Dados brutos e reports, incluindo aquecimento e churn, estão em
[PERF-1A-OBSERVED-EVIDENCE.json](PERF-1A-OBSERVED-EVIDENCE.json).
RSS nesta tabela está em **KiB**; CPU está em **% de um core lógico**.

| Coleta | Duração útil | RSS final; faixa observada | CPU observada | Condição/comprovação |
| --- | --- | --- | --- | --- |
| Referência, antes de editar, GTK sem Core | ~30 s após 10 s | 629.996; 629.996–682.304 | ~3,50% dos processos amostrados; helpers saíram | Documento `visible`, `hasFocus=false`; reports ao final tiveram 0 frames/callbacks. Houve compilação concorrente. Não representa idle em foco nem minimização. |
| Referência 838a42e, repetida em GTK | 30,001 s após 10 s | 660.068; 655.704–696.084 | 108,463% | Documento visível/em foco, GLB real, sem Core, coleta sequencial sem Cargo concorrente. |
| Candidata, GTK no mesmo cenário | 29,996 s após 10 s | 650.304; 650.100–703.516 | 107,914% | Documento visível/em foco, GLB real, sem Core, coleta sequencial sem Cargo concorrente. |
| Tauri DEV candidata, dados isolados | 30,000 s | 671.072; 666.256–675.196 | 104,066% | App real + descendentes; 3 processos no fim e sem churn. Foco/oclusão não atestados e houve Cargo check concorrente; não classificar como gate de idle/foco. |

Nos dois probes em foco houve **1 canvas**, WebGL **2.0**, CSS/drawing buffer
**300×360**, DPR **1**, `/models/Luna.glb` carregado, clipes **Idle/Wave**. O asset
inspecionado possui **4.206.852 bytes**, 4 meshes, 1 skin e 13 textures. Ambos
terminaram com Python host + WebKitNetworkProcess + WebKitWebProcess; helpers
GTK/glycin temporários saíram durante a coleta. O sampler sinaliza esse churn;
CPU de processos nascidos e mortos entre amostras não é recuperável. Nenhum
processo de outro aplicativo foi somado.

| Cadência gráfica, após aquecimento | Referência | Candidata |
| --- | --- | --- |
| FPS nos reports completos após 10 s | 29,95–30,00 | 29,96–30,00 |
| Último report (aprox. 5 s) | 150 frames / 299 callbacks | 150 frames / 297 callbacks |
| Intervalo último report: média / p50 / p95 | 33,38 / 33 / 45 ms | 33,33 / 33 / 47 ms |
| Update+render último report: média / p95 | 13,77 / 18 ms | 14,34 / 19 ms |
| Fetch GLB registrado pelo Resource Timing | 39 ms | 139 ms |

A primeira amostra sem foco reportou fetch de 312 ms e 1 canvas, mesmo sem
frames no fim. Isso reforça que suspensão de callbacks não é teardown. Os tempos
de fetch incluem condições locais/cache e não são tempo total de reentrada.
Os valores de FPS de aquecimento e janelas limítrofes também estão no JSON;
uma janela inicial da candidata reportou 30,12, compatível com amostragem curta
e o orçamento existente. Nenhum parâmetro de cadência foi alterado.

**Não se declara ganho de CPU/RAM com esses números.** As faixas de RSS se
sobrepõem; o host GTK é distinto de Tauri e o workload/estado gráfico do baseline
inicial não coincide com as repetições. A comparação em foco não mostra alteração
grosseira da cadência; não demonstra equivalência estatística, performance em
release, latência de provider ou ausência de leaks. Wakeups/PSS/memória GPU e
contagem confiável de contextos vivos no driver: **N/A nesta sessão**, sem
instrumento factual instalado. Minimização/oclusão nativa e activity/TTFT com
provider comercial: **PENDENTE DE GATE HUMANO**.

### Resultado dos ciclos e continuidade

`node scripts/test-presentation-lifecycle.cjs` executou **20 ciclos** usando
`AvatarViewport`, `SceneRuntime`, `RenderBudget`, `AnimationDirector`,
`AvatarManager`, `LegacyGlbAdapter` e recursos Three reais. O DOM, renderer e
transporte GLTF são doubles determinísticos; isso não é medição de GPU. Dez
ciclos carregam antes de desmontar e dez entregam GLB após desmontar. Verificou:
loop=null, stop/dispose do renderer, listeners removidos, observer desconectado,
canvas removido, refs nulas, mixer stop/uncache, geometries/materials/textures e
bone texture descartadas, ImageBitmap fechado uma vez e ausência de callbacks
de status após eventos/carregamentos tardios. Gerações antigas não alteram
PresentationState. O teste também verifica separação do bundle e ausência da
API DEV no App de produção.

A integração em **WebKitGTK real + React real em StrictMode** executou outros
**20 ciclos** alternando os contratos Economy/Headless e retornando à Presence.
Cada estado detached apresentou:

- 0 canvases, ResizeObservers, RAF pendentes, timers de diagnóstico 3D e
  listeners de focus/blur/visibility;
- sessão **41**, tarefa **81**, streaming e rota Fixed da fixture preservados;
- exatamente **1 start** e **0 cancels** durante os vinte ciclos.

Cada reentrada apresentou 1 canvas, 1 observer, 1 RAF pendente, 1 timer 3D e
3 listeners de orçamento, sem acumulação. Um timer separado do cliente Vite
permaneceu vivo; ele é infraestrutura DEV, não recurso da Presence. Depois do
último detach, completar a tarefa atual atualizou a conversa com três mensagens
da fixture. Uma segunda tarefa **82** foi cancelada pelo controle real do
compositor após remontar Presence: **2 starts / 1 cancel** no total, sem tarefa
pendente. A mesma prova injetou falha WebGL na recriação, observou `error`/zero
canvas e exercitou `Tentar novamente`, terminando em `ready` com 1 canvas.
Os quatro logs de erro no JSON desse teste são somente a falha deliberadamente
injetada em StrictMode; não foram ocultados.

As 11 amostras de memória durante os ciclos (10–20 s do probe) variaram de
**749.580 a 804.252 KiB**. A primeira foi **801.220**, a última **789.232 KiB**.
Não houve crescimento monotônico nessa janela nem múltiplos recursos visuais
observados. Isso é uma amostra curta sob GC/driver, com referências JS/GLB já
carregadas; não prova ausência absoluta de leak, retorno imediato de RSS ao
baseline inicial ou destruição imediata de todos os contextos. Um gate nativo
mais longo permanece necessário.

Bootstrap DEV direto em Economy e no contrato Headless passou em duas WebViews
novas: 0 canvas/observer/RAF/timer 3D/listener visual, nenhuma resource request
com avatar/three/Luna.glb e 0 start/cancel. A janela e a Interaction existiam
normalmente; não é prova de Headless Runtime.

A parte conversacional desse teste usa **IPC sintético** e mensagens artificiais,
sem Rust, SQLite real ou chamadas comerciais. Ela comprova que o grafo React
preserva o controller/callback/cancelamento e refresca os dados da fixture. A
semântica do Core/SQLite real é coberta separadamente pelos testes Rust existentes
(preflight, registro, policies, persistência, Channel failure e cancelamento)
e pelo workload de referência novo. A combinação **Tauri + provider real +
transições físicas** continua **PENDENTE DE GATE HUMANO**, não foi fabricada.

### Boundary de bundle confirmada

Antes: `App-BBkRb0gs.js` **666,22 kB**, com stack 3D estático.
Depois: `App-CF7ZgXG4.js` **35,25 kB** (gzip 10,16 kB) e
`AvatarViewport-HOi4yK0X.js` **633,76 kB** (gzip 160,01 kB), alcançado por
import dinâmico. Bootstrap/index **221,91 kB**; settings continuam superfícies
independentes. O App gerado não contém WebGLRenderer/GLTFLoader/boneTexture/URL
GLB, e o teste de boot confirma ausência de requests 3D quando não selecionado.
Os nomes de hash identificam o build observado, não um contrato persistente.

O warning Vite de chunk >500 kB continua no chunk opt-in 3D. Não foi silenciado
nem tratado como erro; dividir internals Three sem benefício causal não faz
parte desta etapa. Production continua selecionando Presence e portanto carrega
essa dependência sob demanda logo no primeiro mount; a mudança de default fica
exclusivamente para PERF-1B.

### Referência factual de latência do Core

Na execução isolada do workload `perf1a_core_baseline_conversation_fixture`,
sem probes gráficos/testes concorrentes, foram observados:

| TaskId | Registro | Terminal + persistência/fechamento do Channel |
| --- | --- | --- |
| 1 | 750 µs | 1.664.388 µs |
| 2 | 50 µs | 1.642.225 µs |
| 3 | 52 µs | 1.618.033 µs |
| 4 | 47 µs | 1.657.406 µs |
| 5 | 81 µs | 1.630.985 µs |

Cinco chamadas/seleções, dez mensagens persistidas, nenhum erro; o teste completo
levou 9,98 s incluindo setup do fixture. O provider fixture retorna localmente e
não tem espera para simular API: o tempo inclui preflight/SecretStore sintético,
Scheduler e SQLite. Não é um benchmark de provider comercial, TTFT, RTT Tauri
nem comparação de latência entre modos. Core de produção e políticas cognitivas
não mudaram nesta entrega.

### Testes/comandos e findings de execução

| Verificação final | Resultado |
| --- | --- |
| `npm run typecheck` | OK |
| `npm run build` | OK; chunk Presence separado; warning >500 kB preservado |
| `node scripts/test-presentation-lifecycle.cjs` | OK; 20 ciclos determinísticos |
| `python3 scripts/perf1a-webkit-probe.py --lifecycle` | OK; 20 ciclos reais de React/WebGL com IPC fixture, conclusão/cancelamento e recuperação de erro |
| Probes `--boot-contract` Economy e Headless | OK; duas inicializações frescas sem requests/recursos 3D |
| `node scripts/test-provider-operations.cjs` | OK |
| `node scripts/test-provider-operations-dom.cjs` | OK |
| `node scripts/test-allocation-settings.cjs` | OK; 85 checks |
| `cargo check --manifest-path src-tauri/Cargo.toml` | OK |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=2` | OK; **974 passed, 0 failed, 2 ignored**, 398,09 s |
| Core baseline com `--nocapture --test-threads=1` | OK; 1 passed, 975 filtered out |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | OK |
| `git diff --check`, parsing JSON, syntax dos scripts JS/Python | OK |

Os dois ignored são gates já existentes de Codex autenticado/handshake real;
não foram habilitados nem substituídos. Checks Rust continuam emitindo warnings
existentes (15 debug / 38 release, incluindo dead code e itens cfg); nenhum
warning novo foi suprimido. Não foi adicionada dependência ou framework frontend.

Resultados desfavoráveis também foram conservados:

- A primeira chamada de `cargo run` e o primeiro check release falharam por
  permissões Tauri **geradas no cache** ainda apontarem para
  `/home/sam/Projetos/Assistente-3D/...`. Regenerar somente o pacote Tauri com
  `cargo clean ... -p tauri` (debug e release) resolveu; não houve rename,
  mudança de identifier, banco, vault ou path persistente do aplicativo.
- A primeira bateria `cargo test` com concorrência padrão teve **971 passed,
  2 failed, 2 ignored** em 274,76 s. Foram os testes HTTP temporizados
  `cloudflare::tests::local_http_lifecycle_covers_split_valid_eof_timeout_and_internal_code`
  e `fix5_tests::fix5_http_timeout_phases_are_real_and_diagnostics_sanitized`:
  uma janela de 500 ms expirou com elapsed=1.300 ms; a outra esperava StreamIdle
  e recebeu Overall. Houve carga CPU/memória/swap alta e probes simultâneos.
  A repetição **integral** com duas threads passou ambos sem alterar timeout,
  teste, provider ou runtime. A fragilidade sob saturação é finding operacional
  para auditoria; não foi escondida como sucesso da primeira execução.
- Uma tentativa gráfica durante essa saturação excedeu o timeout de reentry do
  probe. A execução sequencial, sem a bateria Rust concorrente, completou os vinte
  ciclos e o erro/retry. Os testes gráficos devem ser medidos separadamente de
  workloads de validação que saturam a máquina.
- Nos probes de baseline **sem Tauri**, o listener geral de settings do `App`
  emite rejeição por falta de `__TAURI_INTERNALS__`. A mesma linha já existe na
  referência 838a42e e os gráficos continuam funcionais. Não é regressão causada
  pela fronteira; a integração fixture fornece esse contrato e o Tauri real o
  possui. Fica registrado para auditoria do suporte a navegador/probes, sem
  ampliar esta entrega para refatorar a Interaction inteira.

A lacuna skeleton/ImageBitmap foi resolvida nesta candidata. Não há finding
novo de produção conhecido que exija um FIX imediato de lifecycle; a auditoria
pode identificar FIX-1/FIX-2 dentro da própria PERF-1A. Os limites de contexto
WebGL/GC, gate nativo e revalidação de provider real permanecem explícitos.

### Gates humanos e auditoria pendentes

- Presence nativa aprovada visualmente: Idle, Wave/raycast, composer/conversation,
  settings, transparência e ergonomia UIP após lazy/reentry;
- foco → sem foco → minimização/restauração física em Wayland, distinguindo
  `visibilityState` efetivo de oclusão do compositor;
- tarefa e provider reais em Tauri durante vinte transições: mesmo TaskId,
  nenhuma chamada extra, rota/policy preservada, streaming/cancelamento/histórico;
- baseline nativo/release e CPU durante essa tarefa, TTFT e latência comparável;
- memória após ciclos e descanso/GC no aplicativo real; contextos/GPU somente
  quando houver instrumento confiável, sem forceContextLoss;
- auditoria independente de contratos, diff, capacidades e evidências.

**Estado da entrega: PERF-1A IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria
independente.** Nenhum PASS final foi declarado. PERF-1B/1C/1D não foram
implementadas nem liberadas por conveniência desta validação.
