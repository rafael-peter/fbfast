#!/usr/bin/env python3
"""Own a disposable Firebird container; never accept an operational database target."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import secrets
import subprocess
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
STATE = ROOT / ".tools/firebird-fixture.json"
IMAGE = (
    "firebirdsql/firebird@sha256:d1e15bdcc0da40bf5e8af24ba6ba1d14b23d0a43fd21faa4ec8b2089feefb3b8"
)
LABEL = "dev.fbfast.disposable-fixture"
NAME = "fbfast-fixture-" + hashlib.sha256(str(ROOT).encode()).hexdigest()[:10]
DATABASE = "/var/lib/firebird/data/fbfast.fdb"


def docker(*args: str, **kwargs: Any) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["docker", *args], text=True, capture_output=True, **kwargs)


def inspect() -> dict[str, Any] | None:
    result = docker("inspect", NAME)
    if result.returncode:
        return None
    info: dict[str, Any] = json.loads(result.stdout)[0]
    if info["Config"]["Labels"].get(LABEL) != str(ROOT):
        raise RuntimeError("Refusing to touch a container without this checkout's fixture label")
    return info


def state() -> dict[str, Any]:
    return dict(json.loads(STATE.read_text()))


def status() -> dict[str, Any]:
    info = inspect()
    if info is None:
        return {"container": NAME, "running": False}
    port = info["NetworkSettings"]["Ports"].get("3050/tcp")
    return {
        "container": NAME,
        "running": info["State"]["Running"],
        "image": info["Image"],
        "host": "127.0.0.1",
        "port": int(port[0]["HostPort"]) if port else None,
        "database": DATABASE,
        "credentials_file": str(STATE.relative_to(ROOT)),
    }


def sql(source: str, password: str) -> subprocess.CompletedProcess[str]:
    # Credentials are passed via the child environment, not interpolated into SQL.
    env = {**os.environ, "ISC_USER": "SYSDBA", "ISC_PASSWORD": password}
    return docker(
        "exec",
        "-i",
        "-e",
        "ISC_USER",
        "-e",
        "ISC_PASSWORD",
        NAME,
        "isql",
        "-bail",
        "-q",
        f"localhost:{DATABASE}",
        input=source,
        env=env,
    )


def configure_wire_crypt(password: str) -> None:
    """Install only fixed aliases on this checkout's disposable server."""
    if inspect() is None:
        raise RuntimeError("Disposable fixture is not running")
    path = "/opt/firebird/databases.conf"
    marker = "# fbfast wire encryption acceptance aliases"
    original = docker("exec", NAME, "cat", path)
    if original.returncode:
        raise RuntimeError("Cannot read disposable fixture configuration")
    aliases = marker + "\n" + "\n".join(
        f"fbfast_crypt_{policy.lower()} = /var/lib/firebird/data/crypt_{policy.lower()}.fdb\n"
        f"{{\n    WireCrypt = {policy}\n}}"
        for policy in ("Disabled", "Enabled", "Required")
    )
    contents = original.stdout.split(marker)[0] + aliases + "\n"
    result = docker("exec", "-i", NAME, "sh", "-c", f"cat > {path}", input=contents)
    if result.returncode:
        raise RuntimeError("Cannot configure disposable fixture encryption aliases")
    for policy in ("disabled", "enabled", "required"):
        database = f"/var/lib/firebird/data/crypt_{policy}.fdb"
        if docker("exec", NAME, "test", "-f", database).returncode == 0:
            continue
        created = sql(
            f"CREATE DATABASE 'localhost:{database}' DEFAULT CHARACTER SET UTF8;\n", password
        )
        if created.returncode:
            raise RuntimeError(f"Cannot initialize encryption fixture: {created.stderr.strip()}")


def start() -> dict[str, Any]:
    existing = inspect()
    if existing is not None:
        if not STATE.is_file() or not existing["State"]["Running"]:
            raise RuntimeError(
                "Existing fixture is stopped or lacks local credentials; use stop then start"
            )
        configure_wire_crypt(state()["admin_password"])
        return status()
    admin_password = secrets.token_hex(24)
    reader_password = secrets.token_hex(24)
    STATE.parent.mkdir(parents=True, exist_ok=True)
    env = {
        **os.environ,
        "FIREBIRD_ROOT_PASSWORD": admin_password,
    }
    result = docker(
        "run",
        "--detach",
        "--name",
        NAME,
        "--label",
        f"{LABEL}={ROOT}",
        "--publish",
        "127.0.0.1::3050",
        "--tmpfs",
        "/var/lib/firebird/data",
        "--env",
        "FIREBIRD_ROOT_PASSWORD",
        "--env",
        "FIREBIRD_DATABASE=fbfast.fdb",
        "--env",
        "FIREBIRD_DATABASE_DEFAULT_CHARSET=UTF8",
        IMAGE,
        env=env,
    )
    if result.returncode:
        raise RuntimeError(f"Cannot start fixture: {result.stderr.strip()}")
    try:
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            ready = sql("SELECT 1 FROM RDB$DATABASE;\n", admin_password)
            if ready.returncode == 0:
                break
            time.sleep(0.25)
        else:
            raise RuntimeError("Disposable Firebird did not become ready in 45 seconds")
        created_user = sql(
            f"CREATE USER FBFAST_READER PASSWORD '{reader_password}' REVOKE ADMIN ROLE;\n",
            admin_password,
        )
        if created_user.returncode:
            detail = created_user.stderr.replace(reader_password, "<redacted>").strip()
            raise RuntimeError(f"Cannot create restricted fixture user: {detail}")
        initialized = sql((ROOT / "tests/integration/fixture.sql").read_text(), admin_password)
        if initialized.returncode:
            raise RuntimeError(f"Fixture SQL failed: {initialized.stderr.strip()}")
        configure_wire_crypt(admin_password)
        public = status()
        with open(STATE, "w", opener=lambda path, flags: os.open(path, flags, 0o600)) as file:
            os.fchmod(file.fileno(), 0o600)
            json.dump(
                {
                    **public,
                    "admin_password": admin_password,
                    "user": "FBFAST_READER",
                    "password": reader_password,
                    "role": "FBFAST_READONLY",
                },
                file,
            )
        return public
    except BaseException:
        if inspect() is not None:
            docker("rm", "--force", "--volumes", NAME)
        raise


def stop() -> dict[str, Any]:
    if inspect() is not None:
        result = docker("rm", "--force", "--volumes", NAME)
        if result.returncode:
            raise RuntimeError(result.stderr.strip())
    STATE.unlink(missing_ok=True)
    return {"container": NAME, "running": False}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("start", "status", "stop"))
    args = parser.parse_args()
    try:
        result = {"start": start, "status": status, "stop": stop}[args.action]()
        print(json.dumps(result, indent=2))
    except (OSError, RuntimeError) as exc:
        parser.exit(1, f"firebird_fixture: {exc}\n")


if __name__ == "__main__":
    main()
