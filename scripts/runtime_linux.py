"""Bundle ELF client dependencies and dynamically loaded ICU with relative RPATH."""
from __future__ import annotations

import os
from pathlib import Path
import re
import shutil
import subprocess

from runtime_common import resources, sha256
from runtime_macos import client_version

# The platform ABI supplies these libraries; all other DT_NEEDED entries must
# resolve to files in the wheel. manylinux policy is also checked on the wheel.
SYSTEM = {"libc.so.6", "libm.so.6", "libdl.so.2", "libpthread.so.0", "librt.so.1",
          "libgcc_s.so.1", "libstdc++.so.6", "libresolv.so.2", "libutil.so.1"}


def elf(path):
    if not path.is_file():
        return False
    with path.open("rb") as file:
        return file.read(4) == b"\x7fELF"


def needed(path):
    output = subprocess.check_output(["patchelf", "--print-needed", str(path)], text=True)
    return output.splitlines()


def resolved(path, library_path):
    env = {**os.environ, "LD_LIBRARY_PATH": library_path}
    result = subprocess.run(["ldd", str(path)], env=env, text=True, capture_output=True, check=True)
    found = {}
    for line in result.stdout.splitlines():
        if "=> not found" in line:
            raise RuntimeError(f"SDK dependency missing for {path.name}: {line.strip()}")
        match = re.match(r"\s*(\S+) => (/\S+)", line)
        if match:
            found[match[1]] = Path(match[2])
    return found


def copy_runtime(root: Path, package: Path):
    runtime = package / "_runtime"
    lib = runtime / "lib"
    lib.mkdir(parents=True)
    source = root / "lib/libfbclient.so"
    shutil.copy2(source, lib / "libfbclient.so")
    resources(root, runtime)
    (runtime / "plugins").mkdir()
    shutil.copy2(root / "plugins/libChaCha.so", runtime / "plugins/libChaCha.so")
    # ICU is dlopen'ed, so DT_NEEDED traversal alone would miss it. Use the build
    # image's pinned distribution; ABI compatibility is enforced by auditwheel.
    search = [root / "lib", Path("/usr/lib64"), Path("/usr/lib")]
    search.extend(Path("/usr/lib").glob("*-linux-gnu"))
    icu = {}
    for directory in search:
        for prefix in ("libicuuc.so", "libicui18n.so", "libicudata.so"):
            for file in directory.glob(prefix + "*"):
                icu.setdefault(file.name, file)
    if not all(any(name.startswith(prefix) for name in icu)
               for prefix in ("libicuuc.so", "libicui18n.so", "libicudata.so")):
        raise RuntimeError("Install ICU development/runtime libraries in the build environment")
    for name, file in icu.items():
        shutil.copy2(file, lib / name)
    library_path = os.pathsep.join([str(root / "lib"), str(lib)])
    pending = [p for p in runtime.rglob("*") if elf(p)]
    visited = set()
    while pending:
        file = pending.pop()
        if file in visited:
            continue
        visited.add(file)
        dependencies = resolved(file, library_path)
        for name in needed(file):
            if name in SYSTEM or name.startswith("ld-linux-"):
                continue
            target = lib / name
            if not target.exists():
                if name not in dependencies:
                    raise RuntimeError(f"Cannot resolve {name} for {file.name}")
                shutil.copy2(dependencies[name], target)
                pending.append(target)
    for file in (p for p in runtime.rglob("*") if elf(p)):
        relative = os.path.relpath(lib, file.parent)
        rpath = "$ORIGIN" if relative == "." else f"$ORIGIN/{relative}"
        subprocess.run(["patchelf", "--force-rpath", "--set-rpath", rpath, str(file)], check=True)
    verify_dependencies(runtime)
    client = lib / "libfbclient.so"
    return {"client_version": client_version(client), "client_library": "_runtime/lib/libfbclient.so",
            "client_original_sha256": sha256(source), "client_sha256": sha256(client)}


def verify_dependencies(runtime: Path):
    files = [p for p in runtime.rglob("*") if elf(p)]
    if not (runtime / "lib/libfbclient.so").is_file():
        raise RuntimeError("Missing bundled libfbclient.so")
    for file in files:
        rpath = subprocess.check_output(["patchelf", "--print-rpath", str(file)], text=True).strip()
        if not rpath.startswith("$ORIGIN") or ":" in rpath:
            raise RuntimeError(f"Non-relocatable RPATH: {file.name}: {rpath}")
        directory = (file.parent / rpath.removeprefix("$ORIGIN").lstrip("/")).resolve()
        if not directory.is_relative_to(runtime.resolve()):
            raise RuntimeError(f"RPATH escapes package: {file.name}")
        for name in needed(file):
            if name not in SYSTEM and not name.startswith("ld-linux-") and not (directory / name).is_file():
                raise RuntimeError(f"Missing ELF dependency for {file.name}: {name}")
