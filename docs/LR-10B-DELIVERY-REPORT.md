# LR-10B — relatório de entrega para auditoria independente

**LR-10B — CANDIDATA CORRIGIDA, AGUARDANDO REAUDITORIA INDEPENDENTE DA LUNA.**
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

## Validação da candidata original (histórico anterior à FIX-1)

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

## Operação e recursos da candidata original (histórico)

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

## LR-10B FIX-1 — Startup Ownership & Recovery Safety

Status: **LR-10B — CANDIDATA CORRIGIDA, AGUARDANDO REAUDITORIA INDEPENDENTE DA LUNA.**
Veredito recebido da auditoria da candidata 45bc5ad: **FIX-AND-RETEST**.
Base desta correção: `45bc5ad0cabfd49ccf1967a8ba7fda5a9d4fea75`, local/remoto
confirmados antes de editar, working tree limpa. Nenhuma branch adicional, PR,
merge, squash, rebase ou force-push. Main permanece 21be638.
Código da FIX-1: `eaba03e98b54f5cde2b5968fbb1cbc4808f20576`.
O commit adicional das evidências será identificado pelo HEAD publicado.

### Causa e saídas investigadas

Confirmado: INSERT starting era anterior à preparação de workspace/logs/sdk-state;
os retornos com ? escapavam sem finalização. A lista negativa no supervisor atribuía
verified=true a erros sem prova. Assim, memória podia permitir outra geração enquanto
SQLite preservava uma intenção pendente que bloqueava recovery depois do restart.

| Saída / fronteira | Correção / evidência exigida |
|---|---|
| leitura da autoridade, validação de artefatos, pin | leitura/ownership incertos bloqueiam; pin conhecido inválido, antes de qualquer efeito, é NoProcessLaunched |
| root, prune, boot ID, tempfile | nenhum SDK invocado; erros de armazenamento permanecem incertos; tempfile não registrado tem RAII |
| INSERT preparando, incluindo erro/commit | transação Immediate verifica ownership anterior; erro é PersistenceUncertain; nenhum launch sem INSERT e commit seguinte |
| workspace, logs, sdk-state e options/path | todas as saídas passam pela guarda e finalização failed_before_launch; stopped/verified requer commit de prova sdk_launch_not_invoked |
| persistir launch_intent | erro bloqueia e registra ausência de invocação quando possível; falha de segurança nunca vira verified por conveniência |
| erro imediatamente antes de invocar SDK | ausência positiva certificada; crash apenas com intenção durável continua incerto |
| Client::start/handshake/timeout, pidfd e status | SDK pode ter criado processos; stop/Drop e guardian não substituem prova kernel, identidade ausente e estado terminal commitado |
| ownership incompleto, ready/versão/protocolo/persistência | startup falha, mantém erro sanitizado original, limpa processos; Ready requer IDs completos e owner principal igual ao SDK |
| stop/cleanup, incluindo falha ao gravar | caminho físico sempre executado; transação liga owner+runs; falha de persistência continua bloqueante |
| Drop/unwind/saída interrompida | guarda conserva fase durável e faulted/unverified, sem certificar cleanup em destructor |
| certificado/row perdido ou inconsistente | validar prova, não bit; artifacts sem row não são adotados; zero linhas afetadas é falha de persistência |

### Contrato e persistência

RuntimeFactory retorna StartupFailure(code, safety, safety_error, runtime_ref), com
quatro classes explícitas. Supervisor usa **somente evidência tipada**; o teste com
código idêntico e quatro classes comprova ausência de classificação por nome de erro.
StartupError segue o run mesmo quando a intenção falha, mantendo vínculo para auditoria.
O finish não herda verified de uma escrita parcial quando startup permanece incerto.
Status distingue bit registrado e cleanup efetivamente respaldado por certificado,
e comunica erro de leitura de ownership em vez de afirmar ausência de runtime.

