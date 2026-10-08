# LR-9B — Execution Broker & Real PTY Runtime

**Estado:** **LR-9B IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**
**Branch:** `lr-9b-execution-broker-pty`.
**Base preservada:** `60763776cc01d315b681d9b37f8af9b069497c80`.

## Recuperação da sessão interrompida — 08/10/2026

Inventário executado antes de qualquer alteração: `git status --short --branch`,
`git branch -vv`, `git log --oneline --decorate --graph --all -30`,
`git diff --stat`, `git diff`, `git diff --cached`,
`git ls-files --others --exclude-standard`, `git stash list`,
`git reflog --date=iso -30`. Também foram examinados reflogs de todas as refs,
stash por stat, arquivos/timestamps, símbolos de execução, remoto via
`ls-remote` e objetos inacessíveis por `git fsck --no-reflogs --unreachable`.

**Cenário A:** branch local já existente e ativa, apontando para a mesma base de
`main` e `origin/main`, sem commit LR-9B. Tracking inicial era `origin/main`,
não uma branch LR-9B remota. `ls-remote` confirmou main na base conhecida e
nenhuma branch LR-9B publicada. Nenhuma branch foi recriada ou trocada.

Working tree inicial:

```text
## lr-9b-execution-broker-pty...origin/main
 M src-tauri/Cargo.lock
 M src-tauri/Cargo.toml
 M src-tauri/src/lib.rs
 M src-tauri/src/presentation.rs
?? src-tauri/src/execution/
```

Index vazio. Os cinco arquivos untracked eram `broker.rs` (179 linhas),
`contract.rs` (210), `mod.rs` (16), `os.rs` (65) e `pty.rs` (217): **687 linhas**.
Não havia `tests.rs`, embora `mod.rs` já o referenciasse, nem documento LR-9B.
Tracked diff: 126 inserções e quatro remoções, incluindo dependency e shutdown.

Reflog relevante:

```text
2026-10-08 03:26:05 -0300 main: pull --ff-only origin main -> 6076377
2026-10-08 03:32:12 -0300 branch: Created from origin/main
2026-10-08 03:32:12 -0300 HEAD: main -> lr-9b-execution-broker-pty
```

Arquivos escritos entre 03:34:51 e 03:39:39, coerentes com a interrupção.
Não foram encontrados commits LR-9B locais/publicados/reflog-only. Os 223
commits inacessíveis examinados são históricos anteriores; nenhum corresponde
à LR-9B, e o mais recente é de 06/10. `stash@{0}`:
`On main: rescue-uip4-fix-after-sigkill`, diff de UIP-4, preservado sem aplicar.

Inventário bruto, fsck, listagem dos commits históricos e backup exato dos nove
arquivos recuperados ficaram em `/tmp/narys-lr9b-recovery/`. Esse backup é local,
não um mecanismo de persistência de produto. Não houve reset, restore, clean,
stash, checkout, switch, rebase, merge ou descarte de conteúdo preexistente.

### Auditoria do parcial antes de continuar

Lidos o plano LR-9, LR-9A, plano operacional, arquitetura, SECURITY,
PERF-1C, `operational_trace/`, Presentation, TaskRegistry/runtime, agent types,
composition root e manifesto. Preservados os contratos, módulos, dependency
`portable-pty = "=0.9.0"`, mecanismo de drenagem nonblocking, output/replay,
policy de ambiente, authority opaca e integração de shutdown recuperados.
Nenhum adapter agentivo, alteração frontend ou artefato fora de escopo foi
identificado/removido.

O check inicial falhou com dez erros de inferência de tipos em duas closures.
A implementação não havia chegado à validação. Correções/complementos:

- tipagem das closures e tratamento imediato de falha do leitor stderr;
- primeira causa timeout/cancellation serializada, sem troca por exit posterior;
- PID factual, wait/reap observável e cleanup bounded, com fallback supervisionado;
- corrida de PGIDs de jobs corrigida usando pidfd para membros da sessão PTY;
- lifecycle trace diferencia exec/PTY e ignora erro de publish;
- testes reais, stress, probe nativo de Presentation, documentação e gates.

