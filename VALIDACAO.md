# M0-A — Relatório de validação

Data: 24/09/2026. Ambiente: Fedora Workstation 44, GNOME/Wayland, Intel HD Graphics 4000, Mesa 26.2.2, Node 24.18.0, npm 12.0.2, Rust/Cargo 1.98.1 instalados no perfil do usuário.

**Estado:** aprovado pelo usuário em 24/09/2026 e encerrado. As verificações técnicas abaixo ocorreram durante a implementação do M0-A; não foram repetidas neste fechamento documental.

## Verificações técnicas do Codex

| Verificação | Resultado |
| --- | --- |
| Licença do asset | README oficial do Three.js r186 declara CC0 1.0; GLB local confere com SHA-256 documentado no README. |
| Estrutura do GLB | glTF binário válido, 14 meshes, 2 skins, clipes `Idle` e `Wave` presentes. |
| `npm run typecheck` | Passou. |
| `npm run build` | Passou. Vite avisa que o bundle Three.js supera 500 kB; aviso de tamanho, sem falha. |
| `cargo check` e compilação Tauri | Passaram após a instalação das bibliotecas Fedora. |
| Navegador Firefox no próprio Fedora | Modelo carregado; canvas presente; WebGL 2.0; `gl.getError() = 0`; renderizador informado: “Intel(R) HD Graphics, or similar”. |
| Interação no navegador | Botão e clique na personagem iniciaram `Wave`; após o clipe, o status voltou a `Idle`. |
| Captura no navegador | O robô humanoide aparece na área 3D com a interface escura; veja [captura-navegador.png](docs/captura-navegador.png). |

### Janela Tauri

Após a instalação das bibliotecas Fedora, `npm run tauri dev` compilou e abriu a janela WebKitGTK. Pela inspeção remota do WebKit feita pelo Codex: GLB carregado, canvas WebGL 2.0, `gl.getError() = 0`, botão e clique na personagem acionaram `Wave`, e o status voltou a `Idle`. A [captura da janela Tauri](docs/captura-tauri.png) mostra o robô acenando.

## Validação manual do usuário — 24/09/2026

O usuário confirmou na aplicação desktop que:

1. A janela Tauri abriu após carregar Rust/Cargo no terminal.
2. O RobotExpressive apareceu na interface.
3. O clique direto na personagem acionou `Wave`.
4. O botão **Acenar** também acionou a animação.
5. Após o gesto, a personagem retornou ao repouso.
6. A aplicação permaneceu aparentemente estável e utilizável durante alguns minutos, sem travamentos importantes percebidos.

Essa observação humana confirma o funcionamento visível do checkpoint. Não foram medidos FPS, consumo de memória, uso de CPU ou estabilidade prolongada.

## Ocorrências de ambiente

- A primeira tentativa do usuário de executar `npm run tauri dev` falhou porque `cargo` não estava no PATH daquele terminal. Executar `. "$HOME/.cargo/env"` e repetir o comando resolveu o problema.
- Antes da instalação das bibliotecas Fedora, um `cargo check` feito pelo Codex falhou por ausência de `glib-2.0.pc`. Após a instalação, `cargo check` e a compilação Tauri passaram.

## Limitação gráfica conhecida

O caminho acelerado do WebKitGTK com a Intel HD Graphics 4000 não desenhou o canvas: um teste mínimo de limpar um canvas WebGL retornou `INVALID_OPERATION (1282)`. Testes com `WEBKIT_DISABLE_DMABUF_RENDERER=1`, `WEBKIT_DISABLE_COMPOSITING_MODE=1` e `GDK_BACKEND=x11` não resolveram esse caminho. Com `LIBGL_ALWAYS_SOFTWARE=1`, o mesmo teste retornou o pixel esperado e a personagem apareceu. O executável Tauri configura essa variável por padrão no Linux antes de iniciar o WebKit; `LIBGL_ALWAYS_SOFTWARE=0 npm run tauri dev` permite repetir o diagnóstico com aceleração de hardware. O Firefox conseguiu utilizar o renderizador Intel.

## Limitações e pendências

- O workaround por software permitiu validar M0-A, mas a aceleração gráfica da janela Tauri permanece uma limitação conhecida; o funcionamento do modelo provisório não resolve definitivamente o problema.
- O desempenho sustentado não foi medido quantitativamente. Uma personagem mais elaborada exigirá nova validação de desempenho na janela Tauri e no hardware real.
- RobotExpressive é apenas o asset de validação. A área de conversa da interface não oferece conversação funcional neste checkpoint.
