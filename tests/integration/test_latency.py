"""Round trips per query, deferred cleanup, inline BLOBs and keepalive (0.3.0)."""
import time

import pytest

ROWS = ("WITH RECURSIVE R(N) AS (SELECT 1 FROM RDB$DATABASE UNION ALL "
        "SELECT N + 1 FROM R WHERE N < 50) ")


def wait_idle(pool, slots=1):
    end = time.monotonic() + 5
    while pool.stats()["idle"] < slots:
        assert time.monotonic() < end, pool.stats()
        time.sleep(0.01)


def test_cached_query_answers_after_two_round_trips(fb, config):
    with fb.Pool(config(pool_size=1, min_idle=1, diagnostics=True,
                        ready_transaction_timeout=None)) as pool:
        for _ in range(3):
            assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
        records = pool.diagnostics()
        assert [r["kind"] for r in records] == ["rows"] * 3
        last = records[-1]
        assert last["ok"] and last["statement_cached"] and last["rows"] == 1
        assert last["roundtrips"] == 2, last
        assert [p["phase"] for p in last["phases"]] == [
            "queue", "begin", "prepare", "execute", "fetch", "cleanup"]
        assert pool.diagnostics() == []  # drained
        wait_idle(pool)
        stats = pool.stats()
        assert stats["deferred_cleanups"] >= 3 and stats["cleanup_errors"] == 0
        assert stats["transaction_restarts"] == 0
        assert stats["wire_roundtrips"] >= 12 and stats["wire_received_bytes"] > 0


def test_session_info_reports_negotiated_capabilities(fb, config):
    with fb.Pool(config(pool_size=1, min_idle=1)) as pool:
        info = pool.session_info()
        assert info is not None
        assert info["charset"] == "UTF8" and info["wire_encrypted"] is True
        assert info["protocol_version"] >= 17
        assert info["inline_blobs"] == (info["protocol_version"] >= 19
                                        and info["max_inline_blob_size"] > 0)
    with fb.Pool(config(pool_size=1, min_idle=1, max_inline_blob_size=0,
                        blob_cache_size=1024)) as pool:
        info = pool.session_info()
        assert info["max_inline_blob_size"] == 0 and info["inline_blobs"] is False
        assert info["blob_cache_size"] == 1024


@pytest.mark.parametrize("charset", ["UTF8", "ISO8859_1"])
def test_text_blobs_travel_inline_and_convert_exactly(fb, config, charset):
    sql = (ROWS + f"SELECT N, CAST('Ação ' || N AS BLOB SUB_TYPE TEXT CHARACTER SET {charset}) "
           "FROM R ORDER BY N")
    expected = [(n, f"Ação {n}") for n in range(1, 51)]
    with fb.Pool(config(pool_size=1, min_idle=1, diagnostics=True)) as pool:
        assert pool.fetch_all(sql) == expected
        inline = pool.session_info()["inline_blobs"]
        trips = pool.diagnostics()[-1]["roundtrips"]
    with fb.Pool(config(pool_size=1, min_idle=1, diagnostics=True,
                        max_inline_blob_size=0)) as pool:
        assert pool.fetch_all(sql) == expected
        without_inline = pool.diagnostics()[-1]["roundtrips"]
    assert without_inline >= 50, without_inline  # at least one per BLOB
    if inline:
        assert trips < 10, trips


def test_latin1_blob_limit_counts_utf8_bytes(fb, config):
    # Three ISO8859_1 bytes become six UTF-8 bytes; the limit applies after conversion.
    sql = ("SELECT CAST(CAST(x'E7E3E9' AS VARCHAR(3) CHARACTER SET OCTETS) "
           "AS BLOB SUB_TYPE TEXT CHARACTER SET ISO8859_1) FROM RDB$DATABASE")
    with fb.Pool(config(pool_size=1, min_idle=1, max_blob_bytes=6)) as pool:
        assert pool.fetch_val(sql) == "çãé"
    with fb.Pool(config(pool_size=1, min_idle=1, max_blob_bytes=5)) as pool:
        with pytest.raises(fb.ResultLimitError):
            pool.fetch_val(sql)
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1


