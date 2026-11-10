# AGENTS.md

Instructions for coding agents (and humans) changing this repository. For
*using* fbfast, read [`python/fbfast/llms.txt`](python/fbfast/llms.txt) and
[`docs/guide/`](docs/guide/) instead.

## What this is

fbfast is a read-only Firebird 5 client for Python. Layers, from the bottom:

| Path | Role |
| --- | --- |
| `crates/fbfast-native/` | C++ bridge (`bridge/*.cpp`) and Rust FFI over the official `fbclient` |
| `crates/fbfast-core/` | Pool, scheduling, execution, decoding and result sinks (rows, JSON, Arrow); `#![forbid(unsafe_code)]` |
| `crates/fbfast-python/` | PyO3 binding, the only crate that may depend on `pyo3` |
| `python/fbfast/` | Public Python API; `_native.pyi` is the stub of the extension |
| `tests/` | `integration` (real server), `models` and `packaging` (no server), `typing` (static contract) |
| `examples/` | Runnable scripts, executed against a disposable server |
| `docs/guide/` | Public guides; `docs/plans`, `docs/decisions`, `docs/progress.md` are internal |

Allowed crate dependencies: `fbfast-python -> fbfast-core -> fbfast-native`.
`scripts/check_structure.py` enforces this.

## Hard rules

- **Disposable database only.** Integration tests, examples and benchmarks run
  against the fixture owned by `scripts/firebird_fixture.py` (a labelled Docker
  container on a tmpfs). Never point them at an operational or customer
  database, never add another target, and never print the credentials stored
  in `.tools/firebird-fixture.json`.
- **Never run Cargo builds in parallel.** `cargo test`, `cargo clippy`,
  `scripts/build_vendor.py` and `scripts/test_native.py` share `target/`; run
  them one at a time.
- **350 lines per source file.** Every `.rs`, `.cpp`, `.h` and `.py` under
  `crates/`, `python/` and `scripts/` must stay at or below 350 lines (above 500
  must be split). `scripts/check_structure.py` fails otherwise. Keep examples
  short as well.
- **API changed? Update everything that describes it** in the same change:
  `python/fbfast/_native.pyi` (stub), docstrings, `docs/guide/` (regenerate
  `api.md` and `configuration.md`), `python/fbfast/llms.txt`, `llms.txt` and
  `llms-full.txt`, `README.md`, `examples/`, `tests/typing/models.py` and
  `tests/models/test_api_surface.py`.
- **Language.** Public documentation (README, `docs/guide/`, docstrings,
  `llms.txt`, examples, error messages) is in English. Plans, decisions and
  progress notes (`docs/plans/`, `docs/decisions/`, `docs/progress.md`) are in
  Portuguese.
- **Invariants no optimization may break:** read-only transactions only;
  `wire_crypt="required"` by default; exact values (no silent rounding); cleanup
  always runs; snapshots stay consistent; SQL that reached the server is never
  repeated automatically.
- **API conventions.** No suffix = blocking; `_async` suffix (and `aclose`) =
  coroutine. Parameters are positional `?`. New features go on `Pool`/`Snapshot`;
  the module-level functions in `python/fbfast/api.py` are frozen. Errors derive
  from `FbFastError` (`python/fbfast/_errors.py`) and always carry `phase`,
  `sqlstate`, `native_codes` and `cleanup_errors`.
- Do not edit generated files by hand: `docs/guide/api.md`,
  `docs/guide/configuration.md`, `llms-full.txt`. Do not commit `target/`,
  `dist/`, `.tools/` or `.venv/`.

## Setup

Rust 1.99.1 (`rust-toolchain.toml`), a C++17 compiler, CPython 3.11-3.15 with
the GIL, Docker for the fixture, and a Firebird 5.0.x SDK (5.0.3 or newer) in
`FBFAST_CLIENT_ROOT` (default `/Library/Frameworks/Firebird.framework`;
`scripts/download_sdk.py` fetches the pinned one). Development tools:
`python -m pip install -r requirements-dev.txt` into `.venv`.

## Verification

Run from the repository root with the project virtual environment. Cheap checks
first; the CI quality job runs the same commands.

```sh
# Static checks (no build, no server)
.venv/bin/ruff check python scripts tests benchmarks examples
.venv/bin/mypy python/fbfast
MYPYPATH=python .venv/bin/mypy tests/typing/models.py
.venv/bin/python scripts/check_structure.py
.venv/bin/python scripts/gen_config_docs.py --check
.venv/bin/python scripts/gen_llms_full.py --check
cargo fmt --all --check

# Rust (one Cargo command at a time)
PYO3_PYTHON=.venv/bin/python cargo clippy --workspace --all-targets -- -D warnings
cargo test -p fbfast-core -p fbfast-native --lib --tests

# Build the package that every Python test imports (rebuild after any source change)
.venv/bin/python scripts/build_vendor.py --release --out dist/vendor
.venv/bin/python scripts/verify_vendor.py --package dist/vendor/fbfast
.venv/bin/python scripts/gen_api_docs.py --check

# Python tests without a server
PYTHONPATH=dist/vendor .venv/bin/python -m pytest tests/models -q
.venv/bin/python -m pytest tests/packaging -q

# Against the disposable fixture
.venv/bin/python scripts/firebird_fixture.py start    # or: status
.venv/bin/python scripts/test_python.py              # tests/integration on dist/vendor
.venv/bin/python scripts/test_native.py              # live Rust/C++ tests (Cargo)
.venv/bin/python scripts/run_examples.py             # every examples/*.py
.venv/bin/python scripts/firebird_fixture.py stop
```

After changing the stub or the API, also run
`python -m mypy.stubtest fbfast._native --allowlist scripts/stubtest_allowlist.txt --ignore-unused-allowlist`
with the built package importable, and regenerate the references with
`scripts/gen_api_docs.py`, `scripts/gen_config_docs.py` and
`scripts/gen_llms_full.py`.

More detail: [development](docs/development.md), [releasing](docs/releasing.md),
[security model](docs/security.md).
