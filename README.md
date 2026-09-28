# Assistente 3D

Aplicação desktop incremental com Tauri 2, React, TypeScript e Three.js.

**Estado:** M0-A aprovado e encerrado. M0-B mantém a candidata Luna integrada e interativa. LR-1 a LR-5 estabeleceram runtime visual desacoplado, tarefas/eventos, segurança, SQLite, Context Builder, Registry, Scheduler e MockProvider. **LR-6 está em PASS completo** com Gemini real, streaming, usage, cancelamento, persistência local e credencial protegida validados no Fedora. Antes da LR-7, o projeto percorre a trilha **UIP-0 → UIP-7**, dedicada à interface de presença desktop e performance; **UIP-0 → UIP-5 estão concluídas; UIP-6A e UIP-6B estão em PASS funcional; UIP-6C é CANDIDATA ao gate humano final**. O trabalho artístico de Blender segue independente. **Após o fechamento da UIP-2 e antes da UIP-3, a Idle procedural foi substituída por uma Idle manual exportada do Blender; isso cria uma nova baseline de asset/performance para as medições seguintes.** Consulte [integração da Idle manual](docs/MANUAL-IDLE-INTEGRATION.md), [plano de UI/performance](docs/UI-PERFORMANCE-PLAN.md), [status técnico](docs/M0-B-STATUS.md) e [plano operacional](docs/PLANO-OPERACIONAL-LUNA.md).

## Direção arquitetural pós-M0

A partir de 25/09/2026, o projeto passa a evoluir em **duas frentes paralelas**: o trabalho artístico de avatar/animação continua no Blender enquanto o núcleo funcional da agente é construído em Rust/Tauri. Uma frente não deve bloquear a outra.

A arquitetura-alvo mantém Tauri 2 + Rust + React/TypeScript + Three.js, promove Rust a **Luna Core** e trata LLMs como recursos cognitivos substituíveis. Avatar e animações serão desacoplados do agente, com direção para VRM/VRMA e Animation Director semântico.

Documentos principais:

- [Arquitetura-alvo da Luna](docs/ARCHITECTURE-LUNA.md)
- [Provedores, SDKs e orquestração de IA](docs/AI-PROVIDERS-ORCHESTRATION.md)
- [Avatar e runtime de animações](docs/AVATAR-ANIMATION-RUNTIME.md)
- [Integração da Idle manual e quebra de baseline](docs/MANUAL-IDLE-INTEGRATION.md)
- [Plano operacional paralelo](docs/PLANO-OPERACIONAL-LUNA.md)
- [Plano de UI e performance](docs/UI-PERFORMANCE-PLAN.md)
- [Segurança LR-3](docs/SECURITY.md)
- [Identidade e memória LR-4](docs/MEMORY-IDENTITY.md)
- [Runtime cognitivo LR-5](docs/COGNITION-RUNTIME.md)

