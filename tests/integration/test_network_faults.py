"""Connection drops through a proxy: reconnect before SQL, never repeat it after."""
import shutil
import sys
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
LONG = ("WITH RECURSIVE R(N) AS (SELECT 1 FROM RDB$DATABASE UNION ALL "
        "SELECT N + 1 FROM R WHERE N < 2000000) SELECT N FROM R")


@pytest.fixture(scope="module")
def proxy():
    if shutil.which("docker") is None:
        pytest.skip("docker is required for the fault-injection proxy")
    sys.path.insert(0, str(ROOT / "benchmarks"))
    sys.path.insert(0, str(ROOT / "scripts"))
    from firebird_fixture import NAME
    from latency_proxy import LatencyProxy
    instance = LatencyProxy(NAME, role="tests")
    instance.start()
    try:
        yield instance
    finally:
        instance.stop()


def test_dropped_idle_connection_reconnects_before_sending_sql(fb, config, proxy, settled):
    # Without a pre-started transaction the first exchange is the transaction
    # start, before any SQL, so a dead attachment is replaced transparently.
    with fb.Pool(config(port=proxy.port, pool_size=1, min_idle=1, diagnostics=True,
                        ready_transaction_timeout=None)) as pool:
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
        settled(pool)  # the drop must hit an idle connection, not its deferred cleanup
        proxy.reset_connections()
        time.sleep(0.2)
        assert pool.fetch_val("SELECT 2 FROM RDB$DATABASE") == 2
        stats = pool.stats()
        assert stats["reconnects"] == 1 and stats["connections_created"] == 2, stats
        phases = [p["phase"] for p in pool.diagnostics()[-1]["phases"]]
        assert "failed_attempt" in phases and "connect" in phases


def test_connection_lost_during_fetch_is_reported_and_not_repeated(fb, config, proxy):
    with fb.Pool(config(port=proxy.port, pool_size=1, min_idle=1, fetch_size=64)) as pool:
        with pytest.raises(fb.QueryError) as caught:
            with pool.stream(LONG, batch_rows=64) as stream:
                next(stream)
                proxy.reset_connections()
                for _ in stream:
                    pass
        assert isinstance(caught.value, fb.DatabaseConnectionError), repr(caught.value)
        assert isinstance(caught.value, ConnectionError)
        assert pool.stats()["reconnects"] == 0
        # The broken attachment was discarded; the pool keeps working.
        assert pool.fetch_val("SELECT 3 FROM RDB$DATABASE") == 3


def test_drop_inside_the_reuse_window_is_reported_not_repeated(fb, config, proxy, settled):
    # A pre-started transaction makes the query itself the first exchange; if
    # the link died meanwhile the SQL may have been sent, so it is not retried.
    with fb.Pool(config(port=proxy.port, pool_size=1, min_idle=1,
                        ready_transaction_timeout=30)) as pool:
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
        settled(pool)
        proxy.reset_connections()
        time.sleep(0.2)
        with pytest.raises(fb.DatabaseConnectionError):
            pool.fetch_val("SELECT 2 FROM RDB$DATABASE")
        assert pool.stats()["reconnects"] == 0
        assert pool.fetch_val("SELECT 3 FROM RDB$DATABASE") == 3


def test_reuse_window_expiry_restores_transparent_reconnect(fb, config, proxy, settled):
    with fb.Pool(config(port=proxy.port, pool_size=1, min_idle=1,
                        ready_transaction_timeout=0.3)) as pool:
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
        settled(pool)
        time.sleep(1.0)  # the idle tick rolled the unused transaction back
        proxy.reset_connections()
        time.sleep(0.2)
        assert pool.fetch_val("SELECT 2 FROM RDB$DATABASE") == 2
        assert pool.stats()["reconnects"] == 1
