"""Public row contracts with an in-memory executor; no database needed."""
import asyncio
from decimal import Decimal
import os
import subprocess
import sys
import threading
from types import SimpleNamespace

import pytest
from pydantic import BaseModel, ConfigDict, Field, ValidationError, field_validator

import fbfast
from fbfast.row_queries import RowQueries


class Order(BaseModel):
    model_config = ConfigDict(strict=True)
    id: int = Field(validation_alias="ID")
    amount: Decimal | None = Field(validation_alias="AMOUNT")


class Executor:
    def __init__(self, rows):
        self.data = rows
        self.calls = []

    def rows(self, sql, params, **options):
        self.calls.append((sql, params, options))
        rows = self.data if options['as_dict'] else [tuple(row.values()) for row in self.data]
        return (rows[0] if rows else None) if options.get('first') else rows

    async def rows_async(self, sql, params, **options):
        return self.rows(sql, params, **options)


class Query(RowQueries):
    def __init__(self, rows):
        self.core = Executor(rows)
        self._config = SimpleNamespace(query_timeout=3.0)

    @property
    def _executor(self):
        return self.core


@pytest.mark.parametrize('asynchronous', [False, True])
@pytest.mark.parametrize('many', [False, True])
@pytest.mark.parametrize('empty', [False, True])
def test_typed_rows_and_empty(asynchronous, many, empty):
    query = Query([] if empty else [{'ID': 7, 'AMOUNT': Decimal('12.3400')}])
    name = 'fetch_all' if many else 'fetch_one'
    if asynchronous:
        name += '_async'
    result = getattr(query, name)('query', [7], model=Order, timeout=None)
    if asynchronous:
        result = asyncio.run(result)
    expected = Order(ID=7, AMOUNT=Decimal('12.3400'))
    assert result == ([] if empty else [expected]) if many else result == (None if empty else expected)
    assert query.core.calls == [('query', [7], {
        **({} if many else {'first': True}), 'as_dict': True, 'timeout': None,
    })]


@pytest.mark.parametrize('name', ['fetch_all', 'fetch_one', 'fetch_all_async', 'fetch_one_async'])
def test_validation_errors_are_not_swallowed(name):
    query = Query([{'ID': 'not-an-int', 'AMOUNT': None}])
    with pytest.raises(ValidationError) as error:
        result = getattr(query, name)('query', model=Order)
        if name.endswith('_async'):
            asyncio.run(result)
    assert error.value.errors()[0]['loc'] == ('ID',)
    # Validation failures do not poison ordinary subsequent fetches.
    assert query.fetch_one('query') == ('not-an-int', None)


def test_defaults_null_and_custom_validator():
    class Label(BaseModel):
        ID: int
        label: str = 'new'

        @field_validator('ID')
        @classmethod
        def positive(cls, value):
            if value < 0:
                raise ValueError('positive only')
            return value

    query = Query([{'ID': '7', 'EXTRA': 'ignored'}])
    assert query.fetch_one('query', model=Label, as_dict=True) == Label(ID=7)
    assert Query([{'ID': 7, 'AMOUNT': None}]).fetch_one('query', model=Order).amount is None
    with pytest.raises(ValidationError):
        Query([{'ID': -1}]).fetch_all('query', model=Label)


def test_invalid_class_rejected_before_query():
    query = Query([])
    with pytest.raises(TypeError, match='BaseModel subclass'):
        query.fetch_all('query', model=dict)
    assert not query.core.calls


def test_async_validation_runs_off_loop():
    loop_thread = threading.get_ident()
    threads = []

    class ThreadModel(BaseModel):
        ID: int

        @field_validator('ID')
        @classmethod
        def record(cls, value):
            threads.append(threading.get_ident())
            return value

    async def run():
        query = Query([{'ID': 1}])
        await query.fetch_all_async('query', model=ThreadModel)
        await query.fetch_one_async('query', model=ThreadModel)

    asyncio.run(run())
    assert len(threads) == 2 and all(thread != loop_thread for thread in threads)


def test_named_delegates(monkeypatch):
    import fbfast.api as api
    query = Query([{'ID': 7, 'AMOUNT': None}])
    names = []

    def get_pool(name):
        names.append(name)
        return query

    monkeypatch.setattr(api, 'get_pool', get_pool)
    assert fbfast.fetch_one('query', model=Order, pool='reports').id == 7
    assert fbfast.fetch_all('query', model=Order, pool='reports')[0].id == 7
    assert asyncio.run(fbfast.fetch_one_async('query', model=Order, pool='reports')).id == 7
    assert asyncio.run(fbfast.fetch_all_async('query', model=Order, pool='reports'))[0].id == 7
    assert names == ['reports'] * 4


def test_base_import_and_raw_rows_without_pydantic():
    # A fresh interpreter blocks Pydantic imports even when it is installed.
    script = '''
import sys
class Block:
    def find_spec(self, fullname, *args):
        if fullname == 'pydantic' or fullname.startswith('pydantic.'):
            raise ImportError('blocked optional dependency')
sys.meta_path.insert(0, Block())
import fbfast
from fbfast.row_queries import RowQueries
class Executor:
    def rows(self, *args, **kwargs): return [(1,)]
class Query(RowQueries):
    _executor = Executor()
    def _timeout(self, value): return None
query = Query()
assert query.fetch_all('query') == [(1,)]
assert 'pydantic' not in sys.modules
try:
    query.fetch_all('query', model=dict)
except ImportError as exc:
    assert 'fbfast[pydantic]' in str(exc)
else:
    raise AssertionError('missing optional dependency did not fail')
'''
    subprocess.run([sys.executable, '-c', script], check=True, env=os.environ.copy())


@pytest.mark.parametrize('as_dict', [False, True])
@pytest.mark.parametrize('name', ['fetch_all', 'fetch_one', 'fetch_all_async', 'fetch_one_async'])
def test_raw_contracts_unchanged(name, as_dict):
    query = Query([{'ID': 7, 'AMOUNT': None}])
    result = getattr(query, name)('query', as_dict=as_dict)
    if name.endswith('_async'):
        result = asyncio.run(result)
    row = {'ID': 7, 'AMOUNT': None} if as_dict else (7, None)
    assert result == ([row] if 'all' in name else row)
    assert query.core.calls[0][2]['timeout'] == 3.0