**Estado LR-6:** PASS completo em 26/09/2026. O adapter Gemini, chat local, streaming, usage, cancelamento, persistência e SecretStore foram validados com API real e reinício do aplicativo. A **UIP-0 — contratos + baseline**, a **UIP-1 — Presence Shell**, a **UIP-2 — Render Budget**, a **UIP-3 — ergonomia da janela**, a **UIP-4 — Composer + conversa atual** e a **UIP-5 — histórico + resumo assíncrono** estão fechadas; **UIP-6A e UIP-6B estão fechadas em PASS funcional; UIP-6C — consolidação e gate final da UIP-6 — é CANDIDATA; UIP-6 completa ainda não é PASS.** LR-7 não foi iniciada. Na UIP-3, always-on-top no Wayland nativo e click-through seguro ficaram como limitações adiadas; na UIP-4, o resize nativo no GNOME/Wayland ficou como dívida de estabilidade espacial após a rejeição do POC de múltiplas WebViews. Consulte [Gemini LR-6](docs/GEMINI-PROVIDER.md) e [plano UIP](docs/UI-PERFORMANCE-PLAN.md).

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
cargo fmt --check --manifest-path src-tauri/Cargo.toml
```

Clique na personagem ou no botão **Acenar**. O modelo deve voltar ao repouso após o gesto.

Na janela Tauri, o painel **LUNA CORE · LR-2** inicia uma tarefa mock de duas etapas, mostra TaskId e eventos reais do Rust, e permite cancelá-la. Após conclusão ou cancelamento, outra tarefa pode ser iniciada. No navegador comum, o avatar continua disponível e o painel informa que o Luna Core requer Tauri.

O diagnóstico **MEMORY · LR-4** apresenta apenas metadados da persistência. Em desenvolvimento, permite importar o bootstrap privado opcional e criar uma conversa artificial. O banco fica no diretório local da aplicação e continua disponível após remover o arquivo de bootstrap. O SQLite não é criptografado nesta fase e nunca armazena credenciais.

O painel **COGNITION · LR-5**, somente em `tauri dev`, executa cenários locais de streaming, retry, fallback, cooldown, budget e cancelamento. Ele mostra contagens e resultado mock sem expor identidade ou memórias. A tarefa requer identidade já importada no SQLite; sem ela, termina com `identity_unavailable`. No navegador comum, o painel apenas informa que requer Luna Core/Tauri.

O diagnóstico **SECURITY · LR-3** mostra disponibilidade do SecretStore e presença de um segredo artificial sem devolver seu valor à UI. Em `tauri dev`, botões permitem gravar/verificar/remover esse teste; os comandos não são registrados no handler release. A CSP e a capability `main-window` limitam a WebView. A chave de unlock fica no credential store do sistema operacional; o arquivo legado é migrado e removido após verificação do snapshot. A chave Gemini é definida, substituída ou removida em **Configurações → IA e modelos** e permanece no SecretStore; a main exibe apenas seu estado. Detalhes e limitações estão em [Segurança LR-3](docs/SECURITY.md).

Nesta máquina, a janela Tauri usa Mesa em software por padrão. O WebKitGTK com aceleração Intel HD 4000 apresentou canvas vazio/erro WebGL 1282, enquanto o modo de software exibiu e animou a personagem. Para repetir o diagnóstico com a GPU, execute `LIBGL_ALWAYS_SOFTWARE=0 npm run tauri dev`.

## Arquivos principais

- `src/App.tsx`: Presence Shell transparente, acionador inferior e acesso DEV aos diagnósticos.
- `src/avatar/AvatarViewport.tsx`: liga o viewport React ao runtime e trata o clique na personagem.
- `src/avatar/runtime/`: cena/WebGL, ciclo de vida do avatar, catálogo semântico e transições Idle/greeting.
- `src/avatar/adapters/LegacyGlbAdapter.ts`: carrega e libera a Luna GLB atual, mapeando os clipes embutidos.
- `src/luna/`: painel de diagnóstico, tipos de eventos, cliente Tauri e mapeamento `TaskEvent → AnimationIntent`.
- `src-tauri/src/luna/`: tipos de tarefa/evento, registro em memória e execução/cancelamento assíncronos.
- `src-tauri/src/security/`: validação de entrada, audit e SecretStore Stronghold acessível só ao Rust.
- `src-tauri/src/persistence/` e `src-tauri/migrations/`: SQLite, identidade versionada, memória operacional, conversa, histórico de tarefas e migration.
- `src-tauri/src/cognition/`: Context Builder, contrato Provider, Registry, Scheduler e MockProvider local.
- `src/styles.css`: Presence Shell transparente e estilos dos diagnósticos DEV.
- `public/models/Luna.glb`: candidata local, sem dependência de rede durante a execução; licença em `public/models/Luna.LICENSE.json`.
- `assets/luna/base.vrm` e `scripts/prepare_luna.py`: fonte original e gerador de **bootstrap legado**; o script não publica mais `public/models/Luna.glb`.
- `scripts/validate_luna_glb.py`: valida o contrato legado `Idle`/`Wave` e sinaliza expansão material do asset.
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

A Luna atual continua sendo uma candidata em evolução. A **Idle manual criada no Blender foi integrada e aprovada pelo usuário como melhora visual clara** em relação ao repouso procedural quase estático do M0-B1. O `Wave` permanece legado e ainda precisa de revisão manual. A troca de `Luna.glb` ocorreu após UIP-2 e antes de UIP-3 e deve ser tratada como quebra de baseline em comparações de performance. Há uma dívida conhecida de exportação Blender nos brincos: o offset já está presente no GLB e não há evidência de origem no runtime Three.js.

Para o contexto cronológico e a oficina de Blender, veja [RETOMADA-BLENDER-LUNA.md](docs/RETOMADA-BLENDER-LUNA.md). O progresso e os riscos técnicos do protótipo permanecem em [M0-B-STATUS.md](docs/M0-B-STATUS.md).

O trabalho artístico não precisa bloquear a evolução estrutural: **LR-1** separou o runtime de avatar/animação sem mudar o asset atual; em paralelo, o usuário pode continuar produzindo Idle e futuras animações no Blender. A sequência completa está em [PLANO-OPERACIONAL-LUNA.md](docs/PLANO-OPERACIONAL-LUNA.md).

O chat Gemini LR-6 funciona na janela Tauri com credencial mantida no SecretStore. Ferramentas operacionais, Android, mensageiros e segundo provider continuam fora do estado implementado. Antes deles, a trilha UIP transforma a interface atual em presença desktop transparente/recolhível e estabelece orçamento de performance.
