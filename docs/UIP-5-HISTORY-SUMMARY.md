# UIP-5A — histórico persistente somente leitura

**Estado: UIP-5A = PASS funcional / FECHADA em 27/09/2026. UIP-5 completa: NÃO concluída.** A etapa foi aceita para avanço com dívidas de observabilidade/UX do provider e refinamentos físicos no Wayland adiados; essas pendências não alteram o contrato de histórico somente leitura.

## Arquitetura e schema

A migration `002_conversation_history.sql` preserva a 001 e adiciona a `conversation_sessions`:

- `kind TEXT NOT NULL DEFAULT 'legacy'`, limitado a `product` e `legacy`;
- `summary_status TEXT NOT NULL DEFAULT 'none'`, com estados reservados `none`, `pending`, `running`, `completed`, `failed`;
- `summary TEXT` e `summary_updated_at TEXT`, ambos nulos por enquanto;
- índice de `kind, updated_at DESC, id DESC`.

O backfill classifica como `product` apenas registros com `title IS NULL` e `status IN ('active','closed')`. Os registros nomeados de diagnóstico LR-4 e singleton LR-6 permanecem `legacy`. O código novo filtra por `kind`, sem comparar títulos. Na auditoria pré-migration do banco local havia quatro sessões que atendiam ao critério de produto (três `active`, uma `closed`) e dois registros diagnósticos nomeados. Sessões criadas pelo fluxo UIP recebem `kind='product'` explicitamente.

`status` continua a indicar o ciclo de vida da sessão. `summary_status` é independente. Nenhum resumo é gerado nesta rodada. Ao fechar uma sessão com ao menos uma mensagem `user` e uma `assistant`, seu estado de resumo passa de `none` para `pending`. Sessões vazias não ficam pendentes nem aparecem na listagem.

## Sessões órfãs

No `setup` do Tauri, logo após abrir/migrar o banco e antes de qualquer ID entrar em `CurrentRunSessions`, `close_orphaned_product_sessions` fecha todas as sessões `product` ainda `active`. Com par user/assistant, marca resumo `pending`; sessões vazias ficam com `summary_status='none'`. O `UPDATE` filtra `status='active'`, de modo que repetir a operação não reescreve sessões fechadas. Registros `legacy` não são tocados. A execução atual começa sem ID no frontend e no registro Rust.

## Comandos e dados

`list_conversation_history` devolve até 50 itens ordenados por `updated_at DESC, id DESC`. A consulta seleciona apenas sessões `product` com mensagens, conta mensagens e busca um trecho da primeira mensagem do usuário; não carrega o conteúdo completo de cada sessão. O DTO contém `id`, `createdAt`, `updatedAt`, `title`, `status`, `summaryStatus`, `messageCount` e `preview`. O título persistido tem prioridade e é limitado a 70 caracteres; sem ele, a primeira mensagem do usuário é truncada a 60 caracteres Unicode, ou aparece `Conversa sem mensagens`. O preview é limitado a 100 caracteres Unicode. Esse fallback não é persistido.

`get_conversation_history_session(session_id)` valida ID positivo e `kind='product'`, então lê as mensagens da sessão em ordem de ID. A operação não pede nem altera `CurrentRunSessions`, `status`, `summary_status` ou mensagens. `get_conversation_session` e `start_gemini_task` mantêm a exigência de ID criado na execução atual. As únicas capabilities novas são `allow-list-conversation-history` e `allow-get-conversation-history-session`.

## UI e contexto cognitivo

A mesma WebView e a mesma telinha têm modos `CURRENT`, `HISTORY_LIST` e `HISTORY_DETAIL`. `Histórico` abre a lista sob demanda; escolher item carrega o detalhe sob demanda. A sessão histórica é somente leitura, com retorno à lista ou à conversa atual. Abertura do histórico fica desabilitada durante streaming. `sessionId`, draft, mensagens, preview e task continuam no controller atual; navegar pelo histórico não os altera. Enviar uma nova mensagem volta ao modo atual.

O contexto do Gemini continua vindo exclusivamente de `outbound_history` da sessão atual, limitada e validada em `CurrentRunSessions`. O teste fake HTTP abriu/listou a sessão histórica com `HISTORICO-SECRETO-55` e confirmou que o marcador não apareceu no outbound da outra sessão. Nenhuma chamada Gemini real foi feita para esse gate.

## Testes e limites

