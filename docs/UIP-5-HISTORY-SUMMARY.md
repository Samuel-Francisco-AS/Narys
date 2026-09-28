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
