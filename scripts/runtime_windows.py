"""Bundle the official Windows remote client and verify PE dependencies."""
from __future__ import annotations

import shutil
from pathlib import Path

from runtime_common import resources, sha256
from runtime_macos import client_version

# OS DLLs are supplied by Windows. VC runtime is bundled from the official SDK.
SYSTEM = {"kernel32.dll", "advapi32.dll", "user32.dll", "ws2_32.dll", "shell32.dll",
          "ole32.dll", "oleaut32.dll", "ntdll.dll", "bcrypt.dll", "secur32.dll",
          "crypt32.dll", "rpcrt4.dll", "shlwapi.dll", "version.dll", "msvcrt.dll",
          "ucrtbase.dll", "psapi.dll", "dbghelp.dll", "iphlpapi.dll", "normaliz.dll",
          "userenv.dll", "gdi32.dll", "comdlg32.dll", "mpr.dll"}


def dependencies(path):
    import pefile
    with pefile.PE(str(path), fast_load=True) as pe:
        pe.parse_data_directories(directories=[
            pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"],
            pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT"],
        ])
        return {entry.dll.decode("ascii").lower()
                for name in ("DIRECTORY_ENTRY_IMPORT", "DIRECTORY_ENTRY_DELAY_IMPORT")
                for entry in getattr(pe, name, [])}


def copy_runtime(root: Path, package: Path):
    runtime = package / "_runtime"
    runtime.mkdir()
    # DLLs stay next to fbclient.dll; Firebird uses that directory as its root.
    names = ["fbclient.dll", "zlib1.dll", "msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"]
    names.extend(p.name for p in root.glob("icu*.dll"))
    if not any(name.startswith("icu") for name in names):
        raise RuntimeError("Windows SDK has no ICU libraries")
    for name in names:
        shutil.copy2(root / name, runtime / name)
    resources(root, runtime)
    (runtime / "plugins").mkdir()
    shutil.copy2(root / "plugins/chacha.dll", runtime / "plugins/chacha.dll")
    verify_dependencies(runtime)
    return {"client_version": client_version(runtime / "fbclient.dll"),
            "client_library": "_runtime/fbclient.dll",
            "client_original_sha256": sha256(root / "fbclient.dll"),
            "client_sha256": sha256(runtime / "fbclient.dll")}


def verify_dependencies(runtime: Path):
    files = {p.name.lower(): p for p in runtime.rglob("*.dll")}
    if "fbclient.dll" not in files:
        raise RuntimeError("Missing bundled fbclient.dll")
    for dll in files.values():
        missing = {name for name in dependencies(dll) if name not in files and name not in SYSTEM
                   and not name.startswith(("api-ms-win-", "ext-ms-win-"))}
        if missing:
            raise RuntimeError(f"Unbundled DLL dependencies for {dll.name}: {sorted(missing)}")
