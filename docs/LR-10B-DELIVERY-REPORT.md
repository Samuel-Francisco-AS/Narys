# LR-10B — relatório de entrega para auditoria independente

**IMPLEMENTAÇÃO CANDIDATA — AGUARDANDO AUDITORIA INDEPENDENTE DA LUNA.**
Não é PASS definitivo. Não há PR, merge, squash, rebase ou force-push.

## Git e escopo

Base `main`/`origin/main`: `21be6382d146c99056d8dc99e6a13e2fa0dbe489`.
Working tree inicial limpa; fetch e fast-forward confirmaram a mesma base.
Branch: `lr-10b-copilot-adapter-supervisor`. Os SHAs finais publicados devem ser
conferidos no Git/relatório da entrega; este documento integra o próprio commit.

A [arquitetura permanente](LR-10B-COPILOT-ADAPTER-SUPERVISOR.md) descreve contratos,
máquina de estados, ownership, persistência, cancelamento, recovery, manutenção e
rollback. A [matriz dos 17 cenários](evidence/lr10b/VALIDATION-MATRIX.md) identifica
cada prova por nome de teste e distingue mock, SDK oficial/peer sintético, IPC real
e host. LR-10A e SERVER-1 conservam o significado dos seus encerramentos.

## Resultado técnico

Copilot registrado como SpecialistAgent no AgentRegistry/Core, com lifecycle
Create/Resume tipado pelo SDK Rust oficial pinado. Não é CognitiveProvider, não
participa da eleição PlanV1 e não concede ferramentas nem inferência por legado.
Codex Planner read-only, Conversation, Scheduler, ExecutionBroker e HumanLocal
mantêm seus contratos. Há um Core, uma autoridade SQLite e um socket Unix local.

Supervisor inicia exclusivamente depois de admissão SQLite durável. Duas demandas
compartilham startup/runtime, no máximo dois leases; último release encerra SDK e
subprocessos próprios. Dois subreapers privados com pidfd cobrem morte isolada de
owner/guardian e descendentes setsid, sem sinalizar processos externos. Cleanup
não comprovado deixa Faulted e bloqueia reentrada. Cancellation não cria restart.

Schema021 registra sessões opacas, runs com TaskId/correlation, estado de TaskGraph,
gaps e recibos de ownership/reap. Attach/detach são observacionais e não cancelam
trabalho. Resume exige estado detached, diretório privado, SDK ID e âncora de
session.start durável; sessão ausente/divergente não cai em create ou send.
Restart marca operações incertas Interrupted, sem reenvio/replay remoto.

TraceBus e server_events têm códigos sanitizados e limites. Observador atrasado ou
indisponível registra gaps; falhas reais de execução/cleanup permanecem falhas.
Operações/status são acessíveis por IPC v1 e CLI headless. Maintenance stop fecha
somente admissão Copilot; Conversation continua disponível.

## Validação

| Verificação | Resultado |
|---|---|
| Core unitários, incluindo 32 testes novos lifecycle/SDK | 64 aprovados |
| Core integração control + protocol (binários/socket reais) | 11 aprovados |
| Domain com desktop-tests | 1059 aprovados; 2 gates reais ignorados |
| Domain doctests de autoridade compile-fail | 2 aprovados |
| Operação/CLI/credenciais unit Python | 17 aprovados |
| Build offline/locked dos dois binários | aprovado |
| rustfmt dos módulos novos + diff --check + py_compile | aprovados |

A aprovação destes testes é resultado local; não é PASS da auditoria LR-10B.

Build e testes: offline/locked, downloads desabilitados, jobs=2, Rust Fedora 1.98.1.
Avisos pré-existentes de dead_code/desktop fixture permanecem; não foram suprimidos.
Os dois testes Codex reais ignorados são gates manuais existentes: não foram
habilitados. Testes de HTTP provedores usam servidores loopback sintéticos.
Os testes históricos de consentimento usam HOME temporário; não criam consentimento
na instalação real, não reutilizam recibos encerrados e não enviam prompts reais.

Evidências brutas reproduzíveis: `evidence/lr10b/core-tests.txt`,
`domain-tests.txt`, `python-tests.txt`, `build.txt`, `host-before.json`,
`host-after.json`, `host-update.txt`, `pins-and-boundaries.json`.

## Operação e recursos

Serviço atualizado e ativo: PID 20432, running, NRestarts=0.
O cgroup contém somente esse PID. Após 50 consultas agent status, geração=0,
Dormant, leases=0, process_id=null, nenhum processo Copilot e três novas tabelas
agent_* vazias. Nenhum create/resume/handshake nativo foi executado no host.

| Medição de 10s | Antes (SERVER-1) | Depois (LR-10B) |
|---|---|---|
| RSS inicial/final KiB | 16596 / 15956 | 26160 / 26416 |
| Delta CPU ticks (100 Hz) | 0 | 0 |
| MemoryCurrent cgroup | 3534848 B | 6377472 B |

Amostra ociosa curta, sem endurance; RSS inclui páginas compartilhadas e muda com
residência/cache. O Core permaneceu baixo em relação ao host de 8 GiB, sem processo
SDK/CLI residente. Não se extrapola esta amostra para inferência real.

