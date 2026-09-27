# UIP-3 — Ergonomia da janela (candidata)

Data: **27/09/2026** (America/Fortaleza). Branch `main`, árvore inicial limpa, `git pull --ff-only` sem alterações. HEAD inicial após o pull: `7afb843ed0ced7c1f045046911f5fc527d605288`. Sem commit/push. **UIP-3 não é PASS completo:** faltam gates humanos de empilhamento, Alt+arrastar e fluidez, e o click-through permanece bloqueado por segurança.

## Baseline pré-UIP-3 — Idle manual

Antes de qualquer alteração de janela, `python scripts/validate_luna_glb.py` passou: `Luna.glb` tem **4.206.852 bytes**, Idle **10,042 s / 462 canais** e Wave **3,583 s / 462 canais**. O Tauri real iniciou com `LIBGL_ALWAYS_SOFTWARE=1` em Fedora/GNOME/Wayland. Foram recolhidos relatórios `[UIP-0]` de aproximadamente 5 s e CPU/RSS da árvore Tauri a cada segundo por 25 s em cada condição. CPU de 100% equivale a um núcleo lógico; RSS é soma de `VmRSS` e pode contar páginas compartilhadas. `update+render` mede trabalho síncrono JS/WebGL, não apresentação física na tela.

| Idle manual, antes da implementação | ACTIVE, focada | BACKGROUND, visível sem foco |
| --- | ---: | ---: |
| Target | 30 FPS | 24 FPS |
| FPS processados, média de 5 relatórios | **29,99** | **24,01** |
| Frames / callbacks | 751 / 1.288 | 601 / 1.329 |
| `update+render` médio | **18,19 ms** | **20,96 ms** |
| CPU agregada média | **165,0%** | **143,1%** |
| RSS agregado médio | **658,7 MiB** | **656,1 MiB** |
| Stage / drawing buffer | 300×360 / 300×360 | 300×360 / 300×360 |
| DPR / renderer pixel ratio | 1 / 1 | 1 / 1 |

Na janela sem foco, uma janela `zenity` ficou ativa e a Luna permaneceu `visibilityState=visible`, `hasFocus=false`; callbacks foram suficientes para 24 FPS. Na focada, `visibilityState=visible`, `hasFocus=true`. `resizeCount=0` e WebGL 2.0 em todos os relatórios. A Idle manual permaneceu em loop ao longo das amostras. Um `pointerdown` no corpo acionou `Luna está acenando.`; depois o estado voltou a `animação de repouso`. O buffer permaneceu 300×360 e `gl.getError()` retornou 0.

**As métricas numéricas da UIP-2 foram obtidas com o GLB anterior** (2.490.812 bytes; 17 canais por clipe). Não são baseline causal para esta fase. A diferença possível de CPU e `update+render` da amostra acima pode vir do novo asset; não houve otimização, redução de canais ou reexportação de Idle/Wave na UIP-3.

## Implementação e APIs da stack instalada

`src/window/WindowController.ts` concentra as chamadas Tauri de janela. O `App` só cria/descarta o controller, apresenta o estado no DEV e intercepta `Alt + botão esquerdo` na captura de `pointerdown` da Presence Shell. Esse evento para antes do listener de raycast do canvas; clique normal segue até o avatar. O controller inicia o drag nativo por `startDragging()`, alterna always-on-top, lê posição e libera o listener `onMoved()` no `dispose()`. Não há operação de janela nem alteração de FPS dentro do Avatar Runtime. `RenderBudget.ts` permanece intacto.

APIs conferidas em `@tauri-apps/api` **2.11.1** e Tauri Rust **2.11.6** instalados: `getCurrentWindow().isAlwaysOnTop()`, `setAlwaysOnTop()`, `startDragging()`, `outerPosition()`, `onMoved()` e `setIgnoreCursorEvents()`. As permissões foram conferidas no schema gerado do projeto. Adicionadas à capability `main-window`:

- `core:window:allow-is-always-on-top`
- `core:window:allow-set-always-on-top`
- `core:window:allow-outer-position`
- `core:window:allow-start-dragging`
- `core:event:allow-listen` e `core:event:allow-unlisten` para `onMoved()`