`portable-pty` já converte EIO Linux do reader em EOF; foi auditado e preservado,
sem reimplementar o tratamento na Narys.

## Contratos e autoridade

`src-tauri/src/execution/` separa `contract`, `broker`, `os`, `pty` e testes.
É API Rust nativa; não há comando Tauri de execução nem DTO Deserialize.

`ExecutionId` é u64 opaco, único/monotônico por processo, AtomicU64 checked,
zero inválido, exaustão explícita sem wrap. `PtySessionId` usa o mesmo domínio
de execução; nunca usa PID ou TaskId como identidade. TaskId e correlation são
opcionais e conservam validação LR-9A.

`ExecutionOrigin::{Human, SpecialistAgent, Worker, CognitiveProvider}` registra
provenance. **Owner/origin não é authority.** `ExecutionAuthority` é capability
opaca sem construtor público/serde. `HumanLocal` é cunhada somente dentro da
fronteira nativa de execution; aceita somente Human. Fixture existe somente
sob `cfg(test)`, restringe programa exato, modo Structured e ambiente Controlled.
Nenhuma authority agentiva existe. Codex/Copilot/Worker/provider falham fechado,
mesmo apresentados junto de authority humana. Agents não escrevem/redimensionam/
fecham a PTY humana; não existe handoff.

O handle retornado após admission é capability de cancelamento daquela execução;
não há cancelamento arbitrário por PID/ID fornecido externamente. Produção
registra uma instância `OnceLock<Arc<ExecutionBroker>>` em managed state. Testes
isolados têm domínio próprio, sem criar instâncias de produto concorrentes.

Resultados contêm request factual, modo, identidade/PID separados, state,
timestamps, exit code, signal, spawn failure, runtime error, reaped,
cleanup_pending e captura. PTY mantém histórico no próprio stream; stdout/stderr
não duplicam esse histórico no resultado PTY.

## Structured Exec e WorkspaceScope

`submit(request, authority)` valida antes de admission, usa executable + argv,
stdin null e pipes stdout/stderr. Não cria `sh -c` implicitamente. `/bin/sh` pode
ser executable explícito e seu script aparece nos args factuais.

`WorkspaceScope` canonicaliza diretórios raízes e cwd, verifica containment por
componentes e rejeita symlink escape de cwd. Raízes são revalidadas antes do uso.
**WorkspaceScope valida a localização inicial autorizada; NÃO é filesystem
sandbox.** Processo pode acessar caminhos externos/rede. Renames entre validação
e spawn não são contidos; sandbox adversarial é trabalho futuro.

`EnvironmentPolicy::Controlled` faz env_clear e só instala os pares explícitos.
Não herda PATH/HOME/tokens/credenciais da Narys. Recomenda-se executable absoluto
e PATH explícito quando necessário; nenhuma futura execução agentiva pode usar
a herança humana por declaração de origin. `HumanInherited` exige Human +
HumanLocal; sessão humana normal pode herdar o ambiente do usuário.

## Budgets

| Constante | Limite |
| --- | ---: |
| MAX_ACTIVE_EXECUTIONS | 8 líderes administrados, incluindo PTYs; não RLIMIT de descendentes |
| MAX_PTY_SESSIONS | 4 |
| MAX_ARGS / MAX_ARG_BYTES | 256 / 64 KiB agregados |
| MAX_PATH_BYTES | 4096 por path antes/depois da canonicalização |
| MAX_SCOPE_ROOTS | 16 |
| MAX_ENV_ENTRIES / MAX_ENV_BYTES | 128 / 64 KiB agregados |
| MAX_TIMEOUT | 3600 s; timeout zero inválido |
| MAX_CAPTURE_BYTES | 2 MiB por stdout e stderr, prefixo |
| PTY_RETAIN_BYTES / PTY_RETAIN_CHUNKS | 2 MiB recentes / 512 chunks |
| READ_CHUNK_BYTES | 8192 |
| MAX_PROC_SCAN_ENTRIES / MAX_PROC_STAT_BYTES | 65.536 / 4096 |
| MAX_INPUT_BYTES / INPUT_QUEUE_CHUNKS | 64 KiB / 8 |
| MAX_BATCH_CHUNKS / MAX_BATCH_BYTES | 32 / 256 KiB |
| MAX_DIMENSION | rows/cols 1..=1000 |
| TERMINATION_GRACE / REAP_GRACE | 250 ms / 1 s |
| DRAIN_GRACE / SHUTDOWN_DEADLINE | 250 ms / 4 s |

