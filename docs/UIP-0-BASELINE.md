# UIP-0 — contratos e baseline desktop

Data: **26/09/2026** (America/Fortaleza). Base inicial: `69674e9f80b8886a4a5974b83008612b3b0ee423`, branch `main`, árvore limpa antes desta rodada. Esta fase registra o protótipo existente; os modos e limites abaixo são contratos futuros, não recursos implementados.

## Contratos entre runtimes

| Sistema | Autoridade e entradas/saídas previstas | Fronteira |
| --- | --- | --- |
| Window/UI Controller | Modos `presence`, `composer`, `conversation`; composição dos painéis; tamanho/posição da janela; futuramente always-on-top, click-through, atalhos e janelas secundárias. Recebe estado e eventos por interfaces explícitas. | Não conhece detalhes internos de Three.js nem de providers. |
| Avatar Runtime | Three.js/WebGL, avatar, `AnimationIntent`, animações e futura entrada `BehaviorIntent`. Hoje `AvatarViewport` faz a ligação com React e `SceneRuntime` é dono do renderer. | Não conhece sessões, providers, credenciais nem layout interno dos painéis React. |
| Conversation Runtime | Futuramente sessão atual, nova conversa, mensagens, histórico por sessões e estados de resumo; consome serviços/eventos do Core. | Não manipula renderer/avatar diretamente. |
| Luna Core | Autoridade sobre tarefas, identidade, memória, cognição, providers, persistência e eventos; expõe comandos/eventos ao frontend. | Não conhece layout, posição da UI ou detalhes do renderer. |

Estados visuais conceituais: `presence` mostra Luna e um acionador mínimo; `composer` acrescenta a entrada de texto; `conversation` mostra painel de sessão e compositor ao lado de Luna. Abrir/recolher esses elementos não deve trocar a sessão. **Abrir `composer` ou `conversation` não pode aumentar a resolução do `CharacterStage`/renderer.** O tamanho de render deve ser controlado independentemente do crescimento da janela e dos painéis. Esses estados ainda não foram implementados nesta fase.

Estados futuros de janela: normal (outras janelas podem cobrir Luna), always-on-top configurável e click-through configurável com recuperação segura por atalho/tray. São responsabilidades do Window/UI Controller; nenhum foi implementado aqui. A política futura prevê teto normal de 30 FPS e perfis por foco/visibilidade, a definir pela UIP-2 com esta medição como referência. A UIP-0 mantém `setAnimationLoop()` irrestrito.

Caminho futuro de comportamento: `DesktopContextEvent → Behavior Engine → BehaviorIntent → Avatar Runtime`. Eventos de contexto não devem gerar regras dentro do renderer. Nenhuma peça desse fluxo foi implementada aqui.

## Instrumentação DEV

`SceneDiagnostics` é instanciado apenas com `import.meta.env.DEV`, dentro do `SceneRuntime`. Acumula tempos em memória e emite `[UIP-0]` em JSON a cada 5 segundos e nas transições de foco/visibilidade. Não usa estado React nem log por frame. Mede intervalo entre callbacks, FPS efetivo por tempo de parede e duração síncrona de `onFrame` + `renderer.render()` (não é tempo GPU). Cada relatório inclui média/p50/p95/min/max, tamanho CSS do container/canvas, drawing buffer, DPR da janela, pixel ratio do renderer, foco, `visibilityState`, versão e string WebGL e contagem de callbacks do `ResizeObserver`. Timer e listeners são removidos no `dispose()`.

O diagnóstico não controla o loop. Uma avaliação A/B de overhead do próprio coletor não foi feita; seu custo é coleta de dois números por callback e agregação a cada 5 segundos. O WebKit pode reduzir o ritmo por conta própria quando a janela está em segundo plano; isso não é um limite implementado pelo app.

## Metodologia reproduzível

