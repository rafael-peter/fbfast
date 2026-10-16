#!/usr/bin/env python3
"""Reject version/tag mismatches and incomplete publication matrices before upload."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import tomllib
from zipfile import ZipFile

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {"aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc",
           "aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"}
PYTHONS = {"3.11", "3.12", "3.13", "3.14"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", type=Path)
    args = parser.parse_args()
    version = tomllib.loads((ROOT / "pyproject.toml").read_text())["project"]["version"]
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    python = re.search(r'__version__ = "([^"]+)"', (ROOT / "python/fbfast/__init__.py").read_text())
    if cargo != version or python is None or python[1] != version:
        raise SystemExit("Set the same version in pyproject.toml, Cargo.toml and python/fbfast/__init__.py")
    if os.environ.get("GITHUB_REF_TYPE") == "tag" and os.environ.get("GITHUB_REF_NAME") != f"v{version}":
        raise SystemExit(f"Release tag must equal v{version}")
    if args.artifacts is None:
        print(f"Version/tag verified: {version}")
        return
    found = set()
    for wheel in args.artifacts.glob("*.whl"):
        with ZipFile(wheel) as archive:
            info = json.loads(archive.read("fbfast/_build_info.json"))
            key = (info["target"], info["python_version"])
            if key in found or info["version"] != version or info["profile"] != "release":
                raise SystemExit(f"Duplicate or mismatched wheel: {wheel.name}")
            for name, expected in info["files"].items():
                content = archive.read("fbfast/" + name.replace("\\", "/"))
                if hashlib.sha256(content).hexdigest() != expected:
                    raise SystemExit(f"Wheel inventory hash mismatch: {wheel.name}: {name}")
            found.add(key)
    expected = {(target, python) for target in TARGETS for python in PYTHONS}
    if found != expected:
        raise SystemExit(f"Incomplete release matrix: missing={expected - found}; unexpected={found - expected}")
    sources = list(args.artifacts.glob("*.tar.gz"))
    if len(sources) != 1 or sources[0].name != f"fbfast-{version}.tar.gz":
        raise SystemExit("Release requires exactly one matching source distribution")
    print(f"Verified {len(found)} wheels and one source distribution for {version}")


if __name__ == "__main__":
    main()