O controller contém uma entrada protegida para `setIgnoreCursorEvents()`, mas `recovery` permanece `unavailable` e a capability **não** concede `core:window:allow-set-ignore-cursor-events`. O botão DEV fica desabilitado. Não foi adicionado plugin ou comando Rust.

O painel DEV mostra modo solicitado de always-on-top, click-through off, recuperação indisponível e posição quando reportada. A Presence normal, janela configurada 320×420, CharacterStage 300×360, canvas, câmera, barra/chão, transparência e avatar não foram modificados.

## Always-on-top e Fedora/GNOME/Wayland

No backend Wayland nativo desta máquina, `setAlwaysOnTop(true)` resolve sem erro, mas `isAlwaysOnTop()` ainda retorna `false` após espera. Não há evidência de que o GNOME tenha efetivado o empilhamento. O [GTK 3 avisa que o gerenciador de janelas pode não honrar `set_keep_above()`](https://gnome.pages.gitlab.gnome.org/gtk/gtk3/method.Window.set_keep_above.html). Por isso o painel reflete o **modo solicitado**, e a confirmação visual nesse backend fica pendente.

Uma execução separada do mesmo Tauri com `GDK_BACKEND=x11 LIBGL_ALWAYS_SOFTWARE=1` no desktop GNOME/Wayland usou XWayland. Nela, **5 ciclos completos on → off** mudaram `_NET_WM_STATE_ABOVE` exatamente junto com o controle DEV; ao desligar, a flag sumiu. O canvas e o contexto WebGL mantiveram a mesma identidade, buffer 300×360 e `gl.getError()=0`; não houve `resizeCount` nos relatórios. Ao reiniciar com always-on-top salvo como true, o modo voltou e a flag `_NET_WM_STATE_ABOVE` reapareceu; depois foi desligado e o valor salvo voltou a false. `isAlwaysOnTop()` retornou um valor antigo imediatamente após `setAlwaysOnTop()` também em XWayland; o controller acompanha o pedido após a chamada bem-sucedida, sem tratar o getter imediato como confirmação do compositor.

XWayland foi uma **sonda**, não uma mudança do backend padrão. A acessibilidade reportou frame externo 320×457 para conteúdo 320×420 nessa execução; não foi possível concluir se há decoração visível. A aparência sem bordas e o empilhamento sobre VS Code/browser exigem observação de Sam antes de adotar esse caminho.

## Drag, click-through e recuperação

O gesto implementado é `Alt + botão esquerdo + arrastar` em qualquer área da Presence Shell fora do overlay DEV. Ele chama `startDragging()` uma vez por início do gesto; não há loop manual de `setPosition()`. Um `pointerdown` sintético com Alt no canvas deixou o estado em Idle e não reportou erro de IPC. Um `pointerdown` normal no mesmo ponto acionou Wave e voltou à Idle. A automação não conseguiu entregar um arrasto físico confiável ao GNOME/Wayland; o conforto e deslocamento efetivo do gesto são gate humano.

Foram feitos **5 reposicionamentos programáticos** da janela XWayland via `XMoveWindow`, com posições diferentes e retorno à posição inicial. O listener `onMoved()` atualizou a posição DEV; canvas/contexto mantiveram identidade, buffer 300×360 e WebGL erro 0. Isso verifica estabilidade após mover, **não** comprova cinco ciclos do gesto Alt+arrastar. No backend Wayland nativo, `outerPosition()` informou `0,0`, que não deve ser usado como posição global confiável.

O [plugin oficial Tauri Global Shortcut](https://v2.tauri.app/plugin/global-shortcut/) existe, mas há [relato aberto de registro sem callback no Fedora/GNOME/Wayland](https://github.com/tauri-apps/plugins-workspace/issues/3267). Neste GNOME, a extensão AppIndicator/KStatusNotifierItem está instalada porém **desabilitada**; um tray não seria recuperação garantida. O portal `org.freedesktop.portal.GlobalShortcuts` e o backend GNOME apareceram no D-Bus, mas exigiriam integração, concessão do atalho e teste físico externo antes de sustentar click-through. Nenhum atalho externo foi comprovado nesta rodada. Portanto **não há ativação de click-through, nem ciclos on/off**, e o app reinicia sempre interativo. A janela inteira passaria os cliques se a API fosse habilitada no futuro; não foi presumida passagem apenas nos pixels transparentes.

## Persistência

Somente o pedido de always-on-top é salvo em `localStorage` (`luna.window.alwaysOnTop`). Posição é exibida, mas **não persistida**: Wayland retorna coordenadas sem valor global e não houve validação segura de múltiplos monitores. Click-through nunca é salvo e inicia off. O modo normal é restaurado ao final dos testes.

## Render Budget e performance durante UIP-3

Política preservada: `visible + focused` → target 30; `visible + unfocused` → target 24; `hidden` → 0 update/render quando o sinal chega. Always-on-top e click-through não criam modos de FPS. A amostra nativa pré-implementação demonstrou 30/24 com o GLB atual. Depois da implementação, uma amostra nativa obteve três relatórios completos focados de **29,98 / 30,09 / 29,97 FPS**, com `update+render` médio de **14,36 ms**, buffer 300×360 e `resizeCount=0`; em seguida houve `blur` e uma janela de ~4,2 s marcou **23,99 FPS**, target 24. A captura de CPU/RSS de 22 s atravessou a troca de foco (157,0% / 664,1 MiB) e não é comparação limpa por modo. O GNOME impediu focar fisicamente a janela Wayland por uma chamada de automação: até uma permissão temporária de teste para `setFocus()` retornou sucesso sem mudar `document.hasFocus()`; essa permissão foi removida. O foco surgiu depois durante a interação na Presence Shell, permitindo a amostra acima. Não foi criado timer alternativo para fabricar callbacks.

Em XWayland, com always-on-top solicitado e outra janela focada, cinco relatórios de ~5 s mostraram `visible`, `hasFocus=false`, target 24, **23,98–24,11 FPS**, ~15,47 ms de `update+render`, CPU agregada ~139,6% e RSS ~650,0 MiB. Com always-on-top desligado e a janela coberta, uma janela começou em 13,19 FPS e as três seguintes tiveram **0 callbacks / 0 frames**, ainda com `visible` e target 24. Isso reproduz o risco WebKitGTK de oclusão. Uma tentativa de foco artificial via X11 produziu `hasFocus=true`, target 30, porém só ~10 callbacks/s; não equivale a foco físico concedido pelo compositor e não foi usada como benchmark 30 FPS. As amostras XWayland não isolam custo de janela frente à baseline Wayland nativa, pois o backend gráfico mudou. Não apareceu perda grosseira de memória nem erro WebGL nos ciclos observados; isto **não é teste de leak**.

## Gates e checklist humano para Sam

1. Always-on-top mantém Luna sobre VS Code/browser?
2. Desligar always-on-top permite outra janela cobri-la?
3. Alt+arrastar é confortável e move a Luna em cinco trajetos reais?
4. Clique normal ainda aciona greeting, sem aceno disparado pelo drag?
5. Luna continua fluida com outra aplicação focada?
6. Click-through realmente deixa clicar na aplicação atrás? **Bloqueado nesta candidata.**
7. Recuperação de click-through funciona sem clicar na Luna? **Bloqueado nesta candidata.**
8. É impossível ficar preso no click-through? Hoje a ativação está desabilitada e não é persistida.
9. Mover a janela não produz glitches visuais?
10. Janela continua transparente, sem bordas e com tamanho correto, inclusive se testar XWayland?
11. Idle manual e aceno continuam normais?
12. Reiniciar o app continua seguro e interativo?

**Estado:** candidata técnica com always-on-top funcional como pedido de API e comprovado no XWayland; drag nativo implementado com gesto físico pendente; click-through bloqueado até recuperação externa comprovada. Sem gate humano, **UIP-3 não é PASS completo**. UIP-4 não foi iniciada.
