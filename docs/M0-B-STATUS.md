# M0-B — Luna: status central

## Idle manual integrada — 26/09/2026

**Estado atual:** M0-B segue aberto, mas a Idle manual criada por Sam no Blender 3.3.21 já substituiu a Idle procedural no asset usado pelo aplicativo. A integração entrou em `main` no commit `862b445`, **depois do fechamento da UIP-2/FIX-1 e antes do início da UIP-3**.

Validação humana no Tauri: aparência preservada, nova Idle em loop e melhora visual clara em relação ao repouso anterior; o `Wave` legado continua funcional e retorna para a nova Idle. O runtime permanece no caminho `LegacyGlbAdapter`/GLB, sem migração VRM/VRMA nesta etapa.

A exportação Blender alterou materialmente o asset: 4.206.852 bytes (~+68,9% sobre M0-B1) e 462 canais por clipe, contra 17 no GLB anterior. Isso pode afetar custo do AnimationMixer, parse, memória e `update+render`. **As medições de UIP-0/UIP-1/UIP-2 foram feitas com o asset anterior e não devem ser usadas como comparação causal direta com UIP-3+ sem nova baseline.** Ver [integração da Idle manual](MANUAL-IDLE-INTEGRATION.md).

`scripts/prepare_luna.py` deixa de publicar o asset final; agora gera somente `assets/luna/generated/Luna-bootstrap.glb`. O GLB distribuído passa a ser exportação Blender e deve ser checado com `python scripts/validate_luna_glb.py`.

A Idle atual é uma primeira candidata manual, não acabamento final. O principal débito artístico de M0-B agora é refazer o `Wave` manualmente e, depois, avançar o experimento VRM/VRMA.

## Parecer humano e decisão de retomada — 24/09/2026

**M0-B segue aberto: integração funcional, animações não aprovadas artisticamente.** O usuário observou que o Idle do M0-B1 continua semelhante a manequim; no Wave a mão se move, mas o corpo permanece rígido e a palma fica orientada para baixo. O usuário relatou boa fluidez aparente no Tauri, sem atribuir esses defeitos a travamento. O ajuste de enquadramento do M0-B1 foi implementado, mas não resolve a linguagem corporal.

**Decisão:** manter a base Luna atual e adiar alterações em roupa/rosto e modelagem autoral. Suspender novas tentativas automáticas de animação enquanto o usuário aprende a criar poses diretamente no Blender 3.3.21. Próxima sessão: primeiro pose estática de repouso com braços/ombros relaxados; depois pose de saudação com palma orientada à câmera. Só depois, se viável, keyframes e integração técnica sob validação. A versão em `main` permanece a referência funcional; não sobrescrever GLB, VRM ou script de geração durante a experiência.

**Guia completo de contexto, decisões, lições e roteiro passo a passo:** [RETOMADA-BLENDER-LUNA.md](RETOMADA-BLENDER-LUNA.md). As verificações técnicas do M0-B1 listadas abaixo continuam válidas como verificações técnicas, não como aprovação da naturalidade.

## M0-B1 — primeira versão refinada pronta para avaliação (24/09/2026)

- A — Inspeção concluída em `main`, HEAD `c554ac9090c1d01c20810822e4aacc2f797168e2`, worktree inicialmente limpa. Conferidos rig real, rotações de repouso e os clipes Idle/Wave exportados; ombros e braços têm ramos auxiliares paralelos.
- B — Idle atualizado no script e GLB regenerado. Tronco, cabeça, ombros e braços agora combinam movimentos pequenos em ciclos de 6 s com extremos idênticos; quadril, pernas e pés permanecem fixos. Estrutura e fechamento do loop conferidos. Wave, enquadramento e validação final ainda pendentes nesta etapa.
- C — Wave regenerado com 3,6 s: ombro, braço, cotovelo e punho entram e saem em tempos distintos, com inclinação discreta da cabeça. Início/fim retornam à pose neutra; render de início, meio e retorno inspecionado sem interseção evidente de cabelo, braço e roupa. O mixer inicia o retorno ao Idle antes do último quadro, preserva o tempo do Idle e ignora cliques repetidos durante o gesto. Enquadramento e validação final pendentes.
- D — Altura de apresentação reduzida de 2,9 para 2,55 unidades (cerca de 12%), com câmera recentrada em 1,30; pés continuam assentados em y=0 e a escala segue uniforme. Enquadramento conferido no navegador em largura de desktop e celular.
- E — `npm run typecheck` e `npm run build` passaram (aviso já conhecido de chunk acima de 500 kB). GLB conferido: 3 skins, clipes Idle (6 s, 181 amostras) e Wave (3,6 s, 109 amostras), 17 canais de rotação cada, quaternions finitos/normalizados e primeiro/último quadro iguais. No Chromium local com WebGL por SwiftShader, botão e clique no canvas acionaram Wave e retornaram a Idle; capturas em 1280×800 e 360×800 mostraram cabeça, pés e mão inteira. Tauri/WebKitGTK abriu com Mesa em software; inspeção remota confirmou canvas/WebGL 2, interação e retorno ao Idle, com erro WebGL 0. Houve uma leitura inicial com quadros suspensos na janela, seguida de repetição bem-sucedida quando voltaram a avançar. Parecer artístico e fluidez sustentada continuam pendentes do usuário.

