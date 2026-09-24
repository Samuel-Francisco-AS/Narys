# M0-B — Luna: status central

Atualizado em 24/09/2026. **Primeira candidata exportada, integrada e verificada tecnicamente. Checkpoint ainda não aprovado pelo usuário.**

## Objetivo e direção congelada
Mulher jovem adulta estilizada, anime suave, serena, acolhedora e quase humana; androide sutil. Cabelo frio, roupa escura elegante, acentos violeta/ciano. Evitar infantilização, sexualização, aparência militar e complexidade cara. A base é ponto de partida; roupa ainda casual e percepção de idade/identidade precisam de avaliação humana.

## Decisões e estratégia
Um único caminho: adaptar base feminina rigada da pixiv, converter para GLB padrão e criar dois clipes simples. Sem novo runtime VRM, dependência npm, física ou sistema de expressões. Preservar loader/mixer/raycast e RobotExpressive para reversão. Quaternius foi considerado na busca inicial, sem download; descartado pela linguagem low-poly mais distante do anime suave. Não foram construídas candidatas concorrentes.

## Milestones
| Etapa | Resultado |
| --- | --- |
| M0 — inspeção | Concluída. Branch `main`, HEAD `7f09468c0480eb2ba9ad475a807973daf64bfe57`, worktree inicialmente limpa. README/VALIDACAO, cena, UI e Tauri inspecionados; nenhum AGENTS.md encontrado no projeto/ancestrais. |
| M1 — direção | Concluída. Direção acima e reutilização de uma única base congeladas. |
| M2 — seleção | Concluída. Licença específica, metadados, thumbnail, rig, materiais e ausência de clipes conferidos. |
| M3 — preparação | Concluída para candidata inicial. GLB exportado, script reproduzível e render inspecionado; saudação também inspecionada em render temporário. |
| M4 — integração | Concluída. Luna.glb carregado, rótulos atualizados; Idle/Wave, botão e raycast preservados. Texturas e mixer liberados no descarte, inclusive carregamento tardio após desmontagem. |
| M5 — verificação | Concluída de forma localizada, conforme tabela abaixo. Aprovação humana e uso prolongado pendentes. |
| M6 — fechamento | Concluída. Diff revisado, `git diff --check` limpo, build final atualizado e processos de teste encerrados. main/HEAD preservados; 5 arquivos rastreados modificados e 9 novos, sem staging/commit. |

