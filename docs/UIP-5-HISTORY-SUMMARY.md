# UIP-5A — histórico persistente somente leitura

**Estado: CANDIDATA. UIP-5 completa: NÃO concluída.** Gate humano pendente.

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

UIP-5B (retomada) e UIP-5C (resumo assíncrono/título) não foram implementadas. `summary_status='pending'` é apenas metadado para trabalho futuro; nenhum worker processa a fila nesta fase.
