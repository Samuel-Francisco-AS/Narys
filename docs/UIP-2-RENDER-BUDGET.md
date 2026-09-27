# UIP-2 — Render Budget

Data: **26/09/2026** (America/Fortaleza). Branch `main`, árvore inicial limpa, `git pull --ff-only` sem alterações. HEAD inicial após sincronização: `1d17b33f55b88bf3d61a5d92f646d83029a3a9b7`. **UIP-2 = PASS completo em 26/09/2026.**

## Política implementada

`RenderBudget`, no Avatar Runtime e sem React, é dono da cadência e do relógio dos frames processados. Lê somente `document.visibilityState` e `document.hasFocus()`; atualiza o modo em `visibilitychange`, `focus` e `blur`; remove listeners no `dispose()`. Não tenta detectar oclusão por outra janela.

| Modo | Condição | Teto de trabalho do SceneRuntime | Motivo no diagnóstico |
| --- | --- | ---: | --- |
| `active` | Documento visível e janela focada | 30 FPS | `focused` |
| `background` | Documento visível e janela sem foco | 24 FPS após FIX-1; 15 FPS na medição inicial abaixo | `blurred` |
| `suspended` | `document.visibilityState === 'hidden'` | 0 FPS de update/render | `hidden` |

`SceneRuntime` mantém `renderer.setAnimationLoop()`. Cada callback consulta a política antes de chamar `onFrame()` ou `renderer.render()`. Callbacks adiantados ou ocultos não atualizam animação, skeleton nem WebGL. O agendamento mantém a cadência média sem executar uma rajada de frames para compensar atrasos. O target é um **teto**, condicionado aos callbacks que o WebKit entrega.

O delta vem do último frame **processado**, não do último callback recebido. A 30 FPS fica perto de 33 ms; a 24 FPS, perto de 42 ms (na medição inicial de 15 FPS, perto de 67 ms). O antigo clamp de 50 ms foi substituído por proteção de 600 ms para permitir também callbacks esparsos de uma janela visível sem foco. Ao entrar em `suspended`, o relógio de animação é descartado; ao sair, o primeiro frame usa delta **0**. Assim, Idle e um aceno em andamento ficam semanticamente congelados enquanto o documento está oculto e continuam da mesma região temporal ao retornar. Um atraso visível acima de 600 ms é limitado; se a plataforma não emitir `visibilitychange` ao minimizar, essa garantia de congelamento não pode ser comprovada pelo app.

A política não recria `SceneRuntime`, renderer ou contexto WebGL, não altera câmera, escala, stage, DPR, pixel ratio, materiais ou luzes. O ajuste de DPR continua `Math.min(window.devicePixelRatio, 1.5)`; na máquina medida os dois valores observados foram 1.

## Diagnóstico DEV

O relatório agregado `[UIP-0]` continua a cada aproximadamente 5 s. Acrescenta `renderMode`, `renderReason`, `targetFps`, `periodRenderMode`, `callbacks`, `frames`, `throttledCallbacks` e `suspendedCallbacks`. `frames` conta apenas update+render concluídos. `periodRenderMode` identifica o modo da janela acumulada quando uma transição gera relatório imediato; o modo atual aparece em `renderMode`. A base dos intervalos diagnósticos é limpa na troca de modo para não incluir o tempo suspenso no primeiro intervalo após a retomada. Não há log por frame. Um timer atrasado ou silencioso quando oculto **não comprova**, por si só, a suspensão do renderer; a transição de modo é registrada quando o evento chega.

## Medição no Tauri real

Fedora/Wayland, WebKitGTK, Tauri dev real, `LIBGL_ALWAYS_SOFTWARE=1` confirmado no ambiente do `WebKitWebProcess`. Mesmo método curto da UIP-0/UIP-1: relatórios DEV de ~5 s por ~30 s e `/proc` da árvore de `target/debug/assistente-3d` a cada ~1 s por 30 s. CPU de 100% equivale a um núcleo lógico. RSS é soma de `VmRSS` e pode contar páginas compartilhadas repetidas. O inspector remoto, Vite e a carga do sistema podem influenciar valores; `update+render` mede tempo síncrono JS/WebGL, não tempo total de GPU/Mesa nem taxa de apresentação na tela.

