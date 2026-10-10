# A9-FIX-3 — experimental configuration integrity contract

Offline, metadata-only investigation. No Copilot launch, private content read,
restore, locks or permissions change. SDK_GUI auth remains a historical real
observation. No new authentication, session, quota or inference test occurs.

## Classification and admission

[config_integrity.py](config_integrity.py) separates observation, writer provenance,
content verification and admission. All classifications retain operational_admission
BLOCKED. They cannot open financial or inference gates.

| Classification | Meaning | Integrity verification |
| --- | --- | --- |
| OBSERVATIONALLY_STABLE | Selected fields agree at two observation points | INCONCLUSIVE for content/authorization |
| METADATA_CHANGE_UNATTRIBUTED | Selected fields differ; writer/semantics unproved | BLOCKED |
| LEGITIMATE_CHANGE_PROVEN | Fixed synthetic revision; owned replacement FD matches path and known bytes verified | PASS_SYNTHETIC_CONTRACT only |
| METADATA_UNAVAILABLE | Missing path or metadata access denied | BLOCKED |
| VERIFICATION_FAILED | Invalid schema/types, unsafe path/type, open race or I/O failure | BLOCKED |
| INCONCLUSIVE | Observation interrupted | INCONCLUSIVE, admission still BLOCKED |

There is **no implemented real-runtime legitimate-write contract**. The only proven
revision owns a newly created 0700 directory and 0600 file with fixed non-sensitive
data. It accepts no external path or legitimacy flag. The replacement FD is retained
across rename; identity and the fixed synthetic revision are checked through it.
This is a cooperative fixture, not same-UID adversarial isolation, external writer
attestation or permission to modify Copilot configuration.

The classifier has no auth/allow-change/tolerance/event parameters. Auth projection
is independent; finance stays BLOCKED and A9 NOT_RUN even for a legitimate synthetic
update. A real legitimate-write contract would need a pinned implementation/operation
specification, proven invocation provenance, allowed transitions and reviewed failure
handling. Documentation that a file is automatically managed does not prove a
particular write, safe contents or absence of another writer.

## Metadata guarantees and limits

snapshot(path) uses directory O_PATH/O_DIRECTORY/O_NOFOLLOW opens and anchored
stat with leaf follow_symlinks=false. Parent/leaf symlinks, traversal, relative
paths and non-regular files fail. Parent device/inode is checked across open.
The leaf is never opened for reading. Only inode/device/size/timestamps/mode/owner/
group/link count and fixed error reasons are returned, without path or exception
prose. Missing/access-denied, interruption and verification failure are distinct.
Unreadable contents can still have accessible stat metadata; readability is not inferred.

This is not continuous observation. Directory detachment after anchoring, changes
between snapshots, ABA/inode reuse, filesystem/clock resolution and concurrency
remain limits. Equal size/new inode does not prove equal bytes. Equal metadata
is not cryptographic equivalence, absence of hidden changes, auth preservation or
approved semantics. A later stable pair cannot legitimize an earlier unknown change.
Write/rename events would not establish writer PID or content safety; no watcher
was implemented here.

The historical adapter accepts only the four published fields. Replay coverage
is historical_four_fields, with no retrospective claims about device, mode,
symlinks or content. Old run_fix2.config_stat follows symlinks and merges OSError
into unavailable. It and all live wrappers remain **unchanged**: this FIX does not
silently substitute offline classification for their existing admission.

## Reproduction without runtime or credentials

```sh
python3 experiments/lr-10a-sdk-runtime/review_a9_config.py --output /tmp/fresh-offline-config-review.json
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir /tmp/fresh-config-synthetic-tests
```

[review_a9_config.py](review_a9_config.py) reads only a SHA256-pinned Git artifact
from A9-FIX-2. No CLI/config-path argument, subprocess or service call. Output is
fresh O_EXCL/O_NOFOLLOW 0600. Exit0 means completed offline review, not integrity
PASS. Changed historical bytes abort before report creation. Neither command queries
personal config or claims the real marker. Tests write/delete their own temp files.

The unchanged FIX-1 runner uses offline/locked Cargo, skip download, jobs=2 and
Python tests. No Copilot executable starts; simulated sends use local protocol
peers only. The concurrent writer uses a pipe barrier and wait/reap, no PID signals.

See [tests](tests/test_config_integrity.py),
[historical replay](evidence/a9-fix-3-historical-review.json),
[synthetic cases](evidence/a9-fix-3-synthetic-cases.json),
[static investigation](evidence/a9-fix-3-static-investigation.json) and
[SDK excerpts](evidence/a9-fix-3-sdk-contract-excerpts.txt).

## Recovery and future observation: PENDING_USER_AUTHORIZATION

No personal restore/rename/copy/delete/read-only mount, chmod, lock, token store or
login action is performed. Unknown changes stop optional work; preserve sanitized
evidence and complete bounded cleanup. Historical SDK auth remains true for its
timestamp; current validity is not retested.

SDK1.0.17 base_directory exports COPILOT_HOME for auth/sessions/telemetry together.
Changing it is not proven separation of existing auth from mutable state. cwd/log-dir
separate output, not global state. session_fs virtualizes session storage, not
global config writes. No such options changed; credential copies/proxies and wide
host mounts remain excluded.

A potential future observation is one pinned Client start/getStatus/auth.getStatus/
stop, no sessions/model/quota/send/retry. Proposed protection: stat-only anchored
phase snapshots, private cwd/logs, current allowlist and unchanged ownership harness.
Before authorization, independently review expected writes and method limits; human
confirmation of an idle Copilot context must not be replaced by POC signals to
external processes. Missing metadata, unsafe path/service/marker state, timeout,
unsafe cleanup or first unclassified mutation must stop optional phases and enter
bounded shutdown/evidence collection, with no restore.

Risk: normal credential resolution may update shared state, and shutdown may write
too. Phase correlation still does not prove writer/semantics. This proposal alone
cannot satisfy the real legitimate-write gate; it is not an automatic rerun. State
PENDING_USER_AUTHORIZATION, not a permission request or executable real-probe procedure.

Headless remains unproved. Product operation without GUI needs a separately verified
credential/state contract; GUI metadata and synthetic legitimate updates are
insufficient. Isolated auth/network/worker containment and production are unchanged.
