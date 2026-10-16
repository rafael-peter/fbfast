import asyncio
from dataclasses import FrozenInstanceError
from datetime import datetime, time, timedelta, timezone
import json
import pickle
from time import perf_counter
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

import pytest


# UTC expectations are independent of both the client's and Python's zone rules.
CASES = [
    ("2024-02-29T06:49:56.123400Z", "+05:45", "06:49:56.123400", "+05:45"),
    ("2024-11-03T05:30:00Z", "America/New_York", "17:00:00", "America/New_York"),
    ("2024-11-03T06:30:00Z", "America/New_York", "11:00:00", "Europe/Paris"),
    ("2024-03-10T06:59:59.999900Z", "America/New_York", None, None),
    ("2024-03-10T07:00:00Z", "America/New_York", None, None),
    ("2018-01-01T02:00:00Z", "America/Sao_Paulo", "03:00:00", "America/Sao_Paulo"),
    ("2024-01-01T03:00:00Z", "America/Sao_Paulo", "23:59:59.999900", "+00:00"),
    ("0001-01-01T00:00:00Z", "+00:00", "00:00:00", "+00:00"),
    ("9999-12-31T23:59:59.999900Z", "+00:00", "23:59:59.999900", "+00:00"),
    (None, None, None, None),
    ("2024-03-01T13:30:00Z", "-14:00", "13:30:00", "-14:00"),
    ("2024-02-28T10:30:00Z", "+14:00", "10:30:00", "+14:00"),
    ("2024-02-29T06:49:56.123400Z", "Asia/Kathmandu", "06:49:56.123400", "Asia/Kathmandu"),
    ("2024-04-06T14:45:00Z", "Australia/Lord_Howe", None, None),
    ("2024-04-06T15:15:00Z", "Australia/Lord_Howe", None, None),
]
SQL = "SELECT STAMP, CLOCK FROM FBFAST_ZONED ORDER BY ID"


def check(fb, rows):
    for (stamp, clock), (utc, zone, utc_time, time_zone) in zip(rows, CASES, strict=True):
        if utc is None:
            assert stamp is None
        else:
            assert stamp.astimezone(timezone.utc) == datetime.fromisoformat(utc)
            if not zone.startswith(("+", "-")):
                assert stamp.tzinfo.key == zone
        if utc_time is None:
            assert clock is None
        else:
            assert clock == fb.TimeWithZone(time.fromisoformat(utc_time), time_zone)
    # Both occurrences of the repeated wall time must keep distinct instants.
    assert rows[1][0].hour == rows[2][0].hour == 1
    assert rows[1][0].fold == 0 and rows[2][0].fold == 1
    assert rows[3][0].hour == 1 and rows[4][0].hour == 3
    assert rows[13][0].fold == 0 and rows[14][0].fold == 1


@pytest.mark.parametrize("session_zone", ["UTC", "America/Sao_Paulo"])
def test_zoned_stored_rows_stream_snapshot_and_lifetime(fb, config, session_zone):
    with fb.Pool(config(pool_size=1, session_time_zone=session_zone, batch_bytes=32)) as pool:
        rows = pool.fetch_all(SQL)
        check(fb, rows)
        with pool.snapshot() as session:
            with session.stream(SQL, batch_rows=2) as stream:
                retained = list(stream)
            assert session.fetch_all("SELECT STAMP, CLOCK FROM FBFAST_ZONED WHERE 1=0") == []
        # Binding the returned values must preserve even an ambiguous DST fold.
        for stamp, clock in rows:
            returned = pool.fetch_one("SELECT CAST(? AS TIMESTAMP WITH TIME ZONE), CAST(? AS TIME WITH TIME ZONE) FROM RDB$DATABASE", [stamp, clock])
            if stamp is not None:
                assert returned[0].astimezone(timezone.utc) == stamp.astimezone(timezone.utc)
                assert returned[0].fold == stamp.fold
            else:
                assert returned[0] is None
            assert returned[1] == clock
    check(fb, [row for batch in retained for row in batch])
    assert pickle.loads(pickle.dumps(rows[0][1])) == rows[0][1]
    with pytest.raises(FrozenInstanceError):
        rows[0][1].zone = "UTC"


