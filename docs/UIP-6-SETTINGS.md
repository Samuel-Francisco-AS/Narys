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

**Estado: UIP-6B = PASS funcional / FECHADA em 28/09/2026.** UIP-6A permanece PASS. **UIP-6C = etapa corrente**; LR-7/LR-8 não foram iniciadas.

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

Durante o gate técnico, um trabalho de fundo registrou `rate_limited` HTTP 429 com Retry-After de 48000 ms; isso não estabelece causalidade entre retry e rate limit. No gate humano posterior, Sam confirmou aplicação visual do FPS, persistência das configurações e funcionamento das superfícies Gerais/IA. Uma conversa Gemini moderada concluiu sem `rate_limited`. Antes de qualquer mensagem do usuário, também foram observadas duas respostas HTTP 503 `unavailable` com Retry-After de 30000 ms, compatíveis com trabalho cognitivo em background; não havia captura persistida do terminal para provar se eram retry da mesma tarefa ou kicks distintos. Esse achado vira dívida explícita de coordenação/background para UIP-6C/LR-8, sem bloquear o PASS da 6B.


## Fechamento da UIP-6B

Em 28/09/2026, a UIP-6B foi fechada em **PASS funcional** após gate humano. Sam confirmou que a redução de FPS é perceptível em tempo real e que restaurar `30/24` retorna ao comportamento esperado; as configurações gerais e cognitivas persistem após reabertura/restart; os controles de retry e call budget permaneceram separados e editáveis; e uma conversa Gemini curta concluiu sem `rate_limited`.

O gate também produziu evidência nova: antes de uma mensagem manual, o terminal mostrou duas ocorrências `503 unavailable` com `Retry-After=30000`. Como `conversation` e `summary` ainda compartilham o mesmo Scheduler/cooldown e o SummaryWorker pode acordar em background, a UIP-6C deve consolidar a coordenação entre foreground/background e o respeito a hints transitórios do provider, sem antecipar o Rate Limit Manager completo da LR-8.

Dívidas não bloqueantes mantidas: custo temporário de WebViews Settings em `tauri dev`, Wayland/AOT, click-through seguro, fallback multi-provider, `rustfmt` global e Rate Limit Manager completo. **Próxima etapa: UIP-6C — consolidação e gate final da UIP-6.**

# UIP-6C — consolidação

**Estado: CANDIDATA ao gate humano em 28/09/2026.** UIP-6A e UIP-6B permanecem PASS funcional; UIP-6 completa ainda não é PASS. As duas linhas HTTP 503 com `Retry-After=30000` observadas no gate da 6B não permitem distinguir retry da mesma tarefa de dois kicks distintos do SummaryWorker.

## Coordenação cognitiva

`conversation` é foreground interativo; `summary` é background oportunista. O TaskRegistry conta tarefas foreground por sessão, inclusive duas tarefas simultâneas, e expõe consulta independente de provider. O registro RAII libera a contagem em término, cancelamento, fechamento do channel ou drop do future. Quando a última tarefa termina, um kick coalescido dá oportunidade ao SummaryWorker. Ele não reclama trabalho enquanto há foreground; se foreground começar após o claim, devolve a sessão a `pending` antes da chamada. Summary já iniciado pode continuar quando uma conversa começa; não há preemption nem mutex que faça a conversa esperar. Os kicks existentes de startup, fechamento/retomada de sessão, mudança de credencial e policy permanecem. Não há polling.

`ProviderError::Unavailable` preserva `retry_after_ms` tipado pelo adapter HTTP, inclusive 503. `Unavailable` com hint e `RateLimited` encerram a tarefa, registram cooldown compartilhado no Scheduler e não recebem retry local imediato. Durante cooldown, a próxima conversa recebe `provider_unavailable` sem chegar ao HTTP; summary volta a `pending` e interrompe o drain. `Unavailable` sem hint e `Timeout` ainda podem receber retry conforme policy, call budget e ausência de chunk. Sem hint em `Unavailable`, nenhum cooldown novo é inventado. Retry-After não altera os timeouts locais. Preemption, filas, jitter, quotas, token bucket e circuit breaker permanecem para LR-8.

