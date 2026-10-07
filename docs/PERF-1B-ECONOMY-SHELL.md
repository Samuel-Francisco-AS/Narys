# PERF-1B — Economy Shell

**Estado: PASS — concluída em 07/10/2026 com dívidas não bloqueantes registradas**

Branch: `perf-1b-economy-shell`
Base: `main@c883119a8fdbcc1009964fb5e43d30860c635c3f`
Data: 07/10/2026

Uma única PERF-1B. PERF-1A continua PASS; 1C e 1D não estão implementadas.
Nenhum PR/merge foi autorizado para esta entrega.

## Objetivo e arquitetura

Economy é a interface padrão, utilizável sem Three.js, GLB, renderer, mixer,
SceneRuntime ou loop 3D. `App` mantém o `useConversationController` acima da
seleção de superfície. Trocar Presentation não remonta esse hook nem muda o
TaskId, Channel, sessão, draft, provider routing, políticas ou cancelamento.
O Core Rust, Scheduler, providers, admission/rate/allocation/handoff continuam
os mesmos; nenhum modo visual é passado às operações cognitivas.

`EconomyShell` usa DOM/CSS Grid/Flex, fundo opaco e SVGs simples. Não há canvas
2D decorativo, blur/backdrop-filter, animação infinita, RAF 2D ou efeitos caros.
O centro abre na conversa; Shell/Home, Relays, Tasks, System e Settings dão
acesso às capabilities reais. TaskGraph, allocation, handoff, providers,
credenciais/modelos e diagnósticos continuam na janela IA/modelos existente.
Tasks mostra o TaskId observado pela Interaction, sem fingir listar todas as
tarefas do Core. Aprovações de ferramentas ainda não têm uma capability de
produto; a UI diz isso explicitamente. Home não fabrica runtime logs.

Conversa reutiliza `Composer`/`ConversationPanel`: streaming, erros, retry pelo
composer, histórico, nova sessão e retomada explícita mantêm seus contratos.
Sair da view desmonta apenas os widgets; o controller de Interaction continua.
Fechar/recarregar a WebView mantém a semântica anterior, inclusive cancelamento
no cleanup do host; não há Headless Runtime nesta entrega.

## Default e Presence opt-in

`PresentationController` e o host iniciam em Economy. A hidratação de settings
só seleciona Presence se houver preferência explícita persistida. Sem settings,
com migration antiga ou falha de leitura, não se carrega 3D por fallback.
Falha de persistência é apresentada; não se finge que a escolha foi salva.

Migration 017 adiciona `shell_settings` ao mesmo SQLite existente. Instalações
novas e upgrades sem preferência recebem `economy`. Nenhum identificador,
namespace, vault, path de armazenamento, package ou database foi renomeado.

Configurações gerais oferecem somente Economy/Presence. Settings na Shell
permite ativação explícita; Presence oferece botão Economy para retornar.
`update_presentation_mode` persiste a opção e emite evento à main. Não há Auto,
heurística, voice, browser automation ou novo agent/provider. O contrato
`headless` e o harness permanecem somente em DEV para os testes da 1A, sem
opção de produto nem promessa de operação sem WebView.

Somente Presence monta `PresenceSurface`; seu import lazy de `AvatarViewport`
continua sendo a fronteira 3D. Economy não contém CharacterStage/canvas oculto.
A reentrada usa uma nova geração; sair usa o teardown/rollback auditado na 1A:
loop, listeners, observer, mixer, assets e renderer descartados. Three.js/assets
não foram removidos ou simplificados. Retornar a Economy após importar Presence
não promete descarregar imediatamente módulos ES já cacheados ou memória GPU;
próxima inicialização em Economy evita inteiramente a carga inicial.

## Layout, janela e persistência

Topo: SVG pequeno + NARYS estático, arraste pela marca, minimizar,
maximizar/restaurar e fechar via APIs Tauri. Nenhuma identidade “perf mode”.
Janela inicial 1120×720 lógico, mínimo 640×480, resize nativo permitido,
sem fullscreen obrigatório. Economy ignora os layouts fixos UIP; ao voltar de
Presence restaura o último tamanho Economy desta execução. Presence preserva
os tamanhos físicos UIP 310×410, 310×490 e 625×490 conforme composer/painel,
com resize desligado somente nesse modo. Always-on-top mantém a preferência
existente; click-through/Wayland debt não foi ampliada.

