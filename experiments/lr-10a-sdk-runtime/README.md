# LR-10A — SDK/runtime experiments

**Candidate only. FIX-AND-RETEST. A9 BLOCKED_REAL / AWAITING_HUMAN_APPROVAL.**
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
`cargo test` builds the executable. Python 3, Linux /proc and, for existing auth,
`/usr/bin/bwrap` are required. Use the installation's **native CLI binary** when
possible, rather than its npm loader. Inspect `--version` first; do not update it.

```sh
python3 experiments/lr-10a-sdk-runtime/measure.py metadata /absolute/path/to/copilot
python3 experiments/lr-10a-sdk-runtime/measure.py metadata-existing-auth /absolute/path/to/copilot
python3 experiments/lr-10a-sdk-runtime/measure.py sessions /absolute/path/to/copilot
python3 experiments/lr-10a-sdk-runtime/measure.py sessions-existing-auth /absolute/path/to/copilot
```

- `metadata` uses disposable COPILOT_HOME. Missing auth there says nothing about
  the user's existing account. It reports auth/catalog/quota errors as safe codes.
- `metadata-existing-auth` lets the CLI resolve its existing credential store
  inside a read-only host mount. No credential files are read by the POC or
  harness; no tokens/logins/status messages are exported. Only disposable
  workspace/log directories are writable. There is **no unguarded fallback**.
- `sessions` creates, subscribes, aborts an empty turn, detaches, attempts resume
  and deletes only the POC-created session, in disposable state. Zero tools,
  deny-all handler and disabled file hooks/skills/instruction discovery.
- `sessions-existing-auth` also overlays the existing `~/.copilot/session-state`
  location with an empty disposable directory. It requires that mount point to
  exist; it never copies, lists, resumes or deletes a user's sessions.

Existing-auth guards are **experimental filesystem write protection**, not a
production sandbox: host data remain readable, devices and network accessible,
and configured extensions/ambient CLI behavior are not proven isolated.
An earlier unguarded investigation changed config.json stat metadata; see the
incident in the evidence. The delivered existing-auth code requires bwrap.

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
