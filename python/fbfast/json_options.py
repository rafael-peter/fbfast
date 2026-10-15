"""Shared literal types for native JSON queries and streams."""
from typing import Literal

JsonOrient = Literal["records", "values"]
JsonIntegers = Literal["safe", "number"]
JsonDecimals = Literal["string", "number"]
JsonBinary = Literal["error", "base64"]
JsonNonFinite = Literal["error", "null"]
JsonFormat = Literal["ndjson", "array"]
JsonSettings = tuple[JsonOrient, JsonIntegers, JsonDecimals, JsonBinary, JsonNonFinite]
