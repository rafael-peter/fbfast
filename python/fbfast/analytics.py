"""Optional analytical consumers. All cells are built in the native Arrow sink."""
from __future__ import annotations

import importlib
import json
from typing import Any, Literal

from ._native import DataConversionError

Unsupported = Literal["error", "string"]


def _mode(unsupported: Unsupported) -> str:
    if unsupported not in ("error", "string"):
        raise ValueError("unsupported must be 'error' or 'string'")
    return "polars_string" if unsupported == "string" else "polars"


def _extra(name: str) -> Any:
    try:
        return importlib.import_module(name)
    except ModuleNotFoundError as error:
        extra = "arrow" if name == "pyarrow" else "polars"
        raise ImportError(f"Install fbfast[{extra}] to use this analytical output") from error


def _convert(native: Any, arrow: bool) -> Any:
    if arrow:
        return _extra("pyarrow").table(native)
    pl = _extra("polars")
    result = pl.from_arrow(native, rechunk=False)
    if isinstance(result, pl.Series) and isinstance(result.dtype, pl.Struct):
        result = result.struct.unnest()
    expected = {field["name"]: _dtype(pl, field["type"]) for field in json.loads(native.schema_spec)}
    if not isinstance(result, pl.DataFrame) or list(result.schema.items()) != list(expected.items()):
        error = DataConversionError("Polars changed the native schema; lossless conversion could not be verified")
        error.phase, error.sqlstate, error.native_codes, error.cleanup_errors = "polars", None, [], []
        raise error
    return result


def _dtype(pl: Any, spec: dict[str, Any]) -> Any:
    kind = spec["kind"]
    if kind == "Datetime":
        return pl.Datetime(time_unit="us", time_zone=spec["zone"])
    if kind == "Decimal":
        return pl.Decimal(precision=spec["precision"], scale=spec["scale"])
    if kind == "Struct":
        return pl.Struct([pl.Field(field["name"], _dtype(pl, field["type"])) for field in spec["fields"]])
    return getattr(pl, kind)