| Idle | UIP-1 final | UIP-2 `active` | UIP-2 `background` |
| --- | ---: | ---: | ---: |
| Modo/target | Irrestrito | `focused` / 30 | `blurred` / 15 |
| FPS processados, média | 55,06 | **30,00** | **15,02** |
| Intervalo médio entre frames, faixa | 18,03–18,25 ms | **33,29–33,39 ms** | **66,60–66,72 ms** |
| `update+render` médio, faixa | 10,94–11,12 ms | **15,06–15,58 ms** | **15,73–16,13 ms** |
| CPU agregada média | 200,8% | **155,4%** | **81,4%** |
| RSS agregado médio | 647,2 MiB | **644,1 MiB** | **642,8 MiB** |
| Drawing buffer | 300×360 | **300×360** | **300×360** |
| DPR / renderer pixel ratio | 1 / 1 | **1 / 1** | **1 / 1** |

As seis janelas estáveis focadas da versão final tiveram FPS `29,98 / 30,11 / 29,96 / 29,99 / 29,98 / 29,98`, 901 frames processados, 1.684 callbacks e 783 callbacks descartados por orçamento. As seis janelas estáveis sem foco tiveram FPS `14,99 / 14,99 / 14,99 / 15,15 / 14,99 / 14,99`, 451 frames, 1.860 callbacks e 1.409 descartados. Em ambas, `resizeCount=0`, `visibilityState=visible`, buffer 300×360, WebGL 2.0 e renderer reportado como `WebKit WebGL`. O cenário sem foco usou uma janela `zenity` de apoio; o app continuou visível (`hasFocus=false`). CPU caiu ~23% na amostra focada em relação à UIP-1 e ~48% adicional no cenário sem foco. As amostras não isolam causalidade absoluta. O tempo síncrono por frame ficou maior que na UIP-1; não há conclusão de que todo frame individual ficou mais barato.

### Hidden/minimized e limites da plataforma

Não foi possível minimizar/restaurar a janela Tauri de forma confiável pelo ambiente Codex no GNOME/Wayland. **Não há métrica real de CPU/RSS minimizada e não há gate real de suspensão/restauração concluído.** Uma prova isolada da política simulou `visibilitychange` e 40 s ocultos: nenhum frame foi permitido, o primeiro delta da retomada foi 0, e os listeners foram removidos no fim. Em Tauri real, uma injeção temporária e reversível de `visibilityState='hidden'` no documento produziu dois relatórios de ~5 s com **0 frames processados**, embora o WebKit continuasse entregando 310/311 callbacks descartados como `suspended`. A restauração do valor real manteve o buffer 300×360 e `gl.getError()=0`. Essas provas verificam a reação ao **sinal**, não a emissão desse sinal ao minimizar pelo compositor.

Houve também uma condição visível sem foco em que o WebKit passou a entregar ~2 callbacks/s e, após outro reinício atrás de uma janela, nenhum callback, mantendo `visibilityState=visible` e `hasFocus=false`. O target continuou 15 FPS, mas a política não pode produzir frames sem callbacks. Esse caso não equivale a `hidden`, não autoriza inferir oclusão real e pode degradar a fluidez. A proteção de delta de 600 ms foi adicionada após observar esse comportamento. A amostra numérica da tabela foi repetida **depois** do ajuste. A fluidez visual nessa condição extrema ainda precisa de Sam.

## Funcionalidade e verificações

- Na amostra Tauri ativa, Luna carregou e Idle continuou por 30 s. Na amostra sem foco, houve 15 FPS processados por 30 s. `gl.getError()` retornou 0 após interação e após retomada sintética; nenhum `webglcontextlost` foi observado. Não se mediu duração visual de Idle, que é cíclico.
- O raycast por `pointerdown` no centro do canvas acionou `Luna está acenando.` e o status voltou ao Idle em **~3,30 s** com a janela focada na versão final. Um segundo aceno atravessou `blur` aos ~0,89 s e `focus` aos ~2,65 s, retornando ao Idle aos ~3,29 s. Na condição WebKit de ~2 callbacks/s **antes** da correção do delta, o aceno levou cerca de 16,4 s, o que expôs o problema. Após a correção, uma prova temporal com o `AnimationDirector` e um clipe Wave de 3,6 s (duração do GLB atual) retornou ao Idle em ~3,30 s a 30 FPS e ~3,33 s a 15 FPS; com 40 s de suspensão após 1 s, em ~43,30 s de tempo de parede. A fluidez visual ainda requer avaliação humana.
- O DEV abriu, permitiu navegar por Core, Memory, Gemini, Cognition e Security, fechou e manteve o buffer 300×360. Não há regra especial de FPS para o DEV; uma amostra focada com DEV aberto registrou 30,08 FPS. A fluidez visual do overlay precisa de confirmação humana nesta rodada.
- Prova isolada do `RenderBudget`: 30 frames em 1 s ativo, 16 em 1 s background (borda do intervalo), delta background ~67 ms, zero frames em 40 s hidden, primeiro delta 0 após retomada e delta de 500 ms para callback esparso enquanto visível.
- `npm run typecheck`, `npm run build` e `git diff --check`: PASS. O aviso conhecido de chunk JS acima de 500 kB permanece. Nenhum arquivo Rust mudou; `cargo check/test` não foram necessários.

