#!/usr/bin/env python3
"""Per-query latency through a controlled-latency proxy on the owned disposable fixture."""
from __future__ import annotations

import argparse
import asyncio
import json
import platform
import shutil
import statistics
import sys
import tempfile
import threading
import time
import zipfile
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from importlib.metadata import version
from pathlib import Path
from typing import Any, Callable

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from firebird_fixture import NAME, sql, state, status  # noqa: E402
from latency_proxy import IMAGE, LatencyProxy  # noqa: E402

WIDE = ", ".join(f"TXT AS T{i}" for i in range(62))
CASES: dict[str, tuple[str, list[Any]]] = {
    "pk": ("SELECT ID, V, TXT FROM FBFAST_BENCH WHERE ID = ?", [543210]),
    "narrow1000": ("SELECT ID, V FROM FBFAST_BENCH WHERE ID <= ? ORDER BY ID", [1000]),
    "narrow100k": ("SELECT ID, V FROM FBFAST_BENCH WHERE ID <= ? ORDER BY ID", [100_000]),
    "wide64": (f"SELECT ID, V, {WIDE} FROM FBFAST_BENCH WHERE ID <= ? ORDER BY ID", [10_000]),
    "blob_small": ("SELECT ID, SMALL_TXT FROM FBFAST_LAT_BLOB WHERE ID <= ? ORDER BY ID", [1000]),
    "blob_medium": ("SELECT ID, MEDIUM_TXT FROM FBFAST_LAT_BLOB WHERE ID <= ? ORDER BY ID", [100]),
    "blob_large": ("SELECT ID, LARGE_TXT FROM FBFAST_LAT_BLOB WHERE ID <= ? ORDER BY ID", [20]),
    "blob_binary": ("SELECT ID, BIN FROM FBFAST_LAT_BLOB WHERE ID <= ? ORDER BY ID", [1000]),
    "blob_latin1": ("SELECT ID, LATIN_TXT FROM FBFAST_LAT_BLOB WHERE ID <= ? ORDER BY ID", [1000]),
    "list_with": ("WITH T AS (SELECT ID, TXT FROM FBFAST_BENCH WHERE ID <= ?) "
                  "SELECT ID / 10 AS G, LIST(TXT, ',') AS L FROM T GROUP BY 1 ORDER BY 1", [1000]),
    "cast_tight": ("SELECT ID, CAST(SMALL_TXT AS VARCHAR(256) CHARACTER SET UTF8) AS T "
                   "FROM FBFAST_LAT_BLOB WHERE ID <= ? ORDER BY ID", [1000]),
    "cast_8191": ("SELECT ID, CAST(SMALL_TXT AS VARCHAR(8191) CHARACTER SET UTF8) AS T "
                  "FROM FBFAST_LAT_BLOB WHERE ID <= ? ORDER BY ID", [1000]),
}
PK_SQL = "SELECT ID, V FROM FBFAST_BENCH WHERE ID = ?"
DRIVERS = ("fbfast", "firebird-driver")
# Exact TPB used by fbfast: READ ONLY, READ COMMITTED READ CONSISTENCY, WAIT 5 s.
TPB = bytes([3, 8, 15, 22, 6, 21, 4, 5, 0, 0, 0])


def install(wheel: Path | None) -> dict[str, Any]:
    """Import a private copy (a wheel or dist/vendor) so rebuilds cannot interfere."""
    target = Path(tempfile.mkdtemp(prefix="fbfast-latency-"))
    if wheel is None:
        shutil.copytree(ROOT / "dist/vendor/fbfast", target / "fbfast", symlinks=True)
    else:
        with zipfile.ZipFile(wheel) as archive:
            archive.extractall(target)
        for path in (target / "fbfast/_runtime").rglob("*"):
            if path.is_file() and (path.suffix in {".dylib", ".so"} or ".so." in path.name
                                   or path.parent.name == "bin"):
                path.chmod(0o755)
    sys.path.insert(0, str(target))
    info: dict[str, Any] = json.loads((target / "fbfast/_build_info.json").read_text())
    info["library"] = str(target / "fbfast" / info["client_library"])
    return info


def attempt(call: Callable[[], Any]) -> tuple[float | None, str | None]:
    start = time.perf_counter()
    try:
        call()
    except Exception as error:  # recorded; a slow link may exceed query_timeout
        return None, f"{type(error).__name__}: {error}"
    return time.perf_counter() - start, None


