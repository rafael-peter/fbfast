use fbfast_native::{AttachOptions, Client, Connection, Metadata, NativeError};
use std::{env, path::Path};

fn connect() -> Connection {
    let client = Client::load(Path::new(&env::var("FBFAST_TEST_CLIENT").unwrap())).unwrap();
    Connection::attach(
        client,
        &AttachOptions {
            database: &env::var("FBFAST_TEST_DSN").unwrap(),
            user: "FBFAST_READER",
            password: &env::var("FBFAST_TEST_PASSWORD").unwrap(),
            role: "FBFAST_READONLY",
            charset: "UTF8",
            time_zone: "UTC",
            app_name: "fbfast-tests",
            wire_crypt: "required",
            wire_compression: false,
            connect_seconds: 5,
        },
    )
    .unwrap()
}

fn integer(row: &[u8], metadata: &Metadata, column: usize) -> i32 {
    let c = &metadata.columns[column];
    assert_eq!(c.sql_type, 496);
    assert_eq!(
        i16::from_ne_bytes(row[c.null_offset as usize..][..2].try_into().unwrap()),
        0
    );
    i32::from_ne_bytes(row[c.offset as usize..][..4].try_into().unwrap())
}

#[test]
#[ignore = "requires the isolated fixture; scripts/test_native.py runs this"]
fn independent_cancel_generation_and_handle_lifetime() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let mut c = connect();
    let control = c.cancel_handle().unwrap();
    c.begin_operation(7).unwrap();
    assert!(!control.request(6).unwrap()); // late request cannot touch generation 7
    c.begin(false, 5, None).unwrap();
    let query = c.prepare("WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) FROM N A CROSS JOIN N B CROSS JOIN N C").unwrap();
    let cancel = control.clone();
    let start = Instant::now();
    let thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        assert!(cancel.request(7).unwrap());
    });
    // Native statement timeout is only a safety cap, far above the cancellation delay.
    let result = c
        .open_cursor(query, &[], 10_000)
        .and_then(|_| c.fetch(1, 1024).map(|_| ()));
    thread.join().unwrap();
    assert!(
        matches!(
            result,
            Err(NativeError::Database {
                kind: fbfast_native::DatabaseErrorKind::Cancelled,
                ..
            })
        ),
        "{result:?}"
    );
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(control.finish(7).unwrap());
    c.disable_cancel().unwrap();
    c.close().unwrap();
    // Control remains valid as local state after attachment disposal.
    assert!(!control.request(7).unwrap());
    assert!(!control.finish(7).unwrap());
    drop(c);
    assert!(!control.request(8).unwrap());

    let mut next = connect();
    let control = next.cancel_handle().unwrap();
    for generation in 10..110 {
        next.begin_operation(generation).unwrap();
        assert!(!control.request(generation - 1).unwrap());
        next.ping().unwrap();
        assert!(!control.finish(generation).unwrap());
    }
    next.close().unwrap();
}

#[test]
#[ignore = "requires the isolated fixture; scripts/test_native.py runs this"]
fn effective_transactions_binding_cache_and_ownership() {
    let mut c = connect();
    c.begin(false, 5, None).unwrap();
    let query = c.prepare("SELECT CAST(MON$READ_ONLY AS INTEGER) AS RO, CAST(MON$ISOLATION_MODE AS INTEGER) AS ISO FROM MON$TRANSACTIONS WHERE MON$TRANSACTION_ID = CURRENT_TRANSACTION").unwrap();
    let schema = c.metadata(query, false).unwrap();
    assert_eq!(
        schema.columns.iter().map(|c| &*c.name).collect::<Vec<_>>(),
        ["RO", "ISO"]
    );
    assert!(!c.plan(query).unwrap().is_empty());
    c.rollback().unwrap();
    c.reset().unwrap();
    for (snapshot, expected) in [(false, 4), (false, 4), (true, 1)] {
        c.begin(snapshot, 0, None).unwrap();
        c.open_cursor(query, &[], 1000).unwrap();
        assert!(c.free_statement(query).is_err()); // pinned cursor
        assert!(c.reset().is_err());
        let rows = c.fetch(2, 1024).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows.eof);
        let row = rows.row(0).unwrap();
        assert_eq!(integer(row, &schema, 0), 1);
        assert_eq!(integer(row, &schema, 1), expected);
        c.close_cursor().unwrap();
        c.rollback().unwrap();
        c.reset().unwrap();
        // Owned row buffers remain valid after rollback/reset.
        assert_eq!(integer(rows.row(0).unwrap(), &schema, 0), 1);
    }
    c.free_statement(query).unwrap();
    assert!(c.metadata(query, false).is_err());
    c.begin(false, 5, None).unwrap();
    let bound = c
        .prepare("SELECT ID FROM FBFAST_TYPES WHERE ID = ?")
        .unwrap();
    let input = c.metadata(bound, true).unwrap();
    let mut bytes = vec![0u8; input.message_length as usize];
    let param = &input.columns[0];
    bytes[param.offset as usize..][..4].copy_from_slice(&2i32.to_ne_bytes());
    assert!(c
        .open_cursor(bound, &bytes[..bytes.len() - 1], 1000)
        .is_err());
    c.open_cursor(bound, &bytes, 1000).unwrap();
    let output = c.metadata(bound, false).unwrap();
    let rows = c.fetch(4, 4096).unwrap();
    assert_eq!(integer(rows.row(0).unwrap(), &output, 0), 2);
    assert!(rows.eof);
    c.close_cursor().unwrap();
    c.rollback().unwrap();
    c.reset().unwrap();
    let mut other = connect();
    assert!(other.metadata(bound, false).is_err());
    other.close().unwrap();
    c.close().unwrap();
    c.close().unwrap();
    assert!(c.ping().is_err());
}