def test_idle_connection_is_pinged_off_the_critical_path(fb, config):
    with fb.Pool(config(pool_size=1, min_idle=1, diagnostics=True,
                        health_check_after=0.3, ready_transaction_timeout=None)) as pool:
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
        end = time.monotonic() + 5
        while pool.stats()["keepalive_pings"] == 0:
            assert time.monotonic() < end
            time.sleep(0.05)
        wait_idle(pool)
        pool.diagnostics()
        assert pool.fetch_val("SELECT 2 FROM RDB$DATABASE") == 2
        phases = [p["phase"] for p in pool.diagnostics()[-1]["phases"]]
        assert "ping" not in phases
        assert pool.stats()["discarded"] == 0


TX = "SELECT CURRENT_TRANSACTION FROM RDB$DATABASE"


def begin_trips(pool):
    phases = pool.diagnostics()[-1]["phases"]
    return next(p["roundtrips"] for p in phases if p["phase"] == "begin")


def test_session_reset_prestarts_the_next_transaction(fb, config, settled):
    with fb.Pool(config(pool_size=1, min_idle=1, diagnostics=True)) as pool:
        seen = []
        for _ in range(4):
            seen.append(pool.fetch_val(TX))
            settled(pool)
        assert len(set(seen)) == 4, seen  # every query still gets its own transaction
        last = pool.diagnostics()[-1]
        assert last["roundtrips"] == 1 and begin_trips_of(last) == 0, last
        assert pool.stats()["transaction_restarts"] >= 3
        # Session state never leaks into the next query's transaction.
        pool.fetch_val("SELECT MARKED FROM FBFAST_MARK_SESSION")
        settled(pool)
        assert pool.fetch_val("SELECT RDB$GET_CONTEXT('USER_SESSION', 'FBFAST_MARK') "
                              "FROM RDB$DATABASE") is None
        # A snapshot first rolls back the unused pre-started transaction, and its
        # SNAPSHOT transaction is never restarted as the next query's transaction.
        mode = ("SELECT MON$READ_ONLY, MON$ISOLATION_MODE FROM MON$TRANSACTIONS "
                "WHERE MON$TRANSACTION_ID = CURRENT_TRANSACTION")
        settled(pool)
        with pool.snapshot() as snapshot:
            assert snapshot.fetch_one(mode) == (1, 1)
        for _ in range(3):
            assert pool.fetch_one(mode) == (1, 4)  # READ ONLY, READ COMMITTED READ CONSISTENCY
            settled(pool)
        with pool.stream(mode) as batches:
            assert [row for batch in batches for row in batch] == [(1, 4)]
        settled(pool)
        assert pool.fetch_one(mode) == (1, 4)


def begin_trips_of(record):
    return next(p["roundtrips"] for p in record["phases"] if p["phase"] == "begin")


def test_unused_prestarted_transaction_is_rolled_back(fb, config, settled):
    with fb.Pool(config(pool_size=1, min_idle=1, diagnostics=True,
                        ready_transaction_timeout=0.3)) as pool:
        pool.fetch_val(TX)
        settled(pool)
        time.sleep(1.0)
        pool.fetch_val(TX)
        assert begin_trips(pool) == 1  # nothing was waiting any more
        assert pool.stats()["discarded"] == 0


def test_reuse_disabled_or_unread_blobs_use_the_full_cleanup(fb, config, settled):
    with fb.Pool(config(pool_size=1, min_idle=1, diagnostics=True,
                        ready_transaction_timeout=None)) as pool:
        for _ in range(2):
            pool.fetch_val(TX)
            settled(pool)
        assert pool.diagnostics()[-1]["roundtrips"] == 2
        assert pool.stats()["transaction_restarts"] == 0
    blobs = ROWS + "SELECT CAST('x' || N AS BLOB SUB_TYPE TEXT CHARACTER SET UTF8) FROM R"
    with fb.Pool(config(pool_size=1, min_idle=1, diagnostics=True)) as pool:
        assert pool.fetch_one(blobs) == ("x1",)  # stops before reading every BLOB
        settled(pool)
        restarts = pool.stats()["transaction_restarts"]
        pool.fetch_val(TX)
        assert begin_trips(pool) == 1
        assert settled(pool)["transaction_restarts"] == restarts + 1