@pytest.mark.parametrize("value", [
    datetime(2024, 11, 3, 1, 30, tzinfo=ZoneInfo("America/New_York"), fold=0),
    datetime(2024, 11, 3, 1, 30, tzinfo=ZoneInfo("America/New_York"), fold=1),
    datetime(2024, 2, 29, 12, 34, 56, 123400, tzinfo=timezone(timedelta(hours=5, minutes=45))),
    datetime(1, 1, 1, tzinfo=timezone.utc),
    datetime(9999, 12, 31, 23, 59, 59, 999900, tzinfo=timezone.utc),
])
def test_zoned_timestamp_bind_preserves_instant_offset_and_fold(fb, config, value):
    with fb.Pool(config(pool_size=1)) as pool:
        sql = "SELECT CAST(? AS TIMESTAMP WITH TIME ZONE) FROM RDB$DATABASE"
        assert pool.explain(sql, [value])
        actual = pool.fetch_val(sql, [value])
        assert actual.astimezone(timezone.utc) == value.astimezone(timezone.utc)
        assert actual.utcoffset() == value.utcoffset()
        assert actual.replace(tzinfo=None) == value.replace(tzinfo=None)
        assert actual.fold == value.fold


@pytest.mark.parametrize("value", [
    datetime(2024, 3, 10, 2, 30, tzinfo=ZoneInfo("America/New_York")),
    datetime(2024, 3, 10, 2, 30, tzinfo=ZoneInfo("America/New_York"), fold=1),
    datetime(2024, 1, 1, tzinfo=timezone(timedelta(seconds=30))),
    datetime(2024, 1, 1, tzinfo=timezone(timedelta(hours=15))),
    datetime(1, 1, 1, tzinfo=timezone(timedelta(hours=1))),
    datetime(9999, 12, 31, 23, tzinfo=timezone(timedelta(hours=-1))),
    datetime(2024, 1, 1, microsecond=1, tzinfo=timezone.utc),
    datetime(2024, 1, 1),
    time(12, tzinfo=timezone.utc),
])
def test_zoned_invalid_bind_rejects_without_loss_and_pool_remains_reusable(fb, config, value):
    with fb.Pool(config(pool_size=1)) as pool:
        with pytest.raises(fb.DataConversionError) as caught:
            pool.fetch_val("SELECT CAST(? AS TIMESTAMP WITH TIME ZONE) FROM RDB$DATABASE", [value])
        assert not caught.value.cleanup_errors
        assert str(value) not in str(caught.value)
        assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1


def test_time_with_zone_validation_and_bind_rejection(fb, config):
    for clock, zone in [(time(0, microsecond=1), "UTC"), (time(0, tzinfo=timezone.utc), "UTC"),
                        (time(0, fold=1), "UTC"), (time(0), ""), (time(0), "secret\0zone")]:
        with pytest.raises(fb.DataConversionError):
            fb.TimeWithZone(clock, zone)
    with fb.Pool(config(pool_size=1)) as pool:
        for zone in ["NoSuch/SecretZone", "+15:00"]:
            with pytest.raises(fb.DataConversionError) as caught:
                pool.fetch_val("SELECT CAST(? AS TIME WITH TIME ZONE) FROM RDB$DATABASE", [fb.TimeWithZone(time(0), zone)])
            assert caught.value.phase == "bind"
            assert zone not in str(caught.value)
        for value in [time(12), time(12, tzinfo=timezone.utc)]:
            with pytest.raises(fb.DataConversionError):
                pool.fetch_val("SELECT CAST(? AS TIME WITH TIME ZONE) FROM RDB$DATABASE", [value])
        with pytest.raises(fb.DataConversionError):
            pool.fetch_val("SELECT CAST(? AS TIME) FROM RDB$DATABASE", [fb.TimeWithZone(time(0), "UTC")])


