# LR-10A — SDK/runtime experiments

**FIX-4 candidate only. FIX_AND_RETEST. A9 BLOCKED, never READY_FOR_A9.**
See the [latest execution report](../../docs/LR-10-LATEST-EXECUTION-REPORT.md).
See [implementation/evidence](../../docs/LR-10A-IMPLEMENTATION-AND-EVIDENCE.md).
This independent Edition 2021 crate never initializes Narys, registers an agent,
changes execution authority, exposes IPC or sends an inference request.

## Build and deterministic verification

Use Rust/Cargo **1.94.0 or newer**, with the committed lockfile. Pin: official
`github-copilot-sdk = 1.0.17`, `default-features = false`, `runtime`.
Always set `COPILOT_SKIP_CLI_DOWNLOAD=1`: `runtime` alone can still acquire runtime
assets through upstream build.rs. `bundle-comparison` checks the feature graph;
with this opt-out it does **not** demonstrate a physically embedded bundle.

From the repository root:

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 cargo check --locked --manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 cargo test --locked --manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 cargo check --locked --manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml --features bundle-comparison
python3 -m unittest discover -s experiments/lr-10a-sdk-runtime/tests -p 'test_*.py' -v
```

For the temporary toolchain used in the evidence, prefix cargo commands with
`RUSTC=/tmp/narys-lr10a-rust-1.94.0/bin/rustc` and
`RUSTDOC=/tmp/narys-lr10a-rust-1.94.0/bin/rustdoc`, and invoke
`/tmp/narys-lr10a-rust-1.94.0/bin/cargo`. Setting only RUSTC incorrectly selects the
host rustdoc for doctests. The toolchain is not installed globally and /tmp is
not persistent; obtain verified official 1.94 components if it has expired.

## Safe manual probes (no inference)

Run through the Linux cleanup/measurement harness, **never the binary alone**.
Python 3, Linux x86_64, pidfds, memfd/seccomp and `/usr/bin/bwrap` are mandatory.
The installed native ELF is pinned by SHA-256 in `boundary.py`; npm loaders,
different binaries and missing libraries fail closed. No runtime is acquired.
Do not run an unprotected CLI to diagnose authentication or dependencies.

```sh
python3 experiments/lr-10a-sdk-runtime/run_fix3.py metadata /absolute/path/to/copilot --output /tmp/fix3-metadata.json
python3 experiments/lr-10a-sdk-runtime/run_fix3.py sessions /absolute/path/to/copilot --output /tmp/fix3-sessions.json
```

- `metadata` uses disposable COPILOT_HOME, Empty mode, no auto-login and an
  offline network namespace. Missing auth says nothing about the user's account.
  Catalog/quota errors are fixed codes; unknown quota is not zero/unlimited.
- Both `*-existing-auth` modes now return `BLOCKED_AUTH_BOUNDARY` before Client
  startup. FIX-2's broad read-only host mount has been retired, with no fallback.
- `sessions` runs the FIX-2 matrix in private COPILOT_HOME: UUID explicit/generated
  IDs, empty abort, store on/off, detach, restart, fresh state and owned deletion.
  Two separate synthetic disk diagnostics never represent SDK persistence.
  Zero tools, deny-all handler and disabled file hooks/skills/instruction discovery.
  All executable real-CLI modes require the same minimal, offline namespace.
  Fresh state changes only its private mount source; personal sessions are absent.

The namespace starts empty (`--tmpfs /`), mounts one native ELF and individual
system libraries read-only, `/fixture` read-only, private `/state` and `/logs`,
tmpfs `/tmp`, namespaced read-only `/proc`, and only `/dev/null`/`urandom`.
No host home, repository, shell, keyring, D-Bus, certificates or resolver files
are mounted. The environment is an allowlist; inherited non-stdio FDs are closed.
Capabilities are dropped; user namespace nesting is disabled; mandatory seccomp
denies keyring access, selected introspection, namespace/mount and io_uring calls.
Seccomp is a denylist, not a complete syscall allowlist. Self/generated-code
execution within private writable paths is not proven impossible.
Network is entirely private/offline, not restricted by provider destination.
No safe authenticated online boundary has been established: A9 remains blocked.
Worker-death/adversarial-supervision limits of FIX-1 remain, without a new supervisor.
The earlier unguarded config incident and original FIX-2 evidence remain historical.

`measure.py` clears graphical display variables and uses a private, single-threaded
Linux subreaper per invocation. Its caller never becomes a subreaper and waits
only for that worker. `/proc/self/task/<pid>/children` plus verified PPID identify
kernel-owned children, including descendants adopted after the root exits or
after `setsid()`/double-fork. PID/start time is checked before and after opening a
pidfd. Recovery signals use **only pidfd_send_signal**, never killpg or numeric-PID
kill. Reaping/adoption uses Linux __WALL (ordinary and clone children) and is
repeated until waitpid reports ECHILD and the kernel child inventory is empty,
within a two-second cleanup budget. Missing ownership evidence, failed signals or absent exhaustion
proof produce failure/inconclusive results; empty observed survivors alone do
not prove cleanup. There is no process-group-based recovery or external reaping.

CPU/RSS metrics still sample /proc every 50 ms independently of recovery. Short
processes can escape metrics without escaping kernel adoption. CPU remains a
lower bound and summed RSS can double-count shared pages. No cmdline/environ or
credential contents are collected. Requires Linux pidfds, a trusted standalone
single-threaded caller with default SIGCHLD handling, and readable proc stat/children.
Unsupported capability/caller state fails before launching the invocation.

Schema 2 adds attributed PID/start-time identities and their basis, unsampled
adopted descendants, kernel_children_exhausted, cleanup_complete, ownership_errors,
recovery signal count, cleanup_ms and a separate operation_outcome. Cleanup states:

| cleanup_status | Meaning |
| --- | --- |
| graceful_no_recovery | Processes exited without harness signals; not proof of SDK shutdown |
| descendants_recovered | Harness forced recovery and verified kernel-child exhaustion |
| timeout_recovered | Operation timed out; harness verified recovery |
| recovery_incomplete | Deadline reached or kernel-child exhaustion cannot be proved |
| inconclusive | Ownership/recovery/capability evidence failed, including a later successful retry |

Forced recovery/timeout always return nonzero even when cleanup_complete is true.
sdk_shutdown_verified remains false: only the fixed SDK report may claim its own
client lifecycle. External processes are not attributed and never signalled.
This experimental harness is not a sandbox, malicious-process containment, or
LR-10B supervisor. A descendant becoming its own nested subreaper, namespace/
reparenting escape, abrupt worker death or an unkillable kernel task is outside
the proven fixture contract; unexpected worker exit reports inconclusive, not
cleanup success. The SDK itself has no process-tree ownership on Linux in the
pinned version. No SDK/CLI real probe was repeated for FIX-1.

## FIX-1 deterministic cleanup verification

The Python command above runs all harness tests without SDK/CLI/inference.
To regenerate the sanitized JSON and per-test text log (overwrites these two
FIX-1 evidence files only):

```sh
python3 experiments/lr-10a-sdk-runtime/tests/test_measure.py --evidence experiments/lr-10a-sdk-runtime/evidence/fix-1-verification.json
```

Fixtures synchronize using a pipe readiness handshake and a post-launch barrier
that waits for root exit before the first metrics sample. PID/start-time manifests
independently verify fixture absence after each invocation; an external control
process is checked alive after harness recovery, then reclaimed by its test owner.
Tests include unsampled children, setsid, live grandchildren revealed by repeated
adoption, double-fork, crash, timeout, identity mismatch, failed signalling and
unverifiable wait exhaustion. Negative proofs remain fail-closed.
See [latest execution report](../../docs/LR-10-LATEST-EXECUTION-REPORT.md) and
[FIX-1 evidence](evidence/fix-1-verification.json). Historical runtime samples use
the prior harness and do not prove this fix against the real SDK/CLI.

SDK errors are projected to a fixed code vocabulary; RPC prose is never used to
guess auth, quota or entitlement. Missing quota is unknown; zero, invalid and
explicit unlimited flags have separate tests. Auto in a catalog does not prove
successful inference or capabilities. Operational completion is never inferred
from session.idle, an abort acknowledgement or assistant text.

Subscriptions are installed with prepare_session/prepare_resume_session before
protocol activity. The POC retains at most 32 correlation IDs and observes at
most 32 events for at most 30 ms per event; missing parents/duplicates/overflow
are diagnostic. The SDK has its own queues, including an unbounded bootstrap
path if callers subscribe late; this POC's projection does not bound all SDK
internals. No event payload, reasoning or tool output is persisted.

## A9 (not authorized)

[Fixture](fixtures/a9-read-only.txt) and
[inert future command/procedure](fixtures/A9-COMMAND-NOT-AUTHORIZED.txt) are ready
for review. No inference implementation or executable A9 mode exists. Resolve
A6/A7 and obtain separate human authorization before adding/running inference.
Do not run the textual future CLI command merely because it is present.

## Evidence

[evidence/](evidence/) contains metadata-only JSON and exact executed test
summaries. Older samples predate cleanup/classification improvements and are
explicitly labeled in the report; do not compare them as identical builds or
claim they all used the final harness. Real session resume failed; mocked
persistence passing does not establish provider persistence. No bundled download,
release-sized bundle benchmark, GUI test, model output or usage event is claimed.

## FIX-2 — persistence investigation (no inference)

This section records the historical FIX-2 procedure. `run_fix2.py` remains
unchanged for audit, but its existing-auth mode is blocked by the current guard.
For retesting the current implementation, use `run_fix3.py` and new evidence names.

[Current report](../../docs/LR-10-LATEST-EXECUTION-REPORT.md) distinguishes fixture
success, local real runtime observations and BLOCKED_REAL for genuine history.
SDK/CLI remain unchanged. Explicit IDs are random UUIDs; Rust SDK 1.0.17 also
locally generates a UUID when no ID is supplied. `enable_session_store` enables
cross-session search/indexing; it is not a transcript flush request.

```sh
python3 experiments/lr-10a-sdk-runtime/run_fix2.py sessions /absolute/path/to/copilot --output /tmp/fix2-isolated.json
python3 experiments/lr-10a-sdk-runtime/run_fix2.py sessions-existing-auth /absolute/path/to/copilot --output /tmp/fix2-existing-auth.json
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 cargo test --offline --locked --manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml -- --test-threads=2
python3 experiments/lr-10a-sdk-runtime/tests/test_measure.py --evidence /tmp/fix2-python-regression.json
```

The historical Python evidence writer hardcodes FIX-1 phase/command labels; an
output filename does not change those labels. The committed FIX-2 regression
JSON corrects only these labels and retains the runner's observations. Do not
regenerate historical FIX-1 evidence while retesting FIX-2.

`run_fix2.py` reuses measure.py unchanged, records streamed artifact hashes and
config stat only. Exit 1 is intentional while the real-history gate is blocked;
inspect per-case outcomes separately from kernel cleanup. There is no send RPC,
resume-to-create fallback, retry or delay intended to force persistence.

On the tested local CLI, empty SDK sessions expose an in-memory start event and
workspace.yaml but no events.jsonl. Detach and restart do not make them resumable.
The matrix reports this observation instead of claiming durable persistence.
A fixture-authored events.jsonl containing only session.start can be read by the
real runtime. That diagnostic has zero model messages and is **not** a workaround
or a genuine provider history. Corrupt synthetic transcripts are not repaired:
allow_transcript_recovery remains false and original fixture bytes are checked.
The local runtime returns -32603, while a current documented -32075 case is
covered separately in fixtures. Safe numeric codes are retained; RPC prose is
never exported or interpreted as an authentication/storage diagnosis.

Only randomly owned IDs in private storage are inspected, queried or deleted.
Storage inspection refuses traversal/symlinks, checks existence/size only, and
never follows workspace_path returned by the CLI. The synthetic diagnostics
read back only their own fixture-authored bytes. Guard configuration requires
the session overlay source to belong to the invocation's private state root.
The original FIX-1 ownership/cleanup implementation and tests are untouched.

## FIX-3 — experimental security boundary (no inference)

Code: [boundary.py](boundary.py), [Rust launcher](src/boundary.rs),
[synthetic native probe](src/bin/boundary-fixture.rs),
[kernel tests](tests/test_boundary.py), [SDK permission fixtures](tests/security.rs).
Build the two native binaries first with the pinned toolchain and download opt-out:

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 cargo build --offline --locked --bins --manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml
python3 experiments/lr-10a-sdk-runtime/tests/test_boundary.py --evidence /tmp/fix3-boundary.json
python3 experiments/lr-10a-sdk-runtime/tests/test_measure.py --evidence /tmp/fix3-python-regression.json
```

