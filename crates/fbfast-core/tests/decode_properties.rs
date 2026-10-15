use fbfast_core::{
    config::{Config, EnvFile},
    decode::{bind, DecodePlan, Value},
};
use fbfast_native::{Column, Metadata};
use proptest::prelude::*;
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

fn config() -> Config {
    Config::resolve_sources(
        [
            ("database".into(), json!("test")),
            ("user".into(), json!("test")),
            ("password".into(), json!("secret")),
        ]
        .into(),
        "FB_",
        EnvFile::Disabled,
        &BTreeMap::new(),
        Path::new("."),
    )
    .unwrap()
}
fn schema(sql_type: u32, length: u32, scale: i32, subtype: i32, charset: u32) -> Metadata {
    Metadata {
        columns: vec![Column {
            name: "VALUE".into(),
            sql_type,
            length,
            charset,
            offset: 0,
            null_offset: length + 2,
            scale,
            subtype,
        }],
        message_length: length + 4,
    }
}

proptest! {
    #[test]
    fn all_int128_bits_survive_bind_and_decode(value in any::<i128>()) {
        let metadata = schema(32752, 16, 0, 0, 0);
        let bytes = bind::encode(&metadata, &[Value::Int(value)]).unwrap();
        let decoder = DecodePlan::new(metadata, &config()).unwrap();
        prop_assert_eq!(decoder.value(0, &bytes).unwrap(), Value::Int(value));
    }

    #[test]
    fn numeric_coefficients_and_scales_are_exact(value in any::<i64>(), scale in -18i32..=0) {
        let metadata = schema(580, 8, scale, 1, 0);
        let expected = Value::Decimal { coefficient: value as i128, scale };
        let bytes = bind::encode(&metadata, std::slice::from_ref(&expected)).unwrap();
        let decoder = DecodePlan::new(metadata, &config()).unwrap();
        prop_assert_eq!(decoder.value(0, &bytes).unwrap(), expected);
    }

    #[test]
    fn utf8_char_preserves_sql_padding_without_transport_padding(value in ".{0,8}") {
        let metadata = schema(452, 32, 0, 0, 4);
        let bytes = bind::encode(&metadata, &[Value::Text(value.clone())]).unwrap();
        let decoder = DecodePlan::new(metadata, &config()).unwrap();
        let expected = format!("{}{}", value, " ".repeat(8 - value.chars().count()));
        prop_assert_eq!(decoder.value(0, &bytes).unwrap(), Value::Text(expected));
    }

    #[test]
    fn arbitrary_short_messages_fail_without_panicking(data in proptest::collection::vec(any::<u8>(), 0..18)) {
        let decoder = DecodePlan::new(schema(32752, 16, 0, 0, 0), &config()).unwrap();
        prop_assert!(decoder.value(0, &data).is_err());
    }
}

#[test]
fn rescaling_does_not_round_or_overflow() {
    assert_eq!(bind::rescale(1234, -2, -1), None);
    assert_eq!(bind::rescale(1230, -2, -1), Some(123));
    assert_eq!(bind::rescale(i128::MAX, 0, -1), None);
    assert_eq!(bind::rescale(i128::MIN, 0, 0), Some(i128::MIN));
    assert_eq!(bind::rescale(0, i32::MIN, i32::MAX), Some(0));
    assert_eq!(bind::rescale(1, i32::MIN, i32::MAX), None);
}

#[test]
fn utf8_character_limit_is_not_only_a_byte_limit() {
    assert!(bind::encode(
        &schema(448, 32, 0, 0, 4),
        &[Value::Text("123456789".into())]
    )
    .is_err());
}
