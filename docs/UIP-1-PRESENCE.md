# UIP-1 — Presence Shell

Data inicial: **26/09/2026** (America/Fortaleza). Branch `main`. HEAD inicial da UIP-1 após `git pull --ff-only`: `d44762cbc6613f921b4ee1d96d4b088bbf64a10c`; árvore inicial limpa. A implementação inicial foi submetida ao gate humano; veja **FIX-1** abaixo. **UIP-1 = PASS completo em 26/09/2026.**

## Implementação inicial

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

## FIX-1 — gate humano

Sam confirmou transparência real sobre o desktop, ausência de borda perceptível, corpo inteiro visível, Idle, clique/greeting e retorno ao Idle. O botão DEV é aceitável nesta fase. O gate identificou três ajustes: Luna ainda grande demais; chevron solto sem aparência de base; abertura de DEV congelando a animação por segundos e, ao menos uma vez, gerando o aviso do GNOME/Fedora de aplicativo sem resposta. **Os ajustes técnicos abaixo ainda dependem da aprovação visual e de uso de Sam.**

### Escala e base inferior

- `SceneRuntime.camera.position`: **`(0, 1.40, 4.7)` → `(0, 2.60, 7.8)`**. `camera.lookAt`: **`(0, 1.30, 0)` → `(0, 2.50, 0)`**. O avatar, rig, mixer, clipes, materiais, luzes e geometria permaneceram intactos.
- Projetando apenas os extremos aproximados do avatar normalizado (`y=0` a `y=2,55`) na câmera de 38° e no stage 420×604, a altura calculada passou de aproximadamente **476 px para 286 px**, cerca de **60% da altura anterior**. Os pés calculados ficam em `y≈582` dentro do stage; isso não é medição visual da silhueta animada. Sam precisa julgar escala, margem e enquadramento no desktop.
- O `CharacterStage`, canvas CSS e drawing buffer continuam em **420×604**. O raycast no centro do canvas continuou acionando greeting e retornando ao Idle em Tauri real após a mudança de câmera.
- O antigo `⌄` solto virou uma base visual de **112×16 px**, com duas linhas finas de 2 px e chevron central. As linhas usam tons claro/escuro para contraste em fundos distintos. O elemento fica **6 px abaixo do limite do stage**; pelos pontos projetados acima, cerca de 28 px abaixo dos pés. É não interativo, com `pointer-events: none`, sem compositor ou ação falsa.

### Investigação do travamento DEV — antes

O overlay antigo montava `LunaCorePanel` e seus quatro filhos diagnósticos ao mesmo tempo. Em desenvolvimento, `React.StrictMode` executa os efeitos de montagem duas vezes. A instrumentação temporária registrou, numa abertura: **2× `lr4_status`, 2× `lr4_get_recent_conversation`, 2× `gemini_status`, 2× `cognition_provider_status` e 2× `security_status`**. `gemini_conversation` inicia depois de `gemini_status`; uma chamada apareceu na janela de 12 s, com a outra sujeita ao atraso da consulta anterior. A tarefa Core não faz invoke inicial. Não foi observado loop de remontagem; a duplicação corresponde aos efeitos DEV do StrictMode.

`security_status` era um comando Rust **síncrono** que chamava `SecretStore::get_secret`, incluindo cofre do sistema/Stronghold e mutex interno. Uma invocação isolada, sem abrir painel, levou cerca de **3,0 s** e coincidiu com um gap de frame de **3.004 ms**. Na abertura completa, as duas chamadas levaram cerca de **11,9 s** para resolver, com gap máximo de **11.942–11.961 ms** em `SceneDiagnostics`; um relatório de 5 s teve **0 frames**. O DOM do overlay apareceu cerca de 15 ms após o clique, mas o frame seguinte ficou retido por quase 12 s. Isso identifica a consulta síncrona ao SecretStore como causa comprovada de bloqueio; a competição com outras consultas ao mesmo cofre é plausível, mas seu peso exato não foi isolado. O aviso gráfico “não está respondendo” foi relatado por Sam e não foi reproduzido automaticamente.

### Correção DEV — depois

