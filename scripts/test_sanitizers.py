#!/usr/bin/env python3
"""Compile the native bridge with sanitizers and run only the test executables."""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
NATIVE = (ROOT / "crates/fbfast-native").resolve()


def compiler_library(compiler: str, name: str) -> Path:
    value = subprocess.check_output([compiler, f"-print-file-name={name}"], text=True).strip()
    path = Path(value)
    if not path.is_absolute() or not path.is_file():
        raise RuntimeError(f"{compiler} did not resolve {name}: {value}")
    return path


def client_library() -> Path:
    root = Path(os.environ.get("FBFAST_CLIENT_ROOT", ""))
    for name in ("libfbclient.so.2", "libfbclient.so"):
        candidate = root / "lib" / name
        if candidate.is_file():
            return candidate.resolve()
    raise RuntimeError("FBFAST_CLIENT_ROOT has no Linux Firebird client library")


def compile_tests(environment: dict[str, str]) -> list[Path]:
    command = [
        "cargo", "test", "-p", "fbfast-native", "--lib", "--tests", "--locked",
        "--no-run", "--message-format=json",
    ]
    process = subprocess.Popen(
        command,
        cwd=ROOT,
        env=environment,
        stdout=subprocess.PIPE,
        text=True,
    )
    assert process.stdout is not None
    executables: set[Path] = set()
    for line in process.stdout:
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            print(line, end="")
            continue
        if message.get("reason") != "compiler-artifact" or not message.get("profile", {}).get("test"):
            continue
        executable = message.get("executable")
        source = message.get("target", {}).get("src_path")
        if executable and source and Path(source).resolve().is_relative_to(NATIVE):
            executables.add(Path(executable).resolve())
    if process.wait() != 0:
        raise subprocess.CalledProcessError(process.returncode, command)
    if not executables:
        raise RuntimeError("Cargo produced no fbfast-native test executables")
    return sorted(executables)


def main() -> None:
    if sys.platform != "linux":
        raise SystemExit("Native sanitizer execution is supported on Linux")
    compiler = shutil.which("g++") or shutil.which("c++")
    if compiler is None:
        raise SystemExit("A C++ compiler is required")
    asan = compiler_library(compiler, "libasan.so")
    ubsan = compiler_library(compiler, "libubsan.so")
    client = client_library()
    build_env = os.environ.copy()
    build_env.update(
        CXXFLAGS="-fsanitize=address,undefined -fno-omit-frame-pointer",
        RUSTFLAGS="-C link-arg=-lasan -C link-arg=-lubsan",
    )
    executables = compile_tests(build_env)
    test_env = build_env.copy()
    test_env.update(
        ASAN_OPTIONS="detect_leaks=1:halt_on_error=1:strict_string_checks=1",
        UBSAN_OPTIONS="halt_on_error=1:print_stacktrace=1",
        FBFAST_SANITIZER_CLIENT=str(client),
    )
    test_env["LD_PRELOAD"] = os.pathsep.join(
        [str(asan), str(ubsan), *filter(None, [os.environ.get("LD_PRELOAD")])]
    )
    test_env["LD_LIBRARY_PATH"] = os.pathsep.join(
        [str(client.parent), *filter(None, [os.environ.get("LD_LIBRARY_PATH")])]
    )
    for executable in executables:
        print(f"Running sanitized test executable: {executable.name}", flush=True)
        subprocess.run([str(executable), "--nocapture"], cwd=ROOT, env=test_env, check=True)


if __name__ == "__main__":
    main()