Uma única WebView foi mantida. `transparent:true`, decorations=false e
shadow=false permanecem na criação para permitir a Presence legada. Economy
aplica fundo DOM e background nativo opacos; Presence restaura alpha zero.
**Limitação:** o flag de transparência de criação não pode ser alterado por
esta API. Não se afirma que Economy usa uma janela nativamente criada sem
suporte a alpha. Recriar janela/WebView introduziria risco à Interaction e
ampliaria o escopo. Transparência da Presence e resize/controles físicos
continuam gates humanos, especialmente em Wayland e outras plataformas.

Laterais recolhíveis/restauráveis, splitters com Pointer Capture e teclado
(setas, 16 px). A esquerda expandida aceita 160–320 px; direita 220–360 px.
Valores não finitos e fora do intervalo são tratados no frontend; o Rust
limita as larguras antes de gravar. Persistência guarda aberto/recolhido e
largura de ambas, além de Presentation. Drag grava no pointerup, nunca por
pixel; gravações de layout são serializadas. Cancelar/perder capture descarta
o preview. Escritas de modo e layout são independentes das gerais e entre si,
evitaram que uma janela de settings aberta sobrescrevesse o layout atual.

Abaixo de 1000 px a esquerda fica compacta; abaixo de 900 px o painel direito
não é montado automaticamente, preservando a preferência ampla. “Operação”
abre on-demand como painel sobreposto; pode ser recolhido. O centro mantém
min-width:0 e scroll próprio. Testes visuais em 1120 e 640 px verificaram que
composer/navegação/controles permanecem acessíveis. Não há layout Android.

## Telemetria

`OperationalSummary`, lazy, lê apenas o snapshot operacional já existente do
Scheduler: captura, provider IDs, chamadas ativas/concurrency, fila/capacidade
e circuito. Não inventa contagem de relays saudáveis ou estado “stable”.
Ausência/falha aparece como indisponível/desatualizado. Dados operacionais
profundos continuam em IA/modelos.

Reutiliza `OperationalPoller`: uma requisição em voo, próximo timeout 1 s após
a captura, pausa em document hidden, ignora respostas stale. O timer pertence
ao resumo, sem atualizar a árvore inteira da Shell. Recolher o painel ou usar
largura estreita desmonta o resumo e cancela o polling/listener. Os testes
observam que o contador de leituras para enquanto recolhido.

CPU/RAM/uptime/OS/kernel não são exibidos como fatos nesta entrega: não havia
contrato nativo apropriado para a árvore de processos e não foi adicionada
biblioteca/sampler permanente ao Core. System indica indisponibilidade.
CPU/RSS comparativos abaixo vêm de `/proc`, fora da UI, sem confundir RSS
agregado, RAM total, heap JS ou memória GPU.

## Bundle e testes de bootstrap

O teste `test-economy-shell.cjs` percorre as dependências **estáticas** do entry,
App e resumo operacional padrão, verificando ausência de WebGLRenderer,
GLTFLoader, AnimationMixer, URL GLB e harness. Também inspeciona todos os JS de
produção para a API DEV da 1A. O stack 3D só aparece no chunk opt-in.
Warning Vite >500 kB permanece nesse chunk e não foi silenciado.

Build observado: bootstrap/index **221,94 kB** (gzip 69,61), App **46,33 kB**
(gzip 13,26), resumo **1,82 kB**, poller compartilhado **1,57 kB**,
AvatarViewport/Three **634,27 kB** (gzip 160,15). A Shell ampliou o App em relação
à 1A (35,25 kB), mas manteve o stack 3D fora da inicialização Economy.
O probe de produção observa zero requests do chunk Avatar/GLB, zero contextos,
zero canvas, zero observers/RAF/frames 3D antes do opt-in. A opção DEV na URL
`?presentation=presence` não funciona em produção; preferência real é obrigatória.

## Metodologia de performance

Mesma metodologia da 1A: 10 s de aquecimento + 30 s úteis, CPU como percentual
de **um core lógico**, RSS soma da árvore relevante (páginas compartilhadas
podem contar duas vezes), identificação de PID/startTicks e churn. Sem PSS,
GPU RAM ou wakeups confiáveis. Ambiente Fedora/GNOME/Wayland, WebKitGTK 2.54,
Mesa software (`LIBGL_ALWAYS_SOFTWARE=1`), mesmo hardware/settings/GLB da 1A.

