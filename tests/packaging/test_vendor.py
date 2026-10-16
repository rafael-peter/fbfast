"""Test observable failures and relocatable runtime, using the delivered artifact."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path
from zipfile import ZipFile

import pytest

ROOT = Path(__file__).resolve().parents[2]
VERSION = tomllib.loads((ROOT / "pyproject.toml").read_text())["project"]["version"]


def test_wheel_and_vendor_contain_identical_package() -> None:
    package = ROOT / "dist/vendor/fbfast"
    wheels = list((ROOT / "dist/wheels").glob(f"fbfast-{VERSION}-cp314-cp314-*.whl"))
    assert len(wheels) == 1
    with ZipFile(wheels[0]) as archive:
        for path in package.rglob("*"):
            if path.is_file() and "__pycache__" not in path.parts:
                assert archive.read("fbfast/" + str(path.relative_to(package))) == path.read_bytes()


def execute(root: Path, source: str) -> subprocess.CompletedProcess[str]:
    script = root / "app.py"
    script.write_text(source)
    env = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith(("PYTHON", "DYLD", "LD_", "FIREBIRD", "FB_", "FBFAST_"))
    }
    env["PATH"] = "/usr/bin:/bin"
    return subprocess.run(
        [sys.executable, "-E", "-s", "-S", str(script)],
        cwd=root,
        env=env,
        capture_output=True,
        text=True,
    )


def test_relocated_client_and_build_info(consumer: Path) -> None:
    result = execute(
        consumer,
        """
import fbfast, json
print(json.dumps({'build': fbfast.build_info(), 'client': fbfast.client_info()}))
""",
    )
    assert result.returncode == 0, result.stderr
    info = json.loads(result.stdout)
    assert info["build"]["profile"] == "release"
    assert Path(info["client"]["root"]).is_relative_to(consumer / "fbfast")


def test_import_and_build_info_do_not_load_client(consumer: Path) -> None:
    shutil.rmtree(consumer / "fbfast/_runtime")
    result = execute(consumer, "import fbfast; print(fbfast.build_info()['version'])")
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == VERSION


@pytest.mark.parametrize("field,value", [("python_version", "3.9"), ("machine", "x86_64")])
def test_abi_diagnosis_before_extension_load(consumer: Path, field: str, value: str) -> None:
    path = consumer / "fbfast/_build_info.json"
    manifest = json.loads(path.read_text())
    manifest[field] = value
    path.write_text(json.dumps(manifest))
    result = execute(consumer, "import fbfast")
    assert result.returncode != 0
    assert "fbfast binary incompatibility" in result.stderr
    assert field in result.stderr
    assert "ModuleNotFoundError" not in result.stderr


def test_missing_dependency_identifies_library(consumer: Path) -> None:
    (consumer / "fbfast/_runtime/lib/libtommath.dylib").unlink()
    result = execute(consumer, "import fbfast; fbfast.client_info()")
    assert result.returncode != 0
    assert "libtommath.dylib" in result.stderr


def test_mixed_build_is_rejected(consumer: Path) -> None:
    path = consumer / "fbfast/_build_info.json"
    manifest = json.loads(path.read_text())
    manifest["build_id"] = "another-build"
    path.write_text(json.dumps(manifest))
    result = execute(consumer, "import fbfast; fbfast.build_info()")
    assert result.returncode != 0
    assert "mixed build" in result.stderr


def test_manifest_detects_modified_file(consumer: Path) -> None:
    (consumer / "fbfast/__init__.py").write_text("# changed")
    result = subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts/verify_vendor.py"),
            "--package",
            str(consumer / "fbfast"),
        ],
        capture_output=True,
        text=True,
    )
    assert result.returncode != 0
    assert "hash mismatch" in result.stderr


def test_cannot_mix_clients(consumer: Path) -> None:
    result = execute(
        consumer,
        """
import fbfast, pathlib, shutil
info = fbfast.client_info()
original = pathlib.Path(info['path'])
other = original.with_name('second-client.dylib')
shutil.copy2(original, other)
fbfast.client_info(str(other))
""",
    )
    assert result.returncode != 0
    assert "already initialized" in result.stderr


def test_fork_rejected_before_client_lock(consumer: Path) -> None:
    result = execute(
        consumer,
        """
import fbfast, os
fbfast.client_info()
pid = os.fork()
if pid == 0:
    try:
        fbfast.client_info()
    except fbfast.ForkSafetyError:
        os._exit(0)
    os._exit(2)
_, status = os.waitpid(pid, 0)
if status != 0:
    raise RuntimeError('inherited client was not rejected')
""",
    )
    assert result.returncode == 0, result.stderr
