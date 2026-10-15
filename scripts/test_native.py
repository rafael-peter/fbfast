#!/usr/bin/env python3
"""Run opt-in native integration tests only against the owned disposable fixture."""

from __future__ import annotations

from pathlib import Path
import shutil
import subprocess
import tempfile

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
        FBFAST_TEST_DSN=f"{live['host']}/{live['port']}:{live['database']}",
        FBFAST_TEST_PASSWORD=fixture["password"],
        FBFAST_TEST_PORT=str(live["port"]),
        PYO3_PYTHON=str(ROOT / ".venv/bin/python"),
    )
    # fbclient may write firebird.log on transport faults. Tests must not mutate
    # the delivered runtime or weaken the artifact's exact-inventory verifier.
    with tempfile.TemporaryDirectory(prefix="fbfast-native-runtime-") as temporary:
        runtime = Path(temporary) / "runtime"
        shutil.copytree(ROOT / "dist/vendor/fbfast/_runtime", runtime, symlinks=True)
        env["FBFAST_TEST_CLIENT"] = str(runtime / "lib/libfbclient.dylib")
        run_tests(env)


def run_tests(env: dict[str, str]) -> None:
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