Dois níveis de evidência, que não devem ser misturados:

- `perf1b-webkit-probe.py`: frontend de **produção** servido por `vite preview`,
  WebKit/WebGL reais, IPC sintético, host Python/GTK sem Rust. Instrumentação
  externa observa bootstrap, requests, contextos, callbacks, canvas e teardown.
  O custo não é RSS Tauri. Callbacks RAF incluem frames pulados pelo budget;
  são descritos como callbacks, não FPS renderizado.
- `perf1b-native-baseline.py`: binário **release**, frontend embutido, Core real,
  dados SQLite isolados, nova árvore em cada cenário, ordem alternada em duas
  rodadas. Não inclui Cargo/Vite/Python sampler no root medido. Foco/oclusão
  não são atestados pelo sampler; confirmação física pertence ao gate humano.

Referência histórica Presence da 1A: WebKit DEV sem Core 650.304 KiB ao final,
107,914% CPU, ~30 FPS; Tauri DEV sem foco atestado e com Cargo concorrente
671.072 KiB/104,066%. **Esses dados históricos não são o comparador release**.
O endurance DEV de 180 s aprovado na 1A incluiu infraestrutura no root; sua
soma (~1,6 GiB) não representa aplicação release.

Comandos reproduzíveis:

```bash
npm run build
npm run preview -- --port 4173
/usr/bin/python3 scripts/perf1b-webkit-probe.py --test
/usr/bin/python3 scripts/perf1b-webkit-probe.py --test --restored-layout --url 'http://127.0.0.1:4173/?presentation=presence'
# Separadamente, sem compilação/testes concorrentes:
/usr/bin/python3 scripts/perf1b-webkit-probe.py --mode presence
/usr/bin/python3 scripts/perf1b-webkit-probe.py --mode economy
cargo build --release --manifest-path src-tauri/Cargo.toml
python3 scripts/perf1b-native-baseline.py --rounds 2
```

## Comparação nativa release observada

Binário SHA-256: `4577fbadde6f927759b62638d82d6650cc9ceea09baf07b1b72c6459b186b423`.
Frontend embutido do build acima. Cada amostra útil durou 30 s após 10 s de
warmup, dados isolados iguais, nenhuma conversa/summary pendente, nenhum teste
ou compilação concorrente. Janelas auxiliares ausentes. Três processos por
amostra (app + WebKitNetworkProcess + WebKitWebProcess), nenhum churn.

| Rodada / ordem | Presence RSS final (KiB) | Economy RSS final (KiB) | Presence CPU (% core) | Economy CPU (% core) |
| --- | ---: | ---: | ---: | ---: |
| 1: Presence → Economy | 669.352 | 569.448 | 134,098 | 9,367 |
| 2: Economy → Presence | 660.452 | 572.400 | 129,400 | 8,600 |
| Média das duas amostras | 664.902 | 570.924 | 131,749 | 8,984 |

Economy teve **93.978 KiB (~91,78 MiB) menos RSS médio**, com CPU menor em
ambas as ordens. As faixas úteis de RSS não se sobrepõem: Presence
651.776–689.940 KiB; Economy 561.956–579.468 KiB. Não se estabeleceu meta de
percentual antecipada. O Core nativo continuou presente; nenhuma qualidade ou
prioridade cognitiva foi reduzida para obter esses resultados.

**Limites:** RSS soma páginas compartilhadas; não é PSS/GPU. Foco/oclusão físicos
não são atestados. Cada modo usa sua janela de produto (Presence compacta,
Economy 1120×720), portanto áreas/layouts/composição também diferem. A Economy
paga o custo do painel operacional real de 1 Hz; Presence não o possui nesse
idle. São duas amostras curtas neste hardware/Mesa software; não se promete
esse ganho em outra GPU/plataforma nem equivalência estatística universal.
A comparação WebKit abaixo mantém a mesma janela 1120×720 para ambos.

## Frontend de produção em WebKitGTK

Mesmo host GTK/WebKit e janela 1120×720, uma coleta por modo, sequencial e sem
compilação/testes concorrentes. Ambas reportaram `visible` e `hasFocus=true`.
IPC sintético, sem Rust/API comercial. Três processos ao final; helpers
bwrap/glycin de startup saíram durante ambas as coletas (churn registrado).