The unchanged historical Python runner discovers all tests but labels evidence
FIX-1; the FIX-3 regression evidence explicitly records that source label and
normalizes provenance. Never overwrite historical evidence with these retests.
Test names and JSON outcomes distinguish real kernel tests, synthetic SDK RPC,
real native CLI without authentication, and blocked future provider execution.
The protocol fixture tests raw absent/NoResult/panicking/pending handlers as
unsafe to treat as explicit denial; mandatory DenyAll overrides those callbacks.
`skipCustomInstructions` is checked in acknowledged `session.options.update`,
not guessed from the initial create payload. Rejection of that required update
fails session creation/resume. This does not prove every CLI option is enforced.
A dynamic `libutil.so.1` dependency was observed beyond ldd's static list; it is
mounted as one file. No general library-directory or personal-directory fallback.
The [A9 specification](fixtures/A9-COMMAND-NOT-AUTHORIZED.txt) remains inert.

## FIX-4 — finite host mediation, not authenticated provider connectivity

The actual SDK 1.0.17 contains `ClientOptions::request_handler`. Its
`CopilotRequestHandler` defaults forward HTTP AND WebSocket traffic to upstream.
The experimental [handler](src/auth_network.rs) overrides BOTH; it never uses the
SDK forwarders. It accepts only an empty-header/body GET for the exact synthetic
metadata operation, maps it to one host-owned IPv4 loopback fixture endpoint,
injects a public synthetic secret in host memory, enforces a one-attempt budget,
uses bounded socket I/O and returns only a fixed validated DTO. Unknown paths,
query strings, methods, headers, bodies, CONNECT and WebSocket fail closed.
Redirects, 401, timeout, unavailability and a provider echoing the fake secret
fail closed. The provider fixture uses local HTTP, NOT remote TLS.

