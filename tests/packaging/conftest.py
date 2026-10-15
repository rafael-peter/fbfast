import shutil
from pathlib import Path

import pytest

PACKAGE = Path(__file__).resolve().parents[2] / "dist/vendor/fbfast"


@pytest.fixture
def consumer(tmp_path: Path) -> Path:
    if not PACKAGE.is_dir():
        pytest.fail("Build dist/vendor/fbfast before running packaging acceptance tests")
    shutil.copytree(PACKAGE, tmp_path / "fbfast")
    return tmp_path
