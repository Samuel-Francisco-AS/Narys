# Assistente 3D — M0-A

Protótipo desktop mínimo para validar uma personagem humanoide 3D no Fedora com Tauri 2, React, TypeScript e Three.js.

## Requisitos no Fedora

- Node.js e npm
- Rust e Cargo ([rustup](https://rustup.rs/))
- Bibliotecas de desenvolvimento do Tauri:

```bash
sudo dnf install webkit2gtk4.1-devel openssl-devel wget libappindicator-gtk3-devel librsvg2-devel libxdo-devel
```

Esses pacotes exigem privilégios de administrador. Instale-os manualmente; o projeto não executa `sudo`.

Se Rust foi instalado via rustup e o terminal ainda não encontra `cargo`, execute `. "$HOME/.cargo/env"` na sessão atual.

## Executar

```bash
npm install
npm run dev
```

Abra o endereço mostrado pelo Vite, normalmente `http://localhost:5173/`. Para a janela desktop:

```bash
npm run tauri dev
```

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
- `public/models/RobotExpressive.glb`: personagem local, sem dependência de rede durante a execução.
- `assets/app-icon.svg`: ícone original do protótipo; `src-tauri/icons/` contém versões geradas pelo CLI.
- `src-tauri/`: configuração e inicialização da janela Tauri.

Fluxo: o React monta `CharacterScene` → Three.js cria o renderizador WebGL e carrega o GLB → `AnimationMixer` toca `Idle` → clique inicia `Wave` → o laço de renderização atualiza a animação e desenha a cena.

## Modelo e licença

`RobotExpressive.glb` vem do [Three.js r186](https://github.com/mrdoob/three.js/tree/r186/examples/models/gltf/RobotExpressive). O [README do asset](https://github.com/mrdoob/three.js/blob/r186/examples/models/gltf/RobotExpressive/README.md) declara **CC0 1.0**. Modelo por Tomás Laulhé; modificações por Don McCurdy. O arquivo incluído tem SHA-256 `047f5e5fb3bb6d378bd1df16ca6137f2a596c99b3a1b5690b4020c05aaf6f319`.

É um robô humanoide de teste, não a identidade visual definitiva do produto. Como o asset é GLB, `@pixiv/three-vrm` não é necessário neste checkpoint.

## Validação M0-A

Consulte [VALIDACAO.md](VALIDACAO.md) para resultados, limitações e passos de inspeção visual.
