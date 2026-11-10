#!/usr/bin/env python3
"""Run every examples/*.py against the owned disposable Firebird fixture.

Like scripts/test_python.py, this refuses to run unless this checkout's fixture
is running, and it tests the built dist/vendor package (rebuild after source
changes). ``--from-env`` instead keeps FB_* variables already set by a caller
that owns its own disposable server (for example a CI smoke job).
"""
from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
import time

from build_vendor import ROOT
from firebird_fixture import state, status

EXAMPLES = ROOT / "examples"
VENDOR = ROOT / "dist/vendor"


def fixture_environment() -> dict[str, str]:
    fixture, live = state(), status()
    if not live["running"] or any(live[key] != fixture[key]
                                  for key in ("host", "port", "database", "image")):
        raise SystemExit("Run scripts/firebird_fixture.py start first")
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(("FB_", "FIREBIRD", "DYLD_"))}
    env.update(
        FB_HOST=str(live["host"]),
        FB_PORT=str(live["port"]),
        FB_DATABASE=str(live["database"]),
        FB_USER="FBFAST_READER",
        FB_PASSWORD=str(fixture["password"]),
        FB_ROLE="FBFAST_READONLY",
    )
    return env


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--from-env", action="store_true",
                        help="use the FB_* variables already set instead of the local fixture")
    parser.add_argument("names", nargs="*", help="example file names (default: all)")
    args = parser.parse_args()
    if not (VENDOR / "fbfast/__init__.py").is_file():
        raise SystemExit("Build dist/vendor first: scripts/build_vendor.py --release")
    env = dict(os.environ) if args.from_env else fixture_environment()
    env["PYTHONPATH"] = os.pathsep.join(filter(None, [str(VENDOR), env.get("PYTHONPATH")]))
    scripts = sorted(EXAMPLES.glob("*.py"))
    if args.names:
        scripts = [path for path in scripts if path.name in args.names or path.stem in args.names]
    if not scripts:
        raise SystemExit("No examples selected")
    failures = []
    # An empty working directory keeps a developer's .env out of the examples.
    with tempfile.TemporaryDirectory(prefix="fbfast-examples-") as cwd:
        for script in scripts:
            print(f"--- {script.name}", flush=True)
            started = time.monotonic()
            try:
                result = subprocess.run([sys.executable, str(script)], cwd=cwd, env=env,
                                        timeout=300)
                code = result.returncode
            except subprocess.TimeoutExpired:
                code = -1
            print(f"--- {script.name}: exit {code} in {time.monotonic() - started:.1f} s",
                  flush=True)
            if code:
                failures.append(script.name)
    if failures:
        raise SystemExit(f"Examples failed: {', '.join(failures)}")
    print(f"All {len(scripts)} examples passed")


if __name__ == "__main__":
    main()
