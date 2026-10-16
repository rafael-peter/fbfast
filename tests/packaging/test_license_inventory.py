"""The packaged inventory follows the actual wheel target's production graph."""
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def test_native_arrow_notices_ship_with_the_wheel_target():
    directory = ROOT / "dist/vendor/fbfast/_licenses"
    dependencies = json.loads((directory / "rust-dependencies.json").read_text())
    names = {item["name"] for item in dependencies}
    assert {"arrow-array", "arrow-buffer", "arrow-data", "arrow-schema"} <= names
    assert "r-efi" not in names  # UEFI only: not linked into a macOS wheel.
    for item in dependencies:
        notices = directory / "rust" / f"{item['name']}-{item['version']}"
        assert notices.is_dir() and any(notices.iterdir())
        assert all(path.stat().st_size for path in notices.iterdir())
