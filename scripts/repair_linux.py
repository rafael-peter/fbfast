"""Validate/repair the complete Linux wheel, including injected runtime ELFs."""
from __future__ import annotations

import platform
from pathlib import Path
import subprocess
import sys
from zipfile import ZipFile

from runtime_common import sha256


def repair(wheel: Path, temp: Path, out: Path, metadata: dict, assemble):
    repaired = temp / "repaired"
    policy = f"manylinux_2_28_{platform.machine()}"
    subprocess.run([sys.executable, "-m", "auditwheel", "repair", "--plat", policy,
                    "--only-plat", "--wheel-dir", str(repaired), str(wheel)], check=True)
    wheels = list(repaired.glob("*.whl"))
    if len(wheels) != 1:
        raise RuntimeError("auditwheel did not produce exactly one policy-compliant wheel")
    # auditwheel may change RPATHs/ELF names. Rebuild the inventory only after
    # those transformations, so the distributed bytes match their checksums.
    staging = temp / "repaired-staging"
    with ZipFile(wheels[0]) as archive:
        archive.extractall(staging)
    client = staging / "fbfast" / metadata["client_library"]
    metadata["client_sha256"] = sha256(client)
    return assemble(wheels[0], staging, out, metadata)
