"""Published deterministic SQL and independent correctness oracle."""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
from itertools import zip_longest

import orjson


@dataclass(frozen=True)
class Case:
    name: str
    rows: int
    width: int = 2
    pk: bool = False
    natural: bool = False

    @property
    def names(self):
        return ["ID", "V"] + [f"T{i}" for i in range(self.width - 2)]

    @property
    def sql(self):
        columns = ["ID", "V"] + [f"TXT AS T{i}" for i in range(self.width - 2)]
        if self.natural:
            return f"SELECT {', '.join(columns)} FROM FBFAST_BENCH ROWS ?"
        predicate = "ID = ?" if self.pk else "ID <= ?"
        return f"SELECT {', '.join(columns)} FROM FBFAST_BENCH WHERE {predicate} ORDER BY ID"

    @property
    def params(self):
        return [543210 if self.pk else self.rows]

    @property
    def batch_row_bound(self):
        # Conservative for decoded Python rows as well as Arrow buffers. This is
        # a batching budget, not a total-result/RSS quota. Matches no universal
        # object-size claim; the actual process RSS is measured separately.
        return 128 + (self.width - 2) * 256

    def expected(self):
        indices = [543210] if self.pk else range(1, self.rows + 1)
        for i in indices:
            text = None if i % 7 == 0 else f"Olá 日本語 {i}"
            yield (i, i * 17 % 100003 - 50001, *([text] * (self.width - 2)))


CASES = {case.name: case for case in (
    Case("pk", 1, pk=True), Case("narrow100", 100), Case("narrow1000", 1000),
    Case("narrow100k", 100_000), Case("narrow1m", 1_000_000),
    Case("wide16", 10_000, 16), Case("wide64", 10_000, 64),
    Case("scan100k", 100_000, natural=True), Case("scan1m", 1_000_000, natural=True),
    Case("scanwide16", 10_000, 16, natural=True), Case("scanwide64", 10_000, 64, natural=True),
)}


def validate(case, rows):
    """Full ordered equality, multiplicity, exact types, NULL and SHA; never assert."""
    digest = hashlib.sha256()
    count = 0
    sentinel = object()
    if case.natural:
        # No ORDER BY: normalize only after measured materialization/RSS. The
        # immutable fixture is inserted in PK order; selection is verified, not
        # assumed. A future changed plan selecting a different prefix fails.
        rows = sorted(rows, key=lambda row: row[0])
    for actual, expected in zip_longest(rows, case.expected(), fillvalue=sentinel):
        if actual is sentinel or expected is sentinel:
            raise ValueError(f"Incorrect row count at {count} in {case.name}")
        if tuple(actual) != expected or any(type(a) is not type(b)
                                           for a, b in zip(actual, expected, strict=True)):
            raise ValueError(f"Incorrect value/type at row {count} in {case.name}")
        digest.update(orjson.dumps(actual))
        digest.update(b"\n")
        count += 1
    return {"rows": count, "sha256": digest.hexdigest()}


def result_rows(output, kind, names):
    if kind == "arrow":
        import pyarrow as pa
        expected = [pa.int32(), pa.int32()] + [pa.large_string()] * (len(names) - 2)
        if output.column_names != names or list(output.schema.types) != expected:
            raise ValueError("Incorrect Arrow names or physical types")
        for batch in output.to_batches(max_chunksize=5000):
            yield from zip(*(column.to_pylist() for column in batch.columns), strict=True)
    elif kind == "polars":
        import polars as pl
        expected = [pl.Int32, pl.Int32] + [pl.String] * (len(names) - 2)
        if output.columns != names or output.dtypes != expected:
            raise ValueError("Incorrect Polars names or physical types")
        yield from output.iter_rows()
    elif kind in ("dicts", "json"):
        if kind == "json":
            output = orjson.loads(output)
        for row in output:
            if list(row) != names:
                raise ValueError("Incorrect result column names or order")
            yield tuple(row[name] for name in names)
    else:
        yield from output