## Asset, procedência e licença
- Base: **VRM1_Constraint_Twist_Sample v1.0.1**, pixiv Inc., `(c) 2022 pixiv Inc.`.
- [Fonte primária fixada no commit 1b4fc0cc7ef39a49d62bb7a66dcfeca8f65316f7](https://github.com/pixiv/three-vrm/blob/1b4fc0cc7ef39a49d62bb7a66dcfeca8f65316f7/packages/three-vrm/examples/models/VRM1_Constraint_Twist_Sample.vrm).
- Licença: **[VRM Public License 1.0](https://vrm.dev/licenses/1.0/)** ([texto inglês](https://vrm.dev/licenses/1.0/pdf/en.pdf)), combinada com as configurações incorporadas ao arquivo. Não é CC0 nem apenas a licença MIT do código three-vrm.
- Configurações: `avatarPermission=everyone`, `commercialUsage=corporation`, `allowRedistribution=true`, `modification=allowModificationRedistribution`, `creditNotation=unnecessary`, `allowAntisocialOrHateUsage=false`; permissões de expressão sexual/violenta/política/religiosa=true. A adaptação mantém os mesmos termos, sem alegar endosso da pixiv e sem garantias.
- Configurações originais completas, origem e aviso de adaptação preservados em `public/models/Luna.LICENSE.json` e nos extras do GLB. Manter esses avisos ao redistribuir.
- Original: `assets/luna/base.vrm`, 10.776.032 bytes, SHA-256 `12c2b97e95e700783a6a550dc0eee2d7880aeedccef9ae67bc4c5a2f0f2631a2`.
- Exportado: `public/models/Luna.glb`, 2,443,300 bytes, SHA-256 `b231d1e08c5982c062deab0829b6751d738db6202e3e8ae18e4948066a041f7d`.

## Preparação e comportamento
- Cabelo prata/lavanda, olhos violeta, camiseta azul-noturno, shorts e pernas cobertas em grafite, cabeça 10% menor, pequeno núcleo ciano no peito e detalhes laterais. A roupa mantém o corte da base; ainda não representa um figurino futurista definitivo.
- Base: 36.470 triângulos, 3 meshes/skins, 13 materiais, nenhum clipe. Candidata: 36.494 triângulos considerando as três instâncias dos detalhes, 3 skins, texturas até 512 px e materiais unlit baratos. Removidos morph targets e extensões/dinâmica VRM; nenhum processamento de cabelo/colisão em runtime.
- **Idle (4 s) e Wave (3,2 s) são clipes criados localmente**, não animações fornecidas pela pixiv. Idle movimenta discretamente tronco/cabeça; Wave ergue e oscila o antebraço em saudação lateral, depois retorna ao repouso pelo mixer existente. Não é captura de movimento nem retargeting do robô.
- Auxiliares do ombro seguem a animação planar dos braços. Não há suporte genérico às constraints VRM; movimentos arbitrários futuros exigem rever esses auxiliares.
- Geração: `python scripts/prepare_luna.py` (Python 3 + Pillow). O script verifica o hash original antes de exportar.
- Prévia: `blender -b -t 2 --python scripts/preview_luna.py` (verificado com Blender 3.3.21).

## Verificações executadas
| Verificação | Resultado / limite |
| --- | --- |
| `npm run typecheck` | Passou. |
| `npm run build` | Passou; aviso de bundle JS acima de 500 kB, sem erro. |
| Estrutura GLB | Cabeçalho/tamanho, clipes Idle/Wave, accessors e skins conferidos; não equivale à validação completa Khronos. |
| Render estático | [luna-preview.png](luna-preview.png), personagem inteira inspecionada. |
| Firefox local | Luna visível, WebGL 2.0, erro 0; botão e evento de ponteiro no corpo acionam Wave e retornam a Idle. [Captura](luna-navegador.png). |
| Tauri/WebKitGTK local | Recompilou e abriu via `npm run tauri dev`; Luna visível. Inspeção remota confirmou botão e evento de ponteiro acionando Wave e retorno a Idle, erro WebGL 0. [Captura do conteúdo da janela](luna-tauri.png). |
| Mesa software | `LIBGL_ALWAYS_SOFTWARE=1` confirmado no ambiente do processo WebKit. O renderer exposto pelo WebKit é mascarado, não usado como evidência de GPU. |
| Preservação M0-A | RobotExpressive mantém SHA-256 `047f5e5fb3bb6d378bd1df16ca6137f2a596c99b3a1b5690b4020c05aaf6f319`. VALIDACAO.md e workaround Tauri preservados. |

Teste desktop usou Vite já aberto, por isso o comando foi `. "$HOME/.cargo/env"` seguido de `WEBKIT_INSPECTOR_HTTP_SERVER=127.0.0.1:9223 npm run tauri dev -- --config '{"build":{"beforeDevCommand":""}}'`. Esse override e o inspector foram apenas para teste, não persistidos. Os testes de interação foram automatizados; não constituem aprovação manual do usuário.

## Arquivos modificados/adicionados
- `README.md`, `docs/M0-B-STATUS.md`: estado e continuidade; VALIDACAO.md histórico não alterado.
- `assets/luna/base.vrm`: fonte original fixada.
- `scripts/prepare_luna.py`, `scripts/preview_luna.py`: preparação e render reproduzíveis.
- `public/models/Luna.glb`, `public/models/Luna.LICENSE.json`: candidata e avisos distribuíveis.
- `src/components/CharacterScene.tsx`, `src/App.tsx`: integração e identificação.
- `index.html`, `src-tauri/tauri.conf.json`: títulos M0-B/Luna.
- `docs/luna-preview.png`, `docs/luna-navegador.png`, `docs/luna-tauri.png`: evidências visuais.

## Riscos e pendências
- **Avaliação manual pendente:** aparência jovem adulta, identidade Luna, roupa, gesto e fluidez. Candidata funcional não significa direção artística aprovada.
- Roupa ainda casual; sem iluminação volumétrica nos materiais unlit. Aceno simples, palma lateral; cabelo pode atravessar manga no gesto, pois não há colisão.
- O Firefox headless emitiu um aviso `RenderCompositorSWGL failed mapping default framebuffer` no log da sessão; as capturas e interações funcionaram e as consultas WebGL retornaram 0. Não confundir esse teste com aprovação manual.
- Mesa por software continua necessário no Tauri. Não medidos FPS, CPU, memória ou estabilidade prolongada; não alegar desempenho sustentado.
- Base usa licença própria VRM, com condições acima; preservar os avisos e revisar usos futuros contra essas condições.

## Próximo passo exato para retomada
1. Ler este arquivo; conferir `git status --short --branch` e `git rev-parse HEAD` sem descartar as alterações existentes.
2. Na raiz, executar `. "$HOME/.cargo/env"` e `npm run tauri dev` (sem outro Vite na porta 5173).
3. Avaliar a Luna por alguns minutos: roupa/idade aparente/identidade, clique direto, botão, retorno ao repouso e fluidez. Registrar o parecer aqui. Se houver ajuste visual, alterar **o script de preparação**, regenerar GLB e prévia; verificar apenas os pontos afetados.
4. Só considerar M0-B aprovado após essa avaliação. Não iniciar outro checkpoint, fazer commit/push ou mudar/criar branch sem nova instrução.

**Modelo já exportado e integrado.** Retomada não exige nova busca de assets nem reconstrução da investigação. Não houve commit, push ou troca/criação de branch nesta execução.
