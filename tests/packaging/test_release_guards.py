"""Release failures must happen before publication or mutation of a system SDK."""
from pathlib import Path
import importlib.util
import json
import os
import subprocess
import sys
import tomllib

import pytest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from ci_fixture import fixture  # noqa: E402

SPEC = importlib.util.spec_from_file_location("fbfast_release", ROOT / "scripts/release.py")
assert SPEC is not None and SPEC.loader is not None
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


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


def test_release_title_comes_from_version_notes(tmp_path):
    notes = tmp_path / "notes.md"
    notes.write_text("# fbfast 1.2.3 — Concrete change\n\nDetails.\n")
    assert release.release_title(notes, "1.2.3") == "1.2.3 — Concrete change"


def test_release_workflow_lookup_requires_exact_commit_and_branch(monkeypatch):
    runs = [
        dict(databaseId=1, status="completed", conclusion="success", headBranch="main",
             headSha="other", createdAt="2026-01-03T00:00:00Z"),
        dict(databaseId=2, status="completed", conclusion="failure", headBranch="other",
             headSha="wanted", createdAt="2026-01-02T00:00:00Z"),
        dict(databaseId=3, status="completed", conclusion="success", headBranch="main",
             headSha="wanted", createdAt="2026-01-01T00:00:00Z"),
    ]
    monkeypatch.setattr(release, "output", lambda *args: json.dumps(runs))
    assert release.workflow_run("build-wheels.yml", "wanted", "main")["databaseId"] == 3


def test_remote_annotated_tag_resolves_to_peeled_commit(monkeypatch):
    monkeypatch.setattr(
        release,
        "output",
        lambda *args: "tag-object\trefs/tags/v1.2.3\ncommit\trefs/tags/v1.2.3^{}",
    )
    assert release.remote_tag_commit("v1.2.3") == "commit"


def test_windows_installs_the_timezone_database():
    project = tomllib.loads((ROOT / "pyproject.toml").read_text())["project"]
    assert "tzdata>=2024.1; sys_platform == 'win32'" in project["dependencies"]