- O clique DEV monta apenas o shell, status e navegação **Core / Memory / Gemini / Cognition / Security**. A seção inicial é vazia. Somente a seção escolhida monta seu componente; trocar de seção desmonta a anterior. Todos os diagnósticos LR-2 a LR-6 continuam acessíveis.
- `security_status` agora retorna por comando Tauri assíncrono e executa a leitura bloqueante em `spawn_blocking`. O painel Security compartilha apenas a leitura **em andamento** entre os dois efeitos de montagem DEV, evitando uma segunda consulta cara; após a conclusão, uma nova montagem pode consultar novamente. Nenhum polling novo foi introduzido.
- No shell vazio, **zero invokes** são disparados. Ao escolher Core, nenhum invoke inicial. Memory inicia `lr4_status` e `lr4_get_recent_conversation`; Gemini inicia `gemini_status` e depois `gemini_conversation`; Cognition inicia `cognition_provider_status`; Security inicia `security_status` **uma vez** (confirmado por linha de audit Rust no ciclo medido). Os efeitos não coalescidos de Memory/Gemini/Cognition ainda são executados duas vezes no StrictMode, mas suas operações são assíncronas ou rápidas e não bloquearam frames na amostra.
- No teste do shell, a inserção no DOM ocorreu cerca de **12 ms** após o clique, o primeiro `requestAnimationFrame` cerca de **39 ms** após o clique, e o maior gap foi **70 ms**. Em cinco ciclos focados, um por seção, os maiores gaps por ciclo foram **96 / 51 / 56 / 34 / 25 ms** (Core/Memory/Gemini/Cognition/Security); nenhum erro de console foi capturado. A seção Security exibiu seu status após cerca de **3,0 s** em uma medição separada, sem bloquear o loop. Nessa medição separada, `hasFocus=false` e o WebKit reduziu o ritmo para ~2 FPS com intervalos de 500 ms; esses intervalos por falta de foco não são comparáveis aos cinco ciclos focados.
- Nos cinco ciclos focados, abrir, selecionar e fechar funcionou em todas as seções; stage e drawing buffer ficaram **420×604** antes, durante e depois, na mesma posição; `gl.getError()=0`. `SceneDiagnostics` continuou reportando e registrou `resizeCount=0`. Não foi visto aviso “não está respondendo” no teste automatizado; Sam deve confirmar isso na interação real.
- Um segundo conjunto curto de cinco ciclos, executado com a janela sem foco e sem callbacks de render, amostrou RSS agregado de aproximadamente **624,7 MiB no início e 626,8 MiB ao fim**, com pico transitório de **1.141,9 MiB** no processo Rust durante consultas ao cofre. Isso não demonstra crescimento persistente, mas o pico merece acompanhamento; cinco ciclos não constituem teste de vazamento. A variação de foco/oclusão também explica por que esta segunda amostra não serve para comparar frame gaps.

Instrumentação temporária de console foi removida do código final. O renderer, `SceneDiagnostics`, `ResizeObserver`, DPR, `setAnimationLoop`, `powerPreference` e workaround Mesa não foram alterados. A janela Tauri continua configurada em 440×660, transparente e sem decoração. No reinício final, a WebView reportou 440×660; a medição inicial da UIP-1 havia reportado 492×712, uma diferença de plataforma ainda sem explicação.

### Testes e gate pendente

`npm run typecheck`, `npm run build`, `git diff --check`, `cargo check` e `cargo test` passaram; **38 testes Rust** passaram. Tauri real com `LIBGL_ALWAYS_SOFTWARE=1` validou Luna carregada, Idle, greeting e retorno ao Idle, WebGL 2.0, ausência de erro GL, cinco seções DEV, cinco ciclos e buffer controlado. Um reinício posterior abriu a janela atrás de outra superfície no GNOME (`hasFocus=false`, sem callbacks de `requestAnimationFrame`); por isso os dados de frame da segunda execução não foram usados para o gate de responsividade. O teste focado anterior já exercitou a mesma câmera final e a correção DEV.

Checklist para Sam:

