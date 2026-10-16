"""Release failures must happen before publication or mutation of a system SDK."""
from pathlib import Path
import os
import subprocess
import sys

import pytest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from ci_fixture import fixture  # noqa: E402


def test_mismatched_tag_is_rejected():
    env = {**os.environ, "GITHUB_REF_TYPE": "tag", "GITHUB_REF_NAME": "v9999.0.0"}
    result = subprocess.run([sys.executable, str(ROOT / "scripts/check_release.py")],
                            env=env, text=True, capture_output=True)
    assert result.returncode != 0
    assert "Release tag must equal" in result.stderr


def test_missing_platform_artifacts_are_rejected(tmp_path):
    env = {k: v for k, v in os.environ.items() if not k.startswith("GITHUB_REF")}
    result = subprocess.run([sys.executable, str(ROOT / "scripts/check_release.py"),
                             "--artifacts", str(tmp_path)], env=env, text=True, capture_output=True)
    assert result.returncode != 0
    assert "Incomplete release matrix" in result.stderr


def test_smoke_fixture_refuses_unowned_sdk_before_touching_configuration(tmp_path):
    config = tmp_path / "firebird.conf"
    config.write_text("Original system configuration\n")
    with pytest.raises(RuntimeError, match="private SDK"):
        with fixture(tmp_path, tmp_path / "database"):
            pytest.fail("Fixture accepted an unmarked SDK")
    assert config.read_text() == "Original system configuration\n"
