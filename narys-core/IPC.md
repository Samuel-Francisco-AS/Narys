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
