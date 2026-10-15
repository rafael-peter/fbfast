from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime, time, timezone
from decimal import Decimal, localcontext
import gc
import os

import pytest


def test_lazy_pool_exact_scalars_and_binding(fb, config):
    pool = fb.Pool(config())
    assert pool.stats()["connections_created"] == 0
    assert pool.stats()["closed"] == 2
    with pool:
        sql = "SELECT ID, LABEL, PADDED, AMOUNT, WIDE_INTEGER, FLAG, DAY_VALUE, TIME_VALUE, STAMP_VALUE FROM FBFAST_TYPES ORDER BY ID"
        with localcontext() as context:
            context.prec = 2  # native numeric conversion must not round through Decimal context
            rows = pool.fetch_all(sql)
        assert rows[0] == (1, "Olá 日本語", "a       ", Decimal("-12345678.9012"), 2**127-1,
                           True, date(2024, 2, 29), time(12, 34, 56, 123400),
                           datetime(2024, 2, 29, 12, 34, 56, 123400))
        assert rows[1] == (2,) + (None,) * 8
        assert pool.fetch_one("SELECT ID FROM FBFAST_TYPES ORDER BY ID") == (1,)
        assert pool.fetch_val("SELECT ID FROM FBFAST_TYPES WHERE ID = ?", [2]) == 2
        assert pool.fetch_one("SELECT ID FROM FBFAST_TYPES WHERE ID = ?", [99]) is None
        assert pool.fetch_all("SELECT ID FROM FBFAST_TYPES WHERE ID = ?", [99]) == []
        assert pool.fetch_val("SELECT AMOUNT FROM FBFAST_TYPES WHERE ID = 2") is None
        assert pool.fetch_all("SELECT ID, LABEL FROM FBFAST_TYPES WHERE ID = 1", as_dict=True) == [{"ID": 1, "LABEL": "Olá 日本語"}]
        cases = [("NUMERIC(18,4)", Decimal("123.4567")), ("INT128", -(2**127)),
                 ("VARCHAR(80)", "'; DELETE FROM FBFAST_TYPES; --"),
                 ("DATE", date(1, 1, 1)), ("TIME", time(23, 59, 59, 999900)),
                 ("TIMESTAMP", datetime(2024, 2, 29, 0, 0, 0, 100)), ("BOOLEAN", False)]
        for sql_type, value in cases:
            assert pool.fetch_val(f"SELECT CAST(? AS {sql_type}) FROM RDB$DATABASE", (value,)) == value
        assert 'Table "FBFAST_TYPES"' in pool.explain("SELECT ID FROM FBFAST_TYPES")
        assert 'Table "FBFAST_TYPES"' in pool.explain("SELECT ID FROM FBFAST_TYPES WHERE ID=?", [1])
        with pytest.raises(fb.DataConversionError):
            pool.explain("SELECT ID FROM FBFAST_TYPES WHERE ID=?", [])
        assert pool.stats()["statement_hits"] > 0
    assert rows[0][4] == 2**127-1
    assert pool.stats()["closed"] == 2
    pool.close_sync()
    with pytest.raises(fb.PoolClosedError):
        pool.fetch_val("SELECT 1 FROM RDB$DATABASE")


def test_errors_cleanup_limits_and_names(fb, config):
    # Keep the cleanup assertion exact: a larger pool may replenish min_idle
    # while this connection is leased and legitimately finish with two idle slots.
    with fb.Pool(config(pool_size=1, max_result_rows=1)) as pool:
        with pytest.raises(fb.ResultLimitError):
            pool.fetch_all("SELECT ID FROM FBFAST_TYPES")
        with pytest.raises(fb.DataConversionError) as duplicate:
            pool.fetch_all("SELECT 1/(ID-ID) AS DUP, ID AS DUP FROM FBFAST_TYPES", as_dict=True)
        assert duplicate.value.phase == "describe"
        assert pool.fetch_one("SELECT ID AS DUP, ID AS DUP FROM FBFAST_TYPES") == (1, 1)
        with pytest.raises(fb.QueryError) as query:
            pool.fetch_all("SELECT DOES_NOT_EXIST FROM FBFAST_TYPES")
        assert query.value.phase == "prepare"
        assert query.value.sqlstate and query.value.native_codes
        assert query.value.cleanup_errors == []
        for sql in ["DELETE FROM FBFAST_TYPES RETURNING ID", "UPDATE FBFAST_TYPES SET ID=3 RETURNING ID"]:
            with pytest.raises(fb.UnsupportedStatementError):
                pool.fetch_all(sql)
        with pytest.raises(fb.DataConversionError):
            pool.fetch_val("SELECT CAST(? AS SMALLINT) FROM RDB$DATABASE", [32768])
        with pytest.raises(fb.DataConversionError):
            pool.fetch_val("SELECT CAST(? AS TIME) FROM RDB$DATABASE", [time(0, 0, 0, 1)])
        with pytest.raises(fb.DataConversionError):
            pool.fetch_val("SELECT CAST(? AS TIMESTAMP) FROM RDB$DATABASE", [datetime.now(timezone.utc)])
        assert pool.fetch_val("SELECT ID FROM FBFAST_TYPES WHERE ID=2") == 2
        assert pool.stats()["idle"] == 1
    wrong_role = fb.Pool(config(role="NOT_GRANTED", min_idle=2))
    with pytest.raises(fb.RoleNotActiveError):
        wrong_role.warmup_sync()
    assert wrong_role.stats()["closed"] == 2


def test_concurrent_queries_respect_attachment_ceiling(fb, config):
    pool = fb.Pool(config(pool_size=3, min_idle=3, max_waiters=16))
    with pool:
        assert pool.stats()["idle"] == 3
        def query(value):
            result = pool.fetch_one("SELECT CURRENT_CONNECTION, CAST(? AS INTEGER) FROM RDB$DATABASE", [value])
            stats = pool.stats()
            assert stats["opening"] + stats["leased"] + stats["cleaning"] + stats["idle"] + stats["closing"] <= 3
            return result
        with ThreadPoolExecutor(max_workers=12) as executor:
            results = list(executor.map(query, range(96)))
        assert [row[1] for row in results] == list(range(96))
        assert len({row[0] for row in results}) == 3
        assert pool.stats()["connections_created"] == 3
        assert pool.stats()["waiting"] == 0
        assert pool.stats()["statement_hits"] >= 90
    assert pool.stats()["closed"] == 3


@pytest.mark.skipif(not hasattr(os, "fork"), reason="POSIX fork guard")
def test_pool_fork_guard_precedes_inherited_mutexes(fb, config):
    with fb.Pool(config()) as pool:
        child = os.fork()
        if child == 0:
            try:
                for operation in (pool.stats, pool.close_sync,
                                  lambda: pool.fetch_val("SELECT 1 FROM RDB$DATABASE"),
                                  lambda: pool._core.value_async("SELECT 1 FROM RDB$DATABASE")):
                    try:
                        operation()
                    except fb.ForkSafetyError:
                        pass
                    else:
                        os._exit(2)
                del pool._core
                gc.collect()
                os._exit(0)
            except BaseException:
                os._exit(3)
        _, status = os.waitpid(child, 0)
        assert os.waitstatus_to_exitcode(status) == 0
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
