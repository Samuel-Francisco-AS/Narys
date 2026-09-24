# Retomada — decisões do projeto e oficina de animação Blender

> Registro de 24/09/2026. Este documento resume decisões e resultados discutidos com o usuário e orienta a próxima sessão de trabalho. Não substitui `VALIDACAO.md` (M0-A), `docs/M0-B-STATUS.md` (estado técnico) nem os testes do aplicativo.

## 1. Produto, objetivos e forma de trabalho

- Produto: assistente virtual com personagem feminina 3D, inicialmente desktop, futuramente Android; capacidade de agente para **executar** tarefas autorizadas, com possível integração a Telegram e/ou WhatsApp.
- Prioridade de entrega: ver a personagem funcionando cedo, manter checkpoints pequenos e observáveis como no GestorFlow; não repetir um período longo de infraestrutura antes de apresentar resultado visual.
- Prioridade pedagógica: arquitetura organizada e código legível, de modo que o usuário consiga abrir um arquivo e entender, com orientação, seu papel e fluxo. Aprendizado progressivo sem transformar o produto num exercício simplório.
- Stack inicial: React + TypeScript + Three.js + Tauri 2; Python para agente/ferramentas quando esse marco começar; Kotlin para recursos Android quando pertinente. Interesse em C++ permanece, mas não é razão para acrescentá-lo ao núcleo sem necessidade técnica. Não introduzir essas camadas agora.
- Personagem desejada: Luna, mulher adulta estilizada com anime suave; cabelo em tons frios, olhos violeta/ciano, roupa escura elegante e detalhes discretos de androide/ciborgue. Não realista, infantilizada ou militarizada. O usuário prioriza agora movimento sobre refinamento de roupa e rosto.

## 2. Acontecimentos e commits

| Marco | Evidência | Resultado |
| --- | --- | --- |
| Fundação M0-A | `9d8a613` | Tauri, interface, RobotExpressive GLB e clipes Idle/Wave; navegador e janela desktop verificados. |
| Fechamento M0-A | `7f09468` | Usuário confirmou personagem, clique, botão, retorno ao Idle e uso aparentemente estável por alguns minutos. M0-A encerrado. |
| Candidata M0-B | `c554ac9` | Base `VRM1_Constraint_Twist_Sample` (pixiv), adaptada para Luna em GLB, com clipes produzidos por script e integração na UI. Técnica verificada, arte ainda não aprovada. |
| Refinamento M0-B1 | `87504c3` | Idle de 6 s, Wave de 3,6 s, composição de mais canais e enquadramento ~12% menor. Build/tipos e interações passaram; avaliação artística do usuário **não aprovou** os movimentos. |

O erro `cargo metadata` ao iniciar o Tauri em outro terminal foi corrigido carregando o ambiente com `. "$HOME/.cargo/env"`. A janela Tauri usa Mesa em software devido a erro WebGL 1282 no caminho acelerado do WebKitGTK/Intel HD 4000; o Firefox pôde utilizar Intel. Não foram medidos FPS, CPU ou memória quantitativamente.

Os dois ensaios de animação automática foram examinados pelo usuário em capturas e gravações. Na primeira candidata, o avatar era feminino mas a pose de repouso parecia um manequim, e o aceno tinha braço rígido e palma mal orientada. Depois de M0-B1, o usuário **não percebeu melhora relevante no Idle** e viu movimento na mão, mas ainda com o corpo duro e a palma voltada para baixo. Ele não atribui o problema a travamentos; o app roda aparentemente bem. A redução do tamanho em tela foi implementada, sem mudar proporções anatômicas.

O consumo de cota informado pelo usuário ilustra a necessidade de checkpoints: Astra consumiu aproximadamente 38% da janela de cinco horas em 13m20s na M0-B inicial; Sol High consumiu cerca de 13% em 15m04s na M0-B1. São observações individuais, **não previsões de consumo futuro**.

## 3. Estado atual e decisões após o parecer humano

- **M0-A aprovado e encerrado. M0-B continua aberto.** A personagem está integrada e interativa; naturalidade de Idle e Wave **não está aprovada**.
- **Preservar a base atual** e adiar a ideia de criar um humanoide autoral do zero ou trocar o asset. Neste momento não fazer nova rodada de recoloração, figurino ou modelagem.
- O problema principal é de **pose, orientação de ossos e linguagem corporal**, não de existência dos clipes ou, segundo observação do usuário, de desempenho.
- O script `scripts/prepare_luna.py` produz `public/models/Luna.glb` diretamente a partir de `assets/luna/base.vrm`. Os clipes foram construídos em código com rotações de ossos; microvariações abaixo de ~1 grau do M0-B1 ficaram pouco perceptíveis. A palma não foi explicitamente orientada em relação à câmera.
- O original VRM inclui rig/estruturas auxiliares. A versão GLB adaptada removeu morph targets e extensões/dinâmica VRM; não assumir que ela suporta todas as expressões ou que importação/reexportação no Blender preserva automaticamente todo o comportamento.
- Preservar licenciamento e procedência conforme `public/models/Luna.LICENSE.json` e `docs/M0-B-STATUS.md` (VRM Public License 1.0, não CC0). Preservar também RobotExpressive, histórico M0-A e fallback Mesa.

