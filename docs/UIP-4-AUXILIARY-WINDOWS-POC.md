# UIP-4-FIX-2A — POC de superfícies auxiliares nativas

Data: 27/09/2026, Fedora/GNOME/Wayland nativo. Branch `main`, árvore inicial limpa; `git pull --ff-only` sem alterações; HEAD inicial e final sem commit: `0ef28b7541eeb8cffea74eab2a6339f0615e56ce`.

**Decisão: FAIL no gate do POC.** A implementação técnica cria, mostra, oculta e reutiliza duas janelas, mantendo a main estável depois da inicialização. A relação espacial real e o comportamento de Alt+drag não puderam ser comprovados na sessão automatizada: Wayland reporta `(0,0)` para todas as janelas e a captura do GNOME foi negada. O Composer de 80 px foi expandido pelo GTK/WebKit para 200 px; o RSS agregado da árvore Tauri cresceu aproximadamente 503 MiB com as duas WebViews. UIP-4 continua **CANDIDATA**. Não migrar os componentes de produção para estas janelas nem iniciar FIX-2B com base neste POC.

**Encerramento UIP-4-FIX-2B-CLEANUP:** após o teste, o código experimental foi removido e a aplicação voltou ao modelo funcional de uma única WebView da UIP-4-FIX. Este documento permanece como registro histórico do FAIL, não como descrição da arquitetura ativa. O posicionamento imprevisível no Wayland e o custo operacional de aproximadamente **+503 MiB de RSS agregado** impedem seguir para uma FIX-2B baseada em múltiplas WebViews. A estabilidade espacial fica para investigação posterior, preferencialmente na UIP-7 ou em uma rodada nativa dedicada.

## Motivo e escopo

O gate humano de Sam mostrou que o resize nativo 310×410 ↔ 310×490 e 310×490 ↔ 625×490 desloca a Luna como uma mola e mascara as transições CSS. `WindowController.setLayout` e a permissão de `setSize` foram removidos. A main monta somente a Luna e os diagnósticos DEV temporários; o acionador e os componentes de conversa de produção ficam temporariamente fora da árvore React. `src/conversation/*`, Gemini, sessões, streaming, TaskId, câmera, FOV, asset, Idle/Wave, CharacterStage e RenderBudget não foram modificados.

## API e relação nativa

- Tauri Rust 2.11.6 instalado: `WebviewWindowBuilder::new`, `WebviewUrl::App`, `inner_size`, `resizable(false)`, `decorations(false)`, `transparent(true)`, `shadow(false)`, `skip_taskbar(true)`, `focused(false)`, `visible(false)`, `parent(&main)`, `build`; depois `show` e `hide`. A main encerra o POC quando é destruída. Botão fechar e close request nas auxiliares chamam `hide`; show posterior reutiliza a mesma janela. Um comando Tauri aceita apenas os labels `composer` e `conversation`, com capabilities restritas às três janelas. Em release o comando retorna erro de indisponibilidade.
- `parent` e `transient_for` **não são variantes distintas no Linux desta stack**: no código instalado de Tauri, `WebviewWindowBuilder::parent` delega ao `WindowBuilder::parent`, que chama `transient_for(&parent.gtk_window()?)`. `WebviewWindowBuilder::transient_for` chega à mesma chamada GTK. Foi testado o caminho `parent`, que efetivamente usa `transient_for`; não houve segunda rodada redundante.
- Não foi usado `setPosition`, `outerPosition` como âncora, XWayland, plugin, always-on-top novo ou click-through. A main conserva apenas o diagnóstico UIP-3 preexistente de always-on-top; o modo salvo foi desligado antes dos ensaios. `shadow(false)` é declarado, mas a própria API o documenta como não suportado no Linux.

## Execução real no Wayland

Tauri dev executado com `GDK_BACKEND=wayland LIBGL_ALWAYS_SOFTWARE=1`. Os controles foram acionados via acessibilidade AT-SPI, sem inspector pesado. Eventos nativos foram registrados somente em criação, show, hide, foco, close, move e resize.