| Métrica | Presence | Economy |
| --- | ---: | ---: |
| RSS final / faixa (KiB) | 665.640 / 665.400–686.596 | 550.128 / 546.916–566.372 |
| CPU (% de um core lógico) | 145,042 | 10,601 |
| Frames 3D no período útil / FPS observado | 900 / ~30,001 FPS | 0 / 0 FPS |
| Contextos WebGL criados / canvas final | 1 / 1 | 0 / 0 |
| GLB Resource Timing | Luna.glb, 84 ms | nenhum request |
| Bootstrap observado (performance.now) | Presence ready: 858 ms | Shell hidratada: 222 ms |
| RAF / ResizeObserver ao final | 1 / 1 | 0 / 0 |

FPS aqui conta timestamps RAF distintos em que ocorreram draw calls WebGL,
sem confundir callbacks pulados pelo render budget com frames renderizados.
O probe intercepta draw methods fora do bundle. Zero draw calls/contextos/
canvas/requests em Economy reforça a fronteira real, em vez de CSS hiding.
A latência de bootstrap acima é da página/frontend nesse host com IPC sintético,
**não do processo Tauri/Core**; warming/cache/rede local afetam esses números.
CPU/RSS desses hosts não devem ser combinados com a tabela nativa.

Dados brutos, snapshots, grafo de requests e resultados de ciclo/boots/testes:
[PERF-1B-OBSERVED-EVIDENCE.json](PERF-1B-OBSERVED-EVIDENCE.json).

## Baseline cognitiva

Não houve alteração de Conversation Runtime, TaskRegistry, Context Builder,
Scheduler, admission/rate, ResourceAllocator, provider/model/thinking/context/
output ou prioridades. Os comandos novos só acessam Presentation/layout.
Os testes Rust comparam políticas antes/depois das preferências e reabrem o
SQLite; a operação representativa da 1A continua sendo executada pelo mesmo
fixture/preflight/Scheduler. Os probes reais React verificam uma única chamada
por envio, TaskId/streaming preservados durante transição e cancelamento correto.
IPC sintético não é provider real nem TTFT comercial. A amostra isolada do
fixture cognitivo e os gates comerciais são registrados com esse limite.

A repetição **isolada**, após as medições gráficas e a suíte integral, usou o
mesmo teste cognitivo da 1A (cinco conversas, cinco seleções/chamadas, dez mensagens
SQLite). Registro: **951 / 221 / 68 / 66 / 97 µs**. Terminal + persistência:
**1.572.716 / 1.563.912 / 1.541.762 / 1.527.030 / 1.539.499 µs**. Exit 0,
9,38 s incluindo setup. A faixa histórica da 1A foi 1.618.033–1.664.388 µs;
esta repetição não mostrou aumento grosseiro, mas **não é uma comparação de
TTFT entre modos, nem prova estatística/rede comercial**. O fixture usa o mesmo
Core/preflight/Scheduler e policies; Presentation não participa da chamada.

## Testes e findings de execução

A primeira execução Rust integral teve **963 passed, 13 failed, 2 ignored**
em 473,61 s. As falhas foram asserções do schema 16/future version 17,
expectativa antiga de permission do snapshot apenas em IA e fixtures que
regrediam artificialmente o user_version mantendo a nova tabela. Não houve
falha HTTP temporizada/qualidade cognitiva nessa rodada. A migration 017 foi
tornada idempotente (`IF NOT EXISTS`/`OR IGNORE`, preservando preferências) e
as expectativas foram atualizadas para schema 17/future 18 e main read-only.
Asserções de policies, rollback, registros e privacidade continuam presentes;
nenhum teste foi removido/ignored para obter sucesso. A suíte foi repetida
integralmente após essas correções.