Os testes de persistência cobrem backfill, `kind`, filtro de legacy/vazias, ordem, limite, contagem, preview/título truncados, detalhe correto, rejeição de ID inválido ou legacy, isolamento A/B, leitura sem mutação e fechamento de órfãs sem tocar em diagnóstico. O teste HTTP fake cobre privacidade cognitiva. `cargo test` passou com 45 testes. Typecheck, build, cargo check e diff-check passaram. `cargo fmt --check` falhou pela formatação Rust preexistente no repositório; o mesmo ocorre com `src-tauri/src/main.rs` do HEAD inicial, sem modificações.

No Tauri real com Mesa em software, foi validada uma única WebView com Presence 310×410, Composer 310×490 e Conversation 625×490; stage e buffer permaneceram 300×360, DPR 1 e `glError 0`. A lista mostrou as quatro sessões de produto do banco local; o detalhe histórico abriu com seis mensagens e o retorno preservou o rascunho atual. O processo foi reiniciado antes da navegação: a UI voltou sem sessão atual e o histórico persistiu. O banco permaneceu com seis sessões (quatro `product`, duas `legacy`), sem nova sessão causada por leitura do histórico. Wave acionado no canvas retornou ao Idle. Alt+drag físico e a percepção visual fina no Wayland permanecem no gate humano.

No fechamento da UIP-5A, UIP-5B (retomada) e UIP-5C (resumo assíncrono/título) ainda não tinham sido implementadas. `summary_status='pending'` é apenas metadado para trabalho futuro; nenhum worker processa a fila nesta fase.


## Fechamento da UIP-5A

Em 27/09/2026, a UIP-5A foi aceita em **PASS funcional**. O histórico persistente somente leitura, a separação `product/legacy`, o fechamento de sessões órfãs, a listagem leve, o detalhe sob demanda e a proteção do contexto cognitivo foram mantidos. O teste fake HTTP provou que conteúdo de sessão histórica não entra no outbound da sessão corrente; restart preserva histórico e inicia sem sessão corrente automática.

Durante o gate humano surgiram falhas reais do provider Gemini (`unavailable`, `rate_limited` e, em sequência, `provider_unavailable` por cooldown). Essas falhas não foram atribuídas à UIP-5A. Foram adicionados diagnósticos DEV sanitizados para indisponibilidade, rate limit e ausência de provider por cooldown, sem alterar retry/fallback. A política temporária do chat foi elevada de 512 para 4096 tokens, mantendo `thinking=low`, e a decisão de tornar esses parâmetros configuráveis na UIP-6 foi documentada.

Dívidas não bloqueantes: mensagem de erro mais amigável para cooldown/rate limit, drift de `rustfmt`, refinamentos visuais/Alt+drag no Wayland e eventual exclusão da sessão corrente da lista histórica caso a UX humana indique necessidade. **Próxima etapa: UIP-5B — retomada explícita de sessão.**

## UIP-5B — retomada explícita de sessão (PASS funcional / FECHADA)

Abrir o detalhe histórico continua somente leitura. Apenas o botão **Retomar**, visível para sessão `product` fechada com mensagens, chama `resume_conversation_session(target_session_id, current_session_id?)`. O comando exige IDs explícitos, rejeita `legacy`, sessão vazia, target ativo e `summary_status='running'` (`summary_busy`). ID corrente igual ao target é rejeitado; a UI oculta a sessão corrente da lista. Não há busca pela sessão mais recente nem retomada automática.

O backend segura `CurrentRunSessions` durante a validação, a transação SQLite e a atualização do registro. Na troca A→B, A deve pertencer à execução atual e estar `active/product`; A é fechada pela semântica existente (`none→pending` somente com user e assistant), enquanto B muda de `closed` para `active`. O banco confirma as duas alterações junto com a leitura do DTO completo de B antes de o registro mudar de `{A}` para `{B}`. Sem A, `{}` vira `{B}`. Falha de validação ou transação deixa banco e registro como estavam. Uma task Gemini ainda em voo para A causa `session_busy`; o registro da task cobre também a fase de gravação final. Se o processo cair no intervalo mínimo entre commit e atualização do registro, o registro desaparece com o processo e o startup seguinte fecha B como órfã; nenhuma sessão é retomada automaticamente.