## Gate humano pendente para Sam

1. Com `LIBGL_ALWAYS_SOFTWARE=1 npm run tauri dev` e inspector/diagnóstico, observar Luna focada por 20–30 s: Idle fluido, `renderMode=active`, target 30, sem alteração visual de câmera/escala/barra.
2. Acionar greeting por clique; confirmar duração e fluidez próximas do aceno anterior e retorno ao Idle. Abrir/fechar DEV e navegar seções com a janela focada; target deve permanecer 30.
3. Tirar foco mantendo Luna visível; confirmar `background`, target 24 após FIX-1, movimento temporal normal; recuperar foco e confirmar `active`, target 30. Testar também um aceno iniciado antes de perder foco.
4. Minimizar por 20–30 s; se possível observar CPU/RSS. Restaurar e verificar reaparecimento, continuidade do Idle/aceno sem salto ou aceleração, primeiro frame sem delta enorme, target 30, buffer 300×360, `gl.getError()=0` e ausência de context loss. Registrar `visibilityState`: se não virar `hidden`, informar essa limitação específica da plataforma.

**UIP-2 = PASS completo.** UIP-3 ainda não foi iniciada; nenhuma ergonomia de janela foi implementada. Para UIP-3, permanece o risco de a política receber poucos ou nenhum callback quando a janela visível fica atrás de outras superfícies no WebKitGTK/Wayland. A etapa futura deve respeitar a fronteira do Avatar Runtime e revalidar foco/visibilidade em novos modos de janela, sem presumir detecção de oclusão.

## FIX-1 — background de 15 para 24 FPS

Sam observou no gate humano que **15 FPS com a janela visível sem foco parecia travado demais** para a Luna como companhia desktop. Esta FIX altera apenas `BACKGROUND_FPS` de 15 para 24. `active` continua em 30; `suspended` continua com zero update/render. Política, relógio/delta, retomada, renderer, Presence Shell, stage, câmera e DPR não mudaram. Branch `main`, árvore inicial limpa, HEAD inicial `53953edb5391edcb56e650a71b4663410e084a3f`; sem commit/push nesta rodada.

### Medição Tauri real

Mesma metodologia curta descrita acima: Fedora/Wayland, Tauri dev com `LIBGL_ALWAYS_SOFTWARE=1` confirmado no `WebKitWebProcess`, janela `zenity` de apoio para manter a Luna `visible` e `hasFocus=false`, seis relatórios estáveis de ~5 s e CPU/RSS da árvore Tauri por 30 s. São execuções distintas; Vite, inspector, WebKit e carga do sistema podem variar. FPS significa frames efetivamente processados pelo SceneRuntime, não apresentação comprovada na tela.

| Idle visível sem foco | Background 15 anterior | FIX-1 background 24 |
| --- | ---: | ---: |
| Target | 15 FPS | **24 FPS** |
| FPS processados, média | 15,02 | **24,00** |
| Intervalo médio entre frames, faixa | 66,60–66,72 ms | **41,57–41,74 ms** |
| `update+render` médio, faixa | 15,73–16,13 ms | **20,81–21,23 ms** |
| CPU agregada média | 81,4% | **141,7%** |
| RSS agregado médio | 642,8 MiB | **628,4 MiB** |
| Frames processados em seis relatórios | 451 | **721** |
| Callbacks recebidos | 1.860 | **1.554** |
| Callbacks descartados pelo orçamento | 1.409 | **833** |
| Buffer; DPR/pixel ratio | 300×360; 1/1 | **300×360; 1/1** |

