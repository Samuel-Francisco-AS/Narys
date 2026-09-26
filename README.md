# Assistente 3D

Aplicação desktop incremental com Tauri 2, React, TypeScript e Three.js.

**Estado:** M0-A aprovado e encerrado. M0-B tem a candidata Luna integrada e interativa, mas **as animações Idle/Wave ainda não foram aprovadas artisticamente** após o refinamento M0-B1. A LR-1 separou o runtime visual sem alterar os assets ou movimentos. A LR-2 acrescentou tarefas mock, cancelamento e eventos via Tauri Channel. A LR-3 preparou CSP, permissões por comando, validação de entrada, audit e um teste Rust de Stronghold sem credenciais reais. A LR-4 adicionou SQLite local para identidade versionada, memória, conversa diagnóstica e histórico resumido de tarefas. O trabalho artístico seguinte continua sendo a aula prática de poses no Blender. Consulte [status técnico](docs/M0-B-STATUS.md), [retomada e plano da aula](docs/RETOMADA-BLENDER-LUNA.md) e [validação M0-A](VALIDACAO.md).

## Direção arquitetural pós-M0

A partir de 25/09/2026, o projeto passa a evoluir em **duas frentes paralelas**: o trabalho artístico de avatar/animação continua no Blender enquanto o núcleo funcional da agente é construído em Rust/Tauri. Uma frente não deve bloquear a outra.

A arquitetura-alvo mantém Tauri 2 + Rust + React/TypeScript + Three.js, promove Rust a **Luna Core** e trata LLMs como recursos cognitivos substituíveis. Avatar e animações serão desacoplados do agente, com direção para VRM/VRMA e Animation Director semântico.

Documentos principais:

- [Arquitetura-alvo da Luna](docs/ARCHITECTURE-LUNA.md)
- [Provedores, SDKs e orquestração de IA](docs/AI-PROVIDERS-ORCHESTRATION.md)
- [Avatar e runtime de animações](docs/AVATAR-ANIMATION-RUNTIME.md)
- [Plano operacional paralelo](docs/PLANO-OPERACIONAL-LUNA.md)
- [Segurança LR-3](docs/SECURITY.md)
- [Identidade e memória LR-4](docs/MEMORY-IDENTITY.md)

**Importante:** esses documentos descrevem a direção e o plano; o estado implementado é o M0 visual, o runtime separado na LR-1, o núcleo de tarefas/eventos da LR-2, a fundação de segurança da LR-3 e a persistência LR-4.

## Requisitos no Fedora

