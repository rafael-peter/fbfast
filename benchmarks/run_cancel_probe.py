#!/usr/bin/env python3
"""Measure caller interruption and actual slot release on the disposable fixture."""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[1]
LONG = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) FROM N A CROSS JOIN N B CROSS JOIN N C"


def samples() -> None:
    sys.path.insert(0, sys.argv[2])
    import fbfast

    config = fbfast.FirebirdConfig(**json.loads(os.environ["FBFAST_PROBE_CONFIG"]))
    output = []
    with fbfast.Pool(config) as pool:
        connection = pool.fetch_val("SELECT CURRENT_CONNECTION FROM RDB$DATABASE")
        for index in range(10):
            sent = []

            def interrupt():
                sent.append(time.perf_counter_ns())
                os.kill(os.getpid(), signal.SIGINT)

            timer = threading.Timer(0.1, interrupt)
            timer.start()
            try:
                pool.fetch_val(LONG, timeout=None)
                raise AssertionError("Heavy query completed without interruption")
            except KeyboardInterrupt:
                returned = time.perf_counter_ns()
            finally:
                timer.join()
            until = time.monotonic() + 3
            while True:
                stats = pool.stats()
                if not sum(stats[key] for key in ("opening", "leased", "cleaning", "closing")):
                    released = time.perf_counter_ns()
                    break
                if time.monotonic() >= until:
                    raise AssertionError("Native operation did not release its slot")
                time.sleep(0.001)
            assert stats["native_cancels"] == index + 1
            assert stats["discarded"] == index + 1
            replacement = pool.fetch_val("SELECT CURRENT_CONNECTION FROM RDB$DATABASE")
            assert replacement != connection
            connection = replacement
            output.append({"sample": index, "caller_ms": (returned - sent[0]) / 1e6,
                           "slot_released_ms": (released - sent[0]) / 1e6})
    assert pool.stats()["control_threads"] == 0
    print(json.dumps({"build_id": fbfast.build_info()["build_id"], "samples": output}))


def main() -> None:
    fixture = json.loads((ROOT / ".tools/firebird-fixture.json").read_text())
    live = json.loads(subprocess.check_output(
        [sys.executable, str(ROOT / "scripts/firebird_fixture.py"), "status"], text=True))
    if not live["running"] or any(live[key] != fixture[key] for key in ("host", "port", "database", "image")):
        raise RuntimeError("Start and revalidate the owned disposable container")
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(("FB_", "FIREBIRD", "DYLD_", "PYTHON"))}
    env["FBFAST_PROBE_CONFIG"] = json.dumps(dict(
        host=live["host"], port=live["port"], database=live["database"],
        user="FBFAST_READER", password=fixture["password"], role="FBFAST_READONLY",
        pool_size=1, min_idle=1, env_file=False))
    with tempfile.TemporaryDirectory(prefix="fbfast-cancel-") as directory:
        shutil.copytree(ROOT / "dist/vendor/fbfast", Path(directory) / "fbfast")
        result = subprocess.run([sys.executable, "-E", "-s", __file__, "--samples", directory],
                                env=env, text=True, capture_output=True, check=True, timeout=40)
    report = json.loads(result.stdout)
    report.update(scenario="SIGINT during aggregate query; timeout=None", server_image=live["image"],
                  correctness="every cancelled attachment retired; replacement query succeeds",
                  limitations=["10 local TCP/Docker samples; diagnostic, not a throughput comparison",
                               "slot release observed by 1 ms polling; not a network-partition guarantee"])
    destination = ROOT / "benchmarks/results" / datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ-cancel.json")
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(report, indent=2) + "\n")
    print(destination.relative_to(ROOT))


if __name__ == "__main__":
    samples() if len(sys.argv) > 1 and sys.argv[1] == "--samples" else main()