[network-fixture](src/bin/network-fixture.rs) is a synthetic JSON-RPC peer, not
Copilot. [fix4_boundary.py](fix4_boundary.py) reuses the FIX-3 mount/seccomp/env/net
plan byte-for-byte except substituting the one approved locally built ELF with
the SAME dependency closure and selecting the strict SDK argv validator. No host
socket, home, DNS, certificate store or network namespace is exposed. Parent AND
child direct TCP attempts fail while the host-mediated operation succeeds.
The new binary must be built along with `boundary-fixture` before testing.

SDK 1.0.17 start discards setProvider.success; the POC requires a separate positive
ACK validation before explicit metadata calls. A false ACK accepted by the SDK
blocks POC admission, without retries/fallback. The real offline probe confirmed
success=true; startup callbacks in a general authenticated design remain unproven.

The protocol is the SDK's existing owned stdio connection, NOT a general HTTP
proxy. It authenticates no provider/account. Runtime-claimed request/session/
agent IDs do not prove caller identity; a child inheriting the runtime's stdio
could impersonate requests. This finite operation limits that channel but does
not prove a general inference or per-agent security boundary. A compromised host
process of the same UID, SDK buffering before policy, worker death and adversarial
children remain outside the proven containment.

Reproduction from repository root (existing Rust/Cargo >=1.94; this FIX actually
used the preinstalled Fedora Rust/Cargo 1.98.1 because the prior /tmp toolchain
expired; NO production MSRV/Edition change):

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 RUSTC=/usr/bin/rustc RUSTDOC=/usr/bin/rustdoc \
  /usr/bin/cargo build --offline --locked --bins --manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml
