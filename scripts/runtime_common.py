"""Platform dispatch and common remote-client resources for wheels."""
from __future__ import annotations

import hashlib
import importlib
import platform
import shutil
from pathlib import Path


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def implementation():
    module = {"Darwin": "runtime_macos", "Linux": "runtime_linux", "Windows": "runtime_windows"}
    try:
        return importlib.import_module(module[platform.system()])
    except KeyError:
        raise RuntimeError(f"Unsupported runtime platform: {platform.system()}") from None


def copy_runtime(root, package):
    return implementation().copy_runtime(root, package)


def verify_dependencies(runtime):
    return implementation().verify_dependencies(runtime)


def resources(source: Path, runtime: Path):
    for name in ("firebird.msg", "tzdata", "intl"):
        origin = source / name
        if origin.is_dir():
            shutil.copytree(origin, runtime / name, symlinks=False)
        elif origin.is_file():
            shutil.copy2(origin, runtime / name)
        else:
            raise RuntimeError(f"Missing Firebird runtime resource: {origin}")
    (runtime / "firebird.conf").write_text(
        "Providers = Remote\nWireCrypt = Required\nAuthClient = Srp256, Srp\n"
    )