Atualizado em 24/09/2026. **Candidata M0-B1 refinada e verificada tecnicamente. Checkpoint ainda não aprovado pelo usuário.**

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
- Exportado após M0-B1: `public/models/Luna.glb`, 2.490.812 bytes, SHA-256 `f5ceffb00548b9666eb114e67bea8c4e8236c74c4ab29208b0326ada63de5c93`.

## Preparação e comportamento
- Cabelo prata/lavanda, olhos violeta, camiseta azul-noturno, shorts e pernas cobertas em grafite, cabeça 10% menor, pequeno núcleo ciano no peito e detalhes laterais. A roupa mantém o corte da base; ainda não representa um figurino futurista definitivo.
- Base: 36.470 triângulos, 3 meshes/skins, 13 materiais, nenhum clipe. Candidata: 36.494 triângulos considerando as três instâncias dos detalhes, 3 skins, texturas até 512 px e materiais unlit baratos. Removidos morph targets e extensões/dinâmica VRM; nenhum processamento de cabelo/colisão em runtime.
- **Idle (6 s) e Wave (3,6 s) são clipes criados localmente**, não animações fornecidas pela pixiv. Idle coordena respiração, pequena inclinação do tronco/cabeça e acompanhamento dos braços, sem deslocar quadril e pés. Wave envolve ombro, elevação do braço direito, flexão do cotovelo, saudação discreta do punho e retorno suave. Não é captura de movimento nem retargeting do robô.
- Auxiliares paralelos do ombro/braço seguem o movimento. Não há suporte genérico às constraints VRM; movimentos arbitrários futuros exigem rever esses auxiliares.
- Geração histórica M0-B1: `python scripts/prepare_luna.py` (Python 3 + Pillow). Após a integração manual de 26/09, esse script gera apenas um **bootstrap legado** em `assets/luna/generated/` e não pode mais sobrescrever o GLB distribuído.
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
- Roupa ainda casual; sem iluminação volumétrica nos materiais unlit. O aceno mantém a palma vista de lado; a separação entre cabelo, braço e roupa foi conferida apenas em quadros amostrados, sem colisão ou física.
- O Firefox headless emitiu um aviso `RenderCompositorSWGL failed mapping default framebuffer` no log da sessão; as capturas e interações funcionaram e as consultas WebGL retornaram 0. Não confundir esse teste com aprovação manual.
- Mesa por software continua necessário no Tauri. Não medidos FPS, CPU, memória ou estabilidade prolongada; não alegar desempenho sustentado.
- Base usa licença própria VRM, com condições acima; preservar os avisos e revisar usos futuros contra essas condições.

## Próximo passo exato para retomada — após Idle manual

1. Preservar `public/models/Luna.glb` atual como candidata visual da Idle e executar `python scripts/validate_luna_glb.py` em novas exportações.
2. Antes de comparar números de UIP-3+ com UIP-2, capturar uma nova baseline curta com a Idle manual sob a política 30/24/0 FPS; não misturar mudança de asset com mudança de janela.
3. Retomar o Blender para criar o **Wave manual**, corrigindo palma/orientação, ombro, cotovelo, overlap e retorno à Idle.
4. Manter a direção de migração VRM 1.0 + VRMA como trilha separada; não bloquear UIP-3 por essa migração.
5. A fonte `.blend` da animação manual ainda é local. Até versioná-la ou migrar para VRMA, não declarar o GLB final como totalmente reproduzível apenas a partir do repositório.

**Referência integrada atual:** `main` contém a Idle manual funcional e o Wave legado. M0-B permanece aberto até revisão do gesto e fechamento artístico/técnico correspondente.
