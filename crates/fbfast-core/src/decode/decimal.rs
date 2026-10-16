//! Exact decimal text validation; never routes decimals through binary floats.
use super::plan::conversion;
use crate::error::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecimalParts {
    Finite {
        negative: bool,
        digits: String,
        exponent: i64,
    },
    Infinity {
        negative: bool,
    },
    Nan {
        negative: bool,
        signaling: bool,
        payload: String,
    },
}

impl DecimalParts {
    pub fn parse(text: &str) -> Option<Self> {
        let negative = text.starts_with('-');
        let text = text.strip_prefix(['-', '+']).unwrap_or(text);
        if text == "Infinity" {
            return Some(Self::Infinity { negative });
        }
        for (prefix, signaling) in [("sNaN", true), ("NaN", false)] {
            if let Some(payload) = text.strip_prefix(prefix) {
                return payload
                    .bytes()
                    .all(|b| b.is_ascii_digit())
                    .then(|| Self::Nan {
                        negative,
                        signaling,
                        payload: payload.trim_start_matches('0').into(),
                    });
            }
        }
        let (mantissa, exponent) = match text.split_once(['e', 'E']) {
            Some((mantissa, exponent)) => (mantissa, exponent.parse::<i64>().ok()?),
            None => (text, 0),
        };
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        if whole.is_empty()
            || !whole
                .bytes()
                .chain(fraction.bytes())
                .all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let exponent = exponent.checked_sub(i64::try_from(fraction.len()).ok()?)?;
        let digits = format!("{whole}{fraction}");
        let digits = digits.trim_start_matches('0');
        Some(Self::Finite {
            negative,
            digits: if digits.is_empty() {
                "0".into()
            } else {
                digits.into()
            },
            exponent,
        })
    }

    fn normalized(mut self) -> Option<Self> {
        if let Self::Finite {
            digits, exponent, ..
        } = &mut self
        {
            if digits == "0" {
                *exponent = 0;
            } else {
                let length = digits.trim_end_matches('0').len();
                *exponent = exponent.checked_add(i64::try_from(digits.len() - length).ok()?)?;
                digits.truncate(length);
            }
        }
        Some(self)
    }

    pub fn same_value(&self, other: &Self) -> bool {
        let Some(left) = self.clone().normalized() else {
            return false;
        };
        let Some(right) = other.clone().normalized() else {
            return false;
        };
        left == right
    }

    pub fn scaled_i128(self, scale: i32) -> Option<i128> {
        let Self::Finite {
            negative,
            digits,
            exponent,
        } = self.normalized()?
        else {
            return None;
        };
        if digits == "0" {
            return Some(0);
        }
        let zeros = usize::try_from(exponent.checked_sub(scale as i64)?).ok()?;
        if digits.len().checked_add(zeros)? > 39 {
            return None;
        }
        format!(
            "{}{digits}{}",
            if negative { "-" } else { "" },
            "0".repeat(zeros)
        )
        .parse()
        .ok()
    }

    /// A validated JSON number, preserving signed zero and the decimal exponent.
    pub fn json_number(&self) -> Option<String> {
        match self {
            Self::Finite {
                negative,
                digits,
                exponent,
            } => Some(format!(
                "{}{digits}e{exponent}",
                if *negative { "-" } else { "" }
            )),
            _ => None,
        }
    }
}

pub fn encode(
    connection: &mut fbfast_native::Connection,
    kind: u32,
    text: &str,
) -> Result<Vec<u8>, Error> {
    let original =
        DecimalParts::parse(text).ok_or_else(|| conversion("Invalid decimal parameter"))?;
    let data = connection.decimal_bytes(kind, text)?;
    let returned = connection.decimal_text(kind, &data)?;
    let returned = DecimalParts::parse(&returned)
        .ok_or_else(|| conversion("Invalid native decimal representation"))?;
    let zero_quantum_changed = matches!((&original, &returned),
        (DecimalParts::Finite { digits: a, exponent: x, .. },
         DecimalParts::Finite { digits: b, exponent: y, .. }) if a == "0" && b == "0" && x != y);
    if zero_quantum_changed || !original.same_value(&returned) {
        return Err(conversion(
            "Decimal cannot be represented exactly in the target DECFLOAT",
        ));
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_rescale_and_equality_preserve_special_values_and_zero_sign() {
        let parse = |text| DecimalParts::parse(text).unwrap();
        for (a, b) in [
            ("1.2300", "123e-2"),
            ("-0.000", "-0E+1000"),
            ("1E+6144", "10E+6143"),
            ("sNaN42", "sNaN042"),
        ] {
            assert!(parse(a).same_value(&parse(b)));
        }
        for (a, b) in [
            ("0", "-0"),
            ("NaN42", "sNaN42"),
            ("NaN42", "-NaN42"),
            ("NaN42", "NaN43"),
            ("1.23456", "1.2346"),
            ("1e-6176", "0"),
        ] {
            assert!(!parse(a).same_value(&parse(b)));
        }
        assert_eq!(parse("-1.2300").scaled_i128(-2), Some(-123));
        assert_eq!(parse("1.234").scaled_i128(-2), None);
        assert_eq!(parse("Infinity").scaled_i128(0), None);
        assert_eq!(
            parse("170141183460469231731687303715884105727").scaled_i128(0),
            Some(i128::MAX)
        );
        assert_eq!(
            parse("-170141183460469231731687303715884105728").scaled_i128(0),
            Some(i128::MIN)
        );
        assert_eq!(
            parse("170141183460469231731687303715884105728").scaled_i128(0),
            None
        );
        assert_eq!(parse("1e99999999999").scaled_i128(0), None);
        assert_eq!(parse("-0.00").json_number().as_deref(), Some("-0e-2"));
        for invalid in [
            "",
            " 1",
            "NaN1]",
            "Infinityx",
            "1,2",
            "1.2.3",
            "1E",
            "--1",
            ".1",
            "1e999999999999999999999999",
        ] {
            assert!(DecimalParts::parse(invalid).is_none(), "{invalid}");
        }
    }
}
