# SERVER-1A — registro único de implementação e auditoria

Estado: EM IMPLEMENTAÇÃO. Não equivale a PASS de SERVER-1B, Conversation real ou ferramentas.

- Abertura real: **2026-10-10T15:03:12-03:00**, America/Recife.
- Deadline SERVER-1 (48h): **2026-10-12T15:03:12-03:00**.
- Etapa: SERVER-1A / 1D; checkpoint inicial: inventário e consolidação operacional.
- Base main local/remota verificada: `553b51182bb477d0b777093da5bfda93239fddf9`.
- Branch exclusiva: `narys-server-1-headless-runtime`, criada de origin/main verificada.
- Workspace inicial limpo; nenhuma mudança na main; sem merge/rebase/force-push.
- LR-10A fechada; sem inferência, resume ou uso de consentimento encerrado.

## ADR: monólito modular, contratos compartilhados e autoridade do processo Core

Extrair a implementação operacional para `narys-domain`: Conversation/Context,
Scheduler, providers/registry, TaskGraph, políticas, persistência, agentes, trace e
ExecutionBroker. `narys-core` compõe o serviço; Tauri mantém comandos e apresentação
como adaptadores dos mesmos contratos. Retirar dependências de runtime/eventos
Tauri do domínio. Sem segunda implementação cognitiva e sem promover agentes a
HumanLocal. Integração de Conversation no servidor pertence à SERVER-1B.

Consolidar SQLite em `~/.local/state/narys/core/db/luna.sqlite3`: snapshot SQLite
consistente da base desktop (identidade/histórico/configuração), acrescido das
headless_tasks legadas preservadas em namespace explícito LR-10A. Backups privados,
migração atômica e lease de escritor compartilhado pelo desktop e servidor;
recusar conflitos/future schema. Recibos ficam nos caminhos originais, sem
reinterpretação ou renovação. Não migrar Stronghold.

Trade-offs: diff estrutural maior, porém uma única fonte e testes do domínio sem
GUI; desktop legado fica indisponível para escrita após takeover até adaptar seus
comandos ao IPC (migração deliberada). Não iniciar SERVER-1B nesta execução.
