# UIP-1 — Presence Shell

Data: **26/09/2026** (America/Fortaleza). Branch `main`. HEAD inicial após `git pull --ff-only`: `d44762cbc6613f921b4ee1d96d4b088bbf64a10c`; árvore inicial limpa. Nenhum commit ou push foi feito nesta rodada. **Estado: candidata a PASS técnico; gate visual pendente de Sam.**

## Implementação

- Janela Tauri: de **1120×760**, decorada e opaca, para **440×660 px configurados**, transparente, sem decoração/sombra, não redimensionável e não fullscreen. Não há posicionamento especial. No Fedora/Wayland desta medição, a WebView reportou `innerWidth=492` e `innerHeight=712`, ou seja, **52 px a mais em cada dimensão** que os valores configurados. A causa dessa diferença não foi determinada; o layout foi centralizado dentro da área efetiva sem mudar a resolução do stage.
- `html`, `body`, `#root`, `.presence-shell`, `.character-stage` e `.scene-canvas` têm fundo transparente. Foram removidos o gradiente global, painel e borda da personagem, linha decorativa, topbar, badges, caption, toolbar, botão de aceno e status visíveis no estado Presence. O título HTML passou a `Luna` e a meta de tema escuro foi removida. O renderer já usava `alpha: true`; nenhuma configuração de iluminação, material, câmera, modelo ou animação mudou.
- `CharacterStage` tem **420×604 px CSS fixos**, centralizado na WebView, independentemente dos diagnósticos e de futuras superfícies de UI. O canvas ocupa só essa área. O `ResizeObserver`, `setAnimationLoop()`, DPR e `SceneDiagnostics` permanecem sem alteração. A chevron `⌄` abaixo da personagem é um indicador visual sem clique ou promessa de compositor.
- Os painéis Luna Core/LR-2/LR-3/LR-4/LR-5/LR-6 permanecem acessíveis pelo botão **DEV**, somente em `import.meta.env.DEV`. A sobreposição é absoluta, tem scroll próprio, inicia fechada e não participa do layout do stage. O status da cena aparece apenas ali. A infraestrutura dos painéis e o fluxo de `AnimationIntent` foram preservados.

## Medição curta, Tauri real

Método da [UIP-0](UIP-0-BASELINE.md): Tauri dev real no Fedora 44/Wayland com inspector WebKit em `127.0.0.1:9223`; confirmação de `LIBGL_ALWAYS_SOFTWARE=1` no `WebKitWebProcess`; seis relatórios `[UIP-0]` de aproximadamente 5 s durante **30 s de Idle**, sem interação; CPU e `VmRSS` da árvore `target/debug/assistente-3d` amostrados a cada ~1 s por 30 s. CPU de 100% equivale a um núcleo lógico. RSS é soma de processos e pode contar páginas compartilhadas mais de uma vez. Os scripts temporários da UIP-0 foram reutilizados fora do repositório.

| Métrica Idle | UIP-0 | UIP-1 Presence |
| --- | ---: | ---: |
| Janela configurada | 1120×760 | 440×660 |
| CharacterStage/container CSS | 589,109×1988,063 | 420×604 |
| Canvas CSS | 589×1988 | 420×604 |
| Drawing buffer | 589×1988 | **420×604** |
| DPR / renderer pixel ratio | 1 / 1 | 1 / 1 |
| FPS, média das seis janelas | 27,07 | **51,86** |
| Intervalo médio entre frames, faixa das seis janelas | 32,67–40,67 ms | **19,10–19,44 ms** |
| `update+render` médio, faixa das seis janelas | 11,58–13,96 ms | **11,59–11,78 ms** |
| CPU agregada média | 500,9% | **279,0%** |
| RSS agregado médio | 860,5 MiB | **652,6 MiB** |

Na UIP-1, os FPS por janela foram `51,40 / 52,38 / 52,32 / 51,60 / 51,76 / 51,72`; p50 dos intervalos foi 19 ms em todas elas, e p95 ficou entre 21 e 22 ms. O p95 de `update+render` ficou entre 13 e 14 ms. `resizeCount=0` nas seis janelas, `visibilityState=visible`, `hasFocus=true`, WebGL 2.0 e renderer informado como `WebKit WebGL`. RSS variou de 646,2 a 683,1 MiB durante a amostragem. O buffer contém cerca de **78% menos pixels** que na UIP-0. As duas medições são execuções curtas em momentos distintos; as diferenças de FPS/CPU/RSS acompanham a troca de layout, mas não isolam causalidade nem medem consumo físico exclusivo de RAM.

## Verificações técnicas

- `npm run typecheck`, `npm run build` e `git diff --check`: **PASS**. O build manteve o aviso já existente de chunk JavaScript acima de 500 kB.
- `. "$HOME/.cargo/env"` seguido de `WEBKIT_INSPECTOR_HTTP_SERVER=127.0.0.1:9223 npm run tauri dev`: janela abriu; Luna carregou e o status DEV informou Idle; `gl.getError()=0`. O teste via inspector despachou `pointerdown` no centro real do canvas: o status mudou para `Luna está acenando.` e voltou ao Idle. O movimento e o enquadramento visual ainda exigem inspeção humana.
- Antes, durante e depois de abrir a sobreposição DEV: canvas CSS e drawing buffer permaneceram **420×604**; o centro medido do canvas continuou em `(246, 338)` na WebView observada. A sobreposição não redimensionou o stage. O acionador `⌄` estava presente no DOM.
- Os fundos calculados de `html`, `body`, `#root`, shell, stage e container do canvas foram `rgba(0, 0, 0, 0)`. Isso, a configuração Tauri e `alpha: true` comprovam a cadeia configurada, **não** a composição visual final sobre o desktop. A tentativa de capturar a janela pela API de screenshot do GNOME foi recusada com `AccessDenied: ScreenshotWindow is not allowed`.

## Checklist visual pendente de Sam

Abrir a aplicação no desktop com `npm run tauri dev` e verificar:

1. A Luna parece estar diretamente sobre o desktop?
2. Existe algum retângulo ou fundo residual?
3. As bordas da janela são perceptíveis?
4. A escala da personagem está boa?
5. Ela cabe inteira na janela?
6. O Idle continua visualmente normal?
7. Clique/aceno funciona visualmente e volta ao Idle?
8. O acionador inferior está discreto e bem posicionado?
9. Abrir o diagnóstico DEV altera ou desloca a personagem?
10. O tamanho geral parece próximo de ocupar apenas uma pequena parte da tela?

## Limitações e riscos para UIP-2

- A transparência da composição WebKitGTK/GNOME/Wayland, as bordas e possíveis artefatos de recorte continuam sem aprovação visual. O comportamento exato da diferença **440×660 configurado versus 492×712 reportado** também merece acompanhamento em outros ambientes.
- O loop continua irrestrito e atingiu ~52 callbacks/s com CPU agregada ainda perto de 2,8 núcleos lógicos. A UIP-2 deve medir o efeito do orçamento de render separadamente. `update+render` cobre trabalho síncrono JavaScript/WebGL, não o custo GPU/Mesa completo; FPS é frequência dos callbacks, não apresentação confirmada na tela.
- O overlay DEV cobre a janela pequena quando aberto por design. O gate técnico verificou que não muda o stage; a legibilidade e a convivência visual permanecem para validação humana.

**UIP-2 (Render Budget) e UIP-3 (ergonomia da janela) não foram iniciadas.**