@pytest.mark.parametrize("format", ["array", "ndjson"])
def test_zoned_json_preserves_utc_and_zone_without_python_temporal_conversion(fb, config, monkeypatch, format):
    import fbfast.temporal as temporal

    def forbidden(*args):
        raise AssertionError("JSON must not construct Python temporal values")

    monkeypatch.setattr(temporal, "_timestamp_from_native", forbidden)
    with fb.Pool(config(pool_size=1, batch_bytes=1)) as pool:
        expected = [{"STAMP": None if utc is None else {"utc": utc, "zone": zone},
                     "CLOCK": None if clock is None else {"utc_time": clock, "zone": clock_zone}}
                    for utc, zone, clock, clock_zone in CASES]
        assert json.loads(pool.fetch_json(SQL)) == expected
        with pool.stream_json(SQL, format=format) as chunks:
            data = b"".join(chunks)
        rows = json.loads(data) if format == "array" else [json.loads(line) for line in data.splitlines()]
        assert rows == expected
        assert json.loads(pool.fetch_json(SQL, orient="values")) == [list(row.values()) for row in expected]


@pytest.mark.parametrize("missing", [False, True])
def test_unavailable_or_disagreeing_python_zone_rules_fail_and_json_still_works(fb, config, monkeypatch, missing):
    import fbfast.temporal as temporal

    class IncompatibleZone:
        def __new__(cls, key):
            if missing:
                raise ZoneInfoNotFoundError(key)
            return timezone.utc

    monkeypatch.setattr(temporal, "ZoneInfo", IncompatibleZone)
    sql = "SELECT STAMP FROM FBFAST_ZONED WHERE ID=2"
    with fb.Pool(config(pool_size=1)) as pool:
        with pytest.raises(fb.DataConversionError) as caught:
            pool.fetch_val(sql)
        assert caught.value.phase == "python_conversion"
        assert not caught.value.cleanup_errors
        assert json.loads(pool.fetch_json(sql))[0]["STAMP"] == {"utc": CASES[1][0], "zone": CASES[1][1]}
        assert pool.fetch_val("SELECT COUNT(*) FROM MON$TRANSACTIONS") == 1


def test_zoned_async_snapshot_and_stream(fb, config):
    async def run():
        async with fb.Pool(config(pool_size=1, batch_bytes=32)) as pool:
            async with pool.snapshot_async() as session:
                check(fb, await session.fetch_all_async(SQL))
                async with session.stream_async(SQL, batch_rows=2) as stream:
                    rows = [row async for batch in stream for row in batch]
                check(fb, rows)
                value = rows[2][0]
                actual = await session.fetch_val_async("SELECT CAST(? AS TIMESTAMP WITH TIME ZONE) FROM RDB$DATABASE", [value])
                assert actual.fold == 1
                async with session.stream_json_async(SQL, format="array") as stream:
                    data = b"".join([chunk async for chunk in stream])
                assert json.loads(data)[2]["STAMP"]["utc"] == CASES[2][0]
    asyncio.run(run())


def test_zoned_async_large_conversion_keeps_loop_responsive(fb, config, record_property):
    sql = ("WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL "
           "SELECT I+1 FROM N WHERE I<1000) SELECT FIRST 50000 "
           "TIMESTAMP '2024-11-03 06:30:00 +00:00' AT TIME ZONE 'America/New_York', "
           "TIME '12:00:00 America/New_York' FROM N A CROSS JOIN N B")

    async def run():
        async with fb.Pool(config(pool_size=1)) as pool:
            ticks, materializing = [], []
            stop = False

            async def heartbeat():
                while not stop:
                    ticks.append(perf_counter())
                    if pool.stats()["leased"] == 0:
                        materializing.append(1)
                    await asyncio.sleep(0.002)

            task = asyncio.create_task(heartbeat())
            try:
                rows = await pool.fetch_all_async(sql)
            finally:
                stop = True
                await task
            assert len(rows) == 50000
            assert all(stamp.fold == 1 and stamp.hour == 1 and value.utc_time == time(17) for stamp, value in rows)
            assert len(materializing) > 5
            gap = max(b - a for a, b in zip(ticks, ticks[1:]))
            record_property("zoned_heartbeat_max_gap_ms", gap * 1000)
            record_property("zoned_heartbeat_during_materialization", len(materializing))
            record_property("build_id", fb.build_info()["build_id"])
            assert gap < 0.150
    asyncio.run(run())
