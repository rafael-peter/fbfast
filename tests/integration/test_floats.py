"""Independent SQL values catch swapped FLOAT/DOUBLE codes in decode and bind."""
import json
import math
import struct

import pytest


@pytest.mark.parametrize("sql_type,width,values", [
    ("FLOAT", "f", [1.25, -1.25, 0.0, -0.0, 1e-40, 3.4028234663852886e38]),
    ("DOUBLE PRECISION", "d", [1.25, -1.25, 0.0, -0.0, 1e-310, 1.7976931348623157e308]),
])
def test_floating_literals_bind_and_all_sinks(fb, config, sql_type, width, values):
    with fb.Pool(config(pool_size=1)) as pool:
        literal = f"SELECT CAST(1.25 AS {sql_type}) AS F FROM RDB$DATABASE"
        assert pool.fetch_val(literal) == 1.25
        assert json.loads(pool.fetch_json(literal)) == [{"F": 1.25}]
        sql = f"SELECT CAST(? AS {sql_type}) AS F FROM RDB$DATABASE"
        for value in values:
            expected = struct.pack(width, value)
            outputs = [pool.fetch_val(sql, [value]),
                       pool.fetch_arrow(sql, [value])["F"][0].as_py(),
                       pool.fetch_polars(sql, [value])["F"][0],
                       json.loads(pool.fetch_json(sql, [value]))[0]["F"]]
            assert all(struct.pack(width, output) == expected for output in outputs)
        if width == "f":
            with pytest.raises(fb.DataConversionError):
                pool.fetch_val(sql, [1e100])
        for value in [math.inf, -math.inf, math.nan]:
            actual = pool.fetch_val(sql, [value])
            assert math.isnan(actual) if math.isnan(value) else actual == value
            with pytest.raises(fb.DataConversionError):
                pool.fetch_json(sql, [value])
            assert json.loads(pool.fetch_json(sql, [value], json_non_finite="null")) == [{"F": None}]