## Contratos auditados

As policies de conversa e resumo, orçamento de histórico/input, limite total de calls e retries têm snapshot por tarefa. Request e idle timeout do Gemini também são capturados por tarefa; connect timeout permanece 8 s e o `reqwest::Client` é reutilizado. O histórico outbound só usa a sessão explícita, respeita zero e limites UTF-8; o resumo usa apenas a sessão claimed, preservando primeira fala e recentes quando cabem. `store:false`, streaming, ausência de thinking summaries, endpoint fixo, isolamento de sessão e ausência de memória privada são invariantes. A API key não retorna ao React: Settings recebem apenas `configured`, enquanto o valor permanece no SecretStore, fora de SQLite, localStorage, logs e TaskRecord.

Geral persiste AOT e FPS 30/24 por padrão, envia `general-settings-changed` à main e atualiza RenderBudget sem remount; oculto/suspenso continua 0. IA expõe policies por papel, retry/call budget, histórico/input, timeouts e status da chave. Streaming, thinking summaries, fallback, temperature, top-p, tools e grounding/web seguem read-only ou indisponíveis. As janelas Geral e IA têm labels e WebViews independentes, decoradas, opacas e redimensionáveis; o roteamento por importação dinâmica não carrega Avatar Runtime/Three.js nelas. As capabilities de Settings limitam comandos a leitura/gravação das preferências pertinentes, navegação Geral→IA e chave Gemini na IA; não incluem shell, filesystem, memória ou conversa. Wayland AOT/click-through permanecem dívidas conhecidas.

Migrations 001→005 permanecem intactas. Testes cobrem banco novo, upgrades v3/v4 e reabertura v5, com `integrity_check` e `foreign_key_check` sem violações; o banco local real também retornou `user_version=5`, `integrity_check=ok` e nenhuma violação de chave estrangeira. Nesta 6C em `tauri dev`, o RSS somado aproximado (Tauri + WebKitNetworkProcess + WebKitWebProcess) estabilizou em ~639 MiB na main, ~948 MiB com Geral, chegou transitoriamente a ~1.449 MiB com Geral + IA e caiu para ~650 MiB depois de fechar ambas. Os dois processos WebKitWebProcess das Settings desapareceram; reabrir Geral criou PID novo. O pico inclui RSS do processo Tauri e não equivale a memória exclusiva das janelas. Uma amostra curta da main em software WebGL mediu ~144% de um core no WebKitWebProcess e ~4% no Tauri, a investigar na UIP-7. Build release não foi executado como aplicativo nesta rodada. Drift global de rustfmt e performance global permanecem para trabalhos futuros.

Diagnóstico manual opcional: `LIBGL_ALWAYS_SOFTWARE=1 npm run tauri dev 2>&1 | tee /tmp/luna-gemini.log`. Esse arquivo é externo ao app; o app não grava log persistente nem conteúdo de prompts.

Na instância Tauri real da 6C, o startup encontrou uma sessão summary `pending` e produziu **uma** resposta espontânea HTTP 503 com `retry_after_ms=30000`, seguida de **uma** linha de cooldown do Scheduler. Não apareceu retry local após 1500 ms nem segunda chamada enquanto a instância ficou aberta. Isso evidencia a nova semântica sem atribuir causa às duas linhas do gate da 6B. A árvore AT-SPI mostrou uma main, Geral e IA, com contagem de canvas 1/0/0; abrir IA de novo manteve três janelas. Fechar as Settings deixou apenas a main. O resize Composer solicitado pela main não se materializou no GNOME/Wayland desta execução (frame permaneceu 310×410); a UI de conversa e o gate visual Wave→Idle continuam para Sam, sem mudança em WindowController nesta 6C.
