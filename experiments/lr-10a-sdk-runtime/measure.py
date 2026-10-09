#!/usr/bin/env python3
"""Linux-only experimental measurement and cleanup, never a production supervisor.

A private, single-threaded subreaper owns exactly one invocation. Kernel child
ownership, pidfds and waitpid(ECHILD) prove cleanup independently of /proc metrics.
Only stat/children are read, never cmdline/environ. CPU sampling is a lower bound;
summed RSS may count shared pages twice. No SDK graceful shutdown is inferred.
"""
import argparse
import ctypes
import json
import math
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import threading
import time

POLL_SECONDS = 0.05
CLEANUP_SECONDS = 2.0
MAX_OUTPUT = 4 * 1024 * 1024
# Linux __WALL: do not exclude children created with a non-SIGCHLD clone signal.
WAIT_ALL_CHILDREN = 0x40000000


def identity(pid):
    data = Path(f"/proc/{pid}/stat").read_text()
    fields = data[data.rfind(")") + 2:].split()
    return {
        "ppid": int(fields[1]), "pgrp": int(fields[2]),
        "start_ticks": int(fields[19]),
        "cpu_ticks": int(fields[11]) + int(fields[12]),
        "rss_bytes": int(fields[21]) * os.sysconf("SC_PAGE_SIZE"),
        "state": fields[0],
    }


def snapshot():
    rows = {}
    for entry in Path("/proc").iterdir():
        if entry.name.isdigit():
            try:
                rows[int(entry.name)] = identity(int(entry.name))
            except (OSError, ValueError, IndexError):
                pass
    return rows


def children():
    # The private worker has one thread and never starts unrelated children.
    return [int(pid) for pid in Path(
        f"/proc/self/task/{os.getpid()}/children"
    ).read_text().split()]


class AttributionError(Exception):
    """Safe code only; never include raw process content or exception prose."""


def owned_handle(pid):
    """Pin a kernel child; validate ownership/identity on both sides of pidfd_open.

    No child is reaped during this operation. With SIGCHLD at SIG_DFL its PID
    cannot be reused while it is our unreaped child, even if it exits here.
    pidfd_send_signal subsequently targets that exact process, never a new PID.
    """
    first = identity(pid)
    if first["ppid"] != os.getpid():
        raise AttributionError("external_not_attributed")
    fd = os.pidfd_open(pid)
    try:
        second = identity(pid)
        if second["ppid"] != os.getpid():
            raise AttributionError("ownership_changed")
        if first["start_ticks"] != second["start_ticks"]:
            raise AttributionError("identity_changed")
    except BaseException:
        os.close(fd)
        raise
    return fd, second


def after_launch(proc):
    """No-op synchronization seam for deterministic fixtures, not a CLI option."""


def sampled_tree(root, rows, handles):
    # Metrics only. Process group membership alone is never ownership evidence.
    owned = {pid: rows[pid] for pid, (start, _) in handles.items()
             if pid in rows and rows[pid]["start_ticks"] == start}
    if root in rows and root in handles:
        owned[root] = rows[root]
    while True:
        more = {pid: row for pid, row in rows.items()
                if row["ppid"] in owned and pid not in owned}
        if not more:
            return owned
        owned.update(more)


def inconclusive(code):
    return {"schema_version": 2, "cleanup_status": "inconclusive",
            "cleanup_complete": False, "ownership_errors": [code],
            "sdk_shutdown_verified": False}, 1


