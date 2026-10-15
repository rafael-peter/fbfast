#!/usr/bin/env python3
"""Run opt-in native integration tests only against the owned disposable fixture."""

from __future__ import annotations

import subprocess

from build_vendor import ROOT, tool_environment
from firebird_fixture import state, status


def main() -> None:
    fixture = state()
    live = status()
    if not live["running"] or any(
        live[key] != fixture[key] for key in ("host", "port", "database", "image")
    ):
        raise SystemExit("Run scripts/firebird_fixture.py start first")
    env = tool_environment(ROOT / "unused")
    env.pop("FBFAST_CLIENT_ROOT", None)
    env.update(
        FBFAST_TEST_CLIENT=str(ROOT / "dist/vendor/fbfast/_runtime/lib/libfbclient.dylib"),
        FBFAST_TEST_DSN=f"{live['host']}/{live['port']}:{live['database']}",
        FBFAST_TEST_PASSWORD=fixture["password"],
        FBFAST_TEST_PORT=str(live["port"]),
        PYO3_PYTHON=str(ROOT / ".venv/bin/python"),
    )
    subprocess.run(
        ["cargo", "test", "-p", "fbfast-native", "--test", "lifecycle", "--", "--ignored"],
        cwd=ROOT,
        env=env,
        check=True,
    )
    subprocess.run(
        ["cargo", "test", "-p", "fbfast-core", "--tests", "--", "--ignored"],
        cwd=ROOT,
        env=env,
        check=True,
        timeout=120,
    )


if __name__ == "__main__":
    main()
