#!/usr/bin/env python3
"""Commit one bounded test value in this checkout's owned disposable fixture."""
from __future__ import annotations

import argparse

from firebird_fixture import sql, state, status


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("value", type=int, choices=(10, 20))
    args = parser.parse_args()
    fixture, live = state(), status()
    if not live["running"] or any(
        live[key] != fixture[key] for key in ("host", "port", "database", "image")
    ):
        raise SystemExit("Owned fixture identity changed; refusing mutation")
    result = sql(
        f"UPDATE FBFAST_SNAPSHOT SET VALUE_INTEGER = {args.value} WHERE ID = 1;\nCOMMIT;\n",
        fixture["admin_password"],
    )
    if result.returncode:
        raise SystemExit(f"Fixture mutation failed: {result.stderr.strip()}")


if __name__ == "__main__":
    main()
