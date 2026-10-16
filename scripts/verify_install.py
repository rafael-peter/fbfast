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
import asyncio, importlib.util, json, pathlib, os, fbfast
from decimal import Decimal
from datetime import datetime, time, timezone
root = pathlib.Path(fbfast.__file__).resolve().parent
info = fbfast.client_info()
assert pathlib.Path(info['path']).is_relative_to(root)
assert pathlib.Path(info['root']).is_relative_to(root)
report = {'package': str(root), 'build_id': fbfast.build_info()['build_id'], 'client_version': info['version']}
if 'FBFAST_ACCEPTANCE_CONFIG' in os.environ:
    pool = fbfast.init_pool_sync(fbfast.FirebirdConfig(**json.loads(os.environ['FBFAST_ACCEPTANCE_CONFIG'])))
    try:
        assert fbfast.get_pool() is pool
        assert fbfast.fetch_all('SELECT ID, LABEL FROM FBFAST_TYPES ORDER BY ID') == [(1, 'Olá 日本語'), (2, None)]
        assert fbfast.fetch_one('SELECT MON$READ_ONLY, MON$ISOLATION_MODE FROM MON$TRANSACTIONS WHERE MON$TRANSACTION_ID = CURRENT_TRANSACTION') == (1, 4)
        report['real_query_and_readonly_tpb'] = True
        report['named_pool_delegates'] = True
        assert json.loads(fbfast.fetch_json('SELECT ID, LABEL FROM FBFAST_TYPES ORDER BY ID')) == [{'ID': 1, 'LABEL': 'Olá 日本語'}, {'ID': 2, 'LABEL': None}]
        with fbfast.stream_json('SELECT ID FROM FBFAST_TYPES ORDER BY ID', format='array') as chunks:
            assert json.loads(b''.join(chunks)) == [{'ID': 1}, {'ID': 2}]
        report['native_json'] = True
        assert fbfast.fetch_one('SELECT TEXT_BLOB, BINARY_BLOB FROM FBFAST_TYPES WHERE ID=1') == ('texto em BLOB', bytes.fromhex('00ff80'))
        assert json.loads(fbfast.fetch_json('SELECT TEXT_BLOB FROM FBFAST_TYPES WHERE ID=1')) == [{'TEXT_BLOB': 'texto em BLOB'}]
        report['native_blobs'] = True
        assert fbfast.fetch_val('SELECT CAST(? AS DECFLOAT(34)) FROM RDB$DATABASE', [Decimal('1.2300')]).as_tuple() == Decimal('1.2300').as_tuple()
        report['native_decfloat'] = True
        stamp, clock = fbfast.fetch_one('SELECT STAMP, CLOCK FROM FBFAST_ZONED WHERE ID=2')
        assert stamp.astimezone(timezone.utc) == datetime(2024, 11, 3, 5, 30, tzinfo=timezone.utc)
        assert stamp.tzinfo.key == 'America/New_York'
        assert clock == fbfast.TimeWithZone(time(17), 'America/New_York')
        returned = fbfast.fetch_val('SELECT CAST(? AS TIMESTAMP WITH TIME ZONE) FROM RDB$DATABASE', [stamp])
        assert returned == stamp and returned.fold == stamp.fold
        report['native_time_zones'] = True
        assert importlib.util.find_spec('pyarrow') is None
        assert importlib.util.find_spec('polars') is None
        with fbfast.stream_arrow('SELECT ID FROM FBFAST_TYPES ORDER BY ID', batch_rows=1) as stream:
            assert stream.schema.names == ['ID']
            batches = list(stream)
        assert [batch.num_rows for batch in batches] == [1, 1]
        assert all(type(capsule).__name__ == 'PyCapsule' for capsule in batches[0].__arrow_c_array__())
        async def arrow_batches():
            async with pool.stream_arrow_async('SELECT ID FROM FBFAST_TYPES ORDER BY ID') as stream:
                return [batch async for batch in stream]
        assert sum(batch.num_rows for batch in asyncio.run(arrow_batches())) == 2
        report['arrow_streams_without_extras'] = True
    finally:
        fbfast.close_pool_sync()
    assert pool.stats()['closed'] == 1
print(json.dumps(report))
"""
    return json.loads(run([str(consumer / ".venv/bin/python"), "-E", "-s", "-c", code], consumer, clean))


def verify_analytics(consumer: Path, env: dict[str, str], config: dict[str, object], uv: str) -> dict[str, object]:
    """Install optional consumers separately, proving Polars works without PyArrow."""
    clean = {key: value for key, value in env.items() if not key.startswith(("FB_", "FBFAST_", "FIREBIRD", "DYLD_", "PYTHON", "CARGO", "RUST"))}
    clean["PATH"] = "/usr/bin:/bin"
    clean["FBFAST_ACCEPTANCE_CONFIG"] = json.dumps(config)
    code = """
import importlib.util, json, os, fbfast
from decimal import Decimal
consumer = os.environ['FBFAST_ANALYTICAL_CONSUMER']
if consumer == 'polars':
    assert importlib.util.find_spec('pyarrow') is None
with fbfast.Pool(fbfast.FirebirdConfig(**json.loads(os.environ['FBFAST_ACCEPTANCE_CONFIG']))) as pool:
    if consumer == 'polars':
        result = pool.fetch_polars('SELECT ID, AMOUNT, LABEL FROM FBFAST_TYPES ORDER BY ID')
        assert result.to_dicts()[0] == dict(ID=1, AMOUNT=Decimal('-12345678.9012'), LABEL='Olá 日本語')
        wide = pool.fetch_polars('SELECT WIDE_INTEGER FROM FBFAST_TYPES WHERE ID=1', unsupported='string')
        assert wide[0, 0] == str(2**127 - 1)
        import polars as pl
        with pool.stream_arrow('SELECT ID FROM FBFAST_TYPES ORDER BY ID', batch_rows=1) as stream:
            streamed = pl.from_arrow(stream, rechunk=False).struct.unnest()
        assert streamed['ID'].to_list() == [1, 2]
    else:
        result = pool.fetch_arrow('SELECT ID, AMOUNT, LABEL FROM FBFAST_TYPES ORDER BY ID')
        assert result.to_pylist()[0] == dict(ID=1, AMOUNT=Decimal('-12345678.9012'), LABEL='Olá 日本語')
        wide = pool.fetch_arrow('SELECT WIDE_INTEGER FROM FBFAST_TYPES WHERE ID=1')
        assert wide[0][0].as_py() == Decimal(2**127 - 1)
        import pyarrow as pa
        with pool.stream_arrow('SELECT ID FROM FBFAST_TYPES ORDER BY ID', batch_rows=1) as stream:
            streamed = pa.table(stream)
        assert streamed[0].to_pylist() == [1, 2]
    assert pool.fetch_val('SELECT CAST(1.25 AS DOUBLE PRECISION) FROM RDB$DATABASE') == 1.25
assert len(result) == 2
print(json.dumps({'consumer': consumer, 'build_id': fbfast.build_info()['build_id'], 'rows': len(result)}))
"""
    report = {}
    for kind, dependency in (("polars", "polars>=2"), ("arrow", "pyarrow>=26")):
        run([uv, "add", dependency], consumer, env)
        clean["FBFAST_ANALYTICAL_CONSUMER"] = kind
        report[kind] = json.loads(run([str(consumer / ".venv/bin/python"), "-E", "-s", "-c", code], consumer, clean))
    return report


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
            if config:
                report[kind]["analytics"] = verify_analytics(consumer, env, config, uv)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