def timed(call: Callable[[], Any], samples: int) -> list[float]:
    call()  # warmup: connection, prepare and statement cache
    result = []
    for _ in range(samples):
        start = time.perf_counter()
        call()
        result.append(time.perf_counter() - start)
    return result


class FbFast:
    def __init__(self, config: dict[str, Any], port: int, **options: Any) -> None:
        import fbfast
        self.module = fbfast
        self.pool = fbfast.Pool(fbfast.FirebirdConfig(
            **{**config, "port": port}, pool_size=8, min_idle=1, diagnostics=True,
            query_timeout=300, env_file=False, **options))
        # 0.3.0 made warmup()/close() synchronous; 0.2.1 used warmup_sync()/close_sync().
        self.legacy = not hasattr(self.pool, "aclose")
        self.executor = ThreadPoolExecutor(max_workers=8)
        (self.pool.warmup_sync if self.legacy else self.pool.warmup)()

    def query(self, sql_text: str, params: list[Any]) -> Any:
        return self.pool.fetch_all(sql_text, params)

    def concurrent_threads(self, count: int) -> None:
        # Same shape as the manual pattern this release replaces: pool_size workers.
        list(self.executor.map(lambda i: self.pool.fetch_one(PK_SQL, [i + 1]), range(count)))

    def concurrent_gather(self, count: int) -> None:
        async def run() -> None:
            await asyncio.gather(*(self.pool.fetch_one_async(PK_SQL, [i + 1]) for i in range(count)))
        asyncio.run(run())

    def concurrent_run_all(self, count: int) -> None:
        query = self.module.Query.fetch_one
        self.pool.run_all([query(PK_SQL, [i + 1]) for i in range(count)])

    def concurrent_run_all_async(self, count: int) -> None:
        query = self.module.Query.fetch_one
        asyncio.run(self.pool.run_all_async([query(PK_SQL, [i + 1]) for i in range(count)]))

    def modes(self) -> list[str]:
        extra = [] if self.legacy else ["run_all", "run_all_async"]
        return ["threads", "gather", *extra]

    def roundtrips(self) -> list[int]:
        return [record["roundtrips"] for record in self.pool.diagnostics()]

    def close(self) -> None:
        self.executor.shutdown()
        (self.pool.close_sync if self.legacy else self.pool.close)()


class FirebirdDriver:
    """Tuned baseline: reused connections and prepared statements, no session reset."""

    def __init__(self, config: dict[str, Any], port: int, library: str) -> None:
        from firebird.driver import connect, driver_config
        driver_config.fb_client_library.value = library
        driver_config.db_defaults.config.value = "WireCrypt=Required\nWireCompression=false\nProviders=Remote"
        self.connect = lambda: self._open(connect, config, port)
        self.main = self.connect()
        self.local = threading.local()
        self.connections = [self.main]
        # Persistent workers keep one connection each across samples.
        self.executor = ThreadPoolExecutor(max_workers=8)

    @staticmethod
    def _open(connect: Any, config: dict[str, Any], port: int) -> Any:
        dsn = f"{config['host']}/{port}:{config['database']}"
        connection = connect(dsn, user=config["user"], password=config["password"],
                             role=config["role"], charset="UTF8", session_time_zone="UTC")
        connection.main_transaction.default_tpb = TPB
        return connection

    def query(self, sql_text: str, params: list[Any], connection: Any = None) -> Any:
        connection = connection or self.main
        statements = connection.__dict__.setdefault("_bench_statements", {})
        cursor = connection.cursor()
        statement = statements.get(sql_text) or statements.setdefault(sql_text, cursor.prepare(sql_text))
        cursor.execute(statement, params)
        rows = []
        for row in cursor.fetchall():
            rows.append(tuple(value.read() if hasattr(value, "read") else value for value in row))
        cursor.close()
        connection.rollback()
        return rows

    def _thread_connection(self) -> Any:
        if not hasattr(self.local, "connection"):
            self.local.connection = self.connect()
            self.connections.append(self.local.connection)
        return self.local.connection

    def concurrent_threads(self, count: int) -> None:
        list(self.executor.map(lambda i: self.query(PK_SQL, [i + 1], self._thread_connection()),
                               range(count)))

    def close(self) -> None:
        self.executor.shutdown()
        for connection in self.connections:
            connection.close()


