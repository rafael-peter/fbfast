import os

import pytest


def test_direct_generator_call_is_rejected_by_sql_policy(fb, config):
    with fb.Pool(config()) as pool:
        with pytest.raises(fb.ReadOnlyViolationError):
            pool.fetch_val("SELECT GEN_ID(FBFAST_SIDE_EFFECT_SEQUENCE, 1) FROM RDB$DATABASE")


def test_restricted_role_cannot_use_generator_hidden_in_routine(fb, config):
    with fb.Pool(config()) as pool:
        assert pool.fetch_val(
            "SELECT SEC$ADMIN FROM SEC$USERS WHERE SEC$USER_NAME = CURRENT_USER"
        ) is False
        with pytest.raises(fb.AuthenticationError, match="GENERATOR"):
            pool.fetch_val("SELECT V FROM FBFAST_SIDE_EFFECT")


def test_privileged_routine_proves_read_only_is_not_a_sandbox(fb):
    admin = fb.FirebirdConfig(
        host="127.0.0.1",
        port=int(os.environ["FBFAST_TEST_PORT"]),
        database="/var/lib/firebird/data/fbfast.fdb",
        user="SYSDBA",
        password=os.environ["FBFAST_TEST_ADMIN_PASSWORD"],
        role=None,
        pool_size=1,
        min_idle=1,
        env_file=False,
    )
    with fb.Pool(admin) as pool:
        first = pool.fetch_val("SELECT V FROM FBFAST_SIDE_EFFECT")
        second = pool.fetch_val("SELECT V FROM FBFAST_SIDE_EFFECT")
    assert second == first + 1
