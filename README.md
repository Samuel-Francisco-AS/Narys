# Assistente 3D

Aplicação desktop incremental com Tauri 2, React, TypeScript e Three.js.

**Estado:** M0-B com primeira candidata Luna integrada e verificada tecnicamente; avaliação visual e de fluidez pelo usuário pendente. Progresso, licença e retomada em [docs/M0-B-STATUS.md](docs/M0-B-STATUS.md). M0-A permanece aprovado e encerrado, conforme [VALIDACAO.md](VALIDACAO.md).

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

O M0-B aguarda avaliação manual da candidata na janela Tauri: aparência, botão/clique, retorno ao repouso e fluidez durante alguns minutos. O [status central](docs/M0-B-STATUS.md) contém o próximo passo exato, as capturas e os comandos de reprodução. A roupa ainda conserva uma silhueta casual; a direção futurista pode precisar de refinamento localizado após essa avaliação.

Conversação funcional, agente de IA, ferramentas operacionais, Android e mensageiros permanecem fora do escopo.