def prepare(fixture: dict[str, Any]) -> None:
    if sql("SELECT COUNT(*) FROM FBFAST_BENCH;", fixture["admin_password"]).returncode:
        raise SystemExit("Run benchmarks/run_comparison.py --prepare first (FBFAST_BENCH)")
    if sql("SELECT COUNT(*) FROM FBFAST_LAT_BLOB;", fixture["admin_password"]).returncode:
        source = Path(__file__).with_name("latency_fixture.sql").read_text()
        result = sql(source, fixture["admin_password"])
        if result.returncode:
            raise SystemExit(f"Latency fixture initialization failed: {result.stderr.strip()}")
    print("Disposable latency fixture ready", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--rtt", type=int, nargs="+", default=[0, 5, 20, 50])
    parser.add_argument("--cases", choices=CASES, nargs="+", default=list(CASES))
    parser.add_argument("--drivers", choices=DRIVERS, nargs="+", default=list(DRIVERS))
    parser.add_argument("--concurrency", type=int, nargs="*", default=[8, 32, 128])
    parser.add_argument("--no-bandwidth", action="store_true", help="skip the limited-bandwidth case")
    parser.add_argument("--bandwidth-kib", type=int, default=1024)
    parser.add_argument("--label", default="latency")
    parser.add_argument("--wheel", type=Path, help="Measure this wheel instead of dist/vendor")
    parser.add_argument("--report", type=Path, help="Only re-render report.md of an existing run")
    args = parser.parse_args()
    if args.report is not None:
        args.report = args.report.resolve()
        manifest = json.loads((args.report / "manifest.json").read_text())
        raw = json.loads((args.report / "raw.json").read_text())
        args.samples = manifest["samples"]
        artifact = {"build_id": manifest["build_id"], "version": manifest["version"]}
        write(args, artifact, raw, args.report, args.report.name.split("-")[0])
        return
    fixture, live = state(), status()
    if not live["running"] or any(live[k] != fixture[k] for k in ("host", "port", "database", "image")):
        raise SystemExit("Start and revalidate the owned disposable fixture first")
    if args.prepare:
        prepare(fixture)
    artifact = install(args.wheel)
    if artifact["profile"] != "release":
        raise SystemExit("Benchmarks require a release artifact")
    library = artifact["library"]
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    directory = ROOT / "benchmarks/results" / f"{stamp}-{args.label}"
    directory.mkdir(parents=True)
    config = {k: fixture[k] for k in ("host", "database", "user", "password", "role")}
    proxy = LatencyProxy(NAME)
    proxy.start()
    raw: list[dict[str, Any]] = []
    try:
        for rtt in args.rtt:
            proxy.configure(rtt_ms=rtt)
            for driver in args.drivers:
                client = (FbFast(config, proxy.port) if driver == "fbfast"
                          else FirebirdDriver(config, proxy.port, library))
                try:
                    if driver == "fbfast":
                        client.roundtrips()
                    for case in args.cases:
                        sql_text, params = CASES[case]
                        samples, errors, warm = [], [], 0
                        # Warm until every pooled connection a sequential caller can
                        # reach has the statement cached: at least one call, more while
                        # they stay cheap (at most 8 calls or 2 s).
                        started = time.perf_counter()
                        while not errors and (warm == 0 or (
                                warm < 8 and time.perf_counter() - started < 2.0)):
                            _, error = attempt(lambda: client.query(sql_text, params))
                            errors += [error] if error else []
                            warm += 1
                        if driver == "fbfast":
                            client.roundtrips()  # discard warmup records
                        while not errors and len(samples) < args.samples:
                            seconds, error = attempt(lambda: client.query(sql_text, params))
                            if error:
                                errors.append(error)
                            else:
                                samples.append(seconds)
                        record = {"rtt_ms": rtt, "driver": driver, "case": case,
                                  "samples_s": samples, "errors": errors, "warmup_calls": warm}
                        if driver == "fbfast":
                            record["roundtrips"] = client.roundtrips()
                        raw.append(record)
                        save(directory, raw)
                        median = (f"{statistics.median(record['samples_s']) * 1000:9.2f} ms"
                                  if record["samples_s"] else f"FAILED {errors[0][:60]}")
                        print(f"rtt={rtt:>3} {driver:<16} {case:<12} {median} "
                              f"{record.get('roundtrips', '')}", flush=True)
                    for count in args.concurrency:
                        modes = client.modes() if driver == "fbfast" else ["threads"]
                        for mode in modes:
                            run = getattr(client, f"concurrent_{mode}")
                            samples = timed(lambda: run(count), max(3, args.samples // 2))
                            raw.append({"rtt_ms": rtt, "driver": driver, "case": f"concurrent{count}",
                                        "mode": mode, "samples_s": samples})
                            save(directory, raw)
                            print(f"rtt={rtt:>3} {driver:<16} concurrent{count:<3} {mode:<8} "
                                  f"{statistics.median(samples) * 1000:9.2f} ms", flush=True)
                finally:
                    client.close()
        proxy.configure(rtt_ms=20, rate_kib=args.bandwidth_kib)
        for compression in () if args.no_bandwidth else (False, True):
            client = FbFast(config, proxy.port, wire_compression=compression)
            try:
                negotiated = (client.pool.session_info() or {}).get("wire_compressed")
                for case in ("narrow100k", "blob_medium"):
                    before = client.pool.stats()
                    samples = timed(lambda: client.query(*CASES[case]), max(3, args.samples // 2))
                    after = client.pool.stats()
                    raw.append({"rtt_ms": 20, "rate_kib": args.bandwidth_kib, "driver": "fbfast",
                                "case": case, "wire_compression": compression,
                                "compressed": negotiated, "samples_s": samples,
                                "received_bytes": after["wire_received_bytes"] - before["wire_received_bytes"]})
                    print(f"bandwidth {args.bandwidth_kib} KiB/s compression={compression} "
                          f"(negotiated={negotiated}) {case:<12} "
                          f"{statistics.median(samples) * 1000:9.2f} ms", flush=True)
            finally:
                client.close()
    finally:
        proxy.stop()
    write(args, artifact, raw, directory, stamp)


def save(directory: Path, raw: list[dict[str, Any]]) -> None:
    (directory / "raw.json").write_text(json.dumps(raw, indent=2) + "\n")


def write(args: argparse.Namespace, artifact: dict[str, Any], raw: list[dict[str, Any]],
          directory: Path, stamp: str) -> None:
    manifest = {
        "build_id": artifact["build_id"], "version": artifact["version"], "command": sys.argv,
        "python": sys.version, "platform": platform.platform(), "toxiproxy": IMAGE,
        "firebird_driver": version("firebird-driver"), "samples": args.samples,
        "note": "RTT is added by toxiproxy (half per direction) on top of the local Docker path.",
    }
    (directory / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    save(directory, raw)
    lines = [f"# Latency benchmark {stamp}", "", f"fbfast {artifact['version']} build "
             f"`{artifact['build_id'][:16]}`, {args.samples} samples, median per query.", "",
             "| RTT ms | case | fbfast ms | fbfast round trips | firebird-driver ms |",
             "|---:|---|---:|---:|---:|"]
    by_key = {(r["rtt_ms"], r["driver"], r["case"], r.get("mode")): r for r in raw if "rate_kib" not in r}

    def cell(record: dict[str, Any] | None) -> str:
        if record is None:
            return "-"
        if record["samples_s"]:
            return f"{statistics.median(record['samples_s']) * 1000:.2f}"
        return "failed: " + (record.get("errors") or ["?"])[0].split(":")[0]

    for rtt in args.rtt:
        for case in [*args.cases, *(f"concurrent{n}" for n in args.concurrency)]:
            for mode in ([None] if case in CASES
                         else ["threads", "gather", "run_all", "run_all_async"]):
                fb = by_key.get((rtt, "fbfast", case, mode))
                fd = (by_key.get((rtt, "firebird-driver", case, mode))
                      if mode in (None, "threads") else None)
                trips = fb.get("roundtrips") if fb else None
                label = case if mode is None else f"{case} ({mode})"
                if fb is None and fd is None:
                    continue
                median_trips = f"{statistics.median(trips):.0f}" if trips else "-"
                lines.append(f"| {rtt} | {label} | {cell(fb)} | {median_trips} | {cell(fd)} |")
    lines += ["", "## Limited bandwidth", "", "| case | compression | negotiated | ms | received bytes |",
              "|---|---|---|---:|---:|"]
    for r in raw:
        if "rate_kib" in r:
            lines.append(f"| {r['case']} | {r['wire_compression']} | {r['compressed']} | "
                         f"{cell(r)} | {r['received_bytes']} |")
    (directory / "report.md").write_text("\n".join(lines) + "\n")
    print(f"Results: {directory.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
