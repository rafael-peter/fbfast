"""Collect notices from the exact locked production dependency sources."""

from __future__ import annotations

import json
import shutil
import subprocess
from pathlib import Path


def collect(root: Path, destination: Path, env: dict[str, str], target: str) -> None:
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", target],
            cwd=root,
            env=env,
            text=True,
        )
    )
    # Cargo metadata includes dev/test dependencies too. Traverse only the normal
    # and build edges rooted in the extension shipped to the consumer. Resolve
    # for the wheel's target, excluding dependencies used only on other OSes.
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    extension = next(p["id"] for p in metadata["packages"] if p["name"] == "fbfast-python")
    shipped: set[str] = set()
    pending = [extension]
    while pending:
        identifier = pending.pop()
        if identifier in shipped:
            continue
        shipped.add(identifier)
        for dependency in nodes[identifier]["deps"]:
            if any(kind["kind"] != "dev" for kind in dependency["dep_kinds"]):
                pending.append(dependency["pkg"])
    registry = []
    for package in metadata["packages"]:
        if not package["source"] or package["id"] not in shipped:
            continue
        source = Path(package["manifest_path"]).parent
        notices = [
            p
            for p in source.iterdir()
            if p.is_file()
            and p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE"))
        ]
        if package["license_file"]:
            notices.append(source / package["license_file"])
        if not notices:
            raise RuntimeError(
                f"No license notice found for {package['name']} {package['version']}"
            )
        target = destination / "rust" / f"{package['name']}-{package['version']}"
        target.mkdir(parents=True)
        for notice in set(notices):
            shutil.copy2(notice, target / notice.name)
        registry.append({key: package[key] for key in ("name", "version", "license", "repository")})
    (destination / "rust-dependencies.json").write_text(json.dumps(registry, indent=2) + "\n")
    sysroot = Path(
        subprocess.check_output(["rustc", "--print", "sysroot"], env=env, text=True).strip()
    )
    rust_docs = sysroot / "share/doc/rust"
    target = destination / "rust-standard-library"
    target.mkdir()
    shutil.copy2(rust_docs / "COPYRIGHT-library.html", target)
    shutil.copytree(rust_docs / "licenses", target / "licenses")
