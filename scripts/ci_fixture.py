"""Disposable local server from the extracted SDK, exclusively for wheel smoke tests."""
from __future__ import annotations

from contextlib import contextmanager
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import time


@contextmanager
def fixture(sdk: Path, directory: Path):
    marker = sdk / ".fbfast-ci-sdk.json"
    if not marker.is_file() or json.loads(marker.read_text()).get("root") != str(sdk.resolve()):
        raise RuntimeError("CI fixture requires a private SDK extracted by scripts/download_sdk.py")
    runtime = sdk / "Resources" if (sdk / "Resources").is_dir() else sdk
    runtime = runtime.resolve()
    binary = runtime if os.name == "nt" else runtime / "bin"
    suffix = ".exe" if os.name == "nt" else ""
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    database = (directory / "wheel-smoke.fdb").resolve()
    password = secrets.token_hex(24)
    conf = runtime / "firebird.conf"
    original = conf.read_bytes()
    # The SDK is a private extraction, never the system installation. Restore its
    # config even on failure; the CI workspace owns its separate security DB.
    conf.write_text(f"RemoteServicePort = {port}\nRemoteBindAddress = 127.0.0.1\n"
                    "WireCrypt = Required\nServerMode = Super\n")
    env = {**os.environ, "FIREBIRD": str(runtime), "ISC_USER": "SYSDBA"}
    env["LD_LIBRARY_PATH"] = os.pathsep.join([str(runtime / "lib"), env.get("LD_LIBRARY_PATH", "")])
    server = None
    try:
        path_sql = str(database).replace("'", "''")
        source = (
            f"CREATE DATABASE '{path_sql}' DEFAULT CHARACTER SET UTF8;\n"
            f"CREATE OR ALTER USER FBFAST_CI PASSWORD '{password}';\n"
            "CREATE ROLE FBFAST_CI_ROLE;\n"
            "CREATE TABLE WHEEL_SMOKE (ID INTEGER, LABEL VARCHAR(40) CHARACTER SET UTF8, "
            "AMOUNT NUMERIC(18,4), D DOUBLE PRECISION, STAMP TIMESTAMP WITH TIME ZONE, "
            "NAMED_STAMP TIMESTAMP WITH TIME ZONE);\n"
            "INSERT INTO WHEEL_SMOKE VALUES (1, 'Olá 日本語', 123.4567, 1.25, "
            "TIMESTAMP '2024-02-29 12:34:56.1234 +05:45', "
            "TIMESTAMP '2024-11-03 06:30:00 +00:00' AT TIME ZONE 'America/New_York');\n"
            "INSERT INTO WHEEL_SMOKE (ID) VALUES (2);\n"
            "GRANT SELECT ON WHEEL_SMOKE TO FBFAST_CI_ROLE;\n"
            "GRANT FBFAST_CI_ROLE TO FBFAST_CI;\nCOMMIT;\n"
        )
        result = subprocess.run([str(binary / f"isql{suffix}"), "-bail", "-q", "-ch", "UTF8"],
                                input=source, env=env, capture_output=True, encoding="utf-8", timeout=45)
        if result.returncode:
            detail = (result.stdout + result.stderr).replace(password, "<redacted>")
            raise RuntimeError(f"Disposable SDK fixture initialization failed: {detail}")
        with (directory / "server.log").open("wb") as log:
            command = [str(binary / f"firebird{suffix}")]
            if os.name == "nt":
                command.append("-a")
            server = subprocess.Popen(command, env=env, cwd=runtime, stdout=log, stderr=log)
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                if server.poll() is not None:
                    raise RuntimeError("Disposable Firebird server exited during startup")
                try:
                    with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                        break
                except OSError:
                    time.sleep(0.1)
            else:
                raise TimeoutError("Disposable Firebird server did not start")
            yield {"host": "127.0.0.1", "port": port, "database": str(database),
                   "user": "FBFAST_CI", "password": password, "role": "FBFAST_CI_ROLE"}
    finally:
        if server is not None and server.poll() is None:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=5)
        conf.write_bytes(original)