1. No Fedora, iniciar **Tauri real** a partir da raiz: `. "$HOME/.cargo/env"` e `WEBKIT_INSPECTOR_HTTP_SERVER=127.0.0.1:9223 npm run tauri dev`. O workaround existente em `src-tauri/src/main.rs` define `LIBGL_ALWAYS_SOFTWARE=1` antes do WebKit quando não há override. Confirmar `/proc/<WebKitWebProcess>/environ` contém `LIBGL_ALWAYS_SOFTWARE=1`. Não usar `LIBGL_ALWAYS_SOFTWARE=0` no baseline principal.
2. Aguardar a UI informar `Luna · WebGL ativo · animação de repouso`. Ler os relatórios `[UIP-0]` no console do inspector WebKit em `http://127.0.0.1:9223/`. O endpoint usado nesta coleta foi `/socket/1/1/WebPage`, via protocolo WebKit Inspector `Target.sendMessageToTarget` + `Console.enable`. A conexão apenas lê logs; não é o Vite em navegador comum.
3. Para CPU/RSS, identificar a árvore cujo root é `target/debug/assistente-3d`. Amostrar `/proc/<pid>/stat` (utime+stime, ticks por `SC_CLK_TCK`) e `VmRSS` de `/proc/<pid>/status` a cada ~1 segundo por 30 segundos. Incluir o processo principal e filhos WebKit; repetir a descoberta de filhos em cada amostra. CPU agregada = soma dos deltas de ticks dos PIDs presentes no início/fim do intervalo ÷ duração real × 100; **100% equivale a um núcleo lógico**. RSS agregado = soma aproximada de `VmRSS` dos processos observados, sem descontar páginas compartilhadas. O CLI Tauri e Vite ficam fora desta árvore. Ferramentas temporárias de coleta ficaram em `/tmp/uip0-inspector.mjs` e `/tmp/uip0-proc.py`; não são parte do produto.
4. Idle: sem interação por 30 s. Aceno: acionar o botão `✳ Acenar` a cada 5 s durante 22 s; confirmar texto `Luna está acenando.`. Sem foco: abrir uma janela `zenity` temporária por ~25 s e conferir `hasFocus=false`; a janela de apoio não integra a árvore de CPU do app. Resize de container: alterar temporariamente a largura CSS de `.scene-canvas` para 400 px via inspector e restaurá-la; esse é um teste sintético do observer, não resize manual da janela.

Ambiente observado: Fedora 44, Linux 7.2.5, Intel Core i7-3770 (8 CPUs lógicas), Wayland/GNOME, WebKitGTK 2.52.5, Mesa DRI 26.2.2. Tauri 2, React/Three.js em build de desenvolvimento. A janela configurada é decorada, 1120×760 px, mínimo 720×600. A WebView reportou `innerWidth=1120`, `innerHeight=713`.

## OBSERVADO AUTOMATICAMENTE

