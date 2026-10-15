import decimal
import os
import signal
import socket
import threading
import time

import pytest

LONG = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) FROM N A CROSS JOIN N B CROSS JOIN N C"


def test_keyboard_interrupt_requests_native_cancel(fb, config):
    with fb.Pool(config(pool_size=1, min_idle=1)) as pool:
        original = pool.fetch_val("SELECT CURRENT_CONNECTION FROM RDB$DATABASE")
        timer = threading.Timer(0.1, lambda: os.kill(os.getpid(), signal.SIGINT))
        start = time.monotonic()
        timer.start()
        try:
            with pytest.raises(KeyboardInterrupt):
                pool.fetch_val(LONG, timeout=None)
        finally:
            timer.join()
        assert time.monotonic() - start < 3
        assert pool.fetch_val("SELECT 42 FROM RDB$DATABASE") == 42
        assert pool.fetch_val("SELECT CURRENT_CONNECTION FROM RDB$DATABASE") != original
        assert pool.stats()["native_cancels"] == 1
        assert pool.stats()["control_threads"] == 1
    assert pool.stats()["control_threads"] == 0


def test_deadline_covers_python_result_conversion(fb, config, monkeypatch):
    original_decimal = decimal.Decimal

    class SlowDecimal(original_decimal):
        def __new__(cls, *args):
            time.sleep(0.1)
            return super().__new__(cls, *args)

    with fb.Pool(config(pool_size=1, query_timeout=0.05)) as pool:
        monkeypatch.setattr(decimal, "Decimal", SlowDecimal)
        with pytest.raises(fb.QueryTimeoutError) as error:
            pool.fetch_val("SELECT AMOUNT FROM FBFAST_TYPES WHERE ID=1")
        assert error.value.phase == "python_conversion"
        assert pool.stats()["idle"] == 1  # native cleanup preceded Python conversion
        assert pool.stats()["timeouts"] == 1
        assert pool.fetch_val("SELECT AMOUNT FROM FBFAST_TYPES WHERE ID=1", timeout=None) == original_decimal("-12345678.9012")


def test_native_timeout_has_public_timeout_type(fb, config):
    with fb.Pool(config(pool_size=1)) as pool:
        with pytest.raises(fb.QueryTimeoutError):
            pool.fetch_val(LONG, timeout=0.1)
        assert pool.fetch_val("SELECT 12 FROM RDB$DATABASE") == 12
        assert pool.stats()["timeouts"] == 1


def test_close_wait_checks_python_signals_while_attach_is_pending(fb, config):
    accepted, release = threading.Event(), threading.Event()
    errors = []
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        listener.settimeout(5)

        def peer():
            connection, _ = listener.accept()
            with connection:
                accepted.set()
                release.wait(5)  # bounded even when an assertion fails

        pool = fb.Pool(config(port=listener.getsockname()[1], pool_size=1,
                              acquire_timeout=0.05, close_timeout=5))

        def query():
            try:
                pool.fetch_val("SELECT 1 FROM RDB$DATABASE")
            except BaseException as error:
                errors.append(error)

        server = threading.Thread(target=peer)
        caller = threading.Thread(target=query)
        server.start()
        caller.start()
        timer = None
        try:
            assert accepted.wait(3)
            caller.join(1)
            assert not caller.is_alive()
            assert len(errors) == 1 and isinstance(errors[0], fb.PoolTimeoutError)
            assert pool.stats()["opening"] == 1
            timer = threading.Timer(0.1, lambda: os.kill(os.getpid(), signal.SIGINT))
            timer.start()
            start = time.monotonic()
            with pytest.raises(KeyboardInterrupt):
                pool.close_sync()
            assert time.monotonic() - start < 1
            assert pool.stats()["opening"] == 1
        finally:
            if timer is not None:
                timer.join()
            release.set()
            caller.join(3)
            server.join(3)
            pool.close_sync()
        assert pool.stats()["closed"] == 1
