"""Pinned baseline adapters; no operational DB mutation, implicit casts or reconnects."""
from __future__ import annotations

import time

ADAPTERS = ("fbfast", "firebird-driver", "firebird-driver-reset", "fdb", "fdb-arm64")
FORMATS = ("tuples", "dicts", "json", "arrow", "polars")
BATCH_BYTES = 4 * 1024 * 1024


def extras(kind):
    # Optional consumer import is cold-start work, outside the warm query sample.
    if kind == "arrow":
        import pyarrow
        return pyarrow
    if kind == "polars":
        import polars
        return polars
    if kind == "json":
        import orjson
        return orjson
    return None


class Fbfast:
    def __init__(self, config, library, case, kind, fetch_size):
        import fbfast
        self.case, self.kind = case, kind
        self.pool = fbfast.Pool(fbfast.FirebirdConfig(
            **config, pool_size=1, min_idle=1, fetch_size=fetch_size,
            batch_bytes=BATCH_BYTES, query_timeout=120, env_file=False,
            client_library=library,
        ))
        self.pool.warmup_sync()

    def fetch(self):
        method = {"tuples": "fetch_all", "dicts": "fetch_all", "json": "fetch_json",
                  "arrow": "fetch_arrow", "polars": "fetch_polars"}[self.kind]
        kwargs = {"as_dict": True} if self.kind == "dicts" else {}
        result = getattr(self.pool, method)(self.case.sql, self.case.params, **kwargs)
        return result, {"stages": "native pipeline includes cleanup; not instrumented separately"}

    def close(self):
        self.pool.close_sync()

    def plan(self):
        return self.pool.explain(self.case.sql, self.case.params)


class Baseline:
    def __init__(self, config, library, case, kind, fetch_size, adapter):
        self.case, self.kind, self.adapter = case, kind, adapter
        self.consumer = extras(kind)
        self.batch = min(fetch_size, max(1, BATCH_BYTES // case.batch_row_bound))
        dsn = f"{config['host']}/{config['port']}:{config['database']}"
        kwargs = {key: config[key] for key in ("user", "password", "role")}
        # Exact TPB used by fbfast: version3/read/read_committed/read_consistency/
        # wait/lock_timeout(length=4,value=5), from official ibase.h constants.
        tpb = bytes([3, 8, 15, 22, 6, 21, 4, 5, 0, 0, 0])
        if adapter.startswith("fdb"):
            import fdb
            if adapter == "fdb-arm64":
                from adapters.fdb_arm64 import configure
                configure(fdb, library)
            self.connection = fdb.connect(dsn=dsn, **kwargs, charset="UTF8",
                                          fb_library_name=library, isolation_level=tpb)
        else:
            from firebird.driver import connect, driver_config
            driver_config.fb_client_library.value = library
            driver_config.db_defaults.config.value = (
                "WireCrypt=Required\nWireCompression=false\nProviders=Remote"
            )
            self.connection = connect(dsn, **kwargs, charset="UTF8", session_time_zone="UTC")
            self.connection.main_transaction.default_tpb = tpb
        self.cursor = self.connection.cursor()
        self.cursor.arraysize = self.batch
        # Check the effective isolation/access, not only the construction bytes.
        self.cursor.execute("SELECT MON$READ_ONLY, MON$ISOLATION_MODE FROM MON$TRANSACTIONS "
                            "WHERE MON$TRANSACTION_ID = CURRENT_TRANSACTION")
        if tuple(self.cursor.fetchone()) != (1, 4):
            raise RuntimeError("Baseline TPB differs from READ ONLY READ CONSISTENCY")
        self.cursor.close()
        self.connection.rollback()
        prepare = self.cursor.prep if adapter.startswith("fdb") else self.cursor.prepare
        self.statement = prepare(case.sql)
        self.cursor.close()
        if adapter.startswith("fdb") or self.connection.main_transaction.is_active():
            self.connection.rollback()

    def fetch(self):
        start = time.perf_counter()
        self.cursor.execute(self.statement, self.case.params)
        executed = time.perf_counter()
        first = None
        rows = []
        while batch := self.cursor.fetchmany(self.batch):
            if first is None:
                first = time.perf_counter() - start
            rows.extend(batch)
        fetched = time.perf_counter()
        # The fastest baseline omits reset; the reset variant matches fbfast's
        # rollback + ALTER SESSION RESET, retaining the prepared statement.
        self.cursor.close()
        self.connection.rollback()
        if self.adapter == "firebird-driver-reset":
            self._reset()
        cleaned = time.perf_counter()
        output = self._convert(rows)
        return output, {"execute_seconds": executed - start,
                        "fetch_decode_seconds": fetched - executed,
                        "cleanup_seconds": cleaned - fetched,
                        "conversion_seconds": time.perf_counter() - cleaned,
                        "first_batch_seconds": first, "effective_batch_rows": self.batch}

    def _reset(self):
        # Pinned 2.0.3's public execute_immediate starts a transaction; RESET
        # requires a null transaction. Use its own official interface wrapper.
        attachment = self.connection._att
        result = attachment.vtable.execute(
            attachment, attachment.status, None, 0, b"ALTER SESSION RESET", 3,
            None, None, None, None,
        )
        attachment._check()
        if result:
            raise RuntimeError("ALTER SESSION RESET unexpectedly returned a transaction")

    def _convert(self, rows):
        names = self.case.names
        if self.kind == "tuples":
            return rows
        if self.kind in ("dicts", "json"):
            result = [dict(zip(names, row, strict=True)) for row in rows]
            return self.consumer.dumps(result) if self.kind == "json" else result
        if self.kind == "arrow":
            pa = self.consumer
            types = [pa.int32(), pa.int32()] + [pa.large_string()] * (self.case.width - 2)
            arrays = [pa.array(column, type=kind)
                      for column, kind in zip(zip(*rows, strict=True), types, strict=True)]
            return pa.Table.from_arrays(arrays, names=names)
        pl = self.consumer
        schema = list(zip(names, [pl.Int32, pl.Int32] + [pl.String] * (self.case.width - 2)))
        return pl.DataFrame(rows, schema=schema, orient="row", strict=True)

    def close(self):
        self.cursor.close()
        if not self.adapter.startswith("fdb"):
            self.statement.free()
        self.connection.close()

    def plan(self):
        return self.statement.plan