- Node.js e npm
- Rust e Cargo ([rustup](https://rustup.rs/))
- Bibliotecas de desenvolvimento do Tauri:

```bash
sudo dnf install webkit2gtk4.1-devel openssl-devel wget libappindicator-gtk3-devel librsvg2-devel libxdo-devel
```

Esses pacotes exigem privilégios de administrador. Instale-os manualmente; o projeto não executa `sudo`.

## Executar

```bash
npm install
npm run dev
```

Abra o endereço mostrado pelo Vite, normalmente `http://localhost:5173/`. Para a janela desktop, carregue Rust/Cargo no terminal antes de iniciar o Tauri:

```bash
. "$HOME/.cargo/env"
npm run tauri dev
```

Na validação manual, a primeira tentativa de `npm run tauri dev` falhou porque `cargo` não estava no PATH. O comando acima corrigiu o ambiente da sessão.

Verificações de código:

```bash
npm run typecheck
npm run build
```

Clique na personagem ou no botão **Acenar**. O modelo deve voltar ao repouso após o gesto.

Na janela Tauri, o painel **LUNA CORE · LR-2** inicia uma tarefa mock de duas etapas, mostra TaskId e eventos reais do Rust, e permite cancelá-la. Após conclusão ou cancelamento, outra tarefa pode ser iniciada. No navegador comum, o avatar continua disponível e o painel informa que o Luna Core requer Tauri.

O diagnóstico **MEMORY · LR-4** apresenta apenas metadados da persistência. Em desenvolvimento, permite importar o bootstrap privado opcional e criar uma conversa artificial. O banco fica no diretório local da aplicação e continua disponível após remover o arquivo de bootstrap. O SQLite não é criptografado nesta fase e nunca armazena credenciais.

O diagnóstico **SECURITY · LR-3** mostra disponibilidade do SecretStore e presença de um segredo artificial sem devolver seu valor à UI. Em `tauri dev`, botões permitem gravar/verificar/remover esse teste; em release eles são rejeitados. A CSP e a capability `main-window` limitam a WebView. O snapshot Stronghold ainda usa uma chave local no mesmo diretório: isso prova o mecanismo, mas **não autoriza cadastrar API keys reais**. Detalhes e limitações estão em [Segurança LR-3](docs/SECURITY.md).

Nesta máquina, a janela Tauri usa Mesa em software por padrão. O WebKitGTK com aceleração Intel HD 4000 apresentou canvas vazio/erro WebGL 1282, enquanto o modo de software exibiu e animou a personagem. Para repetir o diagnóstico com a GPU, execute `LIBGL_ALWAYS_SOFTWARE=0 npm run tauri dev`.

## Arquivos principais

- `src/App.tsx`: composição da interface e botão de interação.
- `src/avatar/AvatarViewport.tsx`: liga o viewport React ao runtime e trata o clique na personagem.
- `src/avatar/runtime/`: cena/WebGL, ciclo de vida do avatar, catálogo semântico e transições Idle/greeting.
- `src/avatar/adapters/LegacyGlbAdapter.ts`: carrega e libera a Luna GLB atual, mapeando os clipes embutidos.
- `src/luna/`: painel de diagnóstico, tipos de eventos, cliente Tauri e mapeamento `TaskEvent → AnimationIntent`.
- `src-tauri/src/luna/`: tipos de tarefa/evento, registro em memória e execução/cancelamento assíncronos.
- `src-tauri/src/security/`: validação de entrada, audit e SecretStore Stronghold acessível só ao Rust.
- `src-tauri/src/persistence/` e `src-tauri/migrations/`: SQLite, identidade versionada, memória operacional, conversa, histórico de tarefas e migration.
- `src/styles.css`: layout escuro e responsivo.
- `public/models/Luna.glb`: candidata local, sem dependência de rede durante a execução; licença em `public/models/Luna.LICENSE.json`.
- `assets/luna/base.vrm` e `scripts/prepare_luna.py`: original e preparação reproduzível.
- `public/models/RobotExpressive.glb`: modelo anterior preservado.
- `assets/app-icon.svg`: ícone original do protótipo; `src-tauri/icons/` contém versões geradas pelo CLI.
- `src-tauri/`: configuração e inicialização da janela Tauri.

Fluxo visual: o React monta `AvatarViewport` → `SceneRuntime` cria o renderizador WebGL → o adapter carrega a Luna GLB → `AnimationDirector` toca `Idle` → clique ou botão solicita `greeting` → o diretor toca o clipe legado `Wave` e retorna ao Idle.

Fluxo LR-2: o painel cria um Tauri Channel → `start_mock_task` registra um TaskId → o worker Rust emite eventos de etapas → o painel mostra o stream e solicita intenções ao avatar. Durante a tarefa o avatar permanece em `idle`; na conclusão, `greeting` serve provisoriamente como confirmação. O botão **Cancelar** chama `cancel_task` no Rust. Nenhuma LLM ou API externa participa desse fluxo.

## Modelos e licenças

A candidata Luna adapta `VRM1_Constraint_Twist_Sample` v1.0.1 da pixiv Inc., sob **VRM Public License 1.0** com permissões de modificação e redistribuição e restrição a expressões antissociais/de ódio. A adaptação mantém os mesmos termos. Procedência, hash, configurações e limitações estão no [status central](docs/M0-B-STATUS.md). Os clipes `Idle` e `Wave` foram criados localmente; a base não tinha animações. O app continua usando GLTFLoader, sem runtime VRM.

### Modelo anterior (M0-A)

`RobotExpressive.glb` vem do [Three.js r186](https://github.com/mrdoob/three.js/tree/r186/examples/models/gltf/RobotExpressive). O [README do asset](https://github.com/mrdoob/three.js/blob/r186/examples/models/gltf/RobotExpressive/README.md) declara **CC0 1.0**. Modelo por Tomás Laulhé; modificações por Don McCurdy. O arquivo incluído tem SHA-256 `047f5e5fb3bb6d378bd1df16ca6137f2a596c99b3a1b5690b4020c05aaf6f319`.

É um robô humanoide usado exclusivamente para validar M0-A, não a personagem definitiva. Como o asset é GLB, `@pixiv/three-vrm` não é necessário neste checkpoint.

## Continuidade

A Luna atual é uma candidata técnica funcional, **não uma animação aprovada**: o usuário relatou Idle muito rígido e aceno pouco natural, com a palma voltada para baixo. A apresentação ficou cerca de 12% menor no M0-B1. O próximo passo é aprender no Blender a ajustar primeiro a pose de repouso e depois a pose de saudação, trabalhando numa cópia e preservando o GLB utilizado pelo aplicativo.

Para o contexto cronológico e a oficina de Blender, veja [RETOMADA-BLENDER-LUNA.md](docs/RETOMADA-BLENDER-LUNA.md). O progresso e os riscos técnicos do protótipo permanecem em [M0-B-STATUS.md](docs/M0-B-STATUS.md).

O trabalho artístico não precisa bloquear a evolução estrutural: **LR-1** separou o runtime de avatar/animação sem mudar o asset atual; em paralelo, o usuário pode continuar produzindo Idle e futuras animações no Blender. A sequência completa está em [PLANO-OPERACIONAL-LUNA.md](docs/PLANO-OPERACIONAL-LUNA.md).

Conversação funcional, provedores de IA, ferramentas operacionais, Android e mensageiros continuam **fora do estado implementado atual**, embora já tenham arquitetura e plano documentados.