| Cenário | Observação comprovada | Limite da observação |
| --- | --- | --- |
| Main sozinha | Depois da realização inicial da janela, 310×410; CharacterStage 300×360. | A criação GTK emitiu 362×462 e em seguida 310×410 antes do teste de interação. A variante experimental com `minWidth/maxWidth` fixos manteve 362×462 e foi revertida. |
| Composer | Criado uma vez; pedido 300×80; tamanho real 300×200; recebe foco e expõe um input editável. | Posição visual e relação com a Luna não mensuráveis por AT-SPI/Wayland; teclado físico não comprovado. |
| Conversation | Criada uma vez; tamanho real 300×460; recebeu foco. | Posição visual e proximidade não comprovadas. |
| Ambas | Três frames acessíveis coexistem; cada auxiliar pode receber foco; main permaneceu 310×410. | Não foi possível determinar sobreposição com a Luna ou entre as auxiliares. |
| Hide/show e fechar | Hide retirou o frame visível; show não recriou a janela. O botão fechar do Composer ocultou a janela; fechar a main com ambas abertas encerrou o processo inteiro. | Após ocultar ambas, a main continuou visível, mas não recebeu foco automaticamente (`STATE_ACTIVE=false`). |
| Alt+drag | O gate de drag da main continua no código UIP-3. | Não houve teste físico confiável nem deslocamento global observável nesta sessão; os eventos `Moved` das três janelas reportaram `(0,0)`. Acompanhamento, empilhamento e estabilidade espacial após mover seguem sem prova. |

O método `org.gnome.Shell.Screenshot.Screenshot` retornou `AccessDenied: Screenshot is not allowed`. A tentativa via portal de screenshot não retornou imagem. AT-SPI informou `(0,0)` para os três frames, compatível com a limitação de coordenadas top-level do Wayland; estes valores **não representam placement real**. Não há evidência suficiente para chamar as janelas de próximas, distantes, sobrepostas ou agrupadas pelo GNOME. Uma inspeção visual humana da tela e Alt+drag físico são necessários para resolver esse gate.

O Composer aceitou foco via AT-SPI (`STATE_FOCUSED=true`). As tentativas de injetar texto via AT-SPI e dispositivo virtual `/dev/uinput` não produziram texto; portanto digitação física permanece **não verificada**. A abertura da Conversation não destruiu a main nem o seu canvas, mas mudou o foco para a Conversation.

## WebGL e memória

Após `Show both`, diagnóstico na main: `main 310×410 · canvas 300×360 · mesmo canvas sim · glError 0`. O status seguia `WebGL ativo · animação de repouso`. Não houve `setSize` em show/hide, nem alteração no código de câmera ou RenderBudget. A sonda de identidade cobre o elemento canvas, não prova por si só a identidade interna do contexto/renderer ou a qualidade visual de cada frame.

RSS bruto da árvore Tauri, incluindo processo GTK principal, WebKitNetworkProcess e WebKitWebProcess, sem Vite: **647,5 MiB** com somente a main em execução limpa e **1.150,5 MiB** com Composer e Conversation visíveis, diferença **+503,0 MiB**. Na segunda condição, os processos WebKit auxiliares somaram aproximadamente **259,5 + 270,6 MiB** de RSS. Cada janela criou um WebKitWebProcess. A soma RSS pode contar páginas compartilhadas mais de uma vez e não mede PSS ou vazamento prolongado. O ponto de entrada frontend foi separado por importação dinâmica para não carregar o módulo da Luna nas auxiliares; mesmo assim o custo continuou alto. Ocultar preserva os processos para reutilização, portanto não libera imediatamente esse custo.

## Testes e decisão

`npm run typecheck`, `npm run build`, `CARGO_BUILD_JOBS=1 cargo check`, `CARGO_BUILD_JOBS=1 cargo test` (**42 testes**) e `git diff --check` passaram. `cargo fmt --check` não executou naquela rodada porque foi chamado na raiz do repositório, sem `Cargo.toml`; `cargo-fmt` está disponível. Para este projeto, usar `cargo fmt --check --manifest-path src-tauri/Cargo.toml`. Nenhum commit ou push.

**FAIL de aceitação**, por falta da prova física mais importante (placement e Alt+drag), altura inesperada do Composer e custo de RAM significativo. Isto não afirma que `transient_for` falhou em posicionar as janelas; afirma que não há evidência para construir UIP-4 sobre ele. A investigação futura deve obter observação visual direta no GNOME e considerar o orçamento de memória antes de propor outra abordagem. O código desta POC foi retirado do produto; a variante de múltiplas WebViews não prossegue para FIX-2B.


## Encerramento do experimento

Após o FAIL, a **UIP-4-FIX-2B-CLEANUP** removeu todo o código ativo das WebViews auxiliares e restaurou o produto ao modelo single-WebView da UIP-4-FIX. Este arquivo permanece apenas como registro técnico para evitar que a mesma arquitetura seja reintroduzida sem novas evidências.

A UIP-4 foi posteriormente fechada em **PASS funcional em 27/09/2026**, aceitando a instabilidade espacial do resize nativo no GNOME/Wayland como dívida não bloqueante. Uma eventual solução futura deve ser investigada na UIP-7 ou em rodada nativa dedicada; este POC não deve ser retomado como FIX-2B de produção.
