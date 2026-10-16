#!/usr/bin/env python3
"""Compare complete outputs on the owned fixture; never accepts an external DSN."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
from importlib.metadata import version
import itertools
import json
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from firebird_fixture import sql, state, status  # noqa: E402
from adapters.comparison import ADAPTERS, FORMATS  # noqa: E402
from cases import CASES  # noqa: E402
from comparison_measure import clean_environment, measure  # noqa: E402
from comparison_report import write_report  # noqa: E402


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepare", action="store_true", help="Create only the disposable benchmark table")
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--cases", choices=CASES, nargs="+", default=["narrow100k", "wide16"])
    parser.add_argument("--adapters", choices=ADAPTERS, nargs="+",
                        default=["fbfast", "firebird-driver", "firebird-driver-reset", "fdb-arm64"])
    parser.add_argument("--formats", choices=FORMATS, nargs="+", default=list(FORMATS))
    parser.add_argument("--fetch-sizes", type=int, nargs="+", default=[5000])
    args = parser.parse_args()
    if not 1 <= args.samples <= 100 or any(n not in (256, 1024, 5000, 16384) for n in args.fetch_sizes):
        parser.error("samples must be 1..100; fetch sizes: 256, 1024, 5000, 16384")
    fixture, live = state(), status()
    if not live["running"] or any(live[k] != fixture[k] for k in ("host", "port", "database", "image")):
        raise SystemExit("Start and revalidate the owned disposable fixture first")
    fixture_source = Path(__file__).with_name("fixture.sql")
    if args.prepare:
        exists = sql("SELECT COUNT(*) FROM FBFAST_BENCH;", fixture["admin_password"])
        if exists.returncode:
            result = sql(fixture_source.read_text(), fixture["admin_password"])
            if result.returncode:
                raise SystemExit("Disposable benchmark fixture initialization failed")
        print("Disposable benchmark table ready", flush=True)
    config = {k: fixture[k] for k in ("host", "port", "database", "user", "password", "role")}
    artifact = json.loads((ROOT / "dist/vendor/fbfast/_build_info.json").read_text())
    if artifact["profile"] != "release":
        raise SystemExit("Benchmarks require a release artifact")
    directory = ROOT / "benchmarks/results" / datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ-comparison")
    directory.mkdir(parents=True)
    manifest = {
        "build_id": artifact["build_id"], "artifact": artifact, "command": sys.argv,
        "python": sys.version, "platform": platform.platform(), "machine": platform.machine(),
        "processor": subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip(),
        "server_image": live["image"], "versions": {p: version(p) for p in (
            "firebird-driver", "fdb", "orjson", "psutil", "pyarrow", "polars")},
        "fixture_sha256": hashlib.sha256(fixture_source.read_bytes()).hexdigest(),
        "harness_sha256": {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
                           for p in sorted((ROOT / "benchmarks").rglob("*.py"))},
        "cases": {name: {"sql": CASES[name].sql, "params": CASES[name].params,
                         "rows": CASES[name].rows} for name in args.cases},
        "limitations": [
            "TCP localhost into Docker/OrbStack VM; no injected latency or LAN/WAN claim.",
            "One attachment per worker; sequential samples; no concurrency/queue/tail acceptance yet.",
            "One full query warmup, then one sample per fresh process. Server data cache remains warm.",
            "Adapter order reverses each round. Failed runs stay in raw samples; no best-run filtering.",
            "Full equality/types/NULL/multiplicity and checksums validated AFTER timer and RSS sampling.",
            "Client CPU covers the query; cgroup CPU spans the measured barrier plus docker counter overhead.",
            "Server memory current/peak are whole-container counters; peak is lifetime, not query-only.",
            "RSS samples every 5 ms can miss short peaks; ru_maxrss also records imports and full warmup.",
            "Range queries use ORDER BY PK; scans use ROWS and validate a sorted multiset afterward. SQL server work is included.",
            "Arrow values/types match; SQL metadata is additionally preserved only by fbfast.",
            "FDB uses the bundled client's default crypt policy; fixture requires encryption for all adapters.",
            "fdb-arm64 includes an explicit variadic ctypes ABI workaround; unmodified fdb is separate.",
            "BLOB/modern types, slow streams, controlled RTT and parallel extraction require separate runs.",
        ],
    }
    samples = []
    # Save the exact source used, including a dirty checkout. Workers execute
    # this frozen snapshot so later editing cannot mix harness versions in a run.
    harness_files = list(sorted((ROOT / "benchmarks").rglob("*.py")))
    archive = directory / "harness.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
        for path in harness_files:
            bundle.write(path, str(path.relative_to(ROOT / "benchmarks")))
        bundle.write(fixture_source, "fixture.sql")
        bundle.write(ROOT / "benchmarks/requirements.txt", "requirements.txt")
    manifest["harness_archive_sha256"] = hashlib.sha256(archive.read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix="fbfast-comparison-") as temporary:
        package = Path(temporary) / "fbfast"
        shutil.copytree(ROOT / "dist/vendor/fbfast", package)
        client = package / artifact["client_library"]
        if hashlib.sha256(client.read_bytes()).hexdigest() != artifact["client_sha256"]:
            raise SystemExit("Delivered client hash mismatch")
        env = clean_environment(config, package)
        frozen = Path(temporary) / "harness"
        with zipfile.ZipFile(archive) as bundle:
            bundle.extractall(frozen)
        env["FBFAST_BENCH_WORKER"] = str(frozen / "comparison_worker.py")
        for round_ in range(args.samples):
            adapters = args.adapters if round_ % 2 == 0 else list(reversed(args.adapters))
            for case, kind, size, adapter in itertools.product(args.cases, args.formats, args.fetch_sizes, adapters):
                sample = dict(round=round_, case=case, format=kind, fetch_size=size, adapter=adapter)
                try:
                    sample.update(measure((adapter, kind, case, size), env, live["container"]))
                except (RuntimeError, ValueError, TimeoutError, subprocess.SubprocessError) as error:
                    sample["error"] = str(error)
                samples.append(sample)
                write_report(directory, manifest, samples)
                detail = sample.get("error", f"{sample.get('seconds', 0):.6f}s")
                print(f"{round_ + 1}/{args.samples} {case} {kind} {adapter} fetch={size}: {detail}", flush=True)
    print(directory.relative_to(ROOT), flush=True)
    if any("error" in sample for sample in samples):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
