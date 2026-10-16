"""Copy the remote-client runtime and make every non-system Mach-O edge relative."""

from __future__ import annotations

import hashlib
import shutil
import subprocess
import sys
from pathlib import Path

SYSTEM_PREFIXES = ("/usr/lib/", "/System/Library/")


def run(*args: str) -> str:
    return subprocess.check_output(args, text=True).strip()


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def dependencies(path: Path) -> list[str]:
    return [
        line.strip().split(" (", 1)[0] for line in run("otool", "-L", str(path)).splitlines()[1:]
    ]


def client_version(path: Path) -> str:
    # A loaded DLL cannot be deleted on Windows. Keep the version probe in a
    # short-lived process so the build can clean up its staging directory.
    probe = """
import ctypes, sys
client = ctypes.CDLL(sys.argv[1])
function = client.isc_get_client_version
function.argtypes = [ctypes.c_char_p]
function.restype = None
buffer = ctypes.create_string_buffer(256)
function(buffer)
print(buffer.value.decode("ascii"))
"""
    return subprocess.check_output(
        [sys.executable, "-I", "-c", probe, str(path)], encoding="ascii", timeout=30,
    ).strip()


def resources(root: Path) -> Path:
    for candidate in (root / "Resources", root):
        if (candidate / "lib/libfbclient.dylib").is_file():
            return candidate.resolve()
    raise RuntimeError(f"Firebird client distribution missing lib/libfbclient.dylib in {root}")


def copy_runtime(root: Path, package: Path) -> dict[str, str]:
    source = resources(root)
    runtime = package / "_runtime"
    runtime.mkdir()
    (runtime / "lib").mkdir()
    required = ["libfbclient.dylib", "libtommath.dylib", "libtomcrypt.dylib"]
    # ICU is loaded dynamically for temporal conversions, so otool alone misses it.
    required.extend(p.name for p in (source / "lib").glob("libicu*.dylib"))
    if not any(name.startswith("libicu") for name in required):
        raise RuntimeError("Firebird client distribution has no ICU libraries")
    for name in required:
        origin = source / "lib" / name
        if not origin.is_file():
            raise RuntimeError(f"Missing runtime dependency: {origin}")
        target = runtime / "lib" / name
        if origin.is_symlink():
            target.symlink_to(origin.resolve().name)
        else:
            shutil.copy2(origin, target)
    for name in ("firebird.msg", "tzdata", "intl"):
        origin = source / name
        if origin.is_dir():
            shutil.copytree(origin, runtime / name, symlinks=True)
        elif origin.is_file():
            shutil.copy2(origin, runtime / name)
        else:
            raise RuntimeError(f"Missing Firebird runtime resource: {origin}")
    # Only client capabilities; do not redistribute a server, security DB or engine.
    (runtime / "firebird.conf").write_text(
        "Providers = Remote\nWireCrypt = Required\nAuthClient = Srp256, Srp\n"
    )
    rewrite_libraries(runtime)
    version = client_version(runtime / "lib/libfbclient.dylib")
    import re

    # isc_get_client_version retains an InterBase-compatible V6.3 prefix.
    # Firebird's real major/minor follow the product name; patch is in the banner.
    match = re.search(r"V\d+\.\d+\.(\d+)\.\d+ Firebird (\d+)\.(\d+)", version)
    if not match or match.groups()[1:] != ("5", "0") or int(match[1]) < 3:
        raise RuntimeError(f"Client must be Firebird 5.0.x >= 5.0.3, received {version}")
    return {
        "client_version": version,
        "client_library": "_runtime/lib/libfbclient.dylib",
        "client_original_sha256": sha256(source / "lib/libfbclient.dylib"),
        "client_sha256": sha256(runtime / "lib/libfbclient.dylib"),
    }


def rewrite_libraries(runtime: Path) -> None:
    libraries = [p for p in runtime.rglob("*.dylib") if not p.is_symlink()]
    by_name = {p.name: p for p in runtime.rglob("*.dylib")}
    import os

    for library in libraries:
        edges = dependencies(library)
        subprocess.run(
            ["install_name_tool", "-id", f"@rpath/{library.name}", str(library)], check=True
        )
        for edge in edges[1:]:  # otool's first edge is the dylib's own install name.
            if edge.startswith(SYSTEM_PREFIXES):
                continue
            dependency = by_name.get(Path(edge).name)
            if dependency is None:
                raise RuntimeError(f"Unresolved dependency {edge} required by {library.name}")
            relative = os.path.relpath(dependency, library.parent)
            subprocess.run(
                ["install_name_tool", "-change", edge, f"@loader_path/{relative}", str(library)],
                check=True,
            )
        # Editing load commands invalidates the original arm64 code signature.
        subprocess.run(
            ["codesign", "--force", "--sign", "-", str(library)], check=True, capture_output=True
        )
    verify_dependencies(runtime)


def verify_dependencies(runtime: Path) -> None:
    for library in runtime.rglob("*.dylib"):
        if library.is_symlink():
            if not library.resolve().is_relative_to(runtime.resolve()):
                raise RuntimeError(f"Runtime symlink escapes package: {library}")
            continue
        for edge in dependencies(library)[1:]:
            if edge.startswith(SYSTEM_PREFIXES):
                continue
            if not edge.startswith("@loader_path/"):
                raise RuntimeError(f"Non-relocatable dependency: {library.name}: {edge}")
            resolved = (library.parent / edge.removeprefix("@loader_path/")).resolve()
            if not resolved.is_relative_to(runtime.resolve()) or not resolved.is_file():
                raise RuntimeError(f"Missing or external dependency: {library.name}: {edge}")
