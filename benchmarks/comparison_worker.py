"""One fresh process, one unmeasured full warmup, one measured complete result."""
from __future__ import annotations

import gc
import json
import os
from pathlib import Path
import resource
import sys
import time


def emit(value):
    print(json.dumps(value), flush=True)


def barrier():
    if sys.stdin.readline().strip() != "continue":
        raise RuntimeError("Benchmark controller disconnected")


def main():
    config = json.loads(os.environ["FBFAST_BENCH_CONFIG"])
    package = Path(os.environ["FBFAST_BENCH_PACKAGE"])
    sys.path.insert(0, str(package.parent))
    # All drivers load the exact delivered client and its runtime configuration.
    os.environ["FIREBIRD"] = str(package / "_runtime")
    manifest = json.loads((package / "_build_info.json").read_text())
    library = str(package / manifest["client_library"])
    adapter, kind, name, size = sys.argv[1:]
    from adapters.comparison import Baseline, Fbfast, extras
    from cases import CASES, result_rows, validate
    case = CASES[name]
    imported = time.perf_counter()
    extras(kind)
    if adapter == "fbfast":
        import fbfast
        if fbfast.build_info()["build_id"] != manifest["build_id"]:
            raise RuntimeError("Manifest/native build mismatch")
    elif adapter.startswith("firebird-driver"):
        import firebird.driver  # noqa: F401
    else:
        import fdb  # noqa: F401
    import_seconds = time.perf_counter() - imported
    attached = time.perf_counter()
    session = (Fbfast(config, library, case, kind, int(size)) if adapter == "fbfast"
               else Baseline(config, library, case, kind, int(size), adapter))
    attach_seconds = time.perf_counter() - attached
    try:
        warmup, _ = session.fetch()
        del warmup
        plan = session.plan()
        gc.collect()
        emit({"phase": "ready"})
        barrier()
        cpu = time.process_time()
        start = time.perf_counter()
        output, stages = session.fetch()
        elapsed = time.perf_counter() - start
        cpu = time.process_time() - cpu
        rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        if sys.platform != "darwin":
            rss *= 1024
        metrics = {"phase": "measured", "seconds": elapsed, "client_cpu_seconds": cpu,
                   "process_peak_before_validation_bytes": rss,
                   "import_seconds": import_seconds, "attach_prepare_seconds": attach_seconds,
                   "stages": stages, "build_id": manifest["build_id"], "plan": plan}
        if kind == "json":
            import hashlib
            metrics.update(output_bytes=len(output), output_sha256=hashlib.sha256(output).hexdigest())
        elif kind == "arrow":
            metrics["arrow_buffer_bytes"] = output.get_total_buffer_size()
            metrics["schema"] = str(output.schema.remove_metadata())
        elif kind == "polars":
            metrics["polars_estimated_bytes"] = output.estimated_size()
            metrics["schema"] = str(output.schema)
        emit(metrics)
        barrier()
        correctness = validate(case, result_rows(output, kind, case.names))
        emit({"phase": "validated", **correctness})
    finally:
        session.close()


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        import traceback
        detail = str(error)
        config = json.loads(os.environ["FBFAST_BENCH_CONFIG"])
        for key in ("password", "database", "user", "role", "host"):
            detail = detail.replace(config[key], f"<{key}>")
        frames = [{"file": Path(f.filename).name, "line": f.lineno, "function": f.name}
                  for f in traceback.extract_tb(error.__traceback__)]
        emit({"phase": "error", "error_type": type(error).__name__,
              "detail": detail, "frames": frames})
        raise SystemExit(1) from None
