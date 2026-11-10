"""Validated Pydantic models from SQL rows, sync and async.

Requires the Pydantic extra: pip install "fbfast[pydantic]". Connection settings
come from FB_* environment variables or .env (see basic_query.py).

Firebird returns unquoted column names in UPPER CASE. Either name the fields in
upper case or map them with ``Field(validation_alias=...)``.
"""
from __future__ import annotations

import asyncio

from pydantic import BaseModel, Field

import fbfast


class Relation(BaseModel):
    """Lower-case Python names mapped to the upper-case SQL aliases."""

    relation_id: int = Field(validation_alias="RELATION_ID")
    name: str = Field(validation_alias="NAME")
    is_system: bool = Field(validation_alias="IS_SYSTEM")


class ColumnCount(BaseModel):
    """Field names that match the SQL aliases exactly (upper case)."""

    TABLE_NAME: str
    COLUMNS: int


RELATIONS = """
SELECT RDB$RELATION_ID AS RELATION_ID,
       TRIM(RDB$RELATION_NAME) AS NAME,
       COALESCE(RDB$SYSTEM_FLAG, 0) <> 0 AS IS_SYSTEM
FROM RDB$RELATIONS
WHERE RDB$RELATION_ID < ?
ORDER BY RDB$RELATION_ID
"""

WIDEST = """
SELECT TRIM(RDB$RELATION_NAME) AS TABLE_NAME, COUNT(*) AS COLUMNS
FROM RDB$RELATION_FIELDS
GROUP BY 1
ORDER BY 2 DESC, 1
ROWS 3
"""


def sync_part(pool: fbfast.Pool) -> None:
    relations = pool.fetch_all(RELATIONS, [5], model=Relation)  # list[Relation]
    for relation in relations:
        print(f"  {relation.relation_id:>3} {relation.name:<20} system={relation.is_system}")

    first = pool.fetch_one(RELATIONS, [1], model=Relation)  # Relation | None
    print(f"first: {first!r}")
    missing = pool.fetch_one(RELATIONS, [0], model=Relation)
    print(f"no rows gives {missing!r}")


async def async_part(pool: fbfast.Pool) -> None:
    widest = await pool.fetch_all_async(WIDEST, model=ColumnCount)  # list[ColumnCount]
    for row in widest:
        print(f"  {row.TABLE_NAME:<20} {row.COLUMNS} columns")


def main() -> None:
    config = fbfast.FirebirdConfig(min_idle=1)
    with fbfast.Pool(config) as pool:
        print("relations:")
        sync_part(pool)
        print("widest tables:")
        asyncio.run(async_part(pool))


if __name__ == "__main__":
    main()
