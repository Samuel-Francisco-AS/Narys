#!/usr/bin/env python3
"""Reproduce FIX-2 without inference; reuse the unchanged FIX-1 ownership worker.

The Rust binary enforces a read-only bwrap view. Only config stat is inspected,
never its contents; a blocked real-history gate intentionally returns nonzero.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path

from measure import measure


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def config_stat():
    try:
        stat = (Path.home() / ".copilot/config.json").stat()
        return {"inode": stat.st_ino, "bytes": stat.st_size,
                "mtime_ns": stat.st_mtime_ns, "ctime_ns": stat.st_ctime_ns}
    except OSError:
        return {"state": "unavailable"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("probe", choices=["sessions", "sessions-existing-auth"])
    parser.add_argument("cli", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    directory = Path(__file__).resolve().parent
    binary = directory / "target/debug/narys-lr10a-poc"
    before = config_stat()
    result, code = measure(binary, args.probe, args.cli.resolve(), 90)
    after = config_stat()
    result["fix2_provenance"] = {
        "phase": "LR-10A FIX-2", "observed_at": datetime.now(timezone.utc).isoformat(),
        "binary_sha256": digest(binary), "cli_sha256": digest(args.cli.resolve()),
        "sources_sha256": {name: digest(directory / name)
                           for name in ["src/main.rs", "src/lib.rs", "src/persistence.rs",
                                        "Cargo.toml", "Cargo.lock", "measure.py"]},
        "config_stat_before": before, "config_stat_after": after,
        "config_stat_unchanged": before == after,
        "config_contents_read": False, "inference_requests": 0,
        "driver_exit_code": code,
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"evidence": str(args.output), "cleanup_complete": result.get("cleanup_complete"),
                      "config_stat_unchanged": before == after, "driver_exit_code": code}))
    return code if before == after else 2


if __name__ == "__main__":
    raise SystemExit(main())
