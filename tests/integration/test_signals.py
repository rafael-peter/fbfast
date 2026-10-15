"""Exercise actual OS delivery in fresh processes, including the default action."""
import os
from pathlib import Path
import signal
import subprocess
import sys

import pytest


@pytest.mark.parametrize("number", [signal.SIGINT, signal.SIGTERM])
@pytest.mark.parametrize("mode", ["default", "ignore", "custom"])
def test_host_signal_semantics(fb, number, mode):
    program = r'''
import os, signal, sys, time
sys.path.insert(0, sys.argv[1])
import fbfast
number, mode = int(sys.argv[2]), sys.argv[3]
received = []
handler = {"default": signal.SIG_DFL, "ignore": signal.SIG_IGN,
           "custom": lambda signum, frame: received.append(signum)}[mode]
signal.signal(number, handler)
config = fbfast.FirebirdConfig(
    host="127.0.0.1", port=int(os.environ["FBFAST_TEST_PORT"]),
    database="/var/lib/firebird/data/fbfast.fdb", user="FBFAST_READER",
    password=os.environ["FBFAST_TEST_PASSWORD"], role="FBFAST_READONLY",
    pool_size=1, min_idle=1, env_file=False)
with fbfast.Pool(config) as pool:
    assert pool.fetch_val("SELECT 1 FROM RDB$DATABASE") == 1
    os.kill(os.getpid(), number)
    time.sleep(0.1)
    assert mode != "default", "default termination was swallowed"
    assert received == ([number] if mode == "custom" else [])
    assert pool.fetch_val("SELECT 2 FROM RDB$DATABASE") == 2
'''
    result = subprocess.run(
        [sys.executable, "-E", "-s", "-c", program,
         str(Path(fb.__file__).parent.parent), str(int(number)), mode],
        env=os.environ.copy(), capture_output=True, text=True, timeout=10,
    )
    assert result.returncode == (-number if mode == "default" else 0), result.stderr
