"""Exercise the Rust configuration through the same copied package as consumers."""

from pathlib import Path

from test_vendor import execute


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
