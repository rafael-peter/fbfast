#!/usr/bin/env python3
"""Prove uv wheel and Git-source installs in temporary consumer projects."""
from __future__ import annotations

import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from build_vendor import ROOT, tool_environment
from firebird_fixture import state, status


def run(args: list[str], cwd: Path, env: dict[str, str]) -> str:
    return subprocess.check_output(args, cwd=cwd, env=env, text=True, timeout=180)


def verify(consumer: Path, env: dict[str, str], config: dict[str, object] | None) -> dict[str, object]:
    clean = {key: value for key, value in env.items() if not key.startswith(("FB_", "FBFAST_", "FIREBIRD", "DYLD_", "PYTHON", "CARGO", "RUST"))}
    clean["PATH"] = "/usr/bin:/bin"
    if config:
        clean["FBFAST_ACCEPTANCE_CONFIG"] = json.dumps(config)
    code = """
import json, pathlib, os, fbfast
root = pathlib.Path(fbfast.__file__).resolve().parent
info = fbfast.client_info()
assert pathlib.Path(info['path']).is_relative_to(root)
assert pathlib.Path(info['root']).is_relative_to(root)
report = {'package': str(root), 'build_id': fbfast.build_info()['build_id'], 'client_version': info['version']}
if 'FBFAST_ACCEPTANCE_CONFIG' in os.environ:
    with fbfast.Pool(fbfast.FirebirdConfig(**json.loads(os.environ['FBFAST_ACCEPTANCE_CONFIG']))) as pool:
        assert pool.fetch_all('SELECT ID, LABEL FROM FBFAST_TYPES ORDER BY ID') == [(1, 'Olá 日本語'), (2, None)]
        assert pool.fetch_one('SELECT MON$READ_ONLY, MON$ISOLATION_MODE FROM MON$TRANSACTIONS WHERE MON$TRANSACTION_ID = CURRENT_TRANSACTION') == (1, 4)
        report['real_query_and_readonly_tpb'] = True
print(json.dumps(report))
"""
    return json.loads(run([str(consumer / ".venv/bin/python"), "-E", "-s", "-c", code], consumer, clean))


def main() -> None:
    uv = shutil.which("uv")
    if not uv:
        raise SystemExit("uv is required for installation acceptance")
    env = tool_environment(Path("/Library/Frameworks/Firebird.framework"))
    env["CARGO_TARGET_DIR"] = str(ROOT / "target")
    wheel = ROOT / "dist/wheels/fbfast-0.1.0-cp314-cp314-macosx_11_0_arm64.whl"
    report = {}
    config = None
    if (ROOT / ".tools/firebird-fixture.json").is_file():
        fixture, live = state(), status()
        if live["running"] and all(live[key] == fixture[key] for key in ("host", "port", "database", "image")):
            config = dict(host=live["host"], port=live["port"], database=live["database"],
                          user="FBFAST_READER", password=fixture["password"], role="FBFAST_READONLY",
                          pool_size=1, min_idle=1, env_file=False)
    with tempfile.TemporaryDirectory(prefix="fbfast-install-") as temporary:
        base = Path(temporary)
        source = base / "source"
        source.mkdir()
        # Explicit allowlist excludes credentials, virtualenvs and build outputs.
        for name in ("Cargo.toml", "Cargo.lock", "pyproject.toml", "rust-toolchain.toml", "build_backend.py", "README.md"):
            shutil.copy2(ROOT / name, source / name)
        for name in ("crates", "python", "scripts", "third_party"):
            shutil.copytree(ROOT / name, source / name, ignore=shutil.ignore_patterns("__pycache__", "*.so", "_runtime", "_licenses", "_build_info.json"))
        run(["git", "init", "-q"], source, env)
        run(["git", "add", "."], source, env)
        run(["git", "-c", "user.name=fbfast local verification", "-c", "user.email=local@invalid", "commit", "-qm", "Disposable source-install fixture"], source, env)
        revision = run(["git", "rev-parse", "HEAD"], source, env).strip()
        for kind, dependency in (("wheel", str(wheel)), ("git", f"git+{source.as_uri()}@{revision}")):
            consumer = base / kind
            run([uv, "init", "--bare", "--vcs", "none", "--python", sys.executable, str(consumer)], base, env)
            run([uv, "add", dependency], consumer, env)
            assert (consumer / "uv.lock").is_file()
            report[kind] = verify(consumer, env, config)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
