"""Diagnose a copied package before trying to load its Python extension."""

from __future__ import annotations

import json
import platform
import sys
import sysconfig
from pathlib import Path
from typing import Any

_PACKAGE = Path(__file__).resolve().parent


def _manifest() -> dict[str, Any]:
    path = _PACKAGE / "_build_info.json"
    return json.loads(path.read_text()) if path.exists() else {}


def _check_compatibility(info: dict[str, Any]) -> None:
    expected = {
        "python_implementation": sys.implementation.name,
        "python_version": f"{sys.version_info.major}.{sys.version_info.minor}",
        "machine": platform.machine(),
        "system": platform.system(),
        "soabi": sysconfig.get_config_var("SOABI"),
    }
    for key, actual in expected.items():
        if key in info and info[key] != actual:
            raise ImportError(
                f"fbfast binary incompatibility: {key}={info[key]!r}; "
                f"this interpreter requires {actual!r}. Install or build fbfast for this target."
            )
    if sysconfig.get_config_var("Py_GIL_DISABLED"):
        raise ImportError(
            "fbfast requires CPython with the GIL; free-threaded builds are unsupported"
        )


_IMPORT_MANIFEST = _manifest()
_check_compatibility(_IMPORT_MANIFEST)
# Compute the default once at import. Constructing Pool does no filesystem I/O.
_CLIENT_PATH = _PACKAGE / _IMPORT_MANIFEST.get("client_library", "_runtime/lib/libfbclient.dylib")
try:
    from . import _native
except ImportError as exc:
    raise ImportError(
        f"fbfast native extension could not load from {_PACKAGE}: {exc}. "
        "Install a complete fbfast package for this Python ABI and architecture."
    ) from exc


def build_info() -> dict[str, Any]:
    """Return build/ABI/client hashes without loading the client or connecting."""
    info = _manifest()
    compiled = json.loads(_native.compiled_info())
    for key in ("version", "profile", "target", "build_id"):
        if key in info and info[key] != compiled[key]:
            raise ImportError(
                f"fbfast mixed build: manifest and native extension disagree on {key}"
            )
    return {**info, **compiled}


def client_info(client_library: str | None = None) -> dict[str, Any]:
    """Load and identify the official client, without opening a database."""
    if client_library is None:
        manifest = _manifest()
        relative = manifest.get("client_library")
        if not relative:
            raise RuntimeError("No bundled fbclient; build a vendor package or pass client_library")
        path = (_PACKAGE / relative).resolve()
        if not path.is_relative_to(_PACKAGE):
            raise ImportError("fbfast manifest contains a client path outside the package")
    else:
        path = Path(client_library).expanduser().resolve()
    return dict(_native.load_client(path))
