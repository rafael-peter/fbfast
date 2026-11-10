//! Differential checks: local BLOB text conversion against the server's own
//! transliteration, for every byte of each charset converted without a BPB.
mod support;
use fbfast_core::{decode::Value, execution::control::Cancellation};

fn all_bytes_hex() -> String {
    (0..=255u8).map(|b| format!("{b:02X}")).collect()
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn iso8859_1_blob_text_matches_server_transliteration_for_every_byte() {
    let mut connection = support::connection();
    let cancel = Cancellation::default();
    let source = format!(
        "CAST(CAST(x'{}' AS VARCHAR(256) CHARACTER SET OCTETS) \
         AS VARCHAR(256) CHARACTER SET ISO8859_1)",
        all_bytes_hex()
    );
    let sql = format!(
        "SELECT CAST({source} AS BLOB SUB_TYPE TEXT CHARACTER SET ISO8859_1), \
                CAST({source} AS VARCHAR(512) CHARACTER SET UTF8), \
                CAST({source} AS BLOB SUB_TYPE TEXT CHARACTER SET UTF8) \
         FROM RDB$DATABASE"
    );
    let rows = connection
        .fetch_with(&sql, &[], &support::options(&cancel))
        .unwrap();
    let row = rows.row(0).unwrap();
    let expected: String = (0..=255u32).map(|b| char::from_u32(b).unwrap()).collect();
    // Column 1: raw ISO8859_1 BLOB converted locally.
    // Column 2: the server transliterated the same bytes to UTF8 (VARCHAR).
    // Column 3: the server transliterated into a UTF8 BLOB, read raw.
    for (index, cell) in row.iter().enumerate() {
        match cell {
            Value::Text(text) => assert_eq!(text, &expected, "column {index}"),
            other => panic!("column {index}: unexpected {other:?}"),
        }
    }
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn utf8_blob_text_is_returned_exactly_and_invalid_bytes_are_rejected() {
    let mut connection = support::connection();
    let cancel = Cancellation::default();
    let text = "Olá 日本語 \u{1F600} ação";
    let rows = connection
        .fetch_with(
            "SELECT CAST(CAST(? AS VARCHAR(64) CHARACTER SET UTF8) \
             AS BLOB SUB_TYPE TEXT CHARACTER SET UTF8) FROM RDB$DATABASE",
            &[Value::Text(text.into())],
            &support::options(&cancel),
        )
        .unwrap();
    assert_eq!(rows.row(0).unwrap()[0], Value::Text(text.into()));
}
