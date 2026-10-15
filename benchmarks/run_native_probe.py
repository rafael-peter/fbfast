#!/usr/bin/env python3
"""Run the native lifecycle probe only against this checkout's disposable fixture."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--client-root", type=Path, default=Path("/Library/Frameworks/Firebird.framework")
    )
    args = parser.parse_args()
    fixture = json.loads((ROOT / ".tools/firebird-fixture.json").read_text())
    live = json.loads(
        subprocess.check_output(
            [sys.executable, str(ROOT / "scripts/firebird_fixture.py"), "status"], text=True
        )
    )
    if not live["running"] or any(
        live[key] != fixture[key] for key in ("host", "port", "database", "image")
    ):
        raise RuntimeError(
            "Fixture state changed; start and revalidate the owned disposable container"
        )
    package = ROOT / "dist/vendor/fbfast"
    manifest = json.loads((package / "_build_info.json").read_text())
    library = package / manifest["client_library"]
    if hashlib.sha256(library.read_bytes()).hexdigest() != manifest["client_sha256"]:
        raise RuntimeError("Probe client hash differs from the delivered artifact")
    binary = ROOT / ".tools/native-probe"
    command = [
        "clang++",
        "-std=c++17",
        "-O3",
        "-isystem",
        str(args.client_root / "Headers"),
        str(ROOT / "benchmarks/adapters/native_probe.cpp"),
        "-o",
        str(binary),
    ]
    subprocess.run(command, check=True)
    env = {
        **os.environ,
        "FBFAST_PROBE_DSN": f"{fixture['host']}/{fixture['port']}:{fixture['database']}",
        "FBFAST_PROBE_USER": fixture["user"],
        "FBFAST_PROBE_PASSWORD": fixture["password"],
        "FBFAST_PROBE_ROLE": fixture["role"],
    }
    result = subprocess.run(
        [str(binary), str(library)], env=env, text=True, capture_output=True, check=True, timeout=60
    )
    destination = (
        ROOT / "benchmarks/results" / datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ-native")
    )
    destination.mkdir(parents=True)
    (destination / "samples.csv").write_text(result.stdout)
    (destination / "manifest.json").write_text(
        json.dumps(
            {
                "scenario": "native-monitor-readonly-reset-cache-probe",
                "artifact": manifest,
                "server_image": fixture["image"],
                "platform": platform.platform(),
                "command": "python benchmarks/run_native_probe.py",
                "limitations": [
                    "sequential cache modes; diagnostic, not comparative performance acceptance",
                    "native two-integer result only; not Python/Arrow/JSON materialization",
                    "local TCP into Docker VM; no controlled WAN latency",
                    "30 samples per mode, no tail percentiles",
                ],
                "correctness": {
                    "mon_read_only": 1,
                    "mon_isolation_mode": 4,
                    "verified_each_sample": True,
                },
            },
            indent=2,
        )
        + "\n"
    )
    print(destination.relative_to(ROOT))


if __name__ == "__main__":
    main()
