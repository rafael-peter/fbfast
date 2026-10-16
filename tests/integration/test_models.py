"""Typed results against the disposable Firebird fixture."""
import asyncio
from decimal import Decimal

import pytest
from pydantic import BaseModel, Field, ValidationError


class Order(BaseModel):
    id: int = Field(validation_alias='ID')
    amount: Decimal | None = Field(validation_alias='AMOUNT')


SQL = 'SELECT ID, AMOUNT FROM FBFAST_TYPES ORDER BY ID'


def test_models_pool_snapshot_and_recovery(fb, config):
    with fb.Pool(config(pool_size=1)) as pool:
        rows = pool.fetch_all(SQL, model=Order)
        assert [row.id for row in rows] == [1, 2]
        assert rows[0].amount == Decimal('-12345678.9012')
        assert rows[1].amount is None
        assert pool.fetch_one(SQL, model=Order) == rows[0]
        assert pool.fetch_one('SELECT ID FROM FBFAST_TYPES WHERE ID=99', model=Order) is None
        assert pool.fetch_all('SELECT ID FROM FBFAST_TYPES WHERE ID=99', model=Order) == []
        with pytest.raises(ValidationError):
            pool.fetch_all('SELECT LABEL AS ID, AMOUNT FROM FBFAST_TYPES', model=Order)
        assert pool.fetch_val('SELECT 1 FROM RDB$DATABASE') == 1
        with pool.snapshot() as snapshot:
            assert snapshot.fetch_all(SQL, model=Order) == rows
            assert snapshot.fetch_one(SQL, model=Order) == rows[0]


def test_models_async_and_named_pool(fb, config):
    async def run():
        await fb.init_pool(config(pool_size=1), name='models')
        try:
            rows = await fb.fetch_all_async(SQL, model=Order, pool='models')
            assert await fb.fetch_one_async(SQL, model=Order, pool='models') == rows[0]
            async with fb.get_pool('models').snapshot_async() as snapshot:
                assert await snapshot.fetch_all_async(SQL, model=Order) == rows
                assert await snapshot.fetch_one_async(SQL, model=Order) == rows[0]
        finally:
            await fb.close_pool('models')
    asyncio.run(run())
