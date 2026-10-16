#!/usr/bin/env python3
"""Fetch a pinned official SDK into the workspace; never run its system installer."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import urllib.request
import zipfile


def download(destination: Path) -> Path:
    registry = json.loads(Path(__file__).with_name("release_assets.json").read_text())
    key = f"{platform.system()}-{platform.machine()}"
    if key not in registry["assets"]:
        raise RuntimeError(f"No pinned Firebird SDK for {key}")
    asset = registry["assets"][key]
    destination.mkdir(parents=True, exist_ok=True)
    archive = destination / asset["name"]
    if not archive.exists():
        url = f"https://github.com/FirebirdSQL/firebird/releases/download/{registry['tag']}/{asset['name']}"
        pending = archive.with_suffix(archive.suffix + ".pending")
        with urllib.request.urlopen(url, timeout=120) as source, pending.open("wb") as out:
            while chunk := source.read(1024 * 1024):
                out.write(chunk)
        pending.replace(archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != asset["sha256"]:
        raise RuntimeError(f"SDK checksum mismatch: {archive.name}")
    unpacked = destination / key
    if platform.system() == "Darwin":
        if not unpacked.exists():
            subprocess.run(["pkgutil", "--expand-full", str(archive), str(unpacked)], check=True)
        roots = list(unpacked.glob("*.pkg/Payload"))
        root = next((p for p in roots if (p / "Headers/firebird/Interface.h").is_file()), None)
    elif platform.system() == "Windows":
        if not unpacked.exists():
            with zipfile.ZipFile(archive) as bundle:
                bundle.extractall(unpacked)
        root = unpacked
    else:
        root = unpacked / "opt/firebird"
        if not unpacked.exists():
            with tarfile.open(archive) as outer:
                payload = next(m for m in outer.getmembers() if m.name.endswith("/buildroot.tar.gz"))
                with tarfile.open(fileobj=outer.extractfile(payload), mode="r:gz") as inner:
                    # Ignore the installer's /usr symlinks. Only the local SDK
                    # subtree is extracted, with path/link traversal filtering.
                    members = [m for m in inner.getmembers()
                               if m.name == "./opt/firebird" or m.name.startswith("./opt/firebird/")]
                    inner.extractall(unpacked, members=members, filter="data")
    if root is None or not any((root / p).is_file() for p in (
        "Headers/firebird/Interface.h", "include/firebird/Interface.h",
    )):
        raise RuntimeError("Extracted SDK has no official Interface.h")
    root = root.resolve()
    (root / ".fbfast-ci-sdk.json").write_text(json.dumps({
        "root": str(root), "asset": asset["name"], "sha256": asset["sha256"],
    }) + "\n")
    return root


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path(".tools/firebird-sdk"))
    args = parser.parse_args()
    root = download(args.out.resolve())
    if output := os.environ.get("GITHUB_ENV"):
        with open(output, "a") as file:
            file.write(f"FBFAST_CLIENT_ROOT={root}\n")
    print(root)


if __name__ == "__main__":
    main()
