# A9_HOST_ASSISTED — authorized preparation, BLOCKED_PRE_SEND

This profile is separate from A9_ISOLATED. It runs native CLI 1.0.91 with Rust
SDK 1.0.17 on the user's host. It is NOT a sandbox. FIX1–4 files/fixtures/evidence,
including boundary.py, remain unchanged. The user's one real attempt is conditional
on verified authentication, entitlement, model pricing and no additional billing.
No real send was made in this execution.

The independent `a9-host-assisted` executable currently admits ONLY metadata
preflight, through `run_a9_host.py` and the unchanged FIX-1 subreaper. It intentionally
has NO live send entry point or boolean override. Authentication was unavailable
and pricing/paid-fallback enforcement could not be verified, so linking a live
sender would add an unverified path. SDK send exists only in `tests/host_assisted.rs`
against `fixtures/a9_host_cli.py`, a local synthetic RPC peer without network.

Reproduction (metadata only; no inference, no login/logout):

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 RUSTC=/usr/bin/rustc RUSTDOC=/usr/bin/rustdoc \
  /usr/bin/cargo build --offline --locked --bin a9-host-assisted --manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml
python3 experiments/lr-10a-sdk-runtime/run_a9_host.py /absolute/path/to/pinned/native/copilot --output /tmp/a9-host-new-preflight.json
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir /tmp/a9-host-new-tests
```

Output paths must be fresh; no historical evidence is overwritten. The preflight
returns nonzero when blocked. Normal stored-account/gh resolution is enabled;
credential environment overrides are removed. The POC does not acquire tokens,
read configs or inspect personal sessions. HOME and existing session-bus/runtime
paths are available to the native CLI under the explicit host-assisted acceptance;
this is not a least-privilege authentication boundary. No graphical variables,
GUI activation, login API, proxy, keychain copy or production integration is added.

Preflight creates private 0700 workspace/log directories in /tmp and a 0400
synthetic fixture; readonly mode is not kernel isolation from its owner. It does
not create sessions in personal storage. `base_directory` is absent for metadata
because COPILOT_HOME can affect auth selection. Private authenticated session state
is NOT proven; a future send must establish it before create/send. No global config
is automatically restored; only safe stat is compared before and after.

The stable directory `~/.local/state/narys` is prepared at 0700 with anchored
no-follow directory opens. No attempt file is created in a blocked preflight.
The `claim_attempt` primitive atomically creates `lr10a-a9-host-attempt.json` at
0600, writes ATTEMPTED and fsyncs file/directory BEFORE send. Any existing entry,
including corrupt/truncated/symlink entries, prevents a new claim. A crash or
failed sync leaves the entry in place. Only synthetic directories exercised the
claim/send sequence here. The state lies outside the repository and is not Git data.

A future financial admission must establish actual billing units/pricing,
overage denial and absence of paid fallback; request quota alone and post-call soft
session limits are insufficient. The current live executable always fails closed
on that missing contract even if a fixture supplies an apparent zero multiplier.
One SDK send is not necessarily one provider model call or billable request.

See [current report](../../docs/LR-10-LATEST-EXECUTION-REPORT.md),
[real preflight](evidence/a9-host-real-preflight.json),
[owned regressions](evidence/a9-host-owned-tests.json) and
[verification](evidence/a9-host-verification.json). No LR-10B advancement, no isolated
boundary claim, no real conversation persistence or billing delta is implied.

## A9-FIX-1 — headless authentication diagnosis, metadata ONLY

The [diagnostic](diagnose_a9_auth.py) uses the SAME metadata-only binary and
unchanged FIX-1 ownership harness. It compares the A9 baseline with restoration
of validated existing PATH directories and individual removal of session-bus /
runtime context. Optional XDG/GH_CONFIG_DIR restoration is included only when
those non-secret variables actually exist. No full environment is copied.
Token names are recorded as presence booleans; their values are never acquired.
PATH values and personal context paths are not written into evidence.

```sh
python3 experiments/lr-10a-sdk-runtime/diagnose_a9_auth.py /absolute/path/to/pinned/native/copilot --output /tmp/new-a9-auth-matrix.json
```

Run only in the accepted HOST_ASSISTED_NOT_SANDBOX profile. Each variant starts
a fresh Client/CLI and calls auth status, models and quota; no session method.
The existing private marker directory must already be safe. Diagnosis performs
only anchored metadata checks; it never prepares the directory or claims a file.
An existing marker blocks the diagnostic. Fresh evidence is reserved at 0600.
Config is stat-only. Changes, cleanup failure or timeout stop the matrix without
restoration/fallback. A non-activating NameHasOwner query checks that the existing
credential service is already owned before CLI startup; it never starts/unlocks
that service. GUI presence or changed service metadata blocks verification.

Observed final result: **AUTH_NOT_RECOVERED** in all four real variants. Current
catalog/quota remain unavailable/unknown. The cause is unproved; daemon/socket
presence does not establish unlocked credentials or entitlement. There is no
validated wrapper correction, token workaround or financial bypass. Optional
context variables were absent and were not invented. The historical FIX-2 broad
mount is not reproduced. No gh auth status/token, login, keyring inspection or
credential export was necessary. Tests with synthetic auth booleans prove the
projection/fail-closed behavior, never the real user's authentication.

See [final matrix](evidence/a9-fix-1-real-auth-matrix.json) and
[contract comparison](evidence/a9-fix-1-contract.json). A9 real remains NOT_RUN,
with zero real inference/session operations and no ATTEMPTED entry. Authentication
recovery, financial admission and real A9 remain three distinct gates.

## A9-FIX-2 — GUI-present metadata profile, independent of headless

[diagnose_a9_gui_auth.py](diagnose_a9_gui_auth.py) requires an already running
GNOME session, an already owned Secret Service and the login collection's boolean
`Locked=false`. It never starts/unlocks a service. Its final property query uses
`--auto-start=no --allow-interactive-authorization=no`. No Items, secrets, config
contents or account identifiers are inspected. The host profile is explicitly
**HOST_ASSISTED_WITH_GUI_NOT_SANDBOX**. Display variables are not passed to the
CLI; the existing user's session bus/runtime context and HOME are passed, with
PATH=/usr/bin. Token overrides, arbitrary inherited variables and COPILOT_HOME
are excluded. This does not establish headless credential availability.

The [independent candidate manifest](runtime-gui-candidate.json) pins the existing
native CLI 1.0.95 by SHA256. The historical 1.0.91 SHA in run_a9_host.py is unchanged.
The old native path now contains different bytes; the POC did not install or
download them. Current package manifests say 1.0.89 while native/RPC say 1.0.95;
package metadata is not executable identity. No verified old image was available
at that path for a simultaneous comparison. Version and GUI/login changes cannot
be individually credited with recovery.

The metadata binary remains unchanged: no executable send/session path, no marker
claim, no explicit token, no real tools, built-in MCPs disabled. DenyAll/zero tools
session policy is regression-tested synthetically; no real session was created
to exercise it. This profile does not prove system-wide plugin/MCP isolation.

**Do not automatically rerun the real diagnostic after this result.** The one
SDK metadata invocation returned authenticated=true, models=[auto] and quota,
but config.json inode/timestamps changed during that window. Attribution is
unknown; only stat metadata was observed. No contents were inspected/restored
and no further real probe was run. Integrity verification and financial admission
remain BLOCKED. Audit this change before further native probes or future A9.

The executed source is preserved in commit
`42756450fb8b942fdc2f7ec42b6ece364f3e5c73`; its original
[metadata evidence](evidence/a9-fix-2-gui-sdk-metadata.json) is unchanged. The top
SDK_GUI field there says BLOCKED because that source conflated integrity and auth;
the nested real preflight.auth.authenticated is true. The final classifier separates
these observations and is tested only synthetically after the incident. The added
busctl flags and required no-auto-update check also have synthetic-only retests.

Reproduction specification (metadata only; subject to the integrity audit above):
`python3 experiments/lr-10a-sdk-runtime/diagnose_a9_gui_auth.py /absolute/path/to/manifest-pinned/native/copilot --output /tmp/fresh-gui-metadata.json`.
Fresh evidence uses O_EXCL/O_NOFOLLOW and 0600. Identity/help inspection uses a
disposable HOME; authenticated metadata uses normal host credential resolution.
Both run inside the unchanged ownership harness with bounded timeouts. Existing
attempt entries block execution and are never claimed or read. Unknown versions,
missing flags, locked/unknown storage, unsafe cleanup or changed configuration
never trigger fallback. SDK authentication observation is not financial admission.

See [inventory](evidence/a9-fix-2-contract-inventory.json),
[verification](evidence/a9-fix-2-verification.json) and
[final synthetic/kernel regressions](evidence/a9-fix-2-final-validation/a9-host-owned-tests.json).
SDK_AUTHENTICATED_WITH_GUI is observed; SDK_AUTHENTICATED_HEADLESS is NOT_PROVEN.
FINANCIAL_ADMISSION remains BLOCKED; A9_REAL_INFERENCE remains NOT_RUN.

## A9-FIX-3 — offline configuration integrity review

The [integrity contract](CONFIG-INTEGRITY.md) classifies the historical metadata
change independently of SDK auth. [Offline replay](review_a9_config.py) is pinned
to the unchanged A9-FIX-2 artifact; it never launches CLI or queries personal
config. Stable metadata is not content equivalence, and a known synthetic revision
is not approval of a host write. Writer attribution remains INCONCLUSIVE and real
config integrity BLOCKED. No personal recovery, permission change, lock, token copy
or COPILOT_HOME change occurred. New probes are PENDING_USER_AUTHORIZATION;
live wrappers remain intact. SDK_GUI auth is OBSERVED_REAL_PASS historically,
headless NOT_PROVEN, finance BLOCKED, A9 NOT_RUN, attempt not claimed.

## A9-FIX-4 — operation-scoped metadata gates

The independent [metadata contract](METADATA-CONFIRMATION.md) accepts safe
metadata observations with drift without declaring a legitimate write or byte
integrity. Historical evidence/classification and all older wrappers remain
unchanged. Sessions, finance, actions and headless never inherit this acceptance.
The one-shot status/auth confirmation was **NOT_RUN_CONCURRENCY_UNVERIFIED**:
three same-UID process inspections were unavailable; a later stat-only scan
recorded three executable-metadata EACCES failures without PID correlation. No runtime or
credential-service probe was launched, and no retry or external process signal
was used. A9 marker absent/unclaimed; auth from FIX-2 remains historical only.
