use super::*;

#[test]
fn latin1_maps_every_byte_to_the_same_code_point() {
    let bytes: Vec<u8> = (0..=255).collect();
    let expected: String = (0..=255u32).map(|b| char::from_u32(b).unwrap()).collect();
    assert_eq!(latin1_to_utf8(bytes), expected.into_bytes());
    assert_eq!(latin1_to_utf8(b"plain ascii".to_vec()), b"plain ascii");
    assert_eq!(latin1_to_utf8(vec![0xe7, 0xe3]), "çã".as_bytes());
    assert!(local_text(4).is_some() && local_text(21).is_some());
    assert!(
        local_text(53).is_none(),
        "WIN1252 keeps the server-side filter"
    );
}
use crate::{
    config::{Config, EnvFile},
    execution::{control::QueryOptions, ExecutionConnection},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
};

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn cancellation_and_deadline_between_blob_reads_cleanup_transaction() {
    let settings = Config::resolve_sources(
        [
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
        .into(),
        "FB_",
        EnvFile::Disabled,
        &BTreeMap::new(),
        Path::new("."),
    )
    .unwrap();
    let library = PathBuf::from(env::var("FBFAST_TEST_CLIENT").unwrap());
    let mut connection = ExecutionConnection::connect(settings, &library).unwrap();
    for timeout in [false, true] {
        let cancel = Cancellation::default();
        let options = QueryOptions {
            first: true,
            unique_names: false,
            prevalidated: false,
            cancellation: &cancel,
            deadline: None,
            cleaning: &|| Ok(false),
        };
        let result = connection.operation(
            "SELECT BINARY_VALUE FROM FBFAST_BLOBS WHERE ID=1048577",
            &options,
            |c, entry, _| {
                c.native.open_cursor(entry.id, &[], 5000)?;
                let batch = c.native.fetch(1, 1024)?;
                let offset = entry.plan.schema.columns[0].offset as usize;
                let id = batch.row(0).unwrap()[offset..][..8].try_into().unwrap();
                c.native.open_blob(id, 0)?;
                let (count, eof) = c.native.read_blob(&mut [0; 32768])?;
                assert!(count > 0 && !eof);
                // Deterministically interrupt BETWEEN real reads, with an open
                // BLOB and partially consumed data. No server cancel is needed.
                if !timeout {
                    cancel.cancel();
                }
                Blobs {
                    native: &mut c.native,
                    cancellation: &cancel,
                    deadline: timeout.then(Instant::now),
                    limit: 2 * 1024 * 1024,
                }
                .consume()
            },
        );
        let error = result.unwrap_err();
        assert_eq!(
            error.kind,
            if timeout {
                ErrorKind::QueryTimeout
            } else {
                ErrorKind::Cancelled
            }
        );
        assert!(error.cleanup.is_empty(), "{error:?}");
        assert!(connection.is_healthy());
        let result = connection
            .fetch_rows(
                "SELECT COUNT(*) FROM MON$TRANSACTIONS WHERE MON$ATTACHMENT_ID=CURRENT_CONNECTION",
                &[],
                true,
                None,
            )
            .unwrap();
        assert_eq!(result.cells, [crate::decode::Value::Int(1)]);
        assert_eq!(
            connection
                .fetch_rows(
                    "SELECT BINARY_VALUE FROM FBFAST_BLOBS WHERE ID=1",
                    &[],
                    true,
                    None
                )
                .unwrap()
                .cells,
            [crate::decode::Value::Bytes(vec![0, 255, 128, 0])]
        );
    }
    connection.close().unwrap();
}
