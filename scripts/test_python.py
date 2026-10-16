#!/usr/bin/env python3
"""Test the packaged Python API against the owned, disposable Firebird fixture."""
from __future__ import annotations

import os
import subprocess
import sys

from build_vendor import ROOT
from firebird_fixture import state, status


def main() -> None:
    fixture, live = state(), status()
    if not live["running"] or any(live[key] != fixture[key] for key in ("host", "port", "database", "image")):
        raise SystemExit("Run scripts/firebird_fixture.py start first")
    env = {key: value for key, value in os.environ.items() if not key.startswith(("FB_", "FIREBIRD", "DYLD_", "PYTHONPATH"))}
    env.update(
        FBFAST_TEST_PORT=str(live["port"]),
        FBFAST_TEST_PASSWORD=fixture["password"],
        FBFAST_TEST_ADMIN_PASSWORD=fixture["admin_password"],
    )
    subprocess.run([sys.executable, "-m", "pytest", "tests/integration", "-q",
                    "--junitxml", str(ROOT / ".tools/python-integration.xml"),
                    "-o", "junit_family=xunit1"], cwd=ROOT, env=env, check=True, timeout=240)


if __name__ == "__main__":
    main()
