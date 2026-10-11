# Core IPC v1 — SERVER-1A / SERVER-1B

Socket `$XDG_RUNTIME_DIR/narys-core/control.sock`: Unix0600, diretório0700,
SO_PEERCRED mesmo UID nos dois lados. O UID Linux identifica o cliente local;
processos desse UID não são isolados e isso **não prova intenção humana** nem
concede autoridade agentiva/approval positiva. Sem listener TCP. Uma conexão por request:
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
legados referem-se exclusivamente a lr10a. Conversation usa namespace product.

Conversation/Sessions, TaskGet/TaskCancel e Providers/ProviderConfigure estão
integrados na SERVER-1B. Contratos preparatórios em `ipc.rs`: Approval(approve_once/deny),
ToolRequest(ListFiles, ReadFile, EditFile, Build, Test, Diff)/ToolResult. As operações sem integração
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

## SERVER-1C — CLI oficial e consultas operacionais

O binário Rust `narys` reutiliza `client.rs` com o cliente legado; [CLI.md](CLI.md)
contém a UX humana/JSON, chat e instalação de usuário. Uma tentativa por request,
sem replay em qualquer falha incerta. O cliente verifica UID, versão/correlação,
limites e diretório runtime privado; funciona com fallback `/run/user/UID` quando
SSH não exporta XDG_RUNTIME_DIR. Não abre banco nem possui estado operacional.

Novas consultas v1, compatíveis e estritas:

| Operação | Campos | Resultado |
|---|---|---|
| `tasks` | `namespace` product/lr10a obrigatório, `after` ID default0, `limit` 1–100 default50 | Resumos `{namespace,task_id,state}`, cursor `next_task`, `has_more`; inclui histórico/run sem duplicar ID nem carregar prompts/resultados |
| `models` | nenhum | Catálogo local de defaults integrados e adapters registrados; `remote_catalog_verified=false`, nenhuma chamada remota |

Cursors de tasks/events devem caber em i64 SQLite. Estado de credentials agora
inclui `state=locked|unlocked|unavailable`, compatibilidade do backend e erro seguro;
status não abre sessão Secret Service nem solicita senha. **Unlock não é operação
IPC**: comandos `unlock`/`credentials-unlock`, password/authority/origin são recusados.
`narys credentials unlock` entra exclusivamente no adapter humano incorporado,
com TTY/SSH/logind, serviço GNOME existente e transporte cifrado verificados.

JSON preserva o envelope IPC nas operações únicas. Doctor é uma composição local
de consultas, com envelope v1 e checks; erros locais usam category=cli. Unlock
usa envelope local v1 e category=credentials. Nenhum desses envelopes locais dá
autoridade ao servidor. Os contratos approve_once/deny permanecem recusados com
`capability_not_integrated`, sem execução simulada.

## LR-10B — lifecycle do SpecialistAgent Copilot (candidata)

Adições estritas e compatíveis ao envelope v1; same UID/socket0600 permanecem.
Nenhuma operação aceita prompt, perfil, authority, origin, programa ou env.

| Operação | Campos | Semântica |
|---|---|---|
| agent-status | nenhum | Snapshot supervisor/capabilities/processos/attachments; nunca lança SDK |
| agent-session-create | nenhum | Admissão assíncrona create SDK sem prompt/tools; recibo TaskRef product + cs-HEX |
| agent-session-get | session_ref | Estado autoritativo/prova necessária de resume, sem raw provider ID/path |
| agent-session-resume | session_ref | Resume explícito de Detached comprovada; mesmo ID/anchor; nenhum fallback/send |
| agent-session-attach | session_ref | Receipt ca-HEX de observação; máximo32; sem ownership de tarefa |
| agent-session-detach | attachment_id | Remove attachment idempotentemente; nunca cancela tarefa |
| agent-session-close | session_ref | Close local explícito; recusa tarefa ativa; bloqueia resume |
| agent-runtime-recover | nenhum | Reconciliar prova de cleanup sem launch/replay; limite7s |
| agent-runtime-stop | nenhum | Fecha admissão/cancela/drena especialista; Conversation permanece operacional |

Referências cs-/ca- seguidas de32 hex; IDs/correlação não concedem autoridade.
Tasks/Get/Cancel e Events existentes incluem lifecycle no namespace product.
No máximo2 demandas, sem fila ilimitada; desconexão deixa tarefa no Core.
Pending/Running no restart tornam-se Interrupted, sem reenvio. Attachments são
voláteis e podem ser refeitos. Leases anteriores ao último podem terminar com
cleanup_verified=false até a prova global; a evidência é atualizada posteriormente.
Gap de observação nunca é interpretado como sucesso/falha funcional.

O adapter está registrado, mas eleição de planner/inferência continuam bloqueadas.
Approval/tool requests seguem capability_not_integrated. Submit LR-10A está fechado;
legacy copilot/session-check/resume-check não acionam mais o harness experimental.
[Contrato técnico completo](../docs/LR-10B-COPILOT-ADAPTER-SUPERVISOR.md).

## LR-10C — contratos e bloqueios efetivos da candidata

O histórico acima descreve os gates anteriores. A candidata LR-10C integra
consultas/negação, mantendo aprovação positiva e efeitos nativos bloqueados.
Nenhuma nova rota concede HumanLocal ou contém shell/argv/programa/env.

| Operação | Campos | Resultado |
|---|---|---|
| agent-policy | nenhum | Policy version, default assisted, disponibilidade BLOCKED por perfil e motivos; nunca inicia subprocesso |
| approvals | after u64(default0), limit1..100(default50), pending_only bool(defaultfalse) | Registros sanitizados/cursor/history; sem executar/reconstituir grants |
| approval-get | approval_id ap- +64hex minúsculo | Contexto sanitizado, digest, expiry, estado/reason; authority=false |
| approval | approval_id, decision approve_once/deny | deny reduz authority; approve_once sempre human_approval_channel_unavailable |
| agent-yolo-request | TaskRef product, session_ref cs-HEX, ttl_seconds1..300, acknowledge_unisolated=true | human_yolo_channel_unavailable_execution_disabled; nenhum consentimento por UID |
| agent-yolo-revoke | nenhum | Revoga consentimento volátil; execution_enabled=false |
| tool-request / tool-result | contratos anteriores | agent_execution_boundary_unavailable antes de efeitos |

`capabilities.approvals=true` significa consulta/negação integradas;
`human_approve_once=false`, `agent_tools=false`, `execution_authority_from_ipc=false`
explicitam as indisponibilidades. Não existe emissão operacional de grant por
wire. O socket acessível a mesmo UID não é canal de emissão humana confiável.
Uma resposta recebida nesse socket pode negar, mas não aprovar uma ação.

Estados: pending, approved, denied, expired, cancelled, consumed, interrupted.
Approved aparece em fixtures positivas Core, não é emitido pela IPC operacional.
SQLite schema022 persiste apenas auditoria; capability é privada em memória.
Restart interrompe pending/approved e não restaura YOLO/grants. Cancel de tarefa
revoga authority antes de interromper o lifecycle/worker pertinente.

```sh
narys agent policy
narys approvals --pending
narys approval ap-HEX64
narys approval ap-HEX64 deny
narys approval ap-HEX64 approve-once # indisponível com segurança
narys agent yolo-revoke
```

Paginação também está na CLI: `narys approvals --pending --after SEQUENCE --limit 100`.
`ap-HEX64` no exemplo é placeholder, não um ID válido nem token.
[Boundary/threat model](../docs/LR-10C-AUTHORITY-APPROVAL-SANDBOX.md).
