#!/usr/bin/env python3
"""Validate the exact copied folder in an isolated external consumer process."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from runtime_macos import sha256, verify_dependencies

CONSUMER = """\
import json
import pathlib
import sys
import fbfast
root = pathlib.Path(__file__).resolve().parent
package = root / "fbfast"
if not pathlib.Path(fbfast.__file__).resolve().is_relative_to(package):
    raise RuntimeError("Import escaped the copied package")
info = fbfast.build_info()
if info["profile"] != "release":
    raise RuntimeError("Not a release artifact")
client = fbfast.client_info()
if not pathlib.Path(client["path"]).is_relative_to(package):
    raise RuntimeError("Client library escaped the copied package")
if not pathlib.Path(client["root"]).is_relative_to(package):
    raise RuntimeError("Firebird resource root escaped the copied package")
print(json.dumps({"build": info["build_id"], "client": client, "profile": info["profile"]}))
"""


def verify(package: Path) -> dict[str, object]:
    manifest_path = package / "_build_info.json"
    manifest = json.loads(manifest_path.read_text())
    declared = manifest["files"]
    actual = {
        str(path.relative_to(package))
        for path in package.rglob("*")
        if path.is_file() and "__pycache__" not in path.parts and path != manifest_path
    }
    if actual != set(declared):
        raise RuntimeError(
            f"Package file inventory differs: {actual.symmetric_difference(declared)}"
        )
    for name, expected in declared.items():
        file = (package / name).resolve()
        if not file.is_relative_to(package.resolve()) or sha256(file) != expected:
            raise RuntimeError(f"Package hash mismatch or escaped path: {name}")
    verify_dependencies(package / "_runtime")
    with tempfile.TemporaryDirectory(prefix="fbfast-consumer-") as temporary:
        root = Path(temporary)
        shutil.copytree(package, root / "fbfast", ignore=shutil.ignore_patterns("__pycache__"))
        (root / "consumer.py").write_text(CONSUMER)
        env = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(
                ("PYTHON", "DYLD_", "LD_", "FB_", "FIREBIRD", "FBFAST_", "CARGO", "RUST")
            )
        }
        env["PATH"] = "/usr/bin:/bin"  # No Rust, Homebrew, maturin, or pip on the search path.
        # -E -s -S remove interpreter env, user site and all site-packages. The
        # ordinary script directory is intentionally retained, as in the consumer.
        result = subprocess.run(
            [sys.executable, "-E", "-s", "-S", str(root / "consumer.py")],
            cwd=root,
            env=env,
            capture_output=True,
            text=True,
            check=True,
        )
        return dict(json.loads(result.stdout))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = verify(args.package.resolve())
        print(json.dumps({"verified": True, **result}, indent=2))
    except subprocess.CalledProcessError as exc:
        parser.exit(1, f"verify_vendor: isolated consumer failed:\n{exc.stderr}\n")
    except (OSError, ValueError, KeyError, RuntimeError) as exc:
        parser.exit(1, f"verify_vendor: {exc}\n")


if __name__ == "__main__":
    main()