PTY input tem até oito buffers queued + um pending no supervisor; nenhum
producer aguarda espaço (`try_send`). Caps de admission também limitam threads:
até oito supervisors + dois readers por Structured Exec (no máximo 24 workers).
PTY tem supervisor/writer serializado + reader, sem nova stack assíncrona.

Capture prefix, ring PTY e batches são bounded. Counters cumulativos saturam;
identidades/sequências não fazem wrap. Caps medem payload, não RSS/allocator.
Resultados, handles e batches devolvidos pertencem ao caller: ele deve limitar
a própria coleção, especialmente na LR-9C. Broker remove entradas de workers
terminados e não acumula resultados históricos no registry.

## Drenagem e PTY

Stdout/stderr têm threads simultâneas, descriptors O_NONBLOCK e readers
internos. Capture overflow apenas deixa de reter bytes, contabilizando
`total_bytes`, captured, dropped, truncated. Continua drenando até EOF;
completion/timeout/cancellation não dependem do consumidor.

PTY real via `native_pty_system`, master/slave, child, writer e reader nativos.
Input é bytes bruto; output é bytes sem exigir UTF-8 nem interpretar ANSI.
Sessão guarda request/shell/cwd/owner, dimensões, state, child no supervisor,
master, writer, histórico e cursor. `open_human_shell` valida `$SHELL` absoluto,
regular e executável; fallback `/bin/bash`, `/bin/sh`. Testes usam shell explícito.
Sessão humana aberta por esse helper tem timeout máximo de uma hora.

`send_input`, `resize`, `close`, `wait`, `result`, `replay` cobrem lifecycle.
Resize chama a PTY real (observado por stty), não apenas metadata. Reader segue
sem WebView/subscriber e depois do overflow; evicta FIFO recente por bytes/chunks.
Replay informa latest, retained range, state/dimensions, next_after, has_more,
gap, totals/drop bytes/chunks, read_error/incomplete. Batch valida mínimo de
8192 bytes para garantir avanço mesmo no maior chunk. Sem IPC streaming.

## Cancelamento, process groups e cleanup

Structured Exec cria próprio process group (`CommandExt::process_group(0)`).
Linux usa waitid WNOWAIT para observar exit sem liberar PID/PGID antes do último
signal; depois wait reap do filho direto. Cancellation/timeout são primeira
causa terminal aceita; exit posterior nunca os transforma em completed/failed.
TERM, grace de 250 ms, KILL e reap; readers drenam final com deadline de 250 ms
para pipe mantido por processo escapado não prender Core. Output incompleto é
factual (`incomplete`), separado de drop dos bytes efetivamente observados.

