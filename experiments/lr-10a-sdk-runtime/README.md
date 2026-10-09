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

`measure.py` clears graphical display variables, launches a fresh owned group,
samples /proc every 50 ms, tracks identities by PID/start time and uses a local
Linux subreaper to reclaim escaped descendants. No cmdline/environ or credential
contents are collected. CPU is a sampled lower bound; summed RSS can double-count
shared pages. Unknown/very short-lived descendants can escape sampling, so this
is not proof against adversarial process escape. Recovery is reported as failure,
not SDK graceful shutdown. The harness's process cleanup is experimental only.
The SDK itself has no process-tree ownership on Linux in the pinned version.

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
