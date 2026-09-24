# Assistente 3D

Aplicação desktop incremental com Tauri 2, React, TypeScript e Three.js.

**Estado:** M0-A (fundação e validação gráfica 3D) aprovado pelo usuário em 24/09/2026 e encerrado. O resultado e os limites da validação estão em [VALIDACAO.md](VALIDACAO.md).

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
- `public/models/RobotExpressive.glb`: personagem local, sem dependência de rede durante a execução.
- `assets/app-icon.svg`: ícone original do protótipo; `src-tauri/icons/` contém versões geradas pelo CLI.
- `src-tauri/`: configuração e inicialização da janela Tauri.

Fluxo: o React monta `CharacterScene` → Three.js cria o renderizador WebGL e carrega o GLB → `AnimationMixer` toca `Idle` → clique inicia `Wave` → o laço de renderização atualiza a animação e desenha a cena.

## Modelo e licença

`RobotExpressive.glb` vem do [Three.js r186](https://github.com/mrdoob/three.js/tree/r186/examples/models/gltf/RobotExpressive). O [README do asset](https://github.com/mrdoob/three.js/blob/r186/examples/models/gltf/RobotExpressive/README.md) declara **CC0 1.0**. Modelo por Tomás Laulhé; modificações por Don McCurdy. O arquivo incluído tem SHA-256 `047f5e5fb3bb6d378bd1df16ca6137f2a596c99b3a1b5690b4020c05aaf6f319`.

É um robô humanoide usado exclusivamente para validar M0-A, não a personagem definitiva. Como o asset é GLB, `@pixiv/three-vrm` não é necessário neste checkpoint.

## Continuidade

O próximo checkpoint previsto é **M0-B**, ainda não iniciado: identidade visual e integração de uma personagem feminina 3D estilizada, possivelmente com estética anime, androide ou ciborgue. A personagem candidata deverá ser avaliada quanto a:

- aparência visual;
- licença e procedência dos assets;
- compatibilidade com Three.js;
- esqueleto e recursos de animação;
- expressões e reações possíveis;
- desempenho na janela Tauri no hardware real.

Blender e GPT-6 Astra poderão apoiar trabalhos artísticos posteriores.

Conversação funcional, agente de IA, ferramentas operacionais, Android e mensageiros não fazem parte do escopo concluído em M0-A.
