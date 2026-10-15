#!/usr/bin/env python3
"""Build once with maturin, bundle the runtime, and extract that exact final wheel."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import sysconfig
import tempfile
from pathlib import Path
from zipfile import ZipFile

from runtime_macos import copy_runtime, sha256
from package_licenses import collect as collect_licenses

ROOT = Path(__file__).resolve().parents[1]


def source_hash() -> str:
    digest = hashlib.sha256()
    paths = [
        ROOT / name
        for name in ("Cargo.toml", "Cargo.lock", "pyproject.toml", "rust-toolchain.toml", "build_backend.py", "README.md")
    ]
    for directory in ("crates", "python", "scripts", "third_party"):
        paths.extend(p for p in (ROOT / directory).rglob("*") if p.is_file())
    for path in sorted(paths):
        if "__pycache__" in path.parts or path.suffix in (".so", ".pyc"):
            continue
        digest.update(str(path.relative_to(ROOT)).encode())
        digest.update(path.read_bytes())
    return digest.hexdigest()


def tool_environment(client_root: Path) -> dict[str, str]:
    env = os.environ.copy()
    env["FBFAST_CLIENT_ROOT"] = str(client_root)
    # Optional workspace-local bootstrap; never mutate the user's shell profile.
    cargo = ROOT / ".tools/cargo"
    if not shutil.which("cargo") and (cargo / "bin/cargo").is_file():
        env.update(CARGO_HOME=str(cargo), RUSTUP_HOME=str(ROOT / ".tools/rustup"))
        env["PATH"] = f"{cargo / 'bin'}{os.pathsep}{env.get('PATH', '')}"
    return env


def assemble_wheel(original: Path, staging: Path, out: Path, metadata: dict[str, object]) -> Path:
    from wheel.wheelfile import WheelFile  # type: ignore[import-untyped]

    with ZipFile(original) as archive:
        archive.extractall(staging)
    package = staging / "fbfast"
    metadata["files"] = {
        str(path.relative_to(package)): sha256(path)
        for path in sorted(package.rglob("*"))
        if path.is_file()
    }
    (package / "_build_info.json").write_text(json.dumps(metadata, indent=2) + "\n")
    out.mkdir(parents=True, exist_ok=True)
    final = out / original.name
    # WheelFile regenerates RECORD and its integrity hashes, using wheel's format implementation.
    for record in staging.glob("*.dist-info/RECORD"):
        record.unlink()
    with tempfile.TemporaryDirectory(prefix=".wheel-", dir=out) as temporary:
        pending = Path(temporary) / original.name
        with WheelFile(str(pending), "w") as wheel:
            wheel.write_files(str(staging))
        pending.replace(final)
    return final


def identify_build(client: dict[str, str], env: dict[str, str]) -> dict[str, object]:
    toolchain = {
        "rustc": subprocess.check_output(["rustc", "--version"], env=env, text=True).strip(),
        "maturin": subprocess.check_output(
            [sys.executable, "-m", "maturin", "--version"], text=True
        ).strip(),
        "clang": subprocess.check_output(["clang++", "--version"], text=True).splitlines()[0],
    }
    inputs = {
        "source_hash": source_hash(),
        "client": client,
        "toolchain": toolchain,
        "soabi": sysconfig.get_config_var("SOABI"),
        "profile": "release",
        "flags": {
            key: env[key]
            for key in ("RUSTFLAGS", "CXXFLAGS", "MACOSX_DEPLOYMENT_TARGET")
            if key in env
        },
    }
    build_id = hashlib.sha256(json.dumps(inputs, sort_keys=True).encode()).hexdigest()
    env["FBFAST_BUILD_ID"] = build_id
    return {
        "manifest_version": 1,
        "version": "0.1.0",
        "profile": "release",
        "build_id": build_id,
        "target": "aarch64-apple-darwin",
        "python_implementation": sys.implementation.name,
        "python_version": f"{sys.version_info.major}.{sys.version_info.minor}",
        "soabi": sysconfig.get_config_var("SOABI"),
        "machine": platform.machine(),
        "system": platform.system(),
        "toolchain": toolchain,
        **client,
    }


def compile_wheel(temp: Path, env: dict[str, str]) -> Path:
    subprocess.run(
        [
            sys.executable,
            "-m",
            "maturin",
            "build",
            "--release",
            "--locked",
            "--interpreter",
            sys.executable,
            "--out",
            str(temp / "wheels"),
        ],
        cwd=ROOT,
        env=env,
        check=True,
    )
    originals = list((temp / "wheels").glob("*.whl"))
    if len(originals) != 1:
        raise RuntimeError(f"Expected exactly one wheel, received {len(originals)}")
    return originals[0]


def install_folder(wheel: Path, out: Path) -> Path:
    destination = out.resolve() / "fbfast"
    if destination == ROOT / "python/fbfast":
        raise RuntimeError("Output would replace the Python source package")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".fbfast-", dir=destination.parent) as temporary:
        pending = Path(temporary)
        with ZipFile(wheel) as archive:
            for entry in archive.namelist():
                if entry.startswith("fbfast/"):
                    archive.extract(entry, pending)
        backup = pending / "previous"
        if destination.exists():
            destination.rename(backup)
        try:
            (pending / "fbfast").rename(destination)
        except BaseException:
            if backup.exists():
                backup.rename(destination)
            raise
    return destination


def build_wheel_artifact(client_root: Path, wheel_out: Path) -> Path:
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise RuntimeError(
            "Only the macOS arm64 vendor target is implemented; no untested fallback"
        )
    if not (3, 11) <= sys.version_info[:2] <= (3, 14) or sysconfig.get_config_var(
        "Py_GIL_DISABLED"
    ):
        raise RuntimeError("Build with CPython 3.11-3.14 with the GIL (first tested target: 3.14)")
    client_root = client_root.expanduser().resolve()
    env = tool_environment(client_root)
    if not (ROOT / "Cargo.lock").is_file():
        subprocess.run(["cargo", "generate-lockfile"], cwd=ROOT, env=env, check=True)
    with tempfile.TemporaryDirectory(prefix="fbfast-build-") as temporary:
        temp = Path(temporary)
        staged = temp / "staged"
        package = staged / "fbfast"
        package.mkdir(parents=True)
        client = copy_runtime(client_root, package)
        shutil.copytree(ROOT / "third_party/licenses", package / "_licenses")
        metadata = identify_build(client, env)
        original = compile_wheel(temp, env)
        collect_licenses(ROOT, package / "_licenses", env)
        return assemble_wheel(original, staged, wheel_out, metadata)


def build(release: bool, out: Path, client_root: Path) -> Path:
    if not release:
        raise RuntimeError("Vendor artifacts require --release")
    wheel = build_wheel_artifact(client_root, ROOT / "dist/wheels")
    return install_folder(wheel, out)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--out", type=Path, default=ROOT / "dist/vendor")
    parser.add_argument(
        "--client-root",
        type=Path,
        default=Path(
            os.environ.get("FBFAST_CLIENT_ROOT", "/Library/Frameworks/Firebird.framework")
        ),
    )
    args = parser.parse_args()
    try:
        print(f"Vendor package: {build(args.release, args.out, args.client_root)}")
    except (RuntimeError, OSError, subprocess.CalledProcessError) as exc:
        parser.exit(1, f"build_vendor: {exc}\n")


if __name__ == "__main__":
    main()
