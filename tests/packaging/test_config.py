"""Exercise the Rust configuration through the same copied package as consumers."""

from pathlib import Path

from test_vendor import execute


def test_constructor_reads_environment_then_defaults_without_from_env(consumer: Path) -> None:
    result = execute(
        consumer,
        """
import fbfast, os
os.environ.update(FB_DATABASE='fixture', FB_USER='reader', FB_PASSWORD='secret',
                  FB_HOST='from-environment', FB_QUERY_TIMEOUT='12.5',
                  FB_WIRE_COMPRESSION='yes')
before = dict(os.environ)
config = fbfast.FirebirdConfig()
assert (config.host, config.database, config.user) == ('from-environment', 'fixture', 'reader')
assert config.query_timeout == 12.5 and config.wire_compression is True
assert config.port == 3050 and config.pool_size == 8 and config.min_idle == 2
assert config.wire_crypt == 'required' and config.max_result_rows is None
assert fbfast.FirebirdConfig(host='explicit').host == 'explicit'
del os.environ['FB_HOST']
assert fbfast.FirebirdConfig().host == 'localhost'
os.environ['FB_HOST'] = 'from-environment'
assert dict(os.environ) == before
""",
    )
    assert result.returncode == 0, result.stderr


def test_constructor_requires_credentials_in_some_source(consumer: Path) -> None:
    result = execute(
        consumer,
        """
import fbfast, os
base = dict(FB_DATABASE='fixture', FB_USER='reader', FB_PASSWORD='secret-marker')
for missing in base:
    os.environ.update(base)
    del os.environ[missing]
    try:
        fbfast.FirebirdConfig()
    except fbfast.ConfigurationError as exc:
        assert missing.removeprefix('FB_').lower() in str(exc)
        assert 'secret-marker' not in str(exc)
    else:
        raise AssertionError(f'{missing} silently defaulted')
""",
    )
    assert result.returncode == 0, result.stderr


def test_only_row_limit_remains_in_config(consumer: Path) -> None:
    result = execute(
        consumer,
        """
import fbfast, os
os.environ.update(FB_DATABASE='fixture', FB_USER='reader', FB_PASSWORD='secret',
                  FB_MAX_RESULT_BYTES='1', FB_MAX_RESULT_ROWS='17')
config = fbfast.FirebirdConfig()
assert config.max_result_rows == 17
assert not hasattr(config, 'max_result_bytes')
assert fbfast.FirebirdConfig(max_result_rows=None).max_result_rows is None
try:
    fbfast.FirebirdConfig(max_result_bytes=1)
except fbfast.ConfigurationError as exc:
    assert 'max_result_bytes' in str(exc) and 'unknown option' in str(exc)
else:
    raise AssertionError('removed option still accepted')
""",
    )
    assert result.returncode == 0, result.stderr


def test_invalid_environment_is_not_absent_and_explicit_values_win(consumer: Path) -> None:
    (consumer / ".env").write_text("FB_HOST=from-file\nFB_PORT=3051\n")
    result = execute(
        consumer,
        """
import fbfast, os
os.environ.update(FB_DATABASE='fixture', FB_USER='reader', FB_PASSWORD='secret',
                  FB_HOST='', FB_PORT='invalid')
try:
    fbfast.FirebirdConfig()
except fbfast.ConfigurationError:
    pass
else:
    raise AssertionError('invalid environment fell back to file/default')
config = fbfast.FirebirdConfig(host='explicit', port=3053)
assert config.host == 'explicit' and config.port == 3053
del os.environ['FB_HOST']
del os.environ['FB_PORT']
config = fbfast.FirebirdConfig(env_file=False)
assert config.host == 'localhost' and config.port == 3050
assert config.database == 'fixture'
""",
    )
    assert result.returncode == 0, result.stderr


def test_config_precedence_and_no_environment_mutation(consumer: Path) -> None:
    (consumer / ".env").write_text(
        "FB_DATABASE=from_file\nFB_USER=reader\nFB_PASSWORD=secret-marker\n"
        "FB_PORT=3051\nFB_ROLE=FROM_FILE\nFB_QUERY_TIMEOUT=none\n"
    )
    result = execute(
        consumer,
        """
import fbfast, os
os.environ['FB_PORT'] = '3052'
os.environ['FB_ROLE'] = 'FROM_ENV'
before = dict(os.environ)
config = fbfast.FirebirdConfig(port=3053, role=None)
if (config.port, config.role, config.database, config.query_timeout) != (3053, None, 'from_file', None):
    raise RuntimeError('precedence failed')
if dict(os.environ) != before:
    raise RuntimeError('dotenv changed os.environ')
if 'secret-marker' in repr(config):
    raise RuntimeError('password leaked')
try:
    config.port = 3055
except AttributeError:
    pass
else:
    raise RuntimeError('config is mutable')
""",
    )
    assert result.returncode == 0, result.stderr


def test_custom_prefix_and_disabled_dotenv(consumer: Path) -> None:
    (consumer / ".env").write_text("this is invalid dotenv\n")
    result = execute(
        consumer,
        """
import fbfast, os
os.environ.update(FB_BI_DATABASE='fixture', FB_BI_USER='reader', FB_BI_PASSWORD='secret')
config = fbfast.FirebirdConfig.from_env(prefix='FB_BI_', env_file=False)
if config.database != 'fixture':
    raise RuntimeError('custom prefix failed')
""",
    )
    assert result.returncode == 0, result.stderr


def test_missing_file_and_malformed_secret_are_redacted(consumer: Path) -> None:
    (consumer / ".env").write_text('FB_PASSWORD="secret-marker\n')
    result = execute(
        consumer,
        """
import fbfast
for file in ['missing.env', '.env']:
    try:
        fbfast.FirebirdConfig(env_file=file)
    except fbfast.ConfigurationError as exc:
        if 'secret-marker' in str(exc):
            raise RuntimeError('secret leaked')
    else:
        raise RuntimeError('invalid dotenv was accepted')
""",
    )
    assert result.returncode == 0, result.stderr


def test_read_only_unknown_option_and_numeric_validation(consumer: Path) -> None:
    result = execute(
        consumer,
        """
import fbfast
base = dict(database='fixture', user='reader', password='secret-marker', env_file=False)
try:
    fbfast.FirebirdConfig(**base, read_only=False)
except fbfast.UnsupportedFeatureError:
    pass
else:
    raise RuntimeError('write configuration accepted')
for invalid in [dict(rule='role'), dict(port=65536), dict(query_timeout=float('nan')),
                dict(fetch_size=True), dict(fetch_size=1.5), dict(pool_size=1),
                dict(max_blob_bytes=-1), dict(read_only='maybe')]:
    try:
        fbfast.FirebirdConfig(**base, **invalid)
    except fbfast.ConfigurationError as exc:
        if 'secret-marker' in str(exc):
            raise RuntimeError('secret leaked')
    else:
        raise RuntimeError(f'invalid configuration accepted: {invalid}')
""",
    )
    assert result.returncode == 0, result.stderr