Ao reabrir B, `summary_status='none'`, `summary=NULL` e `summary_updated_at=NULL` invalidam qualquer resumo antigo. Ao fechar B de novo, a regra de `pending` existente se aplica. Não há worker de resumo nesta etapa. O controller adota as mensagens retornadas de B, limpa preview, task, erro e **draft**; draft não vazio da conversa anterior exige confirmação simples na telinha antes da troca. Após sucesso, o modo volta a `CURRENT` e o Composer continua disponível.

O caminho cognitivo continua a exigir ID em `CurrentRunSessions` e sessão `active/product`. O outbound de B usa apenas suas oito mensagens anteriores mais recentes, até 12 KiB; a mensagem nova entra uma vez. A e outras sessões históricas não entram no contexto. Após restart, a normalização fecha B novamente e o registro recomeça vazio; o usuário precisa acionar **Retomar** outra vez.

Os testes cobrem validação de alvo, `summary_busy`, invalidação de resumo, troca A→B, sessão A vazia, rollback em falha de escrita, atualização de `CurrentRunSessions`, bloqueio durante task em voo, restart e outbound fake HTTP isolado com `ORQUIDEA-71`, sem marcadores de A, C, LR-6 ou memória privada. Typecheck, build, cargo check, os 56 testes Rust e diff-check passaram. `cargo fmt --check` continua falhando pelo drift global conhecido, inclusive em `src-tauri/src/main.rs` não alterado. O gate humano foi concluído por Sam e a continuidade estrutural já estava coberta pelo fake HTTP; por isso **UIP-5B = PASS funcional / FECHADA**. A chamada Gemini real não é requisito de fechamento desta subetapa porque indisponibilidade/rate limit do provider é dívida externa ao contrato de retomada. A UIP-5 completa permanece aberta.

No Tauri real, uma instância com Mesa em software abriu histórico e detalhe com “Somente leitura”; o botão mostrou confirmação ao encontrar draft, Cancelar preservou o texto, e Retomar exibiu as mensagens antigas em CURRENT com Composer limpo. Após restart, a UI iniciou sem sessão atual, o detalhe voltou a ser somente leitura, `get_conversation_session` recusou o ID antigo e foi necessário clicar Retomar novamente. A lista continuou funcional. Stage e drawing buffer mediram 300×360, DPR 1 e `glError 0`; o inspector mostrou uma página WebView. Nesta sessão controlada pelo inspector sem foco, `setSize` resolveu mas `innerWidth/innerHeight` permaneceram 310×410 e os callbacks de render ficaram suspensos: layout 625×490, Wave→Idle e aparência física permanecem no gate humano. A sessão usada no teste foi fechada de novo por “Nova conversa”. Nenhuma chamada Gemini real foi necessária.

O gate humano identificou e corrigiu o acesso ao Histórico quando não há sessão ou mensagens atuais: o Composer agora mostra “Conversas” mesmo após restart. Sam validou o botão em sessão limpa, a navegação até Histórico e o fluxo de retomada. Também confirmou: detalhe em “Somente leitura”, `Retomar`, preservação/cancelamento de draft, adoção das mensagens antigas em `CURRENT`, restart sem retomada automática e animações funcionando normalmente.


## Fechamento da UIP-5B

Em 27/09/2026, a UIP-5B foi fechada em **PASS funcional** após gate humano. O fluxo validado foi: abrir sessão histórica em modo somente leitura → acionar **Retomar** → adotar explicitamente a sessão em `CURRENT` → preservar mensagens históricas → continuar com Composer vazio. Um draft não enviado exige confirmação; **Cancelar** preserva o texto e **Retomar** descarta o draft de forma explícita. Após restart, nenhuma sessão é retomada automaticamente e o usuário precisa escolher novamente qual conversa deseja continuar.

A correção final de UX tornou o botão **Conversas** visível mesmo quando `sessionId=null` e `messages=[]`, permitindo acessar Histórico logo após iniciar o aplicativo sem criar uma conversa nova. Isso fecha a lacuna descoberta no gate humano sem alterar persistência, Gemini ou o contrato cognitivo.

O fake HTTP já comprovava que, após retomar B, somente o histórico limitado de B entra no outbound; A e outras sessões continuam isoladas. Dívidas de Gemini/rate limit, mensagens públicas de cooldown, `rustfmt` e Wayland permanecem não bloqueantes. **Próxima etapa: UIP-5C — resumo assíncrono + título de sessão.**

## UIP-5C — resumo assíncrono e título (PASS funcional / FECHADA)

