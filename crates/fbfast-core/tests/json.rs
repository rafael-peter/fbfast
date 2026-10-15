mod support;
use fbfast_core::{
    decode::Value,
    error::{Error, ErrorKind},
    execution::{control::Cancellation, ExecutionConnection},
    sinks::json::{JsonFormat, JsonOptions, JsonOutput},
};
use support::{client, config, connection, options};

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn final_array_chunk_observes_closed_transaction_and_exact_byte_accounting() {
    let mut settings = config();
    settings.batch_bytes = 17;
    let mut c = ExecutionConnection::connect(settings, &client()).unwrap();
    let identity = c
        .fetch_rows(
            "SELECT CURRENT_CONNECTION FROM RDB$DATABASE",
            &[],
            true,
            None,
        )
        .unwrap();
    let attachment = identity.cells[0].clone();
    let mut observer = connection();
    let mut chunks = Vec::new();
    let mut rows = 0;
    let mut saw_active = false;
    let mut saw_closed = false;
    let summary = c
        .stream_json_with(
            "SELECT ID, LABEL FROM FBFAST_TYPES ORDER BY ID",
            &[],
            2,
            (JsonOptions::default(), JsonFormat::Array),
            &options(&Cancellation::default()),
            &mut |chunk: JsonOutput| {
                assert!(chunk.data.len() <= 17);
                let final_chunk = chunk.data == b"]";
                if final_chunk || !saw_active {
                    let count = observer
                        .fetch_rows(
                            "SELECT COUNT(*) FROM MON$TRANSACTIONS WHERE MON$ATTACHMENT_ID=?",
                            std::slice::from_ref(&attachment),
                            true,
                            None,
                        )
                        .unwrap();
                    assert_eq!(count.cells, [Value::Int(if final_chunk { 0 } else { 1 })]);
                    saw_closed |= final_chunk;
                    saw_active |= !final_chunk;
                }
                rows += chunk.rows;
                chunks.push(chunk.data);
                Ok(!final_chunk) // false still means the final chunk was consumed
            },
        )
        .unwrap();
    assert!(saw_active && saw_closed);
    let data = chunks.concat();
    assert_eq!(
        data,
        "[{\"ID\":1,\"LABEL\":\"Olá 日本語\"},{\"ID\":2,\"LABEL\":null}]".as_bytes()
    );
    assert_eq!(summary.rows, 2);
    assert_eq!(rows, 2);
    assert_eq!(summary.bytes, data.len() as u64);
    assert_eq!(summary.batches, chunks.len() as u64);
    c.close().unwrap();
    observer.close().unwrap();
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn json_consumer_stop_or_failure_does_not_close_partial_array() {
    let mut settings = config();
    settings.batch_bytes = 1;
    let mut c = ExecutionConnection::connect(settings, &client()).unwrap();
    for fail in [false, true] {
        let mut data = Vec::new();
        let result = c.stream_json_with(
            "SELECT ID FROM FBFAST_TYPES",
            &[],
            1,
            (JsonOptions::default(), JsonFormat::Array),
            &options(&Cancellation::default()),
            &mut |chunk: JsonOutput| {
                data.extend(chunk.data);
                if fail {
                    Err(Error::new(
                        ErrorKind::DataConversion,
                        "sink",
                        "consumer failure",
                    ))
                } else {
                    Ok(false)
                }
            },
        );
        if fail {
            assert_eq!(result.unwrap_err().kind, ErrorKind::DataConversion);
        } else {
            assert_eq!(result.unwrap().rows, 0);
        }
        assert_eq!(data, b"[");
        assert!(c.is_healthy());
        assert_eq!(
            c.json_with(
                "SELECT 1 AS N FROM RDB$DATABASE",
                &[],
                JsonOptions::default(),
                &options(&Cancellation::default())
            )
            .unwrap()
            .data,
            b"[{\"N\":1}]"
        );
    }
    c.close().unwrap();
}
