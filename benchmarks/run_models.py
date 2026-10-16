#!/usr/bin/env python3
"""Measure model adaptation only: prepared dictionaries, no SQL/network/native fetch.

Run with PYTHONPATH=dist/vendor python benchmarks/run_models.py.
Raw-dict results reuse the prepared list; they are a dispatch baseline, not a
measurement of real dictionary materialization. All model paths validate and
allocate the same objects. Output includes every sample and environment details.
"""
from __future__ import annotations

import argparse
import gc
import json
import platform
import random
import statistics
import time
from decimal import Decimal
from typing import Any

import pydantic
from pydantic import BaseModel
from fbfast.row_queries import RowQueries, _DefaultTimeout


class Order(BaseModel):
    ID: int
    AMOUNT: Decimal
    LABEL: str


class PreparedRows(RowQueries):
    """Replace database work only; exercise the real public adaptation method."""

    def __init__(self, rows: list[dict[str, Any]]) -> None:
        self.data = rows

    @property
    def _executor(self) -> Any:
        return self

    def _timeout(self, value: float | None | _DefaultTimeout) -> None:
        return None

    def rows(self, *args: Any, **kwargs: Any) -> list[dict[str, Any]]:
        return self.data


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--sizes', type=int, nargs='+', default=[1, 1000, 100000])
    parser.add_argument('--rounds', type=int, default=9)
    args = parser.parse_args()
    if args.rounds < 1 or any(size < 1 for size in args.sizes):
        parser.error('sizes and rounds must be positive')
    report: dict[str, Any] = {
        'scope': 'Adaptation only; prepared dictionaries, no database or native materialization',
        'python': platform.python_version(), 'platform': platform.platform(),
        'pydantic': pydantic.__version__, 'results': [],
    }
    rng = random.Random(42)
    for size in args.sizes:
        query = PreparedRows([{'ID': i, 'AMOUNT': Decimal('123.4500'), 'LABEL': 'Order'}
                              for i in range(size)])
        cases = {
            'raw_dict_dispatch': lambda: query.fetch_all('prepared', as_dict=True),
            'manual_constructor': lambda: [Order(**row) for row in query.fetch_all('prepared', as_dict=True)],
            'manual_model_validate': lambda: [Order.model_validate(row) for row in query.fetch_all('prepared', as_dict=True)],
            'model_parameter': lambda: query.fetch_all('prepared', model=Order),
        }
        expected = cases['manual_constructor']()
        for name, operation in cases.items():
            if name != 'raw_dict_dispatch':
                assert operation() == expected
        del expected
        samples: dict[str, list[float]] = {name: [] for name in cases}
        for _ in range(args.rounds):
            order = list(cases)
            rng.shuffle(order)
            for name in order:
                gc.collect()
                started = time.perf_counter()
                result = cases[name]()
                elapsed = time.perf_counter() - started
                assert len(result) == size
                samples[name].append(elapsed)
                del result
        report['results'].append({'rows': size, 'cases': {
            name: {'median_ms': statistics.median(values) * 1000, 'samples_s': values}
            for name, values in samples.items()
        }})
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
