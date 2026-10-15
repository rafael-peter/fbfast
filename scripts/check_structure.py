#!/usr/bin/env python3
"""Enforce the plan's layer graph, module limits, and unsafe-code boundary."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    failures = []
    if "#![forbid(unsafe_code)]" not in (ROOT / "crates/fbfast-core/src/lib.rs").read_text():
        failures.append("Core must forbid unsafe_code")
    for directory in ("crates", "python", "scripts"):
        for path in (ROOT / directory).rglob("*"):
            if path.suffix not in (".rs", ".cpp", ".h", ".py"):
                continue
            lines = len(path.read_text().splitlines())
            if lines > 500:
                failures.append(f"{path.relative_to(ROOT)}: {lines} lines; split required")
            elif lines > 350:
                failures.append(
                    f"{path.relative_to(ROOT)}: {lines} lines; cohesion review required"
                )
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
            cwd=ROOT,
            text=True,
        )
    )
    permitted = {
        "fbfast-python": {"fbfast-core"},
        "fbfast-core": {"fbfast-native"},
        "fbfast-native": set(),
    }
    for package in metadata["packages"]:
        internal = {d["name"] for d in package["dependencies"] if d["name"].startswith("fbfast-")}
        if not internal <= permitted[package["name"]]:
            failures.append(f"Forbidden dependency in {package['name']}: {internal}")
        if package["name"] != "fbfast-python" and any(
            d["name"].startswith("pyo3") for d in package["dependencies"]
        ):
            failures.append(f"Python dependency escaped binding: {package['name']}")
    if failures:
        raise SystemExit("\n".join(failures))
    print("Module sizes and dependency boundaries verified")


if __name__ == "__main__":
    main()