PTY shells usam session própria e job control pode criar vários PGIDs.
Cleanup sinaliza o grupo líder ainda pinned pelo filho não reaped; jobs em
outros grupos usam pidfd_open/pidfd_send_signal, com session revalidada depois
de abrir o descriptor. Isso evita kill por PGID de job já reaped/reutilizado.
Scan de `/proc` é bounded (65.536 entradas; stat até 4096 bytes, inclusive comm
não UTF-8); truncation do scan/falha de signal conhecido produz runtime_error.
Nenhum PID arbitrário vindo do frontend é usado. PTY exige Linux >=5.3 e acesso
a `/proc`/syscalls pidfd; a disponibilidade é verificada antes do spawn, com
falha fechada se kernel/seccomp não suportar. Referências primárias:
[pidfd_open](https://man7.org/linux/man-pages/man2/pidfd_open.2.html),
[pidfd_send_signal](https://man7.org/linux/man-pages/man2/pidfd_send_signal.2.html).
Grupos sobreviventes são finalizados também no exit voluntário do líder.
Filho direto é reaped; netos pertencem a seus pais/init, não são waitable pelo
Broker. Testes verificam nenhum descendente administrado ainda executando.

**Process-group/session cleanup NÃO é sandbox contra programa hostil que
executa setsid/daemonização para escapar.** Não há promessa de contenção
adversarial, limite de forks externos ou sandbox filesystem/rede.

Um processo em sleep ininterruptível do kernel não pode ser garantidamente
reaped por um deadline userspace. Após TERM + KILL + 1 s sem exit observado,
resultado declara `cleanup_pending=true`, sem sucesso fictício. Reader termina
bounded; supervisor existente mantém child/slot contado até exit/reap, atualiza
cleanup facts e não cria fila/thread infinita de reapers. Admission não libera
esse slot prematuramente. Espera de cancel/resultado e Quit continuam bounded.
Morte forçada do app/SIGKILL/crash ou child preso no kernel não é cleanup normal;
esta fase não promete recuperação durável de sessões após restart.

## Presentation / Headless / Trace

`close_presentation` conserva o fluxo PERF-1 de detach/destroy de WebViews,
sem solicitar shutdown do ExecutionBroker. Não muda authority e não interrompe
PTY/Exec já admitidos. Broker e leitores pertencem ao Core.

`request_quit` fecha admission/cancela execution, TaskRegistry e Summary;
wait coordenado existente de até 5 s inclui workers do Broker. RunEvent::Exit
faz fallback idempotente com deadline de 4 s e warning se excedido. O fallback
pode acrescentar até 4 s ao deadline coordenado, não uma espera infinita.

OperationalTraceBus recebe apenas lifecycle factual `exec_*` / `pty_*`,
starting/running/completed/failed/cancelled/timed_out, identity e correlação.
Não recebe argv/path/environment/input/output nem byte PTY. Publish/rejeição/
subscriber cheio/ausente não alteram resultado. Não integra TaskEvent,
Scheduler, TaskGraph, providers ou agents; isso permanece LR-9D.

## Dependency / licença / MSRV

Preservado pin Linux `portable-pty = "=0.9.0"`, default-features false, sem
serde_support ou smol/futures runtime novo. Licença MIT. Native Unix usa nix
term/fs, filedescriptor e serial2; Windows-only entries no lock não habilitam
runtime Windows nesta fase. `libc = "=0.2.189"` já existia na árvore aprovada;
foi tornada dependency direta para fcntl/waitid/signals. Manifesto permanece
`rust-version = "1.77.2"`.

Conferidos Cargo.tree/features, metadata e fonte instalada versionada, além da
[API oficial portable-pty 0.9.0](https://docs.rs/portable-pty/0.9.0/portable_pty/).
MSRV declarado no conjunto novo: nix 0.28.0 = 1.69; serial2 0.2.38 = 1.63;
libc 0.2.189 = 1.65; anyhow 1.0.104 = 1.68; log 0.4.34 = 1.71.
portable-pty, filedescriptor, downcast-rs, shell-words e cfg_aliases não declaram
rust-version; ausência desse campo **não prova** compatibilidade 1.77.2.

Toolchain instalada: somente stable rustup; comandos do ambiente usam Fedora
Rust/Cargo 1.98.1. **Rust 1.77.2 não instalado; check real 1.77.2 não executado.**
Não se declara MSRV validado. A árvore base já contém dependências acima de
1.77.2 (zbus >=1.87, time/darling/ICU/serde_with etc. >=1.88). Nenhuma foi
atualizada para a LR-9B. Resolver a dívida global requer fase própria; não
levantar manifesto nem pinar/refatorar toda a árvore sob pretexto de PTY.

## Testes e evidências

Testes nativos sem rede cobrem contracts/ids/limites/authority/environment,
WorkspaceScope/symlink, argv literal, cwd, bytes stdout/stderr, exit/signal/
spawn failure, capture overflow/drop, timeout/cancellation, group cleanup/reap,
admission/shutdown, lifecycle trace best-effort e independência de UI.
PTY: shell/tty/input/output bytes, pwd/cd/comando/interação, resize via stty,
EOF/exit/close/KILL/reap, budgets/history/cursor/gaps e término dos readers.

Stress Structured: quatro processos concorrentes, dois com 6 MiB em cada
stdout/stderr, um cancelado e um timeout, todos reaped e active/workers zero.
Stress PTY: shell, burst 6 MiB sem leitor/subscriber externo, marker de progresso,
budget/gap/drop, shell vivo, resize, comando pós-overflow, exit e zero sessões.
Sincronização usa estado, markers e deadlines de segurança; não sleeps como
expectativa de conclusão nem rede.

Probe opt-in `lr9b-probe` reutiliza somente a infraestrutura isolada PERF-1C.
`execution/probe.rs` aceita apenas ações nativas fixas do harness de arquivo;
nenhum comando arbitrário, IPC de execução, agent authority ou credencial real.
Build normal não compila esse módulo. `scripts/lr9b-native-probe.py` exercita
Tauri/WebViews reais, três executions, Close Headless, bursts sem Presentation,
reopen mesma sessão/processos, resize e Quit com Exec + PTY ativos. Não envia
chamada a provider; endpoint sintético fica inativo. A evidência desta execução
está registrada nos gates finais abaixo, sem reutilizar prova histórica PERF-1
como se fosse LR-9B.

## Limitações / dívidas LR-9C/D/E

- LR-9C: fronteira humana nativa específica, Home/terminal, ANSI emulator,
  IPC de batches/cursors, cadence, renderer e budgets do caller.
- LR-9D: adapters/sanitização de fontes cognitivas e agentivas reais.
- LR-9E/10/11: approvals/policy completa, sandbox e execução agentiva autorizada,
  ownership handoff, stress integrado, packaging e endurance/RSS/CPU.
- Runtime LR-9B Linux; PTY requer pidfd (kernel >=5.3) e `/proc`.
  Windows/macOS não implementados/atestados.
- Sem PTY persistente após restart, sem log durável de conteúdo, sem benchmark
  universal de RSS e sem contenção contra daemon escape.
- MSRV global inconsistente herdado e 1.77.2 não atestado.
- Gates humanos do futuro Terminal e auditoria independente da Luna permanecem.

Agents continuam sem execution authority. Não há generic shell/execute_command
IPC, shell/filesystem plugin, xterm/React terminal ou mudança de capabilities.


## Gates finais executados — 08/10/2026

| Gate | Resultado observado |
| --- | --- |
| `cargo check --manifest-path src-tauri/Cargo.toml` | exit 0, 19 warnings herdados; nenhum em execution |
| `cargo test --manifest-path src-tauri/Cargo.toml` | **1058 passaram, 0 falhas, 2 ignored herdados**, 229,12 s; main/doc-tests sem testes adicionais |
| `cargo check --manifest-path src-tauri/Cargo.toml --release` | exit 0, 42 warnings herdados; nenhum em execution |
| `npm run typecheck` | exit 0 |
| `npm run build` | exit 0; warning herdado AvatarViewport 634,27 kB, acima de 500 kB |
| `git diff --check` | exit 0; staged também verificado antes do commit |
| `cargo tree --manifest-path src-tauri/Cargo.toml` | exit 0; árvore e features PTY auditadas |
| rustfmt edition 2021 `--check src-tauri/src/execution/*.rs` | exit 0 |
| `cargo fmt --check` global | diferenças históricas, também reproduzidas em build.rs extraído da base; sem reformatar esses arquivos |
| bateria `execution:: -- --nocapture` | **35 passaram, zero falhas/ignored**; inclui stress e cleanup de jobs via pidfd |
| seis scripts Node PERF/Headless/provider/allocation existentes | todos exit 0 |
| build `--features lr9b-probe` + probe Tauri real | exit 0; Close/Reopen/Quit e drenagem sem Presentation confirmados |

Os dois ignored permanecem `real_app_server_handshake` e
`manual_final_codex_agent_bridge_gate`, gates externos autenticados preexistentes.
Nenhum teste removido, enfraquecido ou novo ignore. A rodada integral anterior
passou com 1057 testes; após corrigir a corrida de PGIDs e adicionar teste de job
control, a árvore final passou com 1058.

Stress específico final: quatro Structured Exec concorrentes, dois bursts de
6 MiB por stdout/stderr, cancel + timeout, todos reaped, active/readers zero.
PTY produziu **6.291.553 bytes** no stress; amostra específica reteve
**1.656.876 bytes / 512 chunks**, perdeu **4.634.677 bytes / 1319 chunks**,
com gap factual, comando/resize pós-overflow, exit e zero sessões/readers.
A distribuição em chunks varia conforme o interleaving/read do kernel; os caps
e a reconciliação de total/retained/dropped são assertions, não benchmarks.

### Evidência nativa desta candidata

[LR-9B-NATIVE-EVIDENCE.json](LR-9B-NATIVE-EVIDENCE.json) contém a projeção factual
de 13 snapshots Tauri e SHA-256 do binário/raw evidence. Mantém Execution e
Presentation, IDs/PIDs, estados, bytes retidos/dropped e últimos 128 caracteres
do tail sintético bounded. Raw completo está em
`/tmp/narys-lr9b-native-final.json`; logs/gates em `/tmp/narys-lr9b-recovery/`.

Ambiente: Fedora, Linux **7.2.8-200.fc44.x86_64**, Rust/Cargo **1.98.1**,
Wayland/GNOME, software rendering e DBus/app data/vault sintético isolados.
Resultado observado na rodada final:

- três executions com identidades distintas;
- zero WebViews/janelas e zero WebKitWebProcess em toda a árvore descendente
  durante Headless;
- Structured flood concluiu/reaped sem Presentation: 6.291.456 bytes em cada
  stdout/stderr, capture 2 MiB e drops acima do cap;
- PTY produziu burst > retention, gap/drop e shell continuou vivo;
- reopen preservou instâncias Broker/TaskRegistry e mesmos PIDs/PTY ID;
- resize real observado pelo shell como **39 111**, comando após reopen recebido;
- Quit começou com PTY + Structured Exec vivos, terminou **exit 0 em 0,415 s**;
- nenhum dos três PIDs de execução ficou vivo/zombie após Quit;
- harness não solicitou inferência/provider nem executou comando recebido da UI.

Warnings AT-SPI/portal no DBus isolado não interromperam o gate. Não houve erro
do Broker, novo warning de execution ou mudança de capabilities/React. A prova
é técnica nativa desta candidata; não declara gate humano do Terminal futuro,
benchmark de performance ou aprovação independente da Luna.

Reprodução sem provider/rede no stress/probe:

```sh
cargo test --manifest-path src-tauri/Cargo.toml execution:: -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml execution::tests::structured_multi_process_stress_gate -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml execution::tests::pty_no_subscriber_overflow_cursor_resize_then_command_exit_stress_gate -- --nocapture
cargo build --manifest-path src-tauri/Cargo.toml --features lr9b-probe
python3 scripts/lr9b-native-probe.py --output /tmp/narys-lr9b-native.json
```

**LR-9B IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**
Nenhum PASS definitivo, PR ou merge é declarado.
