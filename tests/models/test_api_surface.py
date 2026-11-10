"""Public API naming, typing helpers and error hierarchy; no database required."""
from __future__ import annotations

import asyncio
import inspect
import pickle
import typing

import pytest

import fbfast as fb


def offline_config(**options):
    return fb.FirebirdConfig(host="127.0.0.1", database="never-opened.fdb", user="NOBODY",
                             password="unused", env_file=False, **options)


def test_lifecycle_rule_no_suffix_blocks_and_async_names_are_coroutines():
    for name in ("warmup", "close", "clear_statement_cache"):
        assert not inspect.iscoroutinefunction(getattr(fb.Pool, name)), name
    for name in ("warmup_async", "aclose", "clear_statement_cache_async"):
        assert inspect.iscoroutinefunction(getattr(fb.Pool, name)), name
    assert not inspect.iscoroutinefunction(fb.Snapshot.close)
    assert inspect.iscoroutinefunction(fb.Snapshot.aclose)
    assert not inspect.iscoroutinefunction(fb.init_pool)
    assert not inspect.iscoroutinefunction(fb.close_pool)
    assert inspect.iscoroutinefunction(fb.init_pool_async)
    assert inspect.iscoroutinefunction(fb.close_pool_async)


def test_deprecated_sync_aliases_warn_and_still_work():
    pool = fb.Pool(offline_config(min_idle=0))
    with pytest.warns(DeprecationWarning, match="Pool.close_sync"):
        pool.close_sync()
    with pytest.warns(DeprecationWarning, match="close_pool_sync"):
        fb.close_pool_sync("absent-name-is-a-no-op")
    with pytest.warns(DeprecationWarning, match="snapshot_async"):
        snapshot = fb.Pool(offline_config(min_idle=0)).snapshot_async()
    assert isinstance(snapshot, fb.Snapshot)


def test_awaiting_a_renamed_sync_method_fails_loudly():
    pool = fb.Pool(offline_config(min_idle=0))

    async def legacy():
        await pool.close()  # type: ignore[func-returns-value]

    with pytest.raises(TypeError):
        asyncio.run(legacy())
    assert pool.stats()["closing_pool"] is True


def test_stats_keys_match_the_typed_dict():
    pool = fb.Pool(offline_config(min_idle=0))
    try:
        stats = pool.stats()
        assert set(stats) == set(typing.get_type_hints(fb.PoolStats))
        assert pool.diagnostics() == []
        assert pool.session_info() is None
    finally:
        pool.close()


def test_default_timeout_and_parameter_types_are_public():
    assert repr(fb.DEFAULT) == "fbfast.DEFAULT"
    assert isinstance(fb.DEFAULT, fb.DefaultTimeout)
    assert pickle.loads(pickle.dumps(fb.DEFAULT)) is fb.DEFAULT
    signature = inspect.signature(fb.Pool.fetch_all)
    assert signature.parameters["timeout"].default is fb.DEFAULT
    assert offline_config().snapshot_timeout == 300.0
    assert offline_config(snapshot_timeout=None).snapshot_timeout is None


@pytest.mark.parametrize(("error", "bases"), [
    (fb.QueryTimeoutError, (fb.QueryError, TimeoutError)),
    (fb.PoolTimeoutError, (fb.PoolError, TimeoutError)),
    (fb.DatabaseConnectionError, (fb.QueryError, ConnectionError)),
    (fb.ConcurrentUseError, (fb.PoolBusyError,)),
    (fb.UsageError, (fb.FbFastError, RuntimeError)),
    (fb.ArgumentError, (fb.FbFastError, ValueError, TypeError)),
    (fb.PoolNotInitializedError, (fb.PoolError,)),
])
def test_error_hierarchy(error, bases):
    for base in bases:
        assert issubclass(error, base)
    instance = error("message")
    assert (instance.phase, instance.sqlstate, instance.native_codes, instance.cleanup_errors) == (
        "python", None, [], [])
    assert error.__module__ == "fbfast"
    clone = pickle.loads(pickle.dumps(instance))
    assert type(clone) is error and str(clone) == "message"


def test_native_configuration_errors_carry_every_attribute():
    with pytest.raises(fb.ConfigurationError) as caught:
        fb.FirebirdConfig(database="x", user="u", password="p", env_file=False, pool_size=0)
    error = caught.value
    assert error.phase == "config" and error.sqlstate is None
    assert error.native_codes == [] and error.cleanup_errors == []
    with pytest.raises(fb.ConfigurationError) as caught:
        fb.FirebirdConfig(database="x", user="u", password="p", env_file=False, unknown=object())
    assert caught.value.phase == "config"


def test_python_side_misuse_uses_the_hierarchy():
    pool = fb.Pool(offline_config(min_idle=0))
    try:
        snapshot = pool.snapshot()
        with pytest.raises(fb.UsageError, match="Enter an active snapshot"):
            snapshot.fetch_all("SELECT 1 FROM RDB$DATABASE")
        with pytest.raises(fb.ArgumentError):
            pool.fetch_polars("SELECT 1 FROM RDB$DATABASE", unsupported="bogus")  # type: ignore[arg-type]
        with pytest.raises(fb.ArgumentError):
            pool.fetch_all("SELECT 1 FROM RDB$DATABASE", model=dict)  # type: ignore[type-var]
    finally:
        pool.close()


def test_documented_defaults_match_the_runtime(monkeypatch):
    import ast
    import os
    import sys
    from pathlib import Path

    root = Path(__file__).resolve().parents[2]
    sys.path.insert(0, str(root / "scripts"))
    from gen_config_docs import entries

    for key in list(os.environ):
        if key.startswith("FB_"):
            monkeypatch.delenv(key)
    config = offline_config()
    for name, _, default, _ in entries():
        if default == "required" or name in ("host", "database", "user", "password"):
            continue
        expected = ast.literal_eval(default.split(" (")[0])
        assert getattr(config, name) == expected, name