Os testes novos verificam default Economy, grafo estático e stripping DEV,
resize nativo sem layouts fixos UIP em Economy, clamp de larguras/persistência
SQLite entre reaberturas, políticas cognitivas intactas e rejeição Auto/Headless
no contrato persistido de produto. No WebKit de produção: conversa/streaming/
cancelamento/completion antes de importar 3D, histórico/nova conversa, tarefas
preservadas nas transições manuais, GLB/render em Presence, teardown sem canvas/
loop/observer, escolha de modo persistida na fixture, painel sem polling ao
recolher e apenas uma gravação por gesto de resize. A prova de disco real é o
Rust; o IPC do probe é sintético. Larguras inválidas restauradas são testadas
em uma nova WebView. Os 20 ciclos reais DEV e 20 determinísticos da 1A, incluindo
seis falhas parciais/retry e cancelamento, continuam sendo executados.

| Validação final | Resultado |
| --- | --- |
| `npm run typecheck` | Exit 0 |
| `npm run build` | Exit 0; chunk 3D opt-in separado |
| `node scripts/test-presentation-lifecycle.cjs` | Exit 0; 20 ciclos + 6 falhas parciais/retry |
| `node scripts/test-economy-shell.cjs` | Exit 0; default, clamp, janela, grafo estático/DEV stripping |
| `node scripts/test-provider-operations.cjs` | Exit 0 |
| `node scripts/test-provider-operations-dom.cjs` | Exit 0 |
| `node scripts/test-allocation-settings.cjs` | Exit 0; 85 checks |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0 |
| `cargo check --release --manifest-path src-tauri/Cargo.toml` | Exit 0 |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=2` | Exit 0; **976 passed, 0 failed, 2 ignored**, 364,41 s |
| `cargo build --release --manifest-path src-tauri/Cargo.toml` | Exit 0; binário amostrado acima |
| Baseline cognitiva isolada da 1A | Exit 0; 1 passed, 977 filtered out |
| Dois testes Shell WebKit de produção | Exit 0; instalação limpa e restore inválido/URL DEV ignorada |
| Lifecycle WebKit DEV da 1A e dois boot contracts | Exit 0; 20 ciclos + erro/retry, Economy/Headless sem requests 3D |
| Comparações WebKit de produção e Tauri release (duas ordens) | Exit 0; dados brutos preservados |
| JSON/AST Python/sintaxe JS e `git diff --check` | OK |

Os dois ignored continuam sendo os gates preexistentes de Codex autenticado/
handshake real. Nenhum deles foi criado para esta entrega.
Warnings preexistentes Rust (15 debug/38 release) e Vite (chunk 3D >500 kB)
foram preservados. `rustfmt` não está instalado nesta toolchain; nenhuma
biblioteca/framework ou dependência foi adicionada.

## Gates humanos e limites

Auditoria independente do diff, contratos, capabilities e evidências permanece
obrigatória. Também permanecem pendentes:

- uso físico da Economy: resize compacto/largo, splitters, restaurar preferências,
  minimizar/maximizar/restaurar/fechar, configurações gerais e IA/modelos;
- Presence nativa: Luna visível, Idle/Wave/raycast, transparência e interação,
  Economy → Presence → Economy sem canvas duplicado;
- conversa com provider real nessas transições: sessão/TaskId, streaming,
  cancelamento, histórico/retry/retomada; nenhuma policy cognitiva diferente;
- foco/desfoco/minimização nativos sem workload concorrente, endurance maior,
  plataformas/DPI além do Fedora observado.

Não há prova absoluta de ausência de leaks/contextos no driver. GC/cache de
módulos e drivers podem reter memória após teardown; a economia de **boot frio**
não deve ser confundida com devolução instantânea de todo RSS após uso de 3D.

## Arquivos principais

`src/App.tsx`, `src/shell/{EconomyShell,OperationalSummary,shellPreferences}`,
`src/shell/economy.css`, `PresentationController`, `WindowController`,
`GeneralSettingsApp`; migration 017 / `persistence/shell_settings.rs`, comandos
isolados em `cognition/settings.rs`, handlers/build manifest/capabilities;
configuração da janela Tauri; scripts e fixtures PERF-1B e adaptação dos probes
1A para Presence explícita. Core/3D/assets/identificadores persistentes preservados.


## Auditoria independente técnica — 07/10/2026

**Resultado:** **PASS técnico. Nenhuma FIX é exigida antes do gate humano.**

A revisão independente do commit
`e30e4d9d0eaa26cbb33353504c914e114e4b319d` confirmou os seguintes pontos.

### Default Economy e fronteira 3D

- `PresentationController` inicia em `economy`;
- `App` inicia com Economy antes da hidratação e só seleciona Presence após
  preferência persistida explícita;
- falha de leitura das preferências permanece fail-safe em Economy;
- `PresenceSurface` continua como boundary lazy do `AvatarViewport`;
- o grafo inicial não depende de Three.js/GLB/WebGL;
- o probe de produção observa zero canvas, contextos WebGL, draw/frames 3D e
  requests do asset/chunk Presence antes do opt-in;
- retornar Presence → Economy reutiliza o teardown auditado na PERF-1A.

A auditoria aceita que módulos ES já carregados após um opt-in não sejam
"descarregados" do cache da mesma WebView. O requisito da 1B é que o caminho
inicial/default Economy não os carregue e que os recursos ativos da Presence
sejam desmontados.

### Persistência e isolamento do Core

A migration 017 adiciona somente `shell_settings` e defaults para `economy`.
Modo de Presentation e layout usam writes próprios, sem regravar policies
cognitivas ou general settings.

Os contratos persistidos de produto aceitam somente `economy|presence`; Auto e
Headless permanecem fora da superfície de produto.

A leitura do painel operacional usa o snapshot em memória do Scheduler e não
executa providers nem lê credenciais/DB. A main recebeu apenas a permissão
read-only correspondente; mutations de policy continuam fora da shell.

Não foi encontrada mudança intencional em provider/model/thinking/context,
output, admission, rate, allocation, handoff ou prioridade de Scheduler.

### Economy Shell

A estrutura implementada corresponde ao escopo aprovado:

- topbar Narys e controles de janela;
- navegação esquerda recolhível;
- workspace central;
- painel operacional direito recolhível;
- splitters com clamp e persistência somente no commit do gesto;
- layout estreito com rail e painel operacional on-demand;
- conversa permanece acima do lifecycle da Presence;
- CPU/RAM/uptime não são exibidos como fatos sem fonte nativa apropriada.

O `OperationalSummary` usa o poller já existente, no máximo ~1 Hz, pausa em
`document.hidden` e é desmontado ao recolher o painel. Não foi introduzido RAF
2D ou loop decorativo contínuo.

### Janela

A configuração passa a 1120×720, mínimo 640×480 e resize habilitado para
Economy. Presence continua com os layouts UIP compactos e resize desabilitado
nesse modo.

A criação nativa ainda mantém `transparent:true` para compatibilidade com
Presence, enquanto Economy aplica backgrounds opacos DOM/nativo. Isso está
corretamente documentado como limitação da janela única; a auditoria não exige
recriar WebView/janela dentro da 1B.

### Evidência de performance

A evidência é suficiente para gate humano:

- comparação Tauri **release** em duas ordens, com processo novo por cenário;
- mesma metodologia de warmup + janela útil;
- nenhuma compilação/dev server incluída na árvore medida;
- Economy: média 570.924 KiB / 8,984% de um core lógico;
- Presence: média 664.902 KiB / 131,749% de um core lógico;
- diferença média de RSS ~91,78 MiB;
- faixas de RSS nativas não se sobrepõem nas duas amostras;
- comparação WebKit adicional usa a mesma janela 1120×720 e também mostra
  diferença ampla de CPU/RSS, Presence ~30 FPS e Economy com 0 frames 3D.

A auditoria **não interpreta** esses números como benchmark universal. O host
usa Mesa software, RSS agregado pode contar páginas compartilhadas e são
amostras curtas. O resultado sustenta somente a conclusão necessária à fase:
neste ambiente, Economy reduz de forma mensurável o custo de Presentation sem
mudar a política cognitiva.

### Testes

A alteração das expectativas de schema/permissões observada no diff corresponde
à migration 017 e à nova leitura operacional da main. Não foi identificada
remoção de assert importante, novo ignore ou relaxamento de policy para fabricar
PASS.

A evidência registrada reporta:

- frontend/typecheck/build verdes;
- bateria de lifecycle da 1A preservada;
- testes específicos da Economy Shell;
- probes WebKit DEV/produção;
- cargo check debug/release;
- cargo build release;
- suíte Rust integral com 976 PASS / 2 ignored preexistentes;
- baseline cognitiva isolada sem mudança de contrato.

Não há GitHub Actions associados ao commit; os resultados continuam sendo
evidência do executor local e o gate humano permanece obrigatório.

### Riscos a observar no gate humano

Dois pontos não justificam FIX preventiva, mas devem ser observados no ambiente
real:

1. `WindowController.setPresentation()` executa várias operações nativas
   sequenciais. Se uma API de janela falhar no meio da transição, o erro é
   exibido e a próxima troca pode recuperar, porém a sequência não implementa
   rollback completo de tamanho/min-size/resizable/background. O gate deve
   confirmar transições reais sem erro no Fedora/Wayland.
2. maximizar/restaurar Economy antes de entrar em Presence deve ser testado
   fisicamente, pois compositor e unidades Logical/Physical podem variar entre
   plataformas.

Esses itens viram FIX somente se houver falha reproduzível no gate.

### Decisão

`PERF-1B = PASS TÉCNICO / AGUARDANDO GATE HUMANO FINAL`.

Não abrir PR, não fazer merge e não avançar para PERF-1C antes do gate e do
fechamento documental.


## Registro de direção futura — NARYS-TERM

Durante o gate humano da Economy Shell foi levantada a possibilidade de a Narys
hospedar um terminal Linux real no workspace central.

A decisão é **registrar e adiar**, sem ampliar a PERF-1B.

A trilha futura
[NARYS-TERM — Terminal Runtime & Interactive Shell Surface](NARYS-TERMINAL-RUNTIME-TRACK.md)
cobre PTY real, emulação visual, sessões humanas/agentivas, ownership,
auditoria e approvals.

A view atual `Shell / Home` permanece deliberadamente uma home/launcher.
Nenhum terminal fictício, output sintético ou shell via campo de texto será
introduzido nesta fase apenas para preencher o workspace.

Esse registro não altera o gate da PERF-1B nem cria nova subfase.


## Fechamento com dívidas não bloqueantes — 07/10/2026

**Decisão:** encerrar **PERF-1B = PASS** e converter os gates humanos/refinamentos
restantes em dívida explícita, sem abrir FIX e sem criar nova subfase.

A decisão se apoia no fato de que o objetivo estrutural da etapa já foi provado:

- Economy é o default de produto;
- Presence é opt-in e lazy;
- o caminho inicial Economy não carrega Three.js/GLB/WebGL;
- a Shell 2D é utilizável;
- conversa, histórico, streaming, cancelamento e settings permanecem funcionais;
- janela Economy é redimensionável;
- laterais são recolhíveis/redimensionáveis e persistidas;
- telemetria exposta é factual;
- comparação release mostrou redução mensurável de CPU/RSS;
- suíte automatizada e auditoria independente técnica passaram;
- 1C/1D não foram antecipadas.

### Dívidas transferidas

As seguintes verificações/refinamentos permanecem registradas, mas **não
bloqueiam PERF-1C**:

1. confirmar fisicamente em mais cenários Fedora/Wayland a transição
   Economy maximizada → Presence compacta → Economy, incluindo falhas parciais
   de APIs nativas de janela;
2. confirmar persistência visual completa após restart em Presence e Economy,
   incluindo ausência de flash 3D perceptível no boot Economy;
3. repetir conversa/provider real durante transições e cancelamento após troca
   caso surjam regressões futuras;
4. endurance humano prolongado adicional da Economy além das medições
   automatizadas/release já preservadas;
5. ergonomia fina da Shell em tamanhos extremos;
6. refinamento visual e informacional de `Shell / Home`;
7. possibilidade de rollback transacional mais forte em
   `WindowController.setPresentation()` se uma falha nativa intermediária for
   reproduzida;
8. comportamento cross-platform de unidades Logical/Physical e maximize/restore;
9. criação de janela nativamente opaca específica para Economy, caso medições
   futuras justifiquem abandonar a janela única `transparent:true`.

### Dívida funcional futura separada

A possibilidade de transformar o workspace em terminal Linux real foi
registrada fora da PERF em
[NARYS-TERM — Terminal Runtime & Interactive Shell Surface](NARYS-TERMINAL-RUNTIME-TRACK.md).

Nenhuma das dívidas acima redefine o gate já comprovado da 1B. Elas só retornam
como blocker se houver regressão ou evidência concreta de risco ao Core,
Interaction ou integridade de dados.

**PERF-1B encerrada em PASS.**

Próximo checkpoint formal:

> **PERF-1C — Headless Runtime**
