# UIP-6A — janelas de configurações e política cognitiva v1

**Estado: UIP-6A = PASS funcional / FECHADA em 28/09/2026.** UIP-6 completa continua aberta. A UIP-6A entrega escolhas efetivas para `conversation` e `summary`; UIP-6B tratará configurações gerais e parâmetros avançados, e UIP-6C consolidará o gate. LR-7 não foi iniciada.

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

O gate humano foi concluído por Sam: abertura pela main, legibilidade, movimento/fechamento independente das janelas e ausência de regressão observável na Luna/Composer foram aprovados. `cargo fmt --check` global ainda acusa drift pré-existente; os arquivos Rust novos foram formatados isoladamente.


Verificações automatizadas da candidata: `npm run typecheck` e `npm run build` passaram; `cargo check` passou; `cargo test` passou com 71 testes; `git diff --check` passou. `cargo fmt --check` global falhou pelo drift conhecido; `rustfmt --check` dos dois módulos Rust novos passou. O build Vite ainda informa chunk principal acima de 500 kB.

### Correção visual do gate humano — WebKitGTK

Sam encontrou os `<select>` fechados de Provider e Thinking com fundo nativo claro e texto selecionado pouco legível no Fedora. A superfície Settings agora declara `color-scheme: dark`; os selects usam cores explícitas, `appearance: none` e uma seta CSS, preservando o `<select>` HTML, foco e teclado. No Tauri real, a árvore AT-SPI confirmou os dois controles em `conversation` e `summary`, suas opções e foco; seleção e salvamento continuaram funcionando, com as policies originais preservadas. Sam confirmou visualmente a correção: fundo, texto, opções abertas e foco ficaram legíveis.


## Fechamento da UIP-6A

Em 28/09/2026, a UIP-6A foi fechada em **PASS funcional** após gate humano. As duas janelas independentes abriram pelo fluxo de produto, puderam ser movidas/fechadas sem afetar a Luna, e a janela de IA mostrou e persistiu policies reais para `conversation` e `summary`. A correção visual dos `<select>` no WebKitGTK foi aprovada por Sam.

O custo de memória das Settings permanece registrado: cada WebView independente adicionou aproximadamente 300 MiB no ambiente `tauri dev`, mas os processos e o RSS correspondente foram liberados ao fechar as janelas. Isso não bloqueia a UIP-6, mas deve ser reavaliado em build de release e na UIP-7.

**Próxima etapa: UIP-6B — configurações gerais editáveis + avaliação dos parâmetros avançados restantes.**

# UIP-6B — Geral editável + retry/capacidade

**Estado: CANDIDATA, aguardando gate humano.** UIP-6A permanece PASS. UIP-6C e LR-7/LR-8 não foram iniciadas.

## Evidência e política de retry

No gate Gemini real, Sam viu respostas `rate_limited` com `max_provider_calls` 2 e 4, enquanto uma mensagem com 1 passou. Isso sugere investigar o retry anterior de 80 ms, mas **não prova causalidade**: o resultado também pode depender da cota ou do estado do provider. A migration 004 muda deliberadamente o backoff inicial de 80 para 1500 ms para conversa, porque 80 ms é operacionalmente agressivo. Os diagnósticos DEV registram apenas provider, tentativa, motivo e espera, sem conteúdo ou segredo.

`max_provider_calls` é o **orçamento total de requests ao provider por tarefa**, incluindo request inicial, retries e eventuais fallbacks autorizados. Um stream HTTP pode produzir muitos chunks e continua sendo uma única chamada. `max_retries` conta somente tentativas extras. O Scheduler respeita o menor limite: calls=1/retries=10 faz uma chamada; calls=4/retries=1 faz até duas; calls=4/retries=3 faz até quatro. A UI alerta quando o orçamento total restringe os retries, mas preserva os valores escolhidos.

`RetryPolicy` é separada de `TaskBudget`. Retry automático exige `retry_enabled`, erro `Timeout`/`Unavailable`, nenhum chunk emitido, limite de retries e call budget disponíveis, e tarefa ativa. O backoff de cada retry é `initial_backoff_ms × 2^(N−1)`, com saturação aritmética e espera cancelável. `RateLimited` registra cooldown e `Retry-After` quando presente, encerra a tarefa e não usa o backoff para insistir na mesma task. Summary começa com retry desligado: o worker devolve falhas transitórias para pending e tenta em kick futuro. LR-8 ainda tratará jitter, quotas, token bucket, fila e circuit breaker.

## Persistência e parâmetros