def _worker_measure(binary, probe, cli, deadline_seconds):
    signal.signal(signal.SIGCHLD, signal.SIG_DFL)
    if not hasattr(os, "pidfd_open") or not hasattr(signal, "pidfd_send_signal"):
        return inconclusive("pidfd_unavailable")
    if ctypes.CDLL(None, use_errno=True).prctl(36, 1, 0, 0, 0) != 0:
        return inconclusive("subreaper_unavailable")
    # Exercise pidfd availability before starting the controlled invocation.
    try:
        fd = os.pidfd_open(os.getpid())
        try:
            signal.pidfd_send_signal(fd, 0)
        finally:
            os.close(fd)
    except OSError:
        return inconclusive("pidfd_unavailable")

    before = snapshot()
    started = time.monotonic()
    handles, attributed, sampled = {}, {}, {}
    errors, recovery_signals, survivors = set(), set(), set()
    reaped = 0
    exhausted = False
    timed_out = False
    peak_rss = peak_processes = 0

    def collect(root):
        try:
            pids = children()
        except (OSError, ValueError):
            errors.add("child_inventory_unavailable")
            return
        for pid in pids:
            if pid in handles:
                continue
            try:
                fd, row = owned_handle(pid)
                handles[pid] = row["start_ticks"], fd
                attributed[(pid, row["start_ticks"])] = {
                    "pid": pid, "start_ticks": row["start_ticks"],
                    "basis": "launched_root" if pid == root else "subreaper_child",
                }
            except FileNotFoundError:
                # Without an identity, do not signal or silently promote an
                # empty final known set to success, even if a later retry works.
                errors.add("identity_unavailable")
            except AttributionError as exc:
                errors.add(str(exc))
            except (OSError, ValueError, IndexError):
                errors.add("identity_unavailable")

    def recover(pid):
        start, fd = handles[pid]
        try:
            signal.pidfd_send_signal(fd, signal.SIGKILL)
            recovery_signals.add((pid, start))
        except ProcessLookupError:
            pass  # Already exited, still must reap / obtain ECHILD.
        except OSError:
            errors.add("recovery_signal_failed")

    with tempfile.TemporaryFile() as capture:
        try:
            proc = subprocess.Popen(
                [str(binary), probe, str(cli)], stdin=subprocess.DEVNULL,
                stdout=capture, stderr=subprocess.DEVNULL, start_new_session=True,
                env={**{k: v for k, v in os.environ.items()
                        if k not in ("DISPLAY", "WAYLAND_DISPLAY")},
                     "NARYS_LR10A_OWNED_HARNESS": "1"},
            )
        except OSError:
            return inconclusive("launch_failed")
        try:
            collect(proc.pid)
            after_launch(proc)
            while proc.poll() is None:
                collect(proc.pid)
                rows = snapshot()
                owned = sampled_tree(proc.pid, rows, handles)
                peak_rss = max(peak_rss, sum(r["rss_bytes"] for r in owned.values()))
                peak_processes = max(peak_processes, len(owned))
                for pid, row in owned.items():
                    sampled[(pid, row["start_ticks"])] = row["cpu_ticks"]
                if time.monotonic() - started >= deadline_seconds:
                    timed_out = True
                    break
                time.sleep(POLL_SECONDS)
        except Exception:
            # Do not expose exception text; still attempt attributable cleanup.
            errors.add("measurement_failed")

        # Popen.poll/wait may already have reaped the root. Drop its PID index
        # immediately; a later child may legitimately receive that numeric PID.
        if proc.returncode is not None:
            pinned = handles.pop(proc.pid, None)
            if pinned:
                os.close(pinned[1])
        cleanup_started = time.monotonic()
        # Reaping the root triggers adoption regardless of setsid()/pgrp and
        # sampling history. Iterate: killing/reaping an adopted ancestor can
        # reveal previously invisible grandchildren on the next iteration.
        while time.monotonic() - cleanup_started < CLEANUP_SECONDS:
            collect(proc.pid)
            for pid, (start, _) in list(handles.items()):
                try:
                    row = identity(pid)
                    if row["start_ticks"] != start or row["ppid"] != os.getpid():
                        errors.add("pinned_identity_changed")
                        continue
                    if row["state"] != "Z":
                        if pid != proc.pid:
                            survivors.add((pid, start))
                        recover(pid)
                except FileNotFoundError:
                    pass
                except (OSError, ValueError, IndexError):
                    errors.add("recovery_identity_unavailable")
            while True:
                try:
                    pid, status = os.waitpid(-1, os.WNOHANG | WAIT_ALL_CHILDREN)
                except ChildProcessError:
                    # Cross-check the kernel child inventory, not the sampled
                    # known set. Neither missing metrics nor wait filters pass.
                    try:
                        exhausted = not children()
                    except (OSError, ValueError):
                        errors.add("final_child_inventory_unavailable")
                    break
                except OSError:
                    errors.add("wait_failed")
                    break
                if pid == 0:
                    break
                if pid == proc.pid:
                    proc.returncode = os.waitstatus_to_exitcode(status)
                else:
                    reaped += 1
                pinned = handles.pop(pid, None)
                if pinned:
                    os.close(pinned[1])
            if exhausted:
                break
            time.sleep(0.01)

        remaining = []
        for pid, (start, fd) in handles.items():
            try:
                row = identity(pid)
                if row["start_ticks"] == start:
                    remaining.append(pid)
            except FileNotFoundError:
                pass
            except (OSError, ValueError, IndexError):
                errors.add("final_identity_unavailable")
            os.close(fd)
        after = snapshot()
        capture.seek(0, os.SEEK_END)
        total_output = capture.tell()
        capture.seek(0)
        out = capture.read(MAX_OUTPUT)

    recognized = False
    try:
        report = json.loads(out)
        recognized = isinstance(report, dict)
    except (ValueError, UnicodeDecodeError):
        pass
    if not recognized:
        report = {"output": "unrecognized", "stdout_bytes": len(out)}
    complete = exhausted and not remaining and not errors
    if not exhausted or remaining:
        status = "recovery_incomplete"
    elif errors:
        status = "inconclusive"
    elif timed_out:
        status = "timeout_recovered"
    elif recovery_signals:
        status = "descendants_recovered"
    else:
        status = "graceful_no_recovery"
    result = {
        "schema_version": 2, "probe": probe,
        "sample_kind": "fresh_process_warm_os_caches", "headless": True,
        "poll_interval_ms": round(POLL_SECONDS * 1000),
        "exit_code": proc.returncode, "stdout_truncated": total_output > len(out),
        "timed_out": timed_out,
        "operation_outcome": ("timeout" if timed_out else "main_exit_zero"
                              if proc.returncode == 0 else "main_exit_nonzero"
                              if proc.returncode is not None else "unknown"),
        "wall_ms": round((time.monotonic() - started) * 1000, 2),
        "cleanup_ms": round((time.monotonic() - cleanup_started) * 1000, 2),
        "peak_process_tree_rss_bytes": peak_rss, "peak_owned_processes": peak_processes,
        "sampled_cpu_seconds_lower_bound": sum(sampled.values()) / os.sysconf("SC_CLK_TCK"),
        "system_processes_before": len(before), "system_processes_after": len(after),
        "observed_owned_processes": len(sampled),
        "attributed_processes": list(attributed.values()),
        "unsampled_attributed_descendants": sum(
            key not in sampled and row["basis"] == "subreaper_child"
            for key, row in attributed.items()),
        "owned_survivors_before_recovery": sorted(pid for pid, _ in survivors),
        "owned_survivors_after_recovery": sorted(remaining),
        "harness_reaped_descendants": reaped,
        "harness_recovery_signals": len(recovery_signals),
        "ownership_errors": sorted(errors), "kernel_children_exhausted": exhausted,
        "cleanup_status": status, "cleanup_complete": complete,
        "ownership_scope": "private_single_invocation_subreaper",
        "external_process_policy": "not_attributed_never_signalled",
        "sdk_shutdown_verified": False, "sdk_report": report,
    }
    failed = (not complete or recovery_signals or survivors or timed_out
              or proc.returncode != 0 or not recognized or total_output > len(out))
    return result, int(bool(failed))


