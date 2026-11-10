"""Several queries at once: run_all / run_many and their async variants (0.3.0)."""
import asyncio
import time
from decimal import Decimal

import pytest

LONG = ("WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N "
        "WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) FROM N A CROSS JOIN N B "
        "CROSS JOIN N C")
VALUES = "SELECT ID, AMOUNT FROM FBFAST_TYPES ORDER BY ID"


def queries(fb):
    Q = fb.Query
    return {
        "all": Q.fetch_all(VALUES),
        "dicts": Q.fetch_all(VALUES, as_dict=True),
        "one": Q.fetch_one("SELECT ID FROM FBFAST_TYPES WHERE ID = ?", [2]),
        "none": Q.fetch_one("SELECT ID FROM FBFAST_TYPES WHERE ID = ?", [77]),
        "val": Q.fetch_val("SELECT AMOUNT FROM FBFAST_TYPES WHERE ID = ?", [1]),
        "json": Q.fetch_json("SELECT ID FROM FBFAST_TYPES ORDER BY ID"),
        "plan": Q.explain("SELECT ID FROM FBFAST_TYPES WHERE ID = ?", [1]),
    }


def sequential(pool):
    return {
        "all": pool.fetch_all(VALUES),
        "dicts": pool.fetch_all(VALUES, as_dict=True),
        "one": pool.fetch_one("SELECT ID FROM FBFAST_TYPES WHERE ID = ?", [2]),
        "none": pool.fetch_one("SELECT ID FROM FBFAST_TYPES WHERE ID = ?", [77]),
        "val": pool.fetch_val("SELECT AMOUNT FROM FBFAST_TYPES WHERE ID = ?", [1]),
        "json": pool.fetch_json("SELECT ID FROM FBFAST_TYPES ORDER BY ID"),
        "plan": pool.explain("SELECT ID FROM FBFAST_TYPES WHERE ID = ?", [1]),
    }


def settle(pool):
    end = time.monotonic() + 5
    while (stats := pool.stats())["leased"] + stats["cleaning"] + stats["waiting"]:
        assert time.monotonic() < end, stats
        time.sleep(0.01)
    return stats


def test_run_all_matches_sequential_results(fb, config):
    with fb.Pool(config(pool_size=3)) as pool:
        named = queries(fb)
        results = pool.run_all(named.values())
        expected = sequential(pool)
        assert {name: results[q] for name, q in named.items()} == expected
        assert expected["val"] == Decimal("-12345678.9012")
        assert list(results) == list(named.values())
        assert results.errors == {}
        group = pool.run_many(named.values(), concurrency=99)
        with group:
            assert group.concurrency == 3
            seen = sorted(done.index for done in group)
        assert seen == list(range(len(named)))
        settle(pool)


def test_more_queries_than_the_waiting_queue(fb, config):
    with fb.Pool(config(pool_size=2, max_waiters=4)) as pool:
        many = [fb.Query.fetch_val("SELECT CAST(? AS INTEGER) FROM RDB$DATABASE", [i])
                for i in range(300)]
        results = pool.run_all(many)
        assert [results[q] for q in many] == list(range(300))
        results = asyncio.run(pool.run_all_async(many))
        assert [results[q] for q in many] == list(range(300))
        assert settle(pool)["timeouts"] == 0


def test_fail_fast_cancels_the_rest_and_collects_errors_otherwise(fb, config):
    with fb.Pool(config(pool_size=2)) as pool:
        bad = fb.Query.fetch_val("SELECT NO_SUCH_COLUMN FROM RDB$DATABASE")
        slow = [fb.Query.fetch_val(LONG, timeout=None) for _ in range(3)]
        with pytest.raises(fb.QueryError):
            pool.run_all([bad, *slow])
        stats = settle(pool)
        assert stats["cancellations"] >= 1
        good = fb.Query.fetch_val("SELECT 1 FROM RDB$DATABASE")
        results = pool.run_all([bad, good], fail_fast=False)
        assert results[good] == 1
        assert set(results.errors) == {bad}
        with pytest.raises(fb.QueryError):
            results[bad]


def test_leaving_the_block_cancels_and_outside_callers_are_served(fb, config):
    with fb.Pool(config(pool_size=2, min_idle=2)) as pool:
        slow = [fb.Query.fetch_val(LONG, timeout=None) for _ in range(6)]
        with pool.run_many(slow, concurrency=1):
            # The group started one long query and holds one of the two slots.
            end = time.monotonic() + 5
            while pool.stats()["leased"] == 0:
                assert time.monotonic() < end
                time.sleep(0.01)
            started = time.monotonic()
            assert pool.fetch_val("SELECT 5 FROM RDB$DATABASE") == 5
            assert time.monotonic() - started < 2
        stats = settle(pool)
        assert stats["cancellations"] >= 1
        assert pool.fetch_val("SELECT 6 FROM RDB$DATABASE") == 6


def test_group_deadline_cancels_unfinished_queries(fb, config):
    with fb.Pool(config(pool_size=2)) as pool:
        slow = [fb.Query.fetch_val(LONG, timeout=None) for _ in range(2)]
        started = time.monotonic()
        with pytest.raises(fb.QueryTimeoutError) as caught:
            pool.run_all(slow, timeout=0.3)
        assert caught.value.phase == "group" and isinstance(caught.value, TimeoutError)
        assert time.monotonic() - started < 5
        settle(pool)


def test_async_groups(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=3)) as pool:
            named = queries(fb)
            results = await pool.run_all_async(named.values())
            expected = sequential(pool)
            assert {name: results[q] for name, q in named.items()} == expected
            slow = [fb.Query.fetch_val(LONG, timeout=None) for _ in range(4)]
            async with pool.run_many_async(slow, concurrency=2) as group:
                assert group.concurrency == 2
                end = time.monotonic() + 5
                while pool.stats()["leased"] < 2:  # entering started the first window
                    assert time.monotonic() < end
                    await asyncio.sleep(0.01)
            with pytest.raises(fb.QueryTimeoutError):
                await pool.run_all_async(slow, timeout=0.3)
            end = time.monotonic() + 5
            while (stats := pool.stats())["leased"] + stats["cleaning"]:
                assert time.monotonic() < end, stats
                await asyncio.sleep(0.01)
            assert await pool.fetch_val_async("SELECT 7 FROM RDB$DATABASE") == 7
    asyncio.run(run())


def test_models_and_invalid_groups(fb, config):
    pydantic = pytest.importorskip("pydantic")

    class Row(pydantic.BaseModel):
        ID: int
        AMOUNT: Decimal | None

    with fb.Pool(config(pool_size=2)) as pool:
        rows = fb.Query.fetch_all(VALUES, model=Row)
        first = fb.Query.fetch_one(VALUES, model=Row)
        results = pool.run_all([rows, first])
        assert results[rows] == pool.fetch_all(VALUES, model=Row)
        assert results[first] == results[rows][0]
        with pytest.raises(fb.ArgumentError):
            pool.run_all([rows, rows])
        with pytest.raises(fb.ArgumentError):
            pool.run_all(["SELECT 1 FROM RDB$DATABASE"])
        with pytest.raises(fb.ArgumentError):
            pool.run_all([rows], concurrency=0)
