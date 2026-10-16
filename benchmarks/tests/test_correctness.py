"""Reject misleading timings from incomplete, reordered or coerced outputs."""
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cases import Case, result_rows, validate  # noqa: E402


@pytest.mark.parametrize("corrupt", [
    lambda rows: rows[:-1],
    lambda rows: [*rows, rows[-1]],
    lambda rows: list(reversed(rows)),
    lambda rows: [(True, rows[0][1]), *rows[1:]],
    lambda rows: [(1, 2), *rows[1:]],
])
def test_corrupt_rows_never_count_as_success(corrupt):
    case = Case("test", 10)
    with pytest.raises(ValueError):
        validate(case, corrupt(list(case.expected())))


def test_null_and_multibyte_values_are_not_normalized():
    case = Case("test", 10, 16)
    rows = list(case.expected())
    validate(case, rows)
    rows[6] = (*rows[6][:2], *([""] * 14))
    with pytest.raises(ValueError):
        validate(case, rows)


@pytest.mark.parametrize("kind", ["arrow", "polars"])
def test_equal_python_values_do_not_hide_wrong_column_width(kind):
    case = Case("test", 1)
    if kind == "arrow":
        import pyarrow as pa
        output = pa.table({"ID": [1], "V": [-49984]})  # inferred int64, expected int32
    else:
        import polars as pl
        output = pl.DataFrame({"ID": [1], "V": [-49984]})
    with pytest.raises(ValueError, match="physical types"):
        validate(case, result_rows(output, kind, case.names))


def test_renamed_dictionary_columns_are_rejected():
    case = Case("test", 1)
    with pytest.raises(ValueError, match="column names"):
        validate(case, result_rows([{"X": 1, "V": -49984}], "dicts", case.names))
