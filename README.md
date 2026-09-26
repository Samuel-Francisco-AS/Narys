# Assistente 3D

Aplicação desktop incremental com Tauri 2, React, TypeScript e Three.js.

**Estado:** M0-A aprovado e encerrado. M0-B tem a candidata Luna integrada e interativa, mas **as animações Idle/Wave ainda não foram aprovadas artisticamente** após o refinamento M0-B1. O próximo trabalho será uma aula prática de poses no Blender, sem substituir a versão funcional. Consulte [status técnico](docs/M0-B-STATUS.md), [retomada e plano da aula](docs/RETOMADA-BLENDER-LUNA.md) e [validação M0-A](VALIDACAO.md).

## Direção arquitetural pós-M0

A partir de 25/09/2026, o projeto passa a evoluir em **duas frentes paralelas**: o trabalho artístico de avatar/animação continua no Blender enquanto o núcleo funcional da agente é construído em Rust/Tauri. Uma frente não deve bloquear a outra.

A arquitetura-alvo mantém Tauri 2 + Rust + React/TypeScript + Three.js, promove Rust a **Luna Core** e trata LLMs como recursos cognitivos substituíveis. Avatar e animações serão desacoplados do agente, com direção para VRM/VRMA e Animation Director semântico.

Documentos principais:

- [Arquitetura-alvo da Luna](docs/ARCHITECTURE-LUNA.md)
- [Provedores, SDKs e orquestração de IA](docs/AI-PROVIDERS-ORCHESTRATION.md)
- [Avatar e runtime de animações](docs/AVATAR-ANIMATION-RUNTIME.md)
- [Plano operacional paralelo](docs/PLANO-OPERACIONAL-LUNA.md)

**Importante:** esses documentos descrevem a direção e o plano; o estado implementado continua sendo o M0 descrito neste README e nos relatórios de validação.

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

Nesta máquina, a janela Tauri usa Mesa em software por padrão. O WebKitGTK com aceleração Intel HD 4000 apresentou canvas vazio/erro WebGL 1282, enquanto o modo de software exibiu e animou a personagem. Para repetir o diagnóstico com a GPU, execute `LIBGL_ALWAYS_SOFTWARE=0 npm run tauri dev`.

## Arquivos principais

- `src/App.tsx`: composição da interface e botão de interação.
- `src/components/CharacterScene.tsx`: cria a cena, carrega o GLB, toca os clipes e informa falhas de WebGL/carregamento.
- `src/styles.css`: layout escuro e responsivo.
- `public/models/Luna.glb`: candidata local, sem dependência de rede durante a execução; licença em `public/models/Luna.LICENSE.json`.
- `assets/luna/base.vrm` e `scripts/prepare_luna.py`: original e preparação reproduzível.
- `public/models/RobotExpressive.glb`: modelo anterior preservado.
- `assets/app-icon.svg`: ícone original do protótipo; `src-tauri/icons/` contém versões geradas pelo CLI.
- `src-tauri/`: configuração e inicialização da janela Tauri.

Fluxo: o React monta `CharacterScene` → Three.js cria o renderizador WebGL e carrega o GLB → `AnimationMixer` toca `Idle` → clique inicia `Wave` → o laço de renderização atualiza a animação e desenha a cena.

## Modelos e licenças

A candidata Luna adapta `VRM1_Constraint_Twist_Sample` v1.0.1 da pixiv Inc., sob **VRM Public License 1.0** com permissões de modificação e redistribuição e restrição a expressões antissociais/de ódio. A adaptação mantém os mesmos termos. Procedência, hash, configurações e limitações estão no [status central](docs/M0-B-STATUS.md). Os clipes `Idle` e `Wave` foram criados localmente; a base não tinha animações. O app continua usando GLTFLoader, sem runtime VRM.

### Modelo anterior (M0-A)

`RobotExpressive.glb` vem do [Three.js r186](https://github.com/mrdoob/three.js/tree/r186/examples/models/gltf/RobotExpressive). O [README do asset](https://github.com/mrdoob/three.js/blob/r186/examples/models/gltf/RobotExpressive/README.md) declara **CC0 1.0**. Modelo por Tomás Laulhé; modificações por Don McCurdy. O arquivo incluído tem SHA-256 `047f5e5fb3bb6d378bd1df16ca6137f2a596c99b3a1b5690b4020c05aaf6f319`.

É um robô humanoide usado exclusivamente para validar M0-A, não a personagem definitiva. Como o asset é GLB, `@pixiv/three-vrm` não é necessário neste checkpoint.

## Continuidade

A Luna atual é uma candidata técnica funcional, **não uma animação aprovada**: o usuário relatou Idle muito rígido e aceno pouco natural, com a palma voltada para baixo. A apresentação ficou cerca de 12% menor no M0-B1. O próximo passo é aprender no Blender a ajustar primeiro a pose de repouso e depois a pose de saudação, trabalhando numa cópia e preservando o GLB utilizado pelo aplicativo.

Para o contexto cronológico e a oficina de Blender, veja [RETOMADA-BLENDER-LUNA.md](docs/RETOMADA-BLENDER-LUNA.md). O progresso e os riscos técnicos do protótipo permanecem em [M0-B-STATUS.md](docs/M0-B-STATUS.md).

O trabalho artístico não precisa mais bloquear a evolução estrutural: o próximo marco funcional recomendado é **LR-1**, uma refatoração sem mudança visual que separa o runtime de avatar/animação; em paralelo, o usuário pode continuar produzindo Idle e futuras animações no Blender. A sequência completa está em [PLANO-OPERACIONAL-LUNA.md](docs/PLANO-OPERACIONAL-LUNA.md).

Conversação funcional, Luna Core, memória, provedores de IA, ferramentas operacionais, Android e mensageiros continuam **fora do estado implementado atual**, embora já tenham arquitetura e plano documentados.
