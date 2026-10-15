"""PEP 517 entry point: uv/pip builds include the same verified native runtime."""

from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / "scripts"))


def _environment() -> Path:
    from build_vendor import tool_environment

    client = Path(os.environ.get("FBFAST_CLIENT_ROOT", "/Library/Frameworks/Firebird.framework"))
    os.environ.update(tool_environment(client))
    return client


def build_wheel(
    wheel_directory: str,
    config_settings: dict[str, Any] | None = None,
    metadata_directory: str | None = None,
) -> str:
    from build_vendor import build_wheel_artifact

    if config_settings:
        raise ValueError("Custom build settings are not supported; use FBFAST_CLIENT_ROOT for the SDK")
    return build_wheel_artifact(_environment(), Path(wheel_directory)).name


def build_sdist(
    sdist_directory: str, config_settings: dict[str, Any] | None = None,
) -> str:
    import maturin

    _environment()
    return str(maturin.build_sdist(sdist_directory, config_settings))
