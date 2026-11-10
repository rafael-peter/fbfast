# Development and build storage

Instructions for agents and the repository rules are summarized in
[AGENTS.md](../AGENTS.md).

## Building from source

Source builds require Rust 1.99.1 (pinned in `rust-toolchain.toml`), a C++17
toolchain, CPython 3.11–3.15 with the GIL, and a Firebird 5.0.x SDK/runtime
(5.0.3 or newer). Published wheels bundle the pinned Firebird 5.0.3 client and
are tested against servers 5.0.3 and 5.0.4. The build does not download the
client implicitly: point `--client-root` (or `FBFAST_CLIENT_ROOT`) at an SDK, or
fetch the pinned one with `scripts/download_sdk.py`. Install development
dependencies from `requirements-dev.txt`.

```sh
python3.14 scripts/build_vendor.py --release --out dist/vendor \
  --client-root /Library/Frameworks/Firebird.framework
python3.14 scripts/verify_vendor.py --package dist/vendor/fbfast
```

Use the appropriate SDK path for your platform. The build writes wheels to
`dist/wheels/` and a complete copyable package to `dist/vendor/fbfast/`.
Install the wheel with uv/pip, or copy the entire vendor package to an importable
location. Do not mix files from different builds. Stop consumers before updating
loaded native libraries, and remove an old vendored copy when switching to pip.

```python
import fbfast

print(fbfast.build_info())   # Build/ABI metadata; no database connection
print(fbfast.client_info())  # Loads the client; no database connection
```

For release automation, see [releasing](releasing.md).

## What belongs in Git?

Commit source, tests, documentation, `Cargo.lock`, build scripts, and license
inputs. `target/`, `dist/`, `.venv/`, `.tools/`, generated native/runtime files,
and macOS `.DS_Store` files are ignored. Build products belong in release/CI
artifacts or the package registry, not Git or Git LFS.

Ignoring a directory keeps it out of new commits; it does not remove local
files. Check tracked build products with `git ls-files target dist` (expected:
no output). Check local usage with `du -sh target dist`.

| Directory | Purpose | Consequence of removal |
| --- | --- | --- |
| `target/debug/` | Rust debug artifacts, dependencies, incremental cache | Next debug build/test recompiles |
| `target/release/` | Release compilation cache | Next release build recompiles |
| `dist/wheels/` | Locally built installable wheels | Rebuild before installing/sharing locally |
| `dist/vendor/` | Extracted package used by acceptance tests | Rebuild before packaging/integration tests |
| Other `dist/` directories | Downloaded CI artifacts and previous platform builds | Re-download archived artifacts or rebuild |

None is required as a permanent source dependency. Keep the release cache and
current vendor/wheel outputs if you build frequently; debug incremental caches
and duplicate cross-platform outputs are good candidates for periodic cleanup.
Keep any unpublished artifact you still need to distribute or reproduce.
Installed consumers outside these directories retain their own copies, but
consumers importing directly from `dist/vendor` need that directory.

When no build/test process is running, Cargo can reclaim its own cache:

```sh
cargo clean --dry-run  # Inspect first
cargo clean           # Remove all target artifacts; the next build is slower
```

Delete only the specific obsolete `dist/` subdirectories you have inspected.
Do not use `git clean -xfd`: it also removes ignored environments, credentials,
SDKs, and local evidence. Recreate the current package with the build
command above; do not edit a generated vendor copy as your source of truth.

## Checks

After installing `requirements-dev.txt` and building the package:

```sh
python -m ruff check python scripts tests benchmarks examples
python -m mypy python/fbfast
python scripts/check_structure.py
python scripts/verify_vendor.py --package dist/vendor/fbfast
PYTHONPATH=dist/vendor python -m pytest tests/models -q
MYPYPATH=python python -m mypy tests/typing/models.py
python -m pytest tests/packaging -q
```

The model tests require Pydantic v2 (included transitively by the development
FastAPI dependency). The package's `pydantic` extra is the explicit consumer
installation contract.

Database integration tests use an owned disposable fixture, not an operational
ERP database:

```sh
python scripts/firebird_fixture.py start
python scripts/test_python.py
python scripts/run_examples.py
```

The integration runner verifies the fixture identity and sets its connection
variables. It tests the built `dist/vendor` package, so rebuild after source
changes. `scripts/run_examples.py` runs every script in `examples/` against the
same fixture with `FB_*` variables. Rust integration/native checks have separate
harnesses under `scripts/`.

Generated documentation must match its sources:

```sh
python scripts/gen_config_docs.py --check   # docs/guide/configuration.md from the stub
python scripts/gen_api_docs.py --check      # docs/guide/api.md from the built package
python scripts/gen_llms_full.py --check     # llms-full.txt from llms.txt and docs/guide
```

Run them without `--check` to regenerate.

Alternatively, `tests/models/test_sdk_models.py` can use the private extracted
SDK server without Docker:

```sh
PYTHONPATH=dist/vendor FBFAST_MODEL_TEST_SDK=/absolute/path/to/private/sdk \
  python -m pytest tests/models/test_sdk_models.py -q
```

It requires permission to bind a loopback TCP port. The normal model unit suite
skips these two checks when the SDK variable is unset. Wheel CI also exercises
Pydantic results through its real-server smoke test.

For a conversion-only comparison (prepared dictionaries, no SQL or network):

```sh
PYTHONPATH=dist/vendor python benchmarks/run_models.py
```

The benchmark compares manual construction, manual `model_validate`, and the
public `model=` path. Its raw dictionary case reuses an existing list; it does
not measure native row decoding or dictionary allocation.
