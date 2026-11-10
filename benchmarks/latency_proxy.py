"""Own a toxiproxy container in front of the disposable fixture; never an external target."""
from __future__ import annotations

import hashlib
import json
import subprocess
import time
import urllib.request
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
IMAGE = "ghcr.io/shopify/toxiproxy@sha256:5d3dae09bc185dcc5ff6cf43abf6062a4a720aff0c4306a471e773acdcd107aa"
LABEL = "dev.fbfast.latency-proxy"
NAME = "fbfast-toxiproxy-" + hashlib.sha256(str(ROOT).encode()).hexdigest()[:10]
PROXY = "firebird"


def _docker(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["docker", *args], text=True, capture_output=True)


class LatencyProxy:
    """Adds round-trip latency, bandwidth limits and faults between client and fixture."""

    def __init__(self, fixture_container: str, role: str = "benchmark") -> None:
        # Separate containers let fault-injection tests run beside a benchmark.
        self.name = f"{NAME}-{role}"
        info = json.loads(_docker("inspect", fixture_container).stdout)[0]
        networks = info["NetworkSettings"]["Networks"]
        self.network, settings = next(iter(networks.items()))
        self.upstream = f"{settings['IPAddress']}:3050"
        self.api_port = 0
        self.port = 0

    def start(self) -> None:
        existing = _docker("inspect", self.name)
        if existing.returncode == 0:
            labels = json.loads(existing.stdout)[0]["Config"]["Labels"]
            if labels.get(LABEL) != str(ROOT):
                raise RuntimeError("Refusing to touch a container without this checkout's label")
            _docker("rm", "-f", self.name)
        started = _docker(
            "run", "-d", "--name", self.name, "--label", f"{LABEL}={ROOT}",
            "--network", self.network, "-p", "127.0.0.1::8474", "-p", "127.0.0.1::26660", IMAGE,
        )
        if started.returncode:
            raise RuntimeError(f"toxiproxy did not start: {started.stderr.strip()}")
        ports = json.loads(_docker("inspect", self.name).stdout)[0]["NetworkSettings"]["Ports"]
        self.api_port = int(ports["8474/tcp"][0]["HostPort"])
        self.port = int(ports["26660/tcp"][0]["HostPort"])
        deadline = time.monotonic() + 20
        while True:
            try:
                self._call("GET", "/version")
                break
            except OSError:
                if time.monotonic() > deadline:
                    raise
                time.sleep(0.2)
        self._call("POST", "/proxies", {"name": PROXY, "listen": "0.0.0.0:26660",
                                        "upstream": self.upstream, "enabled": True})

    def stop(self) -> None:
        _docker("rm", "-f", self.name)

    def configure(self, rtt_ms: int = 0, rate_kib: int | None = None) -> None:
        """RTT is split evenly between both directions; rate limits each direction."""
        for toxic in self._call("GET", f"/proxies/{PROXY}/toxics"):
            self._call("DELETE", f"/proxies/{PROXY}/toxics/{toxic['name']}")
        for stream in ("upstream", "downstream"):
            if rtt_ms:
                self._call("POST", f"/proxies/{PROXY}/toxics", {
                    "name": f"latency_{stream}", "type": "latency", "stream": stream,
                    "attributes": {"latency": rtt_ms // 2, "jitter": 0},
                })
            if rate_kib:
                self._call("POST", f"/proxies/{PROXY}/toxics", {
                    "name": f"bandwidth_{stream}", "type": "bandwidth", "stream": stream,
                    "attributes": {"rate": rate_kib},
                })

    def reset_connections(self) -> None:
        """Drops every open client connection, as a tunnel or NAT restart would."""
        self._call("POST", f"/proxies/{PROXY}", {"enabled": False})
        self._call("POST", f"/proxies/{PROXY}", {"enabled": True})

    def _call(self, method: str, path: str, body: dict[str, Any] | None = None) -> Any:
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.api_port}{path}", data=data, method=method,
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=10) as response:
            payload = response.read()
        return json.loads(payload) if payload else None
