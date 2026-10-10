# A9-FIX-4 — operation-scoped metadata confirmation

This experimental profile is HOST_ASSISTED_WITH_GUI_NOT_SANDBOX. It neither
provides filesystem isolation nor establishes headless authentication. Historical
FIX-2 authentication and unknown configuration mutation remain unchanged.

## Separate operations and guarantees

| Operation | Admission in this candidate |
| --- | --- |
| METADATA_READ_ONLY | Status/auth observations can be accepted with safe metadata access, service and cleanup. Drift alone does not reject an authentic SDK response. |
| SESSION_OPERATIONS | BLOCKED: private authenticated state, deny policy and interference still need operational evidence. |
| INFERENCE | BLOCKED: model, units, maximum cost and no-paid-fallback enforcement remain unverified. |
| AGENT_ACTIONS | BLOCKED here; authentication cannot grant Narys execution authority. |
| HEADLESS_OPERATION | NOT_PROVEN; GUI authentication cannot establish operation without GNOME. |

[metadata_policy.py](metadata_policy.py) reports auth, structural access, drift,
writer attribution and content equivalence separately. A synthetic classifier
never emits PASS_REAL or creates a capability. `admission` is an observational
scope check, not an executable dispatcher. Flags, fixture claims and positive auth
cannot authorize session/send/finance/actions. The old classifier and historical
replay preserve METADATA_CHANGE_UNATTRIBUTED/integrity BLOCKED.

`config_integrity.snapshot(access_uid=...)` adds opt-in structural checks to the
existing anchored stat observer. It requires a regular single-link file, current
UID and mode0600; ancestors must be directories owned by root/current UID without
group/world write. Only root-owned sticky /tmp is excepted for private fixture
roots. Parent identities/access are checked across O_PATH/no-follow opens; no leaf
content is opened. Missing/denied/interrupted/unsafe observations block dependent
verification. Default historical snapshot behavior is retained.

Structural PASS_METADATA_ACCESS is a point observation, not byte integrity,
writer attestation or same-UID race containment. Safe replacement can have a new
inode/size/time and different contents. No content of personal config is examined
or hashed, and no config lock/chmod/read-only mount/restore/copy occurs. Stable
metadata after a historical mutation cannot legitimize the prior write.

## Limited SDK protocol and ownership

[Rust flow](src/metadata_confirmation.rs) only starts a Client, calls status.get
and auth.getStatus, then bounded shutdown. SDK start also performs its standard
protocol handshake; shutdown uses runtime.shutdown/EOF. It has no models/quota,
session or send calls, no retry and no live tool use. Version/protocol mismatch or
failed structural observation stops optional RPCs. Auth booleans survive a later
verification failure; verification still blocks on service/cleanup/structure.

[Executable](src/bin/a9-metadata-confirm.rs) invokes the fixed trusted Python
stat helper at before_start/after_start/after_status/after_auth/after_shutdown.
These observer subprocesses are harness instrumentation, not agent tools. Their
stderr is discarded and only allowlisted JSON is returned. The unchanged client
options select the normal host credential mechanism, private cwd/logs, no token
override, LogLevel::None and --disable-builtin-mcps. No session is created, so
DenyAll/zero tools/session MCP/skills/hooks policies are retained but not exercised
against a real agent. No MCP/tool execution is inferred from mock successes.

The unchanged FIX-1 subreaper worker owns the tree, uses pidfds/PID-start-time and
verifies kernel child exhaustion. Bounded shutdown and recovery are distinct.
Unexpected supervisor death, adversarial reparenting/namespaces, same-UID actors
and uninterruptible tasks remain uncontained; no production supervisor is added.

## One-shot preflight, not a rerun procedure

[Driver](confirm_a9_metadata.py) requires final offline evidence matching source
and binary hashes, the independent native1.0.95 pin, safe absent A9 marker,
structural access, already available unlocked credential service/GUI and two
non-invasive concurrency observations. It clears environment to the existing
non-secret allowlist. No credential values, cmdline, environ, login/logout or
service changes are used. Native version/help are not executed in this FIX;
historical pin plus permitted status response would establish identity.

Concurrency inspects same-UID comm and executable inode/device, not arguments.
Known Copilot, Node/Bun ambiguity or inaccessible entries block. It cannot exclude
disguised other images, activity after the snapshot, or historical writers. This
conservative rule may block on unrelated protected processes; they must not be
terminated or presumed safe. No proc/ptrace policy is changed to obtain approval.

The fixed fresh evidence path prevents driver reruns. If the binary is reached,
it atomically reserves a separate `evidence/a9-fix-4-runtime-reservation.json`
before any Client start, with no alternate path/flag. This is NOT the A9 attempt
marker. A crash/timeout leaves the reservation; it is not automatic retry authority.
Do not delete/override reservations or execute this already completed diagnostic
again. A future real confirmation requires a newly reviewed, separately authorized
execution. The present run stopped before Client start because three same-UID
process inspections were unavailable; service queries and runtime reservation
were therefore not reached. A later independent stat-only scan recorded three
executable-metadata EACCES failures, without PID correlation or writer attribution.

## Offline reproduction and evidence

```sh
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir /tmp/fresh-fix4-synthetic-regressions
```

This command uses cached offline/locked dependencies, jobs2 and local protocol
peers; it never starts the actual Copilot. It must not be confused with the real
confirmation driver. Tests cover stable/atomic/size/time/concurrent drift, unsafe
access, unavailable metadata, positive/negative auth, failed cleanup/service,
scope escalation, version mismatch and the exact restricted SDK method set.

See [offline verification](evidence/a9-fix-4-offline-verification.json),
[synthetic cases](evidence/a9-fix-4-synthetic-cases.json),
[blocked preflight](evidence/a9-fix-4-metadata-confirmation.json),
[final safety](evidence/a9-fix-4-final-safety.json) and
[owned regressions](evidence/a9-fix-4-final-regressions/a9-host-owned-tests.json).
No real SDK/CLI invocation or inference occurred in this FIX. Historical GUI auth
remains observed; current auth, catalog/quota, financial admission and genuine
session persistence are not promoted by this policy or the offline tests.