def measure(binary, probe, cli, deadline_seconds=90):
    """Isolate adoption/reaping from the caller and any of its external children."""
    if (not math.isfinite(deadline_seconds) or deadline_seconds <= 0
            or threading.active_count() != 1):
        return inconclusive("invalid_deadline_or_threaded_caller")
    if signal.getsignal(signal.SIGCHLD) != signal.SIG_DFL:
        return inconclusive("caller_sigchld_not_default")
    with tempfile.TemporaryFile() as channel:
        try:
            worker = os.fork()
        except OSError:
            return inconclusive("ownership_worker_launch_failed")
        if worker == 0:
            try:
                result = _worker_measure(binary, probe, cli, deadline_seconds)
                channel.write(json.dumps(result).encode())
                channel.flush()
                os._exit(0)
            except BaseException:
                os._exit(1)
        try:
            _, status = os.waitpid(worker, 0)
        except (OSError, ChildProcessError):
            return inconclusive("ownership_worker_wait_unavailable")
        if os.waitstatus_to_exitcode(status) != 0:
            return inconclusive("ownership_worker_failed")
        channel.seek(0)
        try:
            result, code = json.load(channel)
            return result, code
        except (ValueError, UnicodeDecodeError):
            return inconclusive("ownership_worker_report_invalid")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("probe", choices=[
        "metadata", "metadata-existing-auth", "sessions", "sessions-existing-auth",
    ])
    parser.add_argument("cli", type=Path)
    parser.add_argument("--binary", type=Path,
                        default=Path(__file__).parent / "target/debug/narys-lr10a-poc")
    args = parser.parse_args()
    result, code = measure(args.binary.resolve(), args.probe, args.cli.resolve())
    print(json.dumps(result, indent=2))
    raise SystemExit(code)