#[test]
#[ignore = "requires the isolated fixture; scripts/test_native.py runs this"]
fn native_error_retains_phase_sqlstate_and_codes() {
    let mut c = connect();
    c.begin(false, 5, None).unwrap();
    let error = c
        .prepare("SELECT NONEXISTENT FROM FBFAST_TYPES")
        .unwrap_err();
    match error {
        NativeError::Database {
            phase,
            sqlstate,
            codes,
            ..
        } => {
            assert_eq!(phase, "prepare");
            assert_eq!(sqlstate.len(), 5);
            assert_ne!(sqlstate, "00000");
            assert!(!codes.is_empty());
        }
        e => panic!("unexpected error: {e}"),
    }
    // Firebird 5 describes DML RETURNING as SELECT. The core tokenizer must
    // reject it before prepare; the effective read-only TPB remains essential.
    if let Ok(write) = c.prepare("DELETE FROM FBFAST_TYPES RETURNING ID") {
        if c.open_cursor(write, &[], 1000).is_ok() {
            assert!(c.fetch(2, 1024).is_err());
        }
        c.close_cursor().unwrap();
    }
    c.rollback().unwrap();
    c.reset().unwrap();
    c.close().unwrap();
}

#[test]
#[ignore = "requires the isolated fixture; scripts/test_native.py runs this"]
fn blob_segments_early_close_and_pending_cancel_cleanup() {
    let mut c = connect();
    let control = c.cancel_handle().unwrap();
    c.begin_operation(301).unwrap();
    c.begin(false, 0, None).unwrap();
    let statement = c
        .prepare("SELECT BINARY_VALUE FROM FBFAST_BLOBS WHERE ID=1048577")
        .unwrap();
    let schema = c.metadata(statement, false).unwrap();
    c.open_cursor(statement, &[], 5000).unwrap();
    let batch = c.fetch(1, 1024).unwrap();
    let offset = schema.columns[0].offset as usize;
    let id: [u8; 8] = batch.row(0).unwrap()[offset..][..8].try_into().unwrap();
    c.open_blob(id, 0).unwrap();
    assert!(c.open_blob(id, 0).is_err());
    assert!(c.close_cursor().is_err());
    assert!(c.rollback().is_err());
    let mut small = [0u8; 7];
    let (count, eof) = c.read_blob(&mut small).unwrap();
    assert!(!eof && count > 0 && count <= small.len());
    assert!(small[..count].iter().all(|b| *b == b'x'));
    c.close_blob().unwrap(); // A partial segment is safely abandoned.
    c.open_blob(id, 0).unwrap();
    let mut data = [0u8; 32768];
    let (count, eof) = c.read_blob(&mut data).unwrap();
    assert!(!eof && count > 0);
    assert!(data[..count].iter().all(|b| *b == b'x'));
    assert!(control.request(301).unwrap());
    // Firebird may serve buffered segments without consuming a cancel request.
    // Core independently checks Cancellation between bounded read calls.
    if let Err(error) = c.read_blob(&mut data) {
        assert!(
            matches!(
                error,
                NativeError::Database {
                    kind: fbfast_native::DatabaseErrorKind::Cancelled,
                    phase: "read_blob",
                    ..
                }
            ),
            "{error:?}"
        );
    }
    assert!(control.finish(301).unwrap());
    c.disable_cancel().unwrap();
    c.close_blob().unwrap();
    c.close_blob().unwrap();
    c.close_cursor().unwrap();
    c.rollback().unwrap();
    c.reset().unwrap();
    c.close().unwrap();
}