1. Luna agora está suficientemente menor?
2. A proporção parece adequada para companhia no desktop?
3. Corpo inteiro continua visível?
4. Há espaço agradável ao redor?
5. Pés estão posicionados próximos da barra?
6. A barrinha parece um pequeno “chão”?
7. O chevron está integrado nela?
8. Está discreta sem ficar apagada?
9. Está perto o suficiente dos pés?
10. Clicar DEV abre sem congelar a Luna?
11. Fedora deixou de mostrar “não está respondendo”?
12. Navegar entre Core/Memory/Gemini/Cognition/Security é responsivo?
13. Fechar DEV devolve a Presence normalmente?
14. Luna não muda de posição ou escala ao abrir DEV?

**UIP-1 permanece aguardando esse gate humano. UIP-2 e UIP-3 não foram iniciadas.**

## FIX-2 — compactação da Presence

Sam aprovou a escala da Luna, a barra/chão e o shell DEV da FIX-1, e confirmou que o congelamento grosseiro ao abrir DEV desapareceu no uso humano. Restou o excesso de área transparente em torno da personagem. Esta FIX-2 reduz os bounds da janela e do `CharacterStage` sem alterar o modelo nem diminuir novamente a altura projetada da Luna. Branch `main`, árvore inicial limpa após `git pull --ff-only`, HEAD inicial `982e17075a479d7234a5629f6db42fbc55fcdaa9`.

| Apresentação | FIX-1 aprovada | FIX-2 candidata |
| --- | ---: | ---: |
| Janela Tauri configurada | 440×660 | **320×420** |
| CharacterStage / canvas CSS | 420×604 | **300×360** |
| Drawing buffer, DPR 1 | 420×604 | **300×360** |
| `camera.position` | `(0, 2.60, 7.8)` | **`(0, 1.50, 4.65)`** |
| `camera.lookAt` | `(0, 2.50, 0)` | **`(0, 1.40, 0)`** |
| FOV | 38° | 38° |
| Altura projetada aproximada de `y=0…2,55` | 285,54 px | **286,22 px** |

A diferença projetada é **+0,68 px (+0,24%)**. Foi compensada com distância e alvo da câmera, mantendo FOV, escala estrutural, GLB, rig, animações e renderer. A área configurada da janela caiu aproximadamente 54%; a quantidade de pixels do stage/buffer caiu aproximadamente 57%. O cálculo de projeção usa os extremos verticais do avatar normalizado, não é uma medição da silhueta em movimento. No stage de 300×360, os extremos calculados ficam em `y≈50` (cabeça) e `y≈336` (pés), com margens verticais de aproximadamente **50 px e 24 px**. Para conferir o aceno completo, uma sonda temporária avançou o `AnimationDirector` em 85 passos de 50 ms (4,25 s, incluindo retorno ao Idle) e projetou o `Box3.setFromObject(root, true)` de cada pose. Os limites conservadores foram `x≈35…212` e `y≈42…347` dentro do canvas 300×360: margem mínima aproximada de **35 px à esquerda, 88 px à direita, 42 px acima e 13 px abaixo**. A sonda foi removida da versão final. Isso verifica o enquadramento geométrico; a inspeção visual de mãos, cabelo e bordas continua com Sam.

A barra aprovada continua **112×16 px**, sem clique, e manteve seu desenho. Ela foi movida junto com o stage: o topo fica **6 px abaixo do canvas** e cerca de **20–30 px abaixo dos pés estimados**, sem invadir o render. O stage está centralizado por posição absoluta; o DEV também é absoluto, tem scroll próprio e não participa de suas dimensões. Nesta execução, a WebView reportou `372×472` apesar dos `320×420` configurados, novamente **+52 px em cada eixo**; em outro reinício de desenvolvimento da mesma FIX reportou `320×420`. A causa no Tauri/WebKitGTK/Wayland não foi identificada. O stage e o buffer ficaram 300×360 em ambos os casos, mas a margem externa percebida pode variar conforme essa diferença da plataforma.

### Tauri real e amostra curta de Idle

Mesma metodologia de amostra da UIP-0: Fedora/Wayland, Tauri dev real, inspector WebKit, `LIBGL_ALWAYS_SOFTWARE=1` confirmado em `/proc/<WebKitWebProcess>/environ`, seis relatórios `SceneDiagnostics` de ~5 s em 30 s de Idle com `visibilityState=visible` e `hasFocus=true`; árvore do processo Tauri amostrada a cada ~1 s por 30 s para CPU e `VmRSS`. CPU de 100% equivale a um núcleo lógico; RSS é soma aproximada que pode incluir páginas compartilhadas repetidas.

