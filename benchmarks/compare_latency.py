#!/usr/bin/env python3
"""Compare two run_latency.py result directories (e.g. 0.2.1 baseline vs 0.3.0)."""
from __future__ import annotations

import argparse
import json
import statistics
from pathlib import Path
from typing import Any


def load(directory: Path) -> tuple[dict[str, Any], dict[tuple[Any, ...], dict[str, Any]]]:
    manifest = json.loads((directory / "manifest.json").read_text())
    raw = json.loads((directory / "raw.json").read_text())
    return manifest, {(r["rtt_ms"], r["driver"], r["case"], r.get("mode"),
                       r.get("wire_compression")): r for r in raw}


def median_ms(record: dict[str, Any] | None) -> float | None:
    if not record or not record["samples_s"]:
        return None
    return statistics.median(record["samples_s"]) * 1000


def trips(record: dict[str, Any] | None) -> str:
    values = (record or {}).get("roundtrips") or []
    # Steady state: the last sample ran on warm connections with cached statements.
    return str(values[-1]) if values else "-"


def cell(value: float | None, failed: dict[str, Any] | None = None) -> str:
    if value is None:
        return "failed" if failed and failed.get("errors") else "-"
    return f"{value:,.1f}"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", type=Path)
    parser.add_argument("new", type=Path)
    parser.add_argument("--out", type=Path, help="default: NEW/comparison.md")
    args = parser.parse_args()
    base_manifest, base = load(args.base)
    new_manifest, new = load(args.new)
    rtts = sorted({key[0] for key in new if key[4] is None})
    cases = list(dict.fromkeys(key[2] for key in new if key[3] is None and key[4] is None))
    lines = [
        f"# Latency: fbfast {base_manifest['version']} vs {new_manifest['version']}", "",
        f"Base `{args.base.name}` (build `{base_manifest['build_id'][:12]}`), new "
        f"`{args.new.name}` (build `{new_manifest['build_id'][:12]}`). Median of "
        f"{new_manifest['samples']} samples per query, milliseconds. Round trips are the "
        "steady-state count from `pool.diagnostics()`. RTT is added by toxiproxy.", "",
        "| RTT | case | 0.2.1 ms | RT | 0.3.0 ms | RT | speedup | firebird-driver ms |",
        "|---:|---|---:|---:|---:|---:|---:|---:|",
    ]
    for rtt in rtts:
        for case in cases:
            old = base.get((rtt, "fbfast", case, None, None))
            now = new.get((rtt, "fbfast", case, None, None))
            driver = new.get((rtt, "firebird-driver", case, None, None))
            a, b = median_ms(old), median_ms(now)
            speed = f"{a / b:,.1f}x" if a and b else "-"
            lines.append(f"| {rtt} | {case} | {cell(a, old)} | {trips(old)} | {cell(b, now)} | "
                         f"{trips(now)} | {speed} | {cell(median_ms(driver), driver)} |")
    counts = sorted({int(key[2].removeprefix("concurrent")) for key in new
                     if key[2].startswith("concurrent")})
    if counts:
        lines += ["", "## Simultaneous small queries (8 connections)", "",
                  "| RTT | queries | 0.2.1 threads | 0.3.0 threads | 0.3.0 run_all | "
                  "0.3.0 run_all_async | firebird-driver threads |",
                  "|---:|---:|---:|---:|---:|---:|---:|"]
    for rtt in rtts:
        for count in counts:
            case = f"concurrent{count}"
            row = [median_ms(base.get((rtt, "fbfast", case, "threads", None))),
                   median_ms(new.get((rtt, "fbfast", case, "threads", None))),
                   median_ms(new.get((rtt, "fbfast", case, "run_all", None))),
                   median_ms(new.get((rtt, "fbfast", case, "run_all_async", None))),
                   median_ms(new.get((rtt, "firebird-driver", case, "threads", None)))]
            lines.append(f"| {rtt} | {count} | " + " | ".join(cell(v) for v in row) + " |")
    bandwidth = [(key, record) for key, record in new.items() if key[4] is not None]
    if bandwidth:
        lines += ["", "## Limited bandwidth (20 ms RTT)", "",
                  "| case | compression | 0.2.1 negotiated | 0.2.1 ms | 0.3.0 negotiated | 0.3.0 ms |",
                  "|---|---|---|---:|---|---:|"]
    for key, record in bandwidth:
        old = base.get(key)
        lines.append(f"| {key[2]} | {key[4]} | {(old or {}).get('compressed', '-')} | "
                     f"{cell(median_ms(old))} | {record['compressed']} | "
                     f"{cell(median_ms(record))} |")
    out = args.out or args.new / "comparison.md"
    out.write_text("\n".join(lines) + "\n")
    print(f"Wrote {out}")


if __name__ == "__main__":
    main()
