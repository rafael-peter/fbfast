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
    fn sql_float_482_uses_four_bytes(bits in any::<u32>()) {
        let value = f32::from_bits(bits);
        prop_assume!(value.is_finite());
        let metadata = schema(482, 4, 0, 0, 0);
        let encoded = bind::encode(&metadata, &[Value::Float(value as f64)]).unwrap();
        prop_assert_eq!(&encoded[..4], &value.to_ne_bytes());
        let mut independent = vec![0; metadata.message_length as usize];
        independent[..4].copy_from_slice(&value.to_ne_bytes());
        let decoder = DecodePlan::new(metadata, &config()).unwrap();
        let Value::Float(decoded) = decoder.value(0, &independent).unwrap() else { panic!("float expected") };
        prop_assert_eq!((decoded as f32).to_bits(), bits);
    }

    #[test]
    fn sql_double_480_uses_eight_bytes(bits in any::<u64>()) {
        let value = f64::from_bits(bits);
        prop_assume!(value.is_finite());
        let metadata = schema(480, 8, 0, 0, 0);
        let encoded = bind::encode(&metadata, &[Value::Float(value)]).unwrap();
        prop_assert_eq!(&encoded[..8], &value.to_ne_bytes());
        let mut independent = vec![0; metadata.message_length as usize];
        independent[..8].copy_from_slice(&value.to_ne_bytes());
        let decoder = DecodePlan::new(metadata, &config()).unwrap();
        let Value::Float(decoded) = decoder.value(0, &independent).unwrap() else { panic!("float expected") };
        prop_assert_eq!(decoded.to_bits(), bits);
    }

    #[test]
    fn decimal_text_bind_preserves_full_integer_coefficient_and_scale(
        value in any::<i128>(), scale in -38i32..=0, padding in 0usize..50
    ) {
        let metadata = schema(32752, 16, scale, 1, 0);
        let input = Value::DecFloat(format!("{value}{}E{}", "0".repeat(padding), scale - padding as i32));
        let bytes = bind::encode(&metadata, &[input]).unwrap();
        let decoder = DecodePlan::new(metadata, &config()).unwrap();
        prop_assert_eq!(decoder.value(0, &bytes).unwrap(), Value::Decimal { coefficient: value, scale });
    }

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