O setup inicia um único `SummaryWorker` após fechar sessões `product` órfãs e devolver sessões `closed/running` a `pending`. Um `Notify` recebe kick inicial e kicks após o fechamento de **Nova conversa**, a troca A→B e a configuração da credencial. Kicks podem ser coalescidos; o worker drena sequencialmente a fila e dorme sem polling. Fechar ou retomar não aguarda a chamada cognitiva. A transição `pending→running` usa claim condicional em transação SQLite e exige sessão `product/closed` com mensagens. `active`, `legacy`, `completed` e `failed` não entram na fila.

O papel cognitivo `summary` usa `Arc<Scheduler>` e uma chamada por sessão. No composition root, ele aponta temporariamente para o scheduler real hoje disponível; o worker não conhece `GeminiProvider`. O teto provisório é `PROTOTYPE_SUMMARY_MAX_OUTPUT_TOKENS=1024` com `max_provider_calls=1`. Thinking segue o comportamento atual do adapter. A UIP-6 tornará provider, modelo, output e thinking persistidos e configuráveis por papel. O chat conserva seus 4096 tokens e o outbound próprio de oito mensagens/12 KiB.

O input é um JSON estruturado `{"task":"session_summary","truncated":bool,"messages":[{"role":"user|assistant","content":"..."}]}` inserido apenas em `ProviderRequest.input`; `history=[]`. Ele contém somente mensagens da sessão claimed, preserva a primeira mensagem do usuário e prioriza as recentes, em ordem cronológica final. O claim lê no máximo a primeira fala do usuário e as 256 mensagens mais recentes, marcando truncamento quando omite mensagens antigas. O limite provisório central do JSON é 32 KiB serializados, com até 8 KiB por mensagem e corte Unicode seguro. A instrução trata o transcript como dado não confiável e proíbe executar pedidos contidos nele. O `ContextBundle` usa identidade sintética mínima, zero memórias e zero mensagens globais; nenhuma memória da Luna ou outra sessão alimenta o resumo. O output deve ser JSON puro com `title` e `summary` não vazios. Markdown/code fence e JSON inválido falham. O parser limita título a 70 e resumo a 1200 caracteres Unicode, rejeita controles e títulos que começam com “Conversa sobre”.

Sucesso persiste título, resumo, `completed` e `summary_updated_at` em um único `UPDATE` condicionado a `product/closed/running`; `updated_at`, `created_at` e mensagens ficam intactos. A lista prioriza o título persistido e usa trecho do resumo como preview; antes disso, mantém o fallback da primeira fala do usuário. O detalhe mostra “Resumo da sessão” acima das mensagens. `pending/running` mostram aviso discreto; `failed` mostra indisponibilidade. Reabrir Histórico recarrega sob demanda, sem timer. Retomar uma sessão concluída invalida resumo e estado, preserva temporariamente o título e, após novo fechamento, cria outro `pending`.

Falhas transitórias (`rate_limited`, `unavailable`, `timeout`, `provider_unavailable`) devolvem `running→pending` e encerram o drain atual; o próximo kick ou startup tenta novamente. Falhas terminais de autenticação, quota, protocolo, output incompleto, parsing ou outras incompatibilidades marcam `failed`. `summary_updated_at` registra apenas conclusão bem sucedida, portanto fica nulo em falhas. Sem credencial configurada, o worker deixa `pending` quieto e espera novo kick; o histórico e o startup seguem disponíveis, sem popup. Cada tentativa efetiva grava `TaskRecord` `conversation_summary` com ID monotônico do registro de tarefas, tempos, estado e erro sanitizado, sem transcript ou output bruto; não fica cancelável pela conversa.

Testes de persistência e fake provider cobrem claim único, isolamento de `active/legacy`, recuperação após reinício, sucesso, falhas, retomada, integridade das mensagens, input limitado/UTF-8, privacidade entre sessões e memória, além de provider lento sem bloquear fechamento. O gate humano foi concluído por Sam e **UIP-5C = PASS funcional / FECHADA**. UIP-5 completa ainda não está concluída; **UIP-5D** será a consolidação e gate final.

No Tauri real com Mesa em software, uma única instância mostrou o Histórico com fallback e indicador “resumindo…”, abriu detalhe com mensagens completas e permitiu Retomar em cerca de 0,14 s. Nova conversa fechou a sessão em cerca de 0,83 s; o SQLite manteve as duas mensagens e `pending`. O provider real respondeu 429; o worker registrou falha sanitizada, devolveu a sessão a `pending` e parou o drain. Após reinício, o registro continuou íntegro e uma WebView foi observada. O canvas mediu 300×360 por acessibilidade. Buffer, DPR e `glError` ainda dependem de inspeção visual/técnica no gate humano; depois do reinício, a automação sem foco não conseguiu reabrir o Composer, embora o processo e o banco estivessem disponíveis.

