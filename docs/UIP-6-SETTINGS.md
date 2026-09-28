# UIP-6A — janelas de configurações e política cognitiva v1

**Estado: CANDIDATA.** UIP-6 completa continua aberta. A UIP-6A entrega escolhas efetivas para `conversation` e `summary`; UIP-6B tratará configurações gerais e parâmetros avançados, e UIP-6C consolidará o gate. LR-7 não foi iniciada.

## Autonomia e execução

`cognitive_role_policies` (migration 003) persiste `role`, `provider_id`, `model`, `thinking_level`, `max_output_tokens`, `max_provider_calls` e `updated_at`. A migration é atômica, preserva os defaults anteriores (`conversation`: Gemini, `gemini-3.8-flash`, low, 4096, 2; `summary`: Gemini, `gemini-3.8-flash`, low, 1024, 1) e não altera 001/002. Atualizações são por role e não afetam o outro role.

O Core usa `CognitiveRole`, `ThinkingLevel` e `CognitiveRolePolicy`. O comando de conversa lê a policy antes de registrar a tarefa. O worker lê a policy de resumo antes de cada execução; requests usam snapshot. `ProviderRequest` carrega provider explícito, modelo, thinking opcional e output opcional. O Scheduler só considera o provider escolhido, respeita `max_provider_calls`, conta usage e aplica teto de output apenas quando há `Some(limit)`. Sem teto da Luna (`None`), o adapter omite `max_output_tokens`; pensando no padrão do provider (`None`), omite `thinking_level`. O provider/modelo ainda tem seus limites reais. A escolha do usuário não é substituída silenciosamente.

Gemini é o único provider selecionável nesta versão. O modelo é um identificador editável, validado por integridade (não vazio, até 128 bytes, sem espaços nas bordas ou controles), sem allowlist de nomes. `low`, `medium` e `high` são explícitos; `minimal` não foi adicionado. Valores inválidos ou provider ausente geram erro sanitizado. Streaming continua ativo e `thinking_summaries=none` continua invariante do adapter nesta fase.

## Superfícies e segurança

A main abre `settings-general` por um botão no Composer. A janela geral abre `settings-ai`. As duas são WebViews nativas independentes, decoradas, opacas e redimensionáveis, sem `parent`, `transient_for` ou posicionamento relativo. Abrir novamente foca a mesma label. Fechar destrói a WebView; reabrir cria outra. `src/main.tsx` seleciona a superfície por `surface=` e usa importação dinâmica: as Settings não importam `App`, `AvatarViewport`, Three.js nem o runtime de animação. O gate físico deve confirmar zero canvas e zero WebGL em ambas.

Capabilities por label: main pode abrir Geral, mantém tarefas/conversa/diagnósticos necessários e não pode definir/remover API key nem policy; Geral só pode abrir IA; IA pode ler settings, salvar policy e definir/remover a chave Gemini. IA e Geral não têm comandos de conversa, memória, shell, filesystem ou avatar. `get_ai_settings` retorna somente estado da credencial; a chave existente nunca retorna ao React. A chave digitada vai diretamente ao comando de gravação e permanece no SecretStore. Não vai ao SQLite ou localStorage. O painel Gemini DEV da main oferece diagnóstico e direciona para Settings.

## Invariantes e próximos passos

`store:false`, isolamento de sessão, validação de input, proteção de segredos, privacidade de memórias e permissões Tauri são invariantes de segurança. Provider, modelo, thinking, output e número de chamadas são preferências do usuário. A janela Geral é apenas um shell informativo nesta 6A. UIP-6B cobrirá controles gerais editáveis e avaliação de parâmetros avançados (sem fingir que streaming e thinking summaries já são editáveis). UIP-6C fará o gate de consolidação. Wayland, avatar, memória e novos providers seguem fora desta rodada.

## Gate técnico da candidata

Uma instância Tauri real em Mesa software (`LIBGL_ALWAYS_SOFTWARE=1`) com dados locais isolados abriu main, Geral e IA. Um harness temporário no setup abriu as janelas em sequência, verificou labels e `webview_windows().len()` igual a 2 e 3, chamou abertura repetida e fechou as duas; o harness foi removido do código final. A árvore AT-SPI mostrou exatamente um canvas na main e zero em cada Settings; o roteamento não carrega Three.js nem cria contexto WebGL nas Settings. O botão “Abrir IA e modelos” na Geral foi acionado com IA já aberta e permaneceu uma única janela dessa label.

RSS aproximado (KiB, processo Tauri + WebKitNetworkProcess + WebKitWebProcess): main 647.324; main + Geral 962.940; main + Geral + IA 1.275.964; após fechar ambas 648.156. Os valores são snapshots, variam com WebKit e não representam memória exclusiva. Cada Settings abriu um WebKitWebProcess separado; ambos desapareceram após `close()`, sem janela escondida. A main preservou um WebKitWebProcess; o CharacterStage continua definido em 300×360 no CSS. Não houve reposicionamento programático da Luna.

Na janela IA real, a policy inicial apareceu como `low/4096` para conversa e `low/1024` para resumo. Ações AT-SPI alteraram conversa `low/4096 → high/provider default → low/4096`; consultas SQLite após salvar confirmaram as transições. A Geral acionou abertura de IA sem duplicá-la. Após reinício com o mesmo banco isolado, a UI e SQLite mostraram `low/4096` e `low/1024`, `user_version=3`. Nenhuma chamada Gemini real foi feita. O fluxo de chave real e percepção visual/espacial completa permanecem para o checklist humano.

A UIP-6A permanece **CANDIDATA** até revisão humana de abertura pelo botão da main, legibilidade, movimento/fechamento independente das janelas e ausência de regressão de animação/Composer. `cargo fmt --check` global ainda acusa drift pré-existente; os arquivos Rust novos foram formatados isoladamente.


Verificações automatizadas da candidata: `npm run typecheck` e `npm run build` passaram; `cargo check` passou; `cargo test` passou com 71 testes; `git diff --check` passou. `cargo fmt --check` global falhou pelo drift conhecido; `rustfmt --check` dos dois módulos Rust novos passou. O build Vite ainda informa chunk principal acima de 500 kB.

### Correção visual do gate humano — WebKitGTK

Sam encontrou os `<select>` fechados de Provider e Thinking com fundo nativo claro e texto selecionado pouco legível no Fedora. A superfície Settings agora declara `color-scheme: dark`; os selects usam cores explícitas, `appearance: none` e uma seta CSS, preservando o `<select>` HTML, foco e teclado. No Tauri real, a árvore AT-SPI confirmou os dois controles em `conversation` e `summary`, suas opções e foco; seleção e salvamento continuaram funcionando, com as policies originais preservadas. A cor renderizada ainda requer confirmação visual de Sam. **UIP-6A permanece CANDIDATA.**