- O processo principal (`assistente-3d`, PID 12738 nesta execução) tinha dois filhos relevantes: `WebKitWebProcess` (12896) e `WebKitNetworkProcess` (12871). O WebKitWebProcess recebeu `LIBGL_ALWAYS_SOFTWARE=1`. `gl.VERSION` foi `WebGL 2.0` e `gl.RENDERER` foi `WebKit WebGL` (string mascarada: não identifica a GPU). O estado DOM indicou avatar carregado e Idle; o botão acionou o status de aceno. Ao fim, `gl.getError()` retornou `0`. Nenhum erro de console foi capturado nas janelas amostradas. Isso não substitui inspeção visual humana do movimento.
- No Idle, container CSS = **589,109×1988,063 px**; canvas CSS = 589×1988 px; drawing buffer = **589×1988 px**; `window.devicePixelRatio=1` e `renderer.getPixelRatio()=1`. O canvas muito alto decorre do layout atual com painéis diagnósticos e deve ser acompanhado, sem correção nesta fase. O limite de DPR configurado no código continua 1,5.
- Idle, 30 s, seis relatórios de ~5 s: FPS **25,15 / 30,58 / 25,97 / 28,14 / 24,60 / 27,98** (média das janelas 27,07); intervalo médio por janela 32,67–40,67 ms, p50 33–39 ms, p95 36–51 ms; `update+render` médio 11,58–13,96 ms, p95 14–18 ms. `visibilityState=visible`, `hasFocus=true`, `resizeCount=0` nas seis janelas.
- Idle, mesmos 30 s: CPU agregada média **500,9%** (aprox. cinco CPUs lógicas); RSS agregado médio **860,5 MiB**, faixa 860,5–860,6 MiB. No último snapshot: WebKitWebProcess 611,6 MiB, Rust/Tauri 187,7 MiB, WebKitNetworkProcess 61,2 MiB.
- Aceno, 22 s: o status `Luna está acenando.` foi observado após o clique. Quatro relatórios de ~5 s: FPS **26,78 / 23,18 / 23,38 / 27,38** (média 25,18); intervalo médio 36,54–43,22 ms, p50 33–41 ms, p95 43–52 ms; `update+render` médio 12,06–14,14 ms, p95 16–18 ms. CPU agregada média **497,4%**; RSS agregado médio **875,8 MiB** (faixa 860,8–889,2 MiB). Diferenças em janelas curtas não bastam para atribuir custo ao aceno.
- Sem foco, janela `zenity` aberta, ~24 s: cinco relatórios consecutivos de ~5 s marcaram `hasFocus=false`, `visibilityState=visible`; FPS **26,78 / 24,39 / 29,78 / 25,78 / 27,96** (média 26,94). CPU agregada média **497,2%**; RSS agregado médio **897,8 MiB** (faixa 880,5–900,4 MiB). Houve `focuschange` para `hasFocus=true` após o fechamento da janela. O app continuou renderizando sem foco nessa amostra.
- Teste sintético de resize do container: largura CSS 589,109 → 400 → 589,109 px; drawing buffer 589 → 400 → 589 px; altura permaneceu 1988 px. O `ResizeObserver` registrou um callback em cada mudança. Nenhuma alteração de código ou estilo ficou aplicada ao app.

## PENDENTE DE VALIDAÇÃO HUMANA

- **Minimizar/restaurar:** não foi possível controlar de forma confiável a janela Tauri no compositor Wayland com as ferramentas disponíveis. Sam deve abrir o inspector, registrar dois relatórios antes, minimizar por 20–30 s, restaurar e registrar os dois primeiros após o retorno. Anotar `visibilityState`, `hasFocus`, FPS, intervalo máximo, CPU/RSS agregados, retorno do Idle e qualquer erro/context loss. A emissão do timer pode ser pausada pelo WebKit; não interpretar silêncio do console como suspensão comprovada do renderer.
- **Resize manual da janela:** redimensionar para menor/maior e registrar tamanho CSS/drawing buffer, `resizeCount`, DPR, eventual erro WebGL e se a Luna permanece visível. O teste sintético acima valida apenas o observer quando o container muda.
- **Avaliação visual:** confirmar Luna visível, fluidez do Idle e do aceno, retorno ao Idle e ausência de artefatos no Tauri/Mesa software. O status DOM e o contexto WebGL foram observados automaticamente; a aparência não recebeu aprovação humana nesta rodada.

## Limitações e pontos para UIP-2/UIP-7

- A soma de RSS conta memória compartilhada mais de uma vez e é uma aproximação de footprint de processos, não memória física exclusiva. O RSS subiu entre cenários sequenciais; não foi feito ciclo prolongado ou repetição controlada para diagnosticar vazamento. A inspeção remota e o Vite de desenvolvimento também podem afetar a medição.
- FPS é a frequência do callback de `setAnimationLoop()`, não taxa de apresentação efetiva da tela. `update+render` mede tempo síncrono em JavaScript/WebGL, não execução completa na GPU/Mesa. p50/p95 são percentis por janela de 5 s, não uma distribuição global de 30 s.
- O canvas atual, com quase 2 mil pixels de altura, é um risco de custo de render. Na UIP-2 comparar novamente dimensões, FPS/intervalos, CPU/RSS e contexto WebGL ao avaliar teto de 30 FPS, DPR e perfis de foco/visibilidade; só então atribuir ganhos. Na UIP-7 comparar também abertura/recolhimento de UI e ciclos para crescimento de memória. A regra de resolução fixa ao abrir `composer`/`conversation` deve ser verificada quando esses modos existirem.