`npm run typecheck`, `npm run build`, `cargo check`, os 62 testes Rust e `git diff --check` passaram. `cargo fmt --check` continua falhando no drift global conhecido, com diffs já em `build.rs` e `context.rs`; o novo `summary.rs` passou em `rustfmt --check` isolado. Nenhuma migration nova foi necessária.


## Fechamento da UIP-5C

Em 27/09/2026, a UIP-5C foi fechada em **PASS funcional** após gate humano. Sam confirmou que **Nova conversa** permanece imediata, que o Histórico continua acessível, que sessões pendentes/running não bloqueiam a UI, que mensagens permanecem preservadas ao retomar/fechar/reiniciar e que o comportamento geral da Luna e das animações continua normal. A conclusão real do resumo pelo Gemini não foi usada como requisito de PASS porque o provider estava sujeito a 429; o caminho `completed` já está coberto por provider fake e testes automatizados.

A arquitetura aprovada mantém resumo como trabalho assíncrono secundário, separado de memória persistente e da conversa corrente. O `SummaryWorker` é provider-agnostic, usa `Scheduler`, não recebe memória privada nem outras sessões e não bloqueia fechamento/retomada. O papel `summary`, o limite de 1024 tokens e o provider atual continuam provisórios até a UIP-6.

Dívidas não bloqueantes seguem fora do caminho crítico: rate-limit/cooldown e mensagens públicas do provider, compartilhamento temporário do mesmo Scheduler entre chat e summary, drift global de `rustfmt` e limitações Wayland já documentadas. **Próxima etapa: UIP-5D — consolidação e gate final da UIP-5.**

## UIP-5D — consolidação (CANDIDATA; gate humano pendente)

Em 28/09/2026, a revisão do diff desde `05cfc1f17b77a881ddcde4f6571157e38450d1ff` confirmou que a trilha UIP-5 alterou persistência de conversa, cognição, comandos/capabilities específicos, UI de conversa e documentação. Não há diff de Avatar Runtime, Idle/Wave, câmera, CharacterStage, RenderBudget, WindowController, arquitetura de WebViews, memória privada, Behavior Engine, ferramentas ou permissões genéricas de filesystem/shell. `assets/luna-source/Luna-source.blend` consta no intervalo histórico por outra trilha e não foi tocado nesta consolidação.

Os testes agora usam `ALFA-A-11`, `BETA-B-22`, `GAMA-C-33`, `PRIVATE-44` e `LEGACY-55`. O fake HTTP prova que, após Retomar B, o outbound contém B e a mensagem atual uma vez, mas não contém A, C, memória privada, legacy nem um resumo `completed` de A. O histórico do chat permanece limitado a oito mensagens anteriores/12 KiB. O fake de summary recebe só a sessão claimed em JSON limitado a 32 KiB, `history=[]`, identidade sintética mínima e zero memórias/mensagens globais. Summary continua metadado de sessão, nunca contexto automático de chat ou memória da Luna.

A leitura do código da UI confirmou que abrir Histórico e voltar preservam o draft, que um draft não vazio exige confirmação para Retomar, que Cancelar o preserva e que a adoção explícita de B limpa o draft e mostra as mensagens de B. **Conversas** permanece disponível com sessão limpa; a leitura de lista/detalhe não cria sessão, e a lista oculta a sessão corrente como candidata. A lista aplica limite 50, ordem `updated_at DESC, id DESC`, título e preview com fallback, resumo concluído como preview e indicadores `pending/running/failed`. O detalhe fechado é somente leitura, preserva todas as mensagens e mostra “Resumo da sessão”, “Resumo sendo preparado.” ou “Resumo indisponível.” conforme o estado. Esses estados de UI foram auditados no código e nos gates anteriores; a navegação física nesta rodada ficou para o gate humano abaixo.

