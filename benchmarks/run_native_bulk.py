#!/usr/bin/env python3
"""Diagnostic C++ int32 materialization, same client/SQL/TPB/reset as comparison."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from firebird_fixture import state, status  # noqa: E402


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--scan", action="store_true")
    args = parser.parse_args()
    if not 1 <= args.samples <= 30 or sys.platform != "darwin":
        parser.error("This diagnostic currently supports macOS only, with 1..30 samples")
    fixture, live = state(), status()
    if not live["running"] or any(live[k] != fixture[k] for k in ("host", "port", "database", "image")):
        raise SystemExit("Owned disposable fixture is not live")
    source = ROOT / "benchmarks/adapters/native_bulk.cpp"
    binary = ROOT / ".tools/native-bulk"
    command = ["clang++", "-std=c++17", "-O3", "-isystem",
               "/Library/Frameworks/Firebird.framework/Headers", str(source), "-o", str(binary)]
    subprocess.run(command, check=True)
    artifact = json.loads((ROOT / "dist/vendor/fbfast/_build_info.json").read_text())
    directory = ROOT / "benchmarks/results" / datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ-native-bulk")
    directory.mkdir(parents=True)
    shutil.copy2(source, directory / source.name)
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("FB_", "FIREBIRD", "DYLD_", "PYTHONPATH", "ISC_"))}
    env.update(FBFAST_PROBE_DSN=f"{fixture['host']}/{fixture['port']}:{fixture['database']}",
               FBFAST_PROBE_USER=fixture["user"], FBFAST_PROBE_PASSWORD=fixture["password"],
               FBFAST_PROBE_ROLE=fixture["role"])
    results = []
    with tempfile.TemporaryDirectory(prefix="fbfast-native-bulk-") as temporary:
        package = Path(temporary) / "fbfast"
        shutil.copytree(ROOT / "dist/vendor/fbfast", package)
        client = package / artifact["client_library"]
        if hashlib.sha256(client.read_bytes()).hexdigest() != artifact["client_sha256"]:
            raise SystemExit("Delivered client hash mismatch")
        env["FIREBIRD"] = str(package / "_runtime")
        for rows in (100_000, 1_000_000):
            output = subprocess.check_output([str(binary), str(client), str(rows), str(args.samples),
                                              "scan" if args.scan else "range"],
                                             env=env, text=True, timeout=180)
            samples = [json.loads(line) for line in output.splitlines()]
            if len(samples) != args.samples or any(s["rows"] != rows for s in samples):
                raise RuntimeError("Incomplete native probe")
            results.extend(samples)
            print(f"native raw int32 {rows} rows: {[s['seconds'] for s in samples]}", flush=True)
    (directory / "samples.json").write_text(json.dumps(results, indent=2) + "\n")
    (directory / "manifest.json").write_text(json.dumps({
        "artifact": artifact, "build_id": artifact["build_id"], "server_image": live["image"],
        "scan": args.scan,
        "compiler": subprocess.check_output(["clang++", "--version"], text=True), "command": command,
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "limitations": ["Diagnostic only: vector<array<int32,2>>, not Python/JSON/Arrow/Polars output.",
                        "Single attachment, same parameterized ordered query, TPB and reset as comparison.",
                        "One full warmup, then samples sequentially in one process; RSS includes warmup.",
                        "Complete count/value/order/NULL validation and FNV1a64 after timer/RSS.",
                        "Local Docker VM TCP; no server/network split and no tail percentiles."],
    }, indent=2) + "\n")
    print(directory.relative_to(ROOT))


if __name__ == "__main__":
    main()
