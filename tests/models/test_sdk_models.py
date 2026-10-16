"""Optional real-server checks using the private SDK fixture (no Docker)."""
import asyncio
import os
from pathlib import Path
import sys

import pytest
from pydantic import BaseModel, Field, ValidationError

import fbfast


@pytest.fixture(scope='module')
def sdk_config(tmp_path_factory):
    sdk = os.environ.get('FBFAST_MODEL_TEST_SDK')
    if not sdk:
        pytest.skip('Set FBFAST_MODEL_TEST_SDK to an extracted private Firebird SDK')
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scripts'))
    from ci_fixture import fixture
    with fixture(Path(sdk).resolve(), tmp_path_factory.mktemp('models-sdk')) as config:
        yield fbfast.FirebirdConfig(**config, env_file=False, pool_size=1, min_idle=1)


class Row(BaseModel):
    id: int = Field(validation_alias='ID')
    label: str | None = Field(validation_alias='LABEL')


SQL = 'SELECT ID, LABEL FROM WHEEL_SMOKE ORDER BY ID'


def test_sdk_sync(sdk_config):
    with fbfast.Pool(sdk_config) as pool:
        expected = [Row(ID=1, LABEL='Olá 日本語'), Row(ID=2, LABEL=None)]
        assert pool.fetch_all(SQL, model=Row) == expected
        assert pool.fetch_one(SQL, model=Row) == expected[0]
        assert pool.fetch_one(SQL.replace('ORDER BY ID', 'WHERE ID=99'), model=Row) is None
        with pytest.raises(ValidationError):
            pool.fetch_all('SELECT LABEL AS ID, LABEL FROM WHEEL_SMOKE', model=Row)
        # The only pool slot is reusable after validation fails.
        assert pool.fetch_val('SELECT 1 FROM RDB$DATABASE') == 1
        with pool.snapshot() as snapshot:
            assert snapshot.fetch_all(SQL, model=Row) == expected
            assert snapshot.fetch_one(SQL, model=Row) == expected[0]


def test_sdk_async(sdk_config):
    async def run():
        await fbfast.init_pool(sdk_config, name='model-sdk')
        try:
            rows = await fbfast.fetch_all_async(SQL, pool='model-sdk', model=Row)
            assert [row.id for row in rows] == [1, 2]
            assert await fbfast.fetch_one_async(SQL, pool='model-sdk', model=Row) == rows[0]
            async with fbfast.get_pool('model-sdk').snapshot_async() as snapshot:
                assert await snapshot.fetch_all_async(SQL, model=Row) == rows
                assert await snapshot.fetch_one_async(SQL, model=Row) == rows[0]
        finally:
            await fbfast.close_pool('model-sdk')
    asyncio.run(run())
