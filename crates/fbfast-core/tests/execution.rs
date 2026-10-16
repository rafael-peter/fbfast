use fbfast_core::{
    config::{Config, EnvFile, Inputs},
    decode::Value,
    error::ErrorKind,
    execution::ExecutionConnection,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
};

fn config() -> Config {
    let values: Inputs = [
        ("host".into(), json!("127.0.0.1")),
        (
            "port".into(),
            json!(env::var("FBFAST_TEST_PORT")
                .unwrap()
                .parse::<u16>()
                .unwrap()),
        ),
        (
            "database".into(),
            json!("/var/lib/firebird/data/fbfast.fdb"),
        ),
        ("user".into(), json!("FBFAST_READER")),
        (
            "password".into(),
            json!(env::var("FBFAST_TEST_PASSWORD").unwrap()),
        ),
        ("role".into(), json!("FBFAST_READONLY")),
    ]
    .into();
    Config::resolve_sources(
        values,
        "FB_",
        EnvFile::Disabled,
        &BTreeMap::new(),
        Path::new("."),
    )
    .unwrap()
}
fn library() -> PathBuf {
    env::var("FBFAST_TEST_CLIENT").unwrap().into()
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn handshake_observes_settings_and_is_not_charged_to_result_budget() {
    for (requested, effective) in [
        ("utc", "UTC"),
        ("GMT", "GMT"),
        ("America/Sao_Paulo", "America/Sao_Paulo"),
        ("+5:30", "+05:30"),
        ("-00:00", "+00:00"),
    ] {
        let mut options = config();
        options.session_time_zone = requested.into();
        options.max_result_rows = Some(1);
        options.max_result_bytes = Some(1);
        let mut c = ExecutionConnection::connect(options, &library()).unwrap();
        let info = c.session_info();
        assert_eq!(info.time_zone, effective);
        assert_eq!(info.charset, "UTF8");
        assert!(info.wire_encrypted);
        assert!(matches!(info.network_protocol.as_str(), "TCPv4" | "TCPv6"));
        let error = c
            .fetch_rows("SELECT ID FROM FBFAST_TYPES", &[], false, None)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::ResultLimit);
        assert!(error.message.contains("max_result_bytes"));
        c.close().unwrap();
    }
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn scalar_values_binding_exactness_and_cleanup() {
    let mut c = ExecutionConnection::connect(config(), &library()).unwrap();
    let sql = "SELECT ID, LABEL, PADDED, AMOUNT, WIDE_INTEGER, FLAG, DAY_VALUE, TIME_VALUE, STAMP_VALUE FROM FBFAST_TYPES ORDER BY ID";
    let first = c.fetch_rows(sql, &[], false, Some(5.0)).unwrap();
    assert_eq!(first.count, 2);
    let row = first.row(0).unwrap();
    assert_eq!(row[0], Value::Int(1));
    assert_eq!(row[1], Value::Text("Olá 日本語".into()));
    assert_eq!(row[2], Value::Text("a       ".into()));
    assert_eq!(
        row[3],
        Value::Decimal {
            coefficient: -123456789012,
            scale: -4
        }
    );
    assert_eq!(row[4], Value::Int(i128::MAX));
    assert_eq!(row[5], Value::Bool(true));
    assert_eq!(row[6], Value::Date(60369));
    assert_eq!(row[7], Value::Time(452961234));
    assert_eq!(
        row[8],
        Value::Timestamp {
            date: 60369,
            time: 452961234
        }
    );
    assert!(first.row(1).unwrap()[1..].iter().all(|v| *v == Value::Null));
    let hits = c.cache_counts().0;
    let second = c.fetch_rows(sql, &[], false, Some(5.0)).unwrap();
    assert_eq!(first.cells, second.cells);
    assert_eq!(c.cache_counts().0, hits + 1);
    let bound = c
        .fetch_rows(
            "SELECT ID FROM FBFAST_TYPES WHERE ID = ?",
            &[Value::Int(2)],
            true,
            Some(5.0),
        )
        .unwrap();
    assert_eq!(bound.row(0), Some([Value::Int(2)].as_slice()));
    let decimal = Value::Decimal {
        coefficient: -123456789012,
        scale: -4,
    };
    let bound = c
        .fetch_rows(
            "SELECT ID FROM FBFAST_TYPES WHERE AMOUNT = ?",
            &[decimal],
            false,
            Some(5.0),
        )
        .unwrap();
    assert_eq!(bound.count, 1);
    let quoted = c
        .fetch_rows(
            "SELECT CAST(? AS VARCHAR(80)) FROM RDB$DATABASE",
            &[Value::Text("'; DELETE FROM FBFAST_TYPES; --".into())],
            false,
            Some(5.0),
        )
        .unwrap();
    assert_eq!(
        quoted.row(0).unwrap()[0],
        Value::Text("'; DELETE FROM FBFAST_TYPES; --".into())
    );
    c.clear_statement_cache().unwrap();
    c.close().unwrap();
    assert_eq!(first.row(0).unwrap()[4], Value::Int(i128::MAX));
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn read_only_errors_limits_role_and_empty_schema() {
    let mut options = config();
    options.max_result_rows = Some(1);
    let mut c = ExecutionConnection::connect(options, &library()).unwrap();
    let err = c
        .fetch_rows("SELECT ID FROM FBFAST_TYPES", &[], false, Some(5.0))
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::ResultLimit);
    assert!(err.cleanup.is_empty());
    assert!(c.is_healthy());
    for sql in [
        "DELETE FROM FBFAST_TYPES RETURNING ID",
        "SELECT GEN_ID(RDB$BACKUP_HISTORY, 1) FROM RDB$DATABASE",
        "SELECT * FROM FBFAST_TYPES FOR UPDATE",
    ] {
        assert!(c.fetch_rows(sql, &[], false, Some(5.0)).is_err());
    }
    let empty = c
        .fetch_rows(
            "SELECT ID AS NAMED FROM FBFAST_TYPES WHERE 1=0",
            &[],
            false,
            Some(5.0),
        )
        .unwrap();
    assert_eq!(empty.count, 0);
    assert_eq!(empty.names, ["NAMED"]);
    let error = c
        .fetch_rows(
            "SELECT NO_SUCH_COLUMN FROM FBFAST_TYPES",
            &[],
            false,
            Some(5.0),
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Query);
    assert!(error.sqlstate.is_some());
    assert!(!error.native_codes.is_empty());
    assert!(error.cleanup.is_empty());
    assert!(!c
        .explain("SELECT ID FROM FBFAST_TYPES", &[], Some(5.0))
        .unwrap()
        .is_empty());
    c.close().unwrap();
    let mut options = config();
    options.role = Some("NOT_GRANTED".into());
    let error = match ExecutionConnection::connect(options, &library()) {
        Ok(mut c) => {
            c.close().unwrap();
            panic!("inactive role accepted");
        }
        Err(e) => e,
    };
    assert_eq!(error.kind, ErrorKind::RoleNotActive);
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn binds_overflow_null_and_byte_budget_are_checked() {
    let mut options = config();
    options.fetch_size = 1;
    options.batch_bytes = 1;
    let mut c = ExecutionConnection::connect(options, &library()).unwrap();
    let sql = "SELECT CAST(? AS SMALLINT) FROM RDB$DATABASE";
    assert!(c
        .fetch_rows(sql, &[Value::Int(32768)], false, Some(5.0))
        .is_err());
    let null = c.fetch_rows(sql, &[Value::Null], false, Some(5.0)).unwrap();
    assert_eq!(null.row(0), Some([Value::Null].as_slice()));
    let negative = c
        .fetch_rows(sql, &[Value::Int(-32768)], false, Some(5.0))
        .unwrap();
    assert_eq!(negative.row(0), Some([Value::Int(-32768)].as_slice()));
    let all = c
        .fetch_rows("SELECT ID FROM FBFAST_TYPES", &[], false, Some(5.0))
        .unwrap();
    assert_eq!(all.count, 2);
    c.close().unwrap();
}
