#!/usr/bin/env python3
"""Compare native JSON with fbfast dict rows + stdlib JSON on the owned fixture."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import gc
import hashlib
import json
import os
from pathlib import Path
import platform
import resource
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

from build_vendor import ROOT
from firebird_fixture import state, status

SQL = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT A.I AS A, B.I AS B FROM N A CROSS JOIN N B"


def worker(mode: str, samples: int) -> None:
    sys.path.insert(0, os.environ["FBFAST_BENCH_PACKAGE"])
    import fbfast

    config = fbfast.FirebirdConfig(**json.loads(os.environ["FBFAST_BENCH_CONFIG"]))
    durations = []
    with fbfast.Pool(config) as pool:
        def fetch() -> bytes:
            if mode == "native":
                return pool.fetch_json(SQL)
            return json.dumps(pool.fetch_all(SQL, as_dict=True), ensure_ascii=False,
                              separators=(",", ":")).encode()
        warmup = fetch()
        expected = hashlib.sha256(warmup).hexdigest()
        size = len(warmup)
        del warmup
        for _ in range(samples):
            gc.collect()
            start = time.perf_counter()
            output = fetch()
            durations.append(time.perf_counter() - start)
            assert len(output) == size and hashlib.sha256(output).hexdigest() == expected
            del output
    rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    if sys.platform != "darwin":
        rss *= 1024
    print(json.dumps({"build_id": fbfast.build_info()["build_id"], "mode": mode,
                      "rows": 1_000_000, "output_bytes": size, "sha256": expected,
                      "seconds": durations, "median_seconds": statistics.median(durations),
                      "process_peak_rss_bytes": rss}))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--worker", choices=("native", "rows"))
    args = parser.parse_args()
    if not 1 <= args.samples <= 10:
        parser.error("samples must be between 1 and 10")
    if args.worker:
        worker(args.worker, args.samples)
        return
    fixture, live = state(), status()
    if not live["running"] or any(live[key] != fixture[key] for key in ("host", "port", "database", "image")):
        raise SystemExit("Start the owned disposable fixture first")
    config = dict(host=live["host"], port=live["port"], database=live["database"],
                  user="FBFAST_READER", password=fixture["password"], role="FBFAST_READONLY",
                  pool_size=1, min_idle=1, env_file=False, query_timeout=30)
    clean = {key: value for key, value in os.environ.items()
             if not key.startswith(("FB_", "FIREBIRD", "DYLD_", "PYTHONPATH"))}
    clean["FBFAST_BENCH_CONFIG"] = json.dumps(config)
    results = {}
    with tempfile.TemporaryDirectory(prefix="fbfast-json-benchmark-") as temporary:
        shutil.copytree(ROOT / "dist/vendor/fbfast", Path(temporary) / "fbfast")
        clean["FBFAST_BENCH_PACKAGE"] = temporary
        for mode in ("native", "rows"):
            output = subprocess.check_output([sys.executable, __file__, "--worker", mode,
                                               "--samples", str(args.samples)],
                                              env=clean, text=True, timeout=180)
            results[mode] = json.loads(output)
    for key in ("build_id", "sha256", "output_bytes", "rows"):
        assert results["native"][key] == results["rows"][key]
    report = {"environment": {"system": platform.system(), "machine": platform.machine(),
                               "python": platform.python_version(), "server_image": live["image"]},
              "scope": "Same fbfast pool/client/SQL; native writer versus dict rows plus stdlib JSON. Warm attach/cache; one unmeasured warmup each; separate processes; no parsing in timed region. RSS includes runtime, allocator and warmup. Not a firebird-driver/fdb baseline.",
              "results": results}
    directory = ROOT / "benchmarks/results"
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / (datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-json.json")
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(path.relative_to(ROOT))


if __name__ == "__main__":
    main()