Os seis FPS da FIX-1 foram `24,00 / 23,98 / 23,98 / 23,98 / 24,12 / 23,96`; `resizeCount=0` em todos. A CPU da amostra subiu **60,3 pontos percentuais**, cerca de **74%** frente à amostra de 15 FPS, e ficou próxima dos 155,4% medidos no `active` da UIP-2 original. É uma troca material entre fluidez e custo; a medição curta não prova que somente os nove FPS adicionais causaram toda a diferença. O RSS menor nesta execução também não é evidência de uma economia causada pelo target.

### Animação e transições

- O Idle permaneceu em andamento nos ~30 s sem foco, com target 24. O raycast por `pointerdown` no centro do canvas acionou o greeting nesse modo e o status voltou ao Idle em **~3,31 s**. `gl.getError()=0`; buffer 300×360.
- Um teste no mesmo Tauri controlou os sinais `hasFocus`/`focus`/`blur` temporariamente pelo inspector: o greeting começou em `active`, entrou em `background` aos ~1,05 s, voltou a `active` aos ~2,43 s e retornou ao Idle aos **~3,29 s**. Após a recuperação sintética, um relatório estável marcou **29,98 FPS**, target 30, buffer 300×360 e nenhum erro de console. A substituição temporária de `hasFocus` foi removida ao final. Isso verifica a política e a continuidade do aceno, **não** equivale a recuperar fisicamente o foco pelo compositor.
- O GNOME/Wayland recusou `org.gnome.Shell.FocusApp` com `AccessDenied`; esta execução não conseguiu controlar a recuperação de foco físico. A transição real já tinha funcionado na implementação UIP-2 anterior, mas precisa de nova observação humana para esta FIX. A limitação anterior de poucos ou nenhum callback visível sem foco continua possível; target 24 é teto, não garantia nessas condições.

### Gate pendente

Sam deve comparar visualmente 24 FPS sem foco com os 15 FPS rejeitados, confirmar se a fluidez agora serve ao uso cotidiano e se o aumento de CPU observado é aceitável. Confirmar também aceno iniciado antes de perder foco, retorno a 30 FPS ao recuperar foco e ausência de salto. O gate de minimizar/restaurar da UIP-2 continua pendente. **FIX-1 aprovada no gate humano; UIP-2 = PASS completo. UIP-3 não foi iniciada.**


## Fechamento humano da UIP-2

Em 26/09/2026, Sam aprovou o comportamento final da política de render após a FIX-1. O perfil visível sem foco em **24 FPS** foi considerado adequado para o uso cotidiano da Luna como companhia no desktop; o perfil anterior de 15 FPS havia sido rejeitado por parecer visualmente travado. O modo focado permanece em **30 FPS** e o modo oculto/suspenso permanece com **0 FPS de update/render** quando o sinal de visibilidade é emitido.

Com os gates técnicos, as medições em Tauri real e o gate humano final, **UIP-2 — Render Budget = PASS completo**.

Política final:
- visible + focused: **30 FPS**;
- visible + unfocused: **24 FPS**;
- hidden/suspended: **0 FPS de update/render**.

A limitação de WebKitGTK/Wayland quanto à entrega de poucos callbacks sem foco e à emissão de `visibilityState=hidden` ao minimizar continua documentada para revalidação durante a UIP-3 e UIP-7.

Próxima etapa: **UIP-3 — ergonomia da janela**.


## Mudança de baseline após o PASS da UIP-2 — Idle manual

Em **26/09/2026**, depois do fechamento humano da UIP-2/FIX-1 e **antes da UIP-3**, o commit `862b445` substituiu `public/models/Luna.glb` pela exportação Blender contendo a nova Idle manual.

Essa alteração não muda a decisão da UIP-2 nem sua política 30/24/0 FPS, mas muda a carga animada que o runtime processa. O GLB passou de 2.490.812 para **4.206.852 bytes** (~+68,9%) e os clipes exportados passaram de 17 para **462 canais por clipe**. A Idle atual dura ~10,04 s; o Wave legado reexportado dura ~3,58 s.

Consequência metodológica:

- os números de CPU, RSS, FPS e `update+render` acima pertencem ao **asset anterior**;
- UIP-3 e etapas posteriores devem registrar uma **nova baseline pós-asset** antes de atribuir diferenças ao trabalho de janela/UI;
- se houver regressão, separar primeiro custo da exportação Blender/canais de animação de custo da ergonomia da janela;
- otimização do GLB (por exemplo, redução de canais constantes) deve ser medida como rodada própria para não misturar variáveis.

Detalhes: [MANUAL-IDLE-INTEGRATION.md](MANUAL-IDLE-INTEGRATION.md).
