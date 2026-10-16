#!/usr/bin/env python3
"""Exercise an installed wheel with its own runtime against the disposable SDK server."""
from __future__ import annotations

import argparse
import asyncio
from decimal import Decimal
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def consumer():
    import fbfast
    from pydantic import BaseModel

    class ModelRow(BaseModel):
        ID: int
        LABEL: str | None
        AMOUNT: Decimal | None

    package = Path(fbfast.__file__).parent.resolve()
    client = fbfast.client_info()
    # samefile handles Windows verbatim paths and filesystem path aliases.
    for key in ("path", "root"):
        path = Path(client[key])
        if not any(parent.exists() and parent.samefile(package) for parent in [path, *path.parents]):
            raise RuntimeError(f"Client {key} escaped the installed wheel")
    config = fbfast.FirebirdConfig(**json.loads(os.environ["FBFAST_CI_CONFIG"]),
                                   env_file=False, pool_size=1, min_idle=1)
    sql = "SELECT ID, LABEL, AMOUNT, D, STAMP FROM WHEEL_SMOKE ORDER BY ID"
    with fbfast.Pool(config) as pool:
        rows = pool.fetch_all(sql)
        if len(rows) != 2 or rows[0][:4] != (1, "Olá 日本語", Decimal("123.4567"), 1.25):
            raise RuntimeError("Scalar/Unicode/Decimal/float mismatch in installed wheel")
        if rows[0][4].isoformat() != "2024-02-29T12:34:56.123400+05:45":
            raise RuntimeError("Bundled ICU/timezone conversion failed")
        if rows[1] != (2, None, None, None, None):
            raise RuntimeError("NULL representation mismatch")
        named = pool.fetch_val("SELECT NAMED_STAMP FROM WHEEL_SMOKE WHERE ID=1")
        if named.isoformat() != "2024-11-03T01:30:00-05:00" or named.fold != 1:
            raise RuntimeError("Named timezone/fold conversion mismatch")
        expected_models = [ModelRow(ID=row[0], LABEL=row[1], AMOUNT=row[2]) for row in rows]
        if pool.fetch_all(sql, model=ModelRow) != expected_models:
            raise RuntimeError("Pydantic model mismatch")
        with pool.snapshot() as snapshot:
            if snapshot.fetch_one(sql, model=ModelRow) != expected_models[0]:
                raise RuntimeError("Pydantic snapshot mismatch")
        if json.loads(pool.fetch_json("SELECT ID, LABEL FROM WHEEL_SMOKE ORDER BY ID")) != [
            {"ID": 1, "LABEL": "Olá 日本語"}, {"ID": 2, "LABEL": None},
        ]:
            raise RuntimeError("Native JSON mismatch")
        with pool.stream(sql, batch_rows=1) as stream:
            if [row for batch in stream for row in batch] != rows:
                raise RuntimeError("Streaming mismatch")
        for format_ in ("arrow", "polars"):
            result = getattr(pool, f"fetch_{format_}")("SELECT ID, LABEL, D FROM WHEEL_SMOKE ORDER BY ID")
            actual = result.to_pylist() if format_ == "arrow" else result.to_dicts()
            if actual != [{"ID": 1, "LABEL": "Olá 日本語", "D": 1.25},
                          {"ID": 2, "LABEL": None, "D": None}]:
                raise RuntimeError(f"{format_} mismatch")
        with pool.stream_arrow(sql, batch_rows=1) as stream:
            batches = list(stream)
        if sum(batch.num_rows for batch in batches) != 2:
            raise RuntimeError("Arrow stream count mismatch")
        try:
            pool.fetch_all("DELETE FROM WHEEL_SMOKE")
        except fbfast.UnsupportedStatementError:
            pass
        else:
            raise RuntimeError("Read-only policy failed")
        if pool.fetch_one("SELECT MON$READ_ONLY, MON$ISOLATION_MODE FROM MON$TRANSACTIONS "
                          "WHERE MON$TRANSACTION_ID = CURRENT_TRANSACTION") != (1, 4):
            raise RuntimeError("Effective transaction is not READ ONLY READ CONSISTENCY")

    async def check_async():
        async with fbfast.Pool(config) as pool:
            if await pool.fetch_all_async(sql) != rows:
                raise RuntimeError("Async query mismatch")
            if await pool.fetch_all_async(sql, model=ModelRow) != expected_models:
                raise RuntimeError("Async Pydantic model mismatch")
            count = 0
            async with pool.stream_arrow_async(sql, batch_rows=1) as stream:
                async for batch in stream:
                    count += batch.num_rows
            if count != 2:
                raise RuntimeError("Async Arrow stream mismatch")
    asyncio.run(check_async())
    print(json.dumps({"verified": True, "build_id": fbfast.build_info()["build_id"], "client": client}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", type=Path)
    parser.add_argument("--consumer", action="store_true")
    args = parser.parse_args()
    if args.consumer:
        consumer()
        return
    if args.sdk is None:
        parser.error("--sdk is required")
    from ci_fixture import fixture
    with tempfile.TemporaryDirectory(prefix="fbfast-ci-fixture-") as temporary:
        with fixture(args.sdk.resolve(), Path(temporary)) as config:
            env = {k: v for k, v in os.environ.items()
                   if not k.startswith(("FB_", "FIREBIRD", "DYLD_", "LD_", "PYTHONPATH", "ISC_"))}
            env["FBFAST_CI_CONFIG"] = json.dumps(config)
            # -I removes the checkout/import environment; only the installed
            # wheel and installed optional consumers are visible to this process.
            subprocess.run([sys.executable, "-I", str(Path(__file__).resolve()), "--consumer"],
                           env=env, check=True, timeout=90)


if __name__ == "__main__":
    main()
