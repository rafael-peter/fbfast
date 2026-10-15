use fbfast_core::{
    config::{Config, EnvFile, Inputs},
    error::ErrorKind,
};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

fn required() -> Inputs {
    [
        ("database".into(), json!("fixture")),
        ("user".into(), json!("reader")),
        ("password".into(), json!("secret-marker")),
    ]
    .into()
}

fn resolve(
    input: Inputs,
    env: BTreeMap<String, String>,
) -> Result<Config, fbfast_core::error::Error> {
    Config::resolve_sources(input, "FB_", EnvFile::Disabled, &env, Path::new("."))
}

#[test]
fn defaults_and_secret_are_safe() {
    let c = resolve(required(), BTreeMap::new()).unwrap();
    assert_eq!(c.pool_size, 8);
    assert_eq!(c.fetch_size, 5000);
    assert_eq!(c.wire_crypt, "required");
    assert_eq!(c.max_result_bytes, Some(256 * 1024 * 1024));
    assert_eq!(c.password.expose(), "secret-marker");
    assert!(!format!("{c:?}").contains("secret-marker"));
    assert!(!serde_json::to_string(&c).unwrap().contains("secret-marker"));
}

#[test]
fn explicit_none_overrides_environment() {
    let mut input = required();
    input.insert("role".into(), json!(null));
    input.insert("query_timeout".into(), json!(null));
    let c = resolve(
        input,
        [
            ("FB_ROLE".into(), "INHERITED".into()),
            ("FB_QUERY_TIMEOUT".into(), "9.5".into()),
        ]
        .into(),
    )
    .unwrap();
    assert_eq!(c.role, None);
    assert_eq!(c.query_timeout, None);
}

#[test]
fn precedence_and_boolean_text() {
    let mut input = required();
    input.insert("port".into(), json!(3052));
    let c = resolve(
        input,
        [
            ("FB_PORT".into(), "3051".into()),
            ("FB_WIRE_COMPRESSION".into(), "YeS".into()),
            ("FB_ROLE".into(), "".into()),
        ]
        .into(),
    )
    .unwrap();
    assert_eq!(c.port, 3052);
    assert!(c.wire_compression);
    assert!(c.role.is_none());
}

#[test]
fn rejects_invalid_values_without_echoing_them() {
    for (field, value) in [
        ("database", json!("")),
        ("user", json!(null)),
        ("password", json!(["secret-marker"])),
        ("port", json!(65536)),
        ("fetch_size", json!(0)),
        ("fetch_size", json!(true)),
        ("fetch_size", json!(1.5)),
        ("query_timeout", json!("NaN")),
        ("acquire_timeout", json!(0)),
        ("wire_compression", json!("secret-marker")),
        ("rule", json!("secret-marker")),
        ("max_result_rows", json!(-1)),
    ] {
        let mut input = required();
        input.insert(field.into(), value);
        let error = resolve(input, BTreeMap::new()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Configuration);
        assert!(error.message.contains(field));
        assert!(!error.message.contains("secret-marker"));
    }
}

#[test]
fn no_writes_and_valid_pool_bounds() {
    let mut input = required();
    input.insert("read_only".into(), json!(false));
    assert_eq!(
        resolve(input, BTreeMap::new()).unwrap_err().kind,
        ErrorKind::UnsupportedFeature
    );
    let mut input = required();
    input.insert("pool_size".into(), json!(1));
    assert!(resolve(input, BTreeMap::new())
        .unwrap_err()
        .message
        .contains("min_idle"));
}

#[test]
fn optional_limits_and_checked_duration() {
    let c = resolve(
        required(),
        [
            ("FB_MAX_RESULT_BYTES".into(), "none".into()),
            ("FB_QUERY_TIMEOUT".into(), "NoNe".into()),
        ]
        .into(),
    )
    .unwrap();
    assert_eq!(c.max_result_bytes, None);
    assert_eq!(c.query_timeout, None);
    let mut input = required();
    input.insert("connect_timeout".into(), json!(4_294_967.296));
    assert!(resolve(input, BTreeMap::new()).is_err());
}