A migration 004 preserva a policy v3 e acrescenta retry, `history_max_messages`, `history_max_bytes`, `summary_input_max_bytes` e preferências Gerais. A migration 005 acrescenta a configuração global Gemini de timeouts HTTP. Ela foi necessária porque o banco local já havia aplicado a versão inicial da 004 durante o gate da candidata; a atualização v4→v5 preserva as preferências existentes. Defaults conversation: Gemini, `gemini-3.8-flash`, low, output 4096, calls 2, retry ligado, 1 tentativa extra, 1500 ms, histórico 8 mensagens/12288 bytes. Defaults summary: mesmo provider/modelo/thinking, output 1024, calls 1, retry desligado, 0 tentativas extras, 1500 ms, input 32768 bytes. `max_output_tokens=NULL` continua sem teto adicional da Luna. O histórico é limitado à sessão atual; zero mensagens ou zero bytes não envia histórico anterior. O resumo preserva primeira fala do usuário, recentes e ordem cronológica quando cabem no orçamento.

A tabela singleton `gemini_provider_settings` (migration 005) persiste timeouts globais Gemini de 45000/15000 ms. A tabela singleton `general_settings` persiste `always_on_top=false`, `active_fps=30`, `background_fps=24`; FPS aceita inteiros de 1 a 60, e background acima de active é permitido com aviso. Suspenso/oculto continua 0. A janela Geral salva e envia `general-settings-changed` somente à main; o `RenderBudget` recebe a configuração sem remontar canvas. A solicitação always-on-top é feita na main imediatamente após salvar e no startup. O compositor Wayland pode ignorá-la; a UI relata solicitação, não sucesso visual. Click-through segue indisponível por falta de recuperação externa segura. Ctrl+Shift+Space é informativo e depende de foco da janela.

## Auditoria de parâmetros avançados

Configuráveis nesta 6B: retry por role, orçamento de histórico de conversa, input de resumo, timeouts HTTP total/idle do Gemini, AOT e FPS. Invariantes técnicos: streaming ativo exigido pelo adapter; `store:false`, isolamento e segurança; connect timeout Gemini 8 s continua propriedade do client e aparece read-only. Request timeout (default 45000 ms) e stream idle timeout (default 15000 ms) são globais, persistidos e editáveis; o adapter captura um snapshot por chamada e aplica o total via `RequestBuilder::timeout`, sem recriar o client. A configuração alterada passa a valer nas chamadas seguintes. Não suportados: thinking summaries, temperature/top-p expostos, tools, grounding/web e fallback multi-provider para seleção fixa de Gemini. A UI identifica esses estados sem oferecer campos falsos.

Na preparação do resumo, o banco lê até 256 mensagens recentes e a primeira fala do usuário, com no máximo 8193 caracteres por mensagem. São limites técnicos de leitura/localidade, mostrados read-only na seção avançada; título (70 caracteres) e resumo (1200) são limites do parser de metadados. A conversa real não injeta memórias no outbound nesta fase (`memory_limit=0`); o Context Builder mantém limites próprios de cinco memórias/seis mensagens para outros caminhos e não altera essa política. O corpo de erro HTTP (64 KiB) e o `Retry-After` limitado a sete dias são guardrails de protocolo, não controles de capacidade do modelo.

As capabilities continuam por janela: Geral recebe apenas leitura/gravação de preferências e abertura de IA; main lê preferências e escuta o evento; IA mantém sua policy e credencial. Nenhuma Settings monta Avatar/Three.js. O custo de WebKitGTK visto na 6A permanece dívida de avaliação em release/UIP-7; fechar as janelas deve liberar seus processos.

## Gate técnico da candidata UIP-6B

`npm run typecheck`, `npm run build`, `cargo check`, `cargo test` (78 testes), `git diff --check` e o teste Node puro de `RenderBudget` passaram. `cargo fmt --check` global segue com o drift anterior; os módulos Rust novos/estruturados da 6B passaram em `rustfmt --check` isolado. O build Vite ainda avisa sobre chunk da main acima de 500 kB.

Em Tauri real com `LIBGL_ALWAYS_SOFTWARE=1`, a árvore AT-SPI mostrou um canvas na main, zero em Geral e IA, e os campos de retry, budgets e timeouts. O banco local migrou v4→v5 e a IA carregou com timeouts 45000/15000. Abrir as três janelas apresentou aproximadamente 1.355.428 KiB de RSS somado (Tauri + WebKitNetworkProcess + três WebKitWebProcess); após fechar as duas Settings, restaram aproximadamente 679.956 KiB e só o WebKitWebProcess da main. São snapshots de processos, não memória exclusiva. Um processo separado por Settings desapareceu ao fechar; nenhuma janela foi escondida permanentemente.

Não houve teste controlado de conversa Gemini nesta rodada. Na segunda inicialização, o log de um trabalho de fundo mostrou `rate_limited` HTTP 429 com Retry-After de 48000 ms; isso não estabelece causalidade entre retry e rate limit. A edição física de FPS/AOT, percepção visual de Wayland e uma conversa Gemini moderada permanecem no gate humano. A candidata não promove UIP-6B a PASS nem inicia UIP-6C.
