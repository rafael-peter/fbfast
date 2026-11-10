"""The configuration options agree across Rust, the stub and the generated guide."""
import ast
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from gen_config_docs import entries  # noqa: E402


def rust_fields():
    source = (ROOT / "crates/fbfast-core/src/config/resolve.rs").read_text()
    return re.findall(r'^    "(\w+)",$', source.split("const FIELDS", 1)[1], re.M)


def stub_parameters():
    tree = ast.parse((ROOT / "python/fbfast/_native.pyi").read_text())
    config = next(node for node in tree.body
                  if isinstance(node, ast.ClassDef) and node.name == "FirebirdConfig")
    init = next(node for node in config.body
                if isinstance(node, ast.FunctionDef) and node.name == "__init__")
    return [arg.arg for arg in init.args.kwonlyargs if arg.arg != "env_file"]


def test_rust_fields_stub_signature_and_docstring_agree():
    documented = [name for name, *_ in entries()]
    assert rust_fields() == stub_parameters() == documented
    for name, env, _, _ in entries():
        assert env == f"FB_{name.upper()}"


def test_generated_guide_is_current():
    result = subprocess.run([sys.executable, str(ROOT / "scripts/gen_config_docs.py"), "--check"],
                            capture_output=True, text=True)
    assert result.returncode == 0, result.stdout + result.stderr


def test_runtime_signature_lists_every_option():
    source = (ROOT / "crates/fbfast-python/src/config.rs").read_text()
    signature = re.search(r'text_signature = "\(\*, (.*)\)"', source)[1]
    assert [part.split("=")[0] for part in signature.split(", ")] == [*rust_fields(), "env_file"]
