# M0-A — Relatório de validação

Data: 24/09/2026. Ambiente: Fedora Workstation 44, GNOME/Wayland, Intel HD Graphics 4000, Mesa 26.2.2, Node 24.18.0, npm 12.0.2, Rust/Cargo 1.98.1 instalados no perfil do usuário.

## Verificações concluídas

| Verificação | Resultado |
| --- | --- |
| Licença do asset | README oficial do Three.js r186 declara CC0 1.0; GLB local confere com SHA-256 documentado no README. |
| Estrutura do GLB | glTF binário válido, 14 meshes, 2 skins, clipes `Idle` e `Wave` presentes. |
| `npm run typecheck` | Passou. |
| `npm run build` | Passou. Vite avisa que o bundle Three.js supera 500 kB; aviso de tamanho, sem falha. |
| `cargo check` final | Passou após a instalação das bibliotecas Fedora. |
| Navegador Firefox no próprio Fedora | Modelo carregado; canvas presente; WebGL 2.0; `gl.getError() = 0`; renderizador informado: “Intel(R) HD Graphics, or similar”. |
| Interação no navegador | Botão e clique na personagem iniciaram `Wave`; após o clipe, o status voltou a `Idle`. |
| Captura no navegador | O robô humanoide aparece na área 3D com a interface escura; veja [captura-navegador.png](docs/captura-navegador.png). |
| `cargo check` inicial | Falhou antes da instalação dos pacotes Fedora porque `glib-2.0.pc` não existia; após a instalação passou. |

## Janela Tauri

Após o usuário instalar as bibliotecas Fedora, `npm run tauri dev` compilou e abriu a janela WebKitGTK. Pela inspeção remota do WebKit: GLB carregado, canvas WebGL 2.0, `gl.getError() = 0`, botão e clique na personagem acionaram `Wave`, e o status voltou a `Idle`. A [captura da janela Tauri](docs/captura-tauri.png) mostra o robô acenando. O usuário confirmou que viu a personagem e o gesto na janela real.

O caminho acelerado padrão do WebKitGTK nesta máquina não desenhou o canvas: um teste mínimo de limpar um canvas WebGL retornou `INVALID_OPERATION (1282)`. Testes com `WEBKIT_DISABLE_DMABUF_RENDERER=1`, `WEBKIT_DISABLE_COMPOSITING_MODE=1` e `GDK_BACKEND=x11` não resolveram o caminho acelerado. Com `LIBGL_ALWAYS_SOFTWARE=1`, o mesmo teste retornou o pixel esperado e a personagem apareceu. O executável Tauri agora define essa variável por padrão no Linux antes de iniciar o WebKit; `LIBGL_ALWAYS_SOFTWARE=0 npm run tauri dev` permite repetir o diagnóstico com aceleração de hardware.

## Limitações e pendências

- A personagem é um robô humanoide de teste; sua aparência ainda não define a identidade final do produto.
- O teste automatizado do navegador verifica estado, WebGL e interação; a inspeção humana da janela confirmou presença e gesto. A fluidez sustentada e o uso de memória ainda precisam de avaliação humana.
- Não há conversa funcional neste checkpoint; a coluna direita é apenas uma área reservada.
- O Firefox usou WebGL com renderizador Intel. A janela Tauri usa Mesa em software devido à falha do WebKitGTK com a GPU Intel HD 4000 neste ambiente; desempenho com aceleração de hardware permanece pendente.