## 4. Próxima sessão: aula prática de Blender, com o usuário

**Objetivo:** orientar o usuário no Blender 3.3.21, assumindo pouca experiência. Primeiro produzir uma **pose de Idle estática convincente**; depois, se a primeira estiver compreendida e salva, começar a **pose de saudação**, corrigindo a palma. Não iniciar outro ciclo aberto de animação no Codex antes dessa experiência.

### Preparação segura

1. Conferir `git status --short --branch`, HEAD e o documento `docs/M0-B-STATUS.md`. Não sobrescrever alterações novas.
2. Abrir o Blender 3.3.21 e usar viewport Solid. Trabalhar numa **cópia de experimento**, nunca sobrescrever `assets/luna/base.vrm`, `public/models/Luna.glb` ou `scripts/prepare_luna.py` durante a aula.
3. Verificar a importação do GLB atual por `File > Import > glTF 2.0`; observar se armature, meshes, animações e controles estão acessíveis. Se houver problema, parar e diagnosticar; não assumir que `base.vrm` é importável no Blender 3.3 sem complemento compatível.
4. Salvar arquivo de trabalho `.blend` em local apropriado, preferencialmente fora dos assets distribuídos até confirmar que é útil e reproduzível. Não editar/exportar diretamente sobre o GLB do aplicativo.

### Parte A — Idle (prioridade)

1. Ensinar navegação na viewport, Outliner, seleção de armature, Pose Mode, rotação local/global e retorno à pose anterior.
2. Identificar visualmente ombros, braços, cotovelos, punhos, tronco e cabeça; conferir eixos e possíveis ossos auxiliares **no rig real**, sem adivinhar índices.
3. Com pose estática, reduzir simetria e rigidez: braços mais relaxados, cotovelos levemente flexionados, ombros baixos e cabeça em orientação confortável. Checar frontal e lateral, roupa/cabelo e posição dos pés.
4. Salvar uma primeira pose e captar imagens de referência. Não gastar tempo com micro-respiração antes de a pose estática parecer humana e confortável.

### Parte B — saudação (depois da pose de Idle)

1. Partir da pose de repouso; erguer braço direito e flexionar cotovelo com o ombro participando.
2. Ajustar rotação do antebraço/punho até que a **palma fique voltada de modo reconhecível para a câmera**, conferindo visão frontal e lateral; cuidado com cabelo, roupa e ossos auxiliares.
3. Salvar pose estática de saudação; somente após conferência visual começar keyframes simples para entrada, pequena oscilação e retorno.
4. Se o rig ou exportação impedir o resultado, documentar o bloqueio e pedir ao Codex **diagnóstico localizado**. Não entrar em tentativa e erro cega em quaternions.

### Portão de integração

Não substituir o GLB do app por exportação manual sem validar armature, integridade do GLB, clipes, licença, clique/botão, retorno ao Idle e comportamento no Tauri. Se o trabalho manual for aproveitado, combinar um pipeline reproduzível (arquivo-fonte Blender e exportação) e **ajustar/substituir conscientemente** `prepare_luna.py`, para que uma regeneração não apague as animações manuais. O Codex pode apoiar essa integração *após* poses aprovadas.

## 5. Critérios para continuidade e limites

- Próxima entrega de aprendizagem: usuário consegue abrir e salvar um `.blend` experimental, identificar ossos e obter uma pose de Idle visivelmente melhor; saudação vem em seguida, sem pressa de implementar tudo numa sessão.
- A aplicação existente continua funcional enquanto a aula acontece. Não declarar M0-B aprovado ou vender animação atual como natural apenas porque os testes automatizados passam.
- Mantêm-se fora deste trabalho: nova personagem/figurino, lip-sync, expressões faciais, agente, chat, Android, mensageiros e modificações no workaround gráfico.
- Para retomar, ler primeiro este documento e `docs/M0-B-STATUS.md`, pedir ao usuário sua tela/resultado do Blender e avançar **passo a passo**. Ao concluir o experimento, registrar o que foi realmente criado, validado e o próximo passo.

**Nota:** as gravações enviadas na conversa motivaram o parecer, mas não estão incluídas automaticamente no repositório. Não declarar capturas ou arquivos `.blend` novos como existentes antes de serem criados e verificados.