| Idle | UIP-1 inicial | FIX-2 |
| --- | ---: | ---: |
| Drawing buffer | 420×604 | **300×360** |
| FPS médio dos seis relatórios | 51,86 | **55,06** |
| Intervalo médio entre frames | 19,10–19,44 ms | **18,03–18,25 ms** |
| `update+render` médio | 11,59–11,78 ms | **10,94–11,12 ms** |
| CPU agregada média | 279,0% | **200,8%** |
| RSS agregado médio | 652,6 MiB | **647,2 MiB** |

Na FIX-2, os FPS por relatório foram `54,77 / 54,56 / 54,96 / 55,56 / 54,96 / 55,53`; p50 dos intervalos foi 18 ms e p95 20–21 ms. O maior intervalo da amostra foi 29 ms. O p95 de `update+render` foi 12–13 ms. `resizeCount=0`, DPR e renderer pixel ratio 1/1, WebGL 2.0 (`WebKit WebGL`) e nenhum erro de console. O RSS observado ficou em 644,4–651,9 MiB. São amostras curtas em execuções distintas; as diferenças acompanham o stage menor, mas **não isolam causalidade** nem medem a taxa efetiva de apresentação ou RAM física exclusiva.

### Gates técnicos e gate humano

- Em Tauri real, Luna carregou, o status informou Idle, `pointerdown` no centro do canvas acionou greeting e o status voltou ao Idle. O raycast permaneceu funcional; `gl.getError()=0` antes e depois. Fundos calculados de `html`, `body`, `#root`, shell, stage e container do canvas permaneceram totalmente transparentes. A aparência final e o recorte durante a animação precisam de inspeção de Sam.
- Cinco ciclos DEV (Core, Memory, Gemini, Cognition, Security) abriram, selecionaram seção e fecharam sem erro de console. Antes/durante/depois, stage e drawing buffer ficaram em **300×360** na mesma posição; `resizeCount=0` nos relatórios coletados. Os maiores gaps de `requestAnimationFrame` por ciclo foram **103 / 52 / 56 / 32 / 32 ms**; não houve pausa de segundos. O overlay manteve scroll próprio e a janela não foi redimensionada. A correção `security_status` em `spawn_blocking` permaneceu intacta.
- `npm run typecheck`, `npm run build` e `git diff --check` passaram. O build manteve o aviso conhecido de chunk acima de 500 kB. Nenhum código Rust foi modificado; não houve motivo para repetir `cargo check/test` nesta FIX.

Checklist pendente de Sam para fechar a UIP-1:

1. A janela invisível agora parece proporcional à Luna?
2. Existe espaço morto demais em alguma direção?
3. A escala da Luna continua igual à versão aprovada?
4. Idle cabe sem recorte?
5. Aceno cabe sem recorte, inclusive mãos e cabeça?
6. Barra continua bem posicionada?
7. DEV continua abrindo normalmente?

**UIP-1 ainda depende desse gate humano; não está marcada como PASS completo. UIP-2 e UIP-3 não foram iniciadas.**


## Fechamento humano da FIX-2

Em 26/09/2026, Sam aprovou visualmente a compactação final da Presence Shell. A janela transparente passou a parecer proporcional à personagem; não houve espaço morto excessivo percebido; a escala da Luna permaneceu equivalente à versão aprovada da FIX-1; Idle e greeting/aceno permaneceram sem recorte perceptível; a barra/chão continuou bem posicionada; e o shell DEV continuou abrindo normalmente, sem retornar o congelamento grosseiro nem o aviso de aplicativo sem resposta.

A auditoria remota confirmou que a FIX-2 se limita ao redimensionamento da janela/stage, compensação de câmera e documentação: janela configurada em **320×420**, CharacterStage/drawing buffer em **300×360**, câmera em `position (0, 1.50, 4.65)` e `lookAt (0, 1.40, 0)`, preservando renderer, animações, modelo e correções DEV anteriores.

Com os gates técnicos, a auditoria remota e o gate humano final, **UIP-1 — Presence Shell = PASS completo**.

Próxima etapa: **UIP-2 — Render Budget**.
