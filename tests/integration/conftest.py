import importlib
import os
from pathlib import Path
import shutil
import sys

import pytest


@pytest.fixture(scope="session")
def fb(tmp_path_factory):
    if not os.environ.get("FBFAST_TEST_PASSWORD"):
        pytest.skip("Run scripts/test_python.py against the owned disposable fixture")
    consumer = tmp_path_factory.mktemp("fbfast-consumer")
    source = Path(__file__).resolve().parents[2] / "dist/vendor/fbfast"
    shutil.copytree(source, consumer / "fbfast")
    sys.path.insert(0, str(consumer))
    module = importlib.import_module("fbfast")
    assert Path(module.__file__).is_relative_to(consumer)
    return module


@pytest.fixture
def config(fb):
    def make(**changes):
        values = dict(host="127.0.0.1", port=int(os.environ["FBFAST_TEST_PORT"]),
                      database="/var/lib/firebird/data/fbfast.fdb", user="FBFAST_READER",
                      password=os.environ["FBFAST_TEST_PASSWORD"], role="FBFAST_READONLY",
                      pool_size=2, min_idle=1, env_file=False)
        values.update(changes)
        return fb.FirebirdConfig(**values)
    return make