**Nenhuma migration/schema/pin/autoridade nova.** O journal version1 fica no
owner_json do schema021. Preparing é barreira positiva porque launch_intent precisa
commitar antes do único Client::start. Recovery considera ref/path/boot/journal,
recibos privados, kernel_children_exhausted e ausência de identidades /proc.
Rows antigas com prova completa permanecem compatíveis; rows legadas incompletas
não ganham uma classificação prelaunch fictícia. Boot inválido/JSON inconsistente
não é certificado. Um boot kernel distinto e válido é prova específica dos PIDs antigos.

Recovery é idempotente, não sinaliza PIDs de recibos, não cria TaskId/sessão nem
repete efeitos. Runs incertos ficam Interrupted; runs failed continuam failed,
inclusive depois de recuperar segurança de processos. A recuperação apenas permite
uma **nova demanda explícita**, que recebe ID e sessão próprios.

### Testes e evidências finais da FIX-1

- **17 testes novos**, com subcasos de fault injection: 16 SDK/SQLite/processos e
  1 contrato mock de classificação. Todos verificam memória e/ou run/owner persistidos,
  e os faults de startup usam ambos. Os 15 cenários exigidos estão individualizados
  na [matriz atualizada](evidence/lr10b/VALIDATION-MATRIX.md#lr-10b-fix-1--startup-ownership--recovery-safety).
- Core: **81 unitários +11 integrações aprovados**, incluindo 49 lifecycle/SDK,
  Conversation, CLI, IPC, cancelamento, manutenção e recovery.
- Domain: **1059 unitários +2 doctests aprovados**, dois gates reais existentes
  ignorados; cobre Scheduler, TaskGraph, Codex read-only, autoridade e armazenamento.
- Python: **17 aprovados**. Build dos dois binários offline/locked aprovado;
  rustfmt e diff --check aprovados. Nenhuma salvaguarda foi desabilitada.
- Triggers ABORT e IGNORE testam falhas reais SQLite. A escrita rejeitada preserva
  o último commit: preparing/starting com verified=0, ou faulted/verified=0. Memória
  e run ficam não verificados. Se um bit antigo não pode ser reparado, status o
  identifica como não comprovado e recovery falha; não certifica zero linhas gravadas.
- Pós-launch: recibo kernel positivo e identidades ausentes, descendente setsid
  reaped, processo externo preservado. Falta/contradição de recibo bloqueia também
  uma factory nova após restart. Cancelamento concorrente mantém erro original.
- Repeated recovery não aumenta counts de runs/sessions/owners nem chamadas create.
  IDs distintos e sequência após 777 são conferidos; todos os peers mantêm send=0.

Logs desta revisão: [core](evidence/lr10b/fix1/core-tests.txt),
[Domain](evidence/lr10b/fix1/domain-tests.txt), [Python](evidence/lr10b/fix1/python-tests.txt),
[build](evidence/lr10b/fix1/build.txt). A evidência anterior permanece histórica;
esta correção substitui as alegações contestadas de startup/ownership.

### Serviço instalado, limites, recuperação e rollback

Atualização controlada pelo instalador existente depois dos testes e commit de
código `eaba03e98b54f5cde2b5968fbb1cbc4808f20576`. A manutenção verificou tarefas ativas antes
de parar/trocar o Core: zero tarefas e workers, runtime Dormant. Protocolo v1 voltou
pronto, serviço active/running, PID 30130, NRestarts=0; cgroup
contém somente o Core. Unidade e drop-in Keyring mantêm SHA-256 anterior. O manifesto
indica worktree dirty porque apenas relatório/evidências estavam pendentes; código
instalado é exatamente o commit acima. Não houve alteração de boot ou serviço externo.

Após **25 consultas lazy**, continua Dormant/geração0/leases0 e nenhuma identidade
Copilot. RSS **26480 KiB (25,86 MiB)**; janela ociosa
**10.05s**, CPU 15→15 ticks, sem incremento na resolução de
100 ticks/s. MemoryCurrent systemd=6918144 bytes é a contabilização do cgroup,
não substitui RSS. Medição curta ociosa, sem afirmar desempenho sob inferência.

SQLite autoritativo continua **user_version21, integrity_check=ok, FK=0**, todas as
tabelas agent_sessions/runs/runtime_owners vazias: o probe não admitiu tarefas.
Backup consistente privado **pós-atualização**:
`/home/sam/.local/state/narys/core/backups/lr10b-fix1-post-update-lo_gjm3n/authority-schema021.sqlite3`,
SHA-256 `02955a370313ccf5fa600b52b50e05259fd8e6a996247dc98d761b2845449480`.
Backup recuperável de binário/unidade/CLI do updater:
`/home/sam/.local/state/narys/core/updates/server-1a-zqqnmnzb`.
Binário instalado SHA-256 `6004aecab7099e80b082da6853e8ff93a400921a5d402f5d807769d101f0c201`.
[host-before](evidence/lr10b/fix1/host-before.json),
[host-after](evidence/lr10b/fix1/host-after.json),
[atualização](evidence/lr10b/fix1/host-update.txt) e
[manifesto](evidence/lr10b/fix1/installation-manifest.json) preservam os dados reais.

Nenhum agent new/resume foi executado contra o CLI autenticado. SDK oficial foi
exercitado somente com peer local sintético; nenhuma inferência, ferramenta agentiva,
quota/billing RPC, download/update de SDK/CLI ou gasto novo provocado. Faturamento
externo continua NOT_VERIFIED. Credenciais, Stronghold, Keyring, boot, SSH e GNOME
não foram alterados. Wrapper Python e unidade systemd também permaneceram idênticos.

NOT_VERIFIED anteriores permanecem: lifecycle/transcripts reais autenticados,
SSH físico durante inferência, custos de conta externa, endurance e MSRV exato 1.94.
Crash pré-launch é fixture sobre o journal commitado; não é SIGKILL de um startup
Copilot autenticado no serviço instalado. A corrida desconhecida entre commit da
intenção e invocação não é inferida como ausência de processos após perda do Core.
Sem certificado persistido/recibo real, conserva faulted no mesmo boot.

Operação: consultar agent status/tasks/events e usar agent recover para reconciliar
provas existentes. Se a incerteza permanecer, agent stop mantém Conversation sem
habilitar o especialista; reparar persistência/recibos somente com evidência e backup.
Não remover ownership, forçar verified, apagar artifacts ou reenviar para contornar
bloqueio. Não se reinicia host ou serviço externo para validar esta FIX.
Rollback imediato: agent stop. Backups de binário/unidade/CLI e snapshot privado
schema021 estão preservados. Schema não mudou, mas voltar ao binário auditado
reintroduz o defeito; mantê-lo com lifecycle Copilot fechado. Nunca restaurar dados
pessoais automaticamente nem baixar user_version. LR-10C–F seguem com seus gates.

### Arquivos desta FIX

Inventário completo relativo à candidata auditada (23 arquivos; código,
documentação e evidências, sem migrations/dependências novas):

- `docs/LR-10-COPILOT-SPECIALIST-AGENT.md`
- `docs/LR-10B-COPILOT-ADAPTER-SUPERVISOR.md`
- `docs/LR-10B-DELIVERY-REPORT.md`
- `docs/evidence/lr10b/SHA256SUMS`
- `docs/evidence/lr10b/VALIDATION-MATRIX.md`
- `docs/evidence/lr10b/fix1/SHA256SUMS`
- `docs/evidence/lr10b/fix1/build.txt`
- `docs/evidence/lr10b/fix1/core-tests.txt`
- `docs/evidence/lr10b/fix1/delivery-checks.json`
- `docs/evidence/lr10b/fix1/domain-tests.txt`
- `docs/evidence/lr10b/fix1/host-after.json`
- `docs/evidence/lr10b/fix1/host-before.json`
- `docs/evidence/lr10b/fix1/host-update.txt`
- `docs/evidence/lr10b/fix1/installation-manifest.json`
- `docs/evidence/lr10b/fix1/python-tests.txt`
- `narys-core/src/copilot/mod.rs`
- `narys-core/src/copilot/sdk.rs`
- `narys-core/src/copilot/sdk/tests.rs`
- `narys-core/src/copilot/sdk/tests/startup_safety.rs`
- `narys-core/src/copilot/startup.rs`
- `narys-core/src/copilot/store.rs`
- `narys-core/src/copilot/supervisor.rs`
- `narys-core/src/copilot/tests.rs`