O ciclo de três execuções foi reproduzido em SQLite temporário: A nasce e fecha `pending`; na segunda execução o registro atual começa vazio, a leitura não ativa A, Retomar explícito permite continuar e novo fechamento volta a `pending`; na terceira execução A continua fechada e exige nova ação. Testes existentes e ampliados cobrem `none→pending→running→completed`, falha transitória `running→pending`, falha terminal `running→failed`, recuperação `closed/running→pending`, invalidação de summary ao retomar com título preservado, claim único, exclusão de `active/legacy/completed/failed`, rollback e preservação de mensagens. Um teste novo fixa que concluir summary não muda `updated_at` nem a ordem da lista. `PRAGMA integrity_check` retornou `ok` e `foreign_key_check` zero violações no teste e no banco local lido sem escrita.

O único `SummaryWorker` é iniciado no setup, recebe `Notify`/kick em startup, close, troca A→B e configuração da credencial, drena sequencialmente e dorme sem polling. Falha transitória encerra o drain para evitar retry agressivo; sem credencial, `pending` espera novo kick. O fake lento permaneceu suspenso enquanto retomada, fechamento e criação de sessão concluíram dentro do timeout de 2 s, antes da liberação do provider. IDs de chat/mock/summary compartilham sequência monotônica; summary grava `TaskRecord.kind=conversation_summary`, sem transcript, prompt, output bruto ou segredo, e apenas código de erro sanitizado. As capabilities novas seguem específicas para lista, detalhe e retomada, sem SQL, shell ou filesystem genéricos. O DTO TypeScript de `ConversationSession` recebeu `createdAt`/`updatedAt` para espelhar o Rust; não houve alteração de schema.

`npm run typecheck`, `npm run build`, `cargo check`, `cargo test` (65 testes) e `git diff --check` passaram. `cargo fmt --check` falha pelo drift global conhecido, inclusive em arquivos preexistentes; `summary.rs` foi formatado isoladamente e passou em `rustfmt --check`. O build mantém aviso de chunk grande, sem regressão identificada nesta rodada.

No Tauri real, uma única instância com `LIBGL_ALWAYS_SOFTWARE=1` iniciou em Presence 310×410. A acessibilidade confirmou canvas 300×360; um `WebKitWebProcess` permaneceu único. Três amostras separadas por 3 s mostraram RSS agregado estável em 619,1 MiB (app 173,7 MiB); em janela ociosa de 5 s, app/WebKit/Network marcaram aproximadamente 0% de CPU. O worker recebeu 429 no startup e deixou os resumos `pending`, sem loop observado. O código preserva os layouts Composer 310×490/Conversation 625×490 e RenderBudget 30/24/0. A automação AT-SPI não conseguiu avançar o botão do Composer nesta sessão sem foco; por isso esses layouts, ciclos de painel, drawing buffer, DPR, `glError` e Wave→Idle **não foram revalidados visualmente na UIP-5D**. Esses valores constam dos gates humanos anteriores e precisam de confirmação curta de Sam antes do PASS final.

Não houve chamada Gemini de chat nesta rodada. O 429 de summary, o compartilhamento temporário de Scheduler, a UX de rate limit, o drift global de `rustfmt` e as limitações Wayland permanecem dívidas conhecidas fora da UIP-5D. **UIP-5D = CANDIDATA; UIP-5 completa ainda não é PASS.**


## Fechamento da UIP-5D e da UIP-5

Em 28/09/2026, a UIP-5D foi encerrada em **PASS funcional** e, com ela, **UIP-5 = PASS funcional / FECHADA**.

A consolidação auditou histórico, retomada explícita, isolamento cognitivo, summary assíncrono, lifecycle de restart, concorrência, Task IDs, integridade SQLite, capabilities e contratos Rust↔TypeScript. Não foram encontrados vazamento entre sessões, perda de mensagens ou regressão funcional que justificasse reabrir as subetapas. `PRAGMA integrity_check` retornou `ok` e `foreign_key_check` não apontou violações.

O gate técnico curto do Tauri confirmou Presence 310×410, canvas 300×360, uma WebView, RSS agregado estável em aproximadamente 619 MiB nas amostras e ausência de loop perceptível do SummaryWorker ocioso. As medições físicas não repetidas nesta rodada — Composer/Conversation, drawing buffer, DPR, `glError` e Wave→Idle — já possuem validações anteriores e passam a ser rechecadas na UIP-7, sem bloquear a transição.

Dívidas mantidas: 429/cooldown e UX correspondente, Scheduler temporariamente compartilhado por `conversation`/`summary`, drift global de `rustfmt`, estabilidade espacial Wayland e revalidação global de performance. **Próxima etapa: UIP-6 — janelas independentes de configuração e política cognitiva controlada pelo usuário.**