SQLite passou de 20 para 21, integrity_check=ok, foreign_key_check sem erros. Todas
as contagens de tabelas antigas permaneceram iguais, exceto um evento runtime/ready
adicional de boot (70→71); dados novos agent_* continuam zero. A comparação é de
contagens/integridade, não de equivalência byte a byte do conteúdo pessoal.
Backup schema020: `/home/sam/.local/state/narys/core/backups/lr10b-schema021-6RzQTu/authority-before-schema021.sqlite3`.
Backup instalado: `/home/sam/.local/state/narys/core/updates/server-1a-ukcfzjfu`.
Unidade systemd e drop-in Keyring têm os mesmos SHA256 antes/depois.
Revisão do código instalado: `f31f4d77ceaa6fe6c183e8ed8bd33ffd48e6f69d`.
Manifest indica working tree dirty porque somente relatório/evidências de entrega
estavam sem commit; o diff de narys-core/narys-domain era vazio após o commit.


Atualização controlada somente de Narys, com checagem de tarefas antes de staging
e imediatamente antes de stop. Binário/unidade/CLI antigos preservados; Core
preserva snapshot consistente de SQLite antes da migração 021. Não se alterou boot,
SSH, GNOME, Stronghold, Keyring nem a unidade systemd. Manifest da instalação
registra revisão do código instalado. Caminhos dos backups constam da evidência.

## Inferência, custos e limites

Nenhuma inferência real Copilot/Codex/provedor foi executada nesta implementação.
O CLI Copilot nativo **não foi iniciado** para testar o host. Seu arquivo/pin foi
verificado por hash local; o SDK oficial rodou somente com peer sintético e send=0.
Não houve download/update do SDK ou CLI. O caminho produtivo LR-10B não contém
session.send, ferramenta nativa ou autorização derivada da LR-10A; submit legado
retorna lr10a_authorization_closed. Não se consultou quota/faturamento.
Nenhum gasto novo foi provocado pelas ações realizadas; a medição de conta externa
é **NOT_VERIFIED**, não uma alegação baseada em recibo fictício.

**NOT_VERIFIED:** lifecycle/retomada com o CLI Copilot nativo autenticado, durabilidade
real de seus transcripts, logout SSH físico durante tarefa Copilot real, consumo
RSS/CPU sob inferência real, endurance prolongado e MSRV exato 1.94. Não foi preciso
executar essas operações para obter as provas determinísticas desta entrega.

Riscos residuais: política deny-all do SDK não constitui sandbox (gate LR-10C);
morte simultânea SIGKILL dos dois reapers fora do cgroup systemd não tem garantia
nesta entrega; perda/corrupção de recibos na mesma inicialização do kernel bloqueia
reentrada de forma conservadora; recuperação não transforma operação incerta em
resultado certo. Backups schema020 não podem ser apontados automaticamente para
schema021, nem restaurados descartando dados novos. Retenção de logs de runtime é
bounded: 32 diretórios; registros autoritativos/sessões exigem política de retenção
futura. Essas limitações não são omitidas nem apresentadas como testes aprovados.
LR-10C–F e LR-11 mantêm seus gates independentes.

## Inventário

51 arquivos alterados/adicionados frente à base, incluindo documentação e evidências.

- `docs/LR-10-COPILOT-SPECIALIST-AGENT.md`
- `docs/LR-10B-COPILOT-ADAPTER-SUPERVISOR.md`
- `docs/LR-10B-DELIVERY-REPORT.md`
- `docs/NARYS-01-AGENT-TOOLS-DELIVERY.md`
- `docs/evidence/lr10b/SHA256SUMS`
- `docs/evidence/lr10b/VALIDATION-MATRIX.md`
- `docs/evidence/lr10b/build.txt`
- `docs/evidence/lr10b/core-tests.txt`
- `docs/evidence/lr10b/delivery-checks.json`
- `docs/evidence/lr10b/domain-tests.txt`
- `docs/evidence/lr10b/host-after.json`
- `docs/evidence/lr10b/host-before.json`
- `docs/evidence/lr10b/host-update.txt`
- `docs/evidence/lr10b/installation-manifest.json`
- `docs/evidence/lr10b/pins-and-boundaries.json`
- `docs/evidence/lr10b/python-tests.txt`
- `narys-core/IPC.md`
- `narys-core/README.md`
- `narys-core/ops/agent_runtime.py`
- `narys-core/ops/install_cli.py`
- `narys-core/ops/update_user.py`
- `narys-core/src/cli.rs`
- `narys-core/src/copilot/mod.rs`
- `narys-core/src/copilot/sdk.rs`
- `narys-core/src/copilot/sdk/tests.rs`
- `narys-core/src/copilot/sdk_policy.rs`
- `narys-core/src/copilot/store.rs`
- `narys-core/src/copilot/supervisor.rs`
- `narys-core/src/copilot/tests.rs`
- `narys-core/src/ipc.rs`
- `narys-core/src/lib.rs`
- `narys-core/src/main.rs`
- `narys-core/src/operations.rs`
- `narys-core/src/runtime.rs`
- `narys-core/src/server.rs`
- `narys-core/src/storage.rs`
- `narys-core/src/worker.rs`
- `narys-core/tests/lr10b_sdk_peer.py`
- `narys-core/tests/protocol.rs`
- `narys-domain/migrations/021_specialist_lifecycle.sql`
- `narys-domain/src/agents/lifecycle.rs`
- `narys-domain/src/agents/mod.rs`
- `narys-domain/src/cognition/allocation_policy/tests.rs`
- `narys-domain/src/cognition/policy.rs`
- `narys-domain/src/cognition/task_graph_runtime/c4_tests.rs`
- `narys-domain/src/cognition/task_graph_runtime/c4_tests/cancellation_fix2_tests.rs`
- `narys-domain/src/cognition/task_graph_runtime/c4_tests/storage_tests.rs`
- `narys-domain/src/persistence/checkpoints/tests.rs`
- `narys-domain/src/persistence/database.rs`
- `narys-domain/src/persistence/migrations.rs`
- `narys-domain/src/persistence/tests.rs`
