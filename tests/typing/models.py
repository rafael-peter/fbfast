"""Static consumer contract; checked with mypy, not executed."""
from typing import Any, assert_type
from pydantic import BaseModel
import fbfast
from fbfast import Pool, Snapshot


class Order(BaseModel):
    ID: int


def sync(pool: Pool, snapshot: Snapshot, dynamic: bool) -> None:
    assert_type(pool.fetch_all('sql', model=Order), list[Order])
    assert_type(pool.fetch_one('sql', model=Order), Order | None)
    assert_type(snapshot.fetch_all('sql', model=Order), list[Order])
    assert_type(snapshot.fetch_one('sql', model=Order), Order | None)
    assert_type(fbfast.fetch_all('sql', model=Order), list[Order])
    assert_type(fbfast.fetch_one('sql', model=Order), Order | None)
    assert_type(pool.fetch_all('sql'), list[tuple[Any, ...]])
    assert_type(pool.fetch_all('sql', as_dict=True), list[dict[str, Any]])
    assert_type(pool.fetch_one('sql', as_dict=True), dict[str, Any] | None)
    assert_type(pool.fetch_one('sql'), tuple[Any, ...] | None)
    assert_type(pool.fetch_all('sql', as_dict=dynamic), list[tuple[Any, ...]] | list[dict[str, Any]])


async def asynchronous(pool: Pool, snapshot: Snapshot) -> None:
    assert_type(await pool.fetch_all_async('sql', model=Order), list[Order])
    assert_type(await pool.fetch_one_async('sql', model=Order), Order | None)
    assert_type(await snapshot.fetch_all_async('sql', model=Order), list[Order])
    assert_type(await snapshot.fetch_one_async('sql', model=Order), Order | None)
    assert_type(await fbfast.fetch_all_async('sql', model=Order), list[Order])
    assert_type(await fbfast.fetch_one_async('sql', model=Order), Order | None)