python3 experiments/lr-10a-sdk-runtime/run_fix4.py rust-tests --artifacts-dir /tmp/narys-fix4-retest --output /tmp/fix4-rust-owned.json
python3 experiments/lr-10a-sdk-runtime/tests/test_measure.py --evidence /tmp/fix4-python.json
python3 experiments/lr-10a-sdk-runtime/run_fix4.py metadata --cli /absolute/path/to/pinned/native/copilot --output /tmp/fix4-offline-metadata.json
```

The Rust runner protects its observation paths from overwrites; if these already
exist, it deliberately stops. Preserve them and select a fresh `--artifacts-dir` and output path
for reproduction rather than deleting historical evidence. It runs Cargo/tests
inside the unmodified FIX-1 subreaper and clears the test environment to fixed
build/evidence variables. The real metadata probe uses the unchanged FIX-3 CLI
boundary, no auth token, and a handler with NEITHER endpoint NOR credential. It
proves registration compatibility only; unauthenticated models/quota remain
unavailable. A missing/unsupported handler is not permission to open networking.

The separate explicit-token fixture deliberately demonstrates SDK token delivery
and inheritance by a synthetic child OUTSIDE the kernel sandbox. It is a negative
control, not an accepted auth design. The injected marker is public test data,
never a real PAT. The sandbox's token-filtering/CLI argv rejection is not widened.
Callbacks returning GitHub tokens also return the token to the CLI over RPC;
credential acquisition on the host does not imply credential containment.

`bytes = 1.12.1` and `futures-util = 0.3.34` are now explicit POC dependencies to
construct the bounded synthetic SDK response; both were already locked/cached
transitively. No package version, SDK/CLI, bundled runtime or production dependency
was updated/acquired. New [evidence](evidence/fix-4-verification.json) links the
current tests separately from prior results. The [latest report](../../docs/LR-10-LATEST-EXECUTION-REPORT.md)
characterizes auth, TLS/DNS and provider integration blockers. A9 remains an
[inert specification](fixtures/A9-COMMAND-NOT-AUTHORIZED.txt), without a send mode.

### H2 — manual unlock without GNOME Shell

[H2 contract and reproduction](HEADLESS-MANUAL-UNLOCK.md) documents a real
GNOME50/libsecret/production-Rust-backend experiment using **synthetic credentials
inside private filesystem, D-Bus, PID and offline namespaces**. This does not
authenticate a personal account or prove cold-start on this Fedora host.
`h2_manual_unlock.py` is a pending human-only existing-login helper, using libsecret
encrypted sessions and a version-pinned, explicitly unsupported GNOME extension;
it starts no daemon, creates no collection and rejects GUI/unverified ownership.
Do not run it from Codex, redirect a password, or use it on personal storage before
the separately reviewed setup/transition. Host GUI, services and credentials were
left untouched; H2 real Copilot gate remains NOT_RUN. Prior boundaries are unchanged.

### H3 — credenciais pessoais sem GUI, metadata-only

[H3](HEADLESS-HOST-VALIDATION.md) comprovou manual unlock da coleção existente e
SDK1.0.17/CLI1.0.95 authenticated=true após parada humana autorizada do GDM.
É headless pós-login gráfico, não cold-start nem sandbox. Stronghold pessoal não
acessado. [Evidência real](evidence/h3-metadata-real.json) usa reserva independente
já consumida: não executar novamente/apagar reserva. Zero inferências/sessões;
finanças, estado privado e ações continuam bloqueados. Serviço user sob demanda,
override somente em /run e rollback GDM verificado; nenhum enable/linger/PAM/boot
permanente. Testes52Rust/158Python, sem código de produção alterado.
