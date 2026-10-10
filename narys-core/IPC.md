# Core IPC v1 — SERVER-1A

Socket `$XDG_RUNTIME_DIR/narys-core/control.sock`: Unix0600, diretório0700,
SO_PEERCRED mesmo UID nos dois lados. O UID Linux é a fronteira de confiança;
processos desse UID não são isolados. Sem listener TCP. Uma conexão por request:
JSON UTF-8, cliente encerra a metade de escrita, servidor responde e fecha.

```json
{"version":1,"request_id":"cli-123","command":{"operation":"status"}}
```

Resposta contém version/request_id/ok e data ou error_code/category. Campos
extras, versão desconhecida e IDs inválidos são recusados. request_id é correlação,
não autorização nem promessa de exactly-once. Submit usa o claim durável anterior
a efeitos; running interrompida no restart nunca é reenviada. Cancel é idempotente;
completed/interrupted/failed são terminais, sem declaração de efeito desfeito.

Limites: request16KiB, response256KiB, 32 conexões concorrentes, leitura/escrita5s,
cliente265s para diagnósticos de runtime já existentes. Saturação fecha conexão;
não repetir mutações automaticamente. Desconexão não cancela tarefa admitida.
Shutdown bloqueia admissão, cancela tarefas, drena conexões/workers e remove socket.
Systemd recebe READY=1 apenas depois de ownership, migrations, recovery e bind.

Implementado: status/capabilities, controles legados prepare/submit/result/cancel,
credentials/stronghold e diagnósticos Copilot já existentes. A autorização LR-10A
continua fechada: submit não ganha consentimento por estar disponível no protocolo.
Não executar probes/inferência/reentrada da LR-10A para validar SERVER-1A.

Eventos sanitizados duráveis: `events` com after/limit (1..128), sequence global,
next_sequence, has_more e complete. Retenção4096; complete=false informa gap por
retenção. TraceBus segue efêmero e separado. Mudança de estado LR-10A e evento
correspondente são persistidos na mesma transação. O cursor não é sessão.

TaskRef `{namespace:"lr10a"|"product",id}` conserva colisões históricas entre
headless_tasks e task_records sem renumerar IDs/recibos. result/cancel/task_id
legados referem-se exclusivamente a lr10a. Produto futuro usará namespace product.

Contratos preparatórios reais em `ipc.rs`: Conversation/Sessions, TaskGet/TaskCancel,
Providers/ProviderConfigure, Approval(approve_once/deny), ToolRequest(ListFiles,
ReadFile, EditFile, Build, Test, Diff)/ToolResult. As operações sem integração
retornam `capability_not_integrated` antes de qualquer efeito. Não são capabilities
concluídas. Nenhum campo wire aceita origin/authority/HumanLocal/programa shell.
Approval futura exige receipt de tarefa/operação/escopo e verificação pelo Core;
texto de LLM, autenticação local ou TaskRef não conferem permissão.

## SERVER-1B — Conversation durável e providers

O envelope continua v1 e os limites/autenticação permanecem iguais. Comandos adicionais:

| Operação | Campos | Resultado |
|---|---|---|
| `session-create` | nenhum | ID de sessão `product`, ativa e vazia |
| `sessions` | `after` (ID, default 0), `limit` (1–100, default 50) | Todas as sessões, inclusive legadas/diagnósticas/vazias; cursor `next_session` |
| `session-get` | `session_id`, `after_message` (default 0), `limit` (1–100) | Metadados, mensagens ordenadas, `next_message`, `has_more`; páginas limitadas a 192 KiB |
| `session-resume` | `session_id` | Seleciona ativa ou reabre sessão histórica de produto elegível; não fecha outras sessões |
| `session-close` | `session_id` | Fecha explicitamente, recusa trabalho em curso; sem inferência automática de summary |
| `conversation` | `session_id`, `text` (1–4096 bytes) | Admissão assíncrona com TaskRef `product`; mensagem do usuário já persistida |
| `task-get` | `task: {namespace, id}` | Resultado, provenance/usage, política, estado e timestamps persistidos; consulta também tarefas históricas |
| `task-cancel` | `task: {namespace, id}` | Solicita cancelamento idempotente; `commit_in_progress` recusa cancelamento após limite de commit; não desfaz efeitos remotos |
| `providers` | nenhum | Adapters, presença booleana de segredos, erro seguro do cofre, permissões gratuitas, política, rate/admission/resilience/telemetry |
| `provider-configure` | `provider_id`, `enabled`, `free_tier_confirmed` (default false) | Permissão persistida; habilitação exige credencial existente e confirmação do operador |
| `conversation-policy` | `policy` (CognitiveRolePolicy camelCase) | Política Conversation persistida usando o contrato compartilhado, Fixed/Preferred/Auto |

`providers.enabled` expressa permissão operacional; `registered` expressa composição do adapter. `configured` só confirma presença local: **não** comprova validade da chave, plano remoto ou quota atual. `ready_local_quota_unverified` não garante que a próxima chamada terá êxito. Todos os adapters são compostos sem abrir Stronghold no boot; permissões são inicialmente desabilitadas e não herdam LR-10A. `free_tier_confirmed=true` é uma declaração do operador, não uma consulta ao billing remoto. Core não faz upgrade, compra nem habilita overage. Confirmar o plano sem cobrança e a quota antes de habilitar; todos os candidatos de uma rota precisam dessa confirmação, incluindo fallbacks. Quota/rate/cooldown continuam sujeitos ao Scheduler existente.

Uma Conversation de primeiro plano é admitida por vez, preservando o contrato do broker de produto. Sessões permanecem ativas após desconexão/restart; criar/selecionar não fecha implicitamente outras sessões. O histórico enviado é apenas o da sessão escolhida, limitado pela política, sem duplicar a mensagem já admitida. Identidade usa o ContextBuilder e a minimização já implementada no adapter; memórias não são enviadas automaticamente nesta capacidade.

SQLite schema020 acrescenta `conversation_runs`, `server_provider_permissions` e detalhes sanitizados em `server_events`. Admissão grava usuário/run/evento em transação; sucesso grava assistant/result/TaskRecord/evento terminal em uma transação. Falhas/cancelamento mantêm a mensagem do usuário. Eventos guardam seleção/modelo/attempt/roteamento e estado, sem chunks/prompt/credenciais. Resultados completos ficam na conversa e no run. Cursor/retention dos eventos seguem os contratos de 1A. No restart, pending/running tornam-se interrupted com `restart_never_retries`, preservando IDs e entrada. Não existe replay/resume remoto automático.

O bootstrap não dispara summaries nem TaskGraph, mesmo que uma configuração histórica tenha summary pendente. Aprovações/ferramentas agentivas permanecem indisponíveis; texto de resposta não executa shell nem ganha HumanLocal.

Entrada terminal mínima (não é a CLI completa de SERVER-1C):

```sh
narys-core providers
narys-core session-create
narys-core conversation 38 'Olá'
narys-core task-get 189
narys-core session-get 38
printf '%s' '{"operation":"provider-configure","provider_id":"groq","enabled":true,"free_tier_confirmed":true}' | narys-core ipc
```

O subcomando `ipc` lê somente um Command JSON tipado de stdin com limite, acrescenta envelope/correlação e não repete mutações. Conteúdo de conversa pode ser passado por stdin para evitar argumentos do processo/histórico do shell. Resultados só são impressos quando solicitados pelo cliente local autenticado.
