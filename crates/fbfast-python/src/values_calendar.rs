//! Pure decimal and calendar arithmetic used when building Python values.
/// Exact decimal text with the same exponent the (sign, digits, exponent)
/// tuple form produced: scale < 0 places the point, scale > 0 uses E notation.
pub(super) fn decimal_text(coefficient: i128, scale: i32) -> String {
    let digits = coefficient.unsigned_abs().to_string();
    let sign = if coefficient < 0 { "-" } else { "" };
    match scale {
        0 => format!("{sign}{digits}"),
        positive if positive > 0 => format!("{sign}{digits}E+{positive}"),
        negative => {
            let places = negative.unsigned_abs() as usize;
            if digits.len() > places {
                let (whole, fraction) = digits.split_at(digits.len() - places);
                format!("{sign}{whole}.{fraction}")
            } else {
                format!("{sign}0.{}{digits}", "0".repeat(places - digits.len()))
            }
        }
    }
}

/// Firebird date (days since 1858-11-17) to the proleptic Gregorian calendar
/// (Howard Hinnant's civil_from_days), limited to Python's year 1..=9999.
pub(super) fn civil(days: i32) -> Option<(i32, u8, u8)> {
    // Python ordinal (1 = 0001-01-01) shifted to the algorithm's 0000-03-01 era start.
    let z = i64::from(days) + 678_576 + 305;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (1..=9999)
        .contains(&year)
        .then_some((year as i32, month, day))
}

/// Firebird time ticks (1/10000 s) to hour, minute, second and microsecond.
pub(super) fn clock(ticks: u32) -> Option<(u8, u8, u8, u32)> {
    let seconds = ticks / 10_000;
    (ticks < 864_000_000).then_some((
        (seconds / 3600) as u8,
        (seconds / 60 % 60) as u8,
        (seconds % 60) as u8,
        ticks % 10_000 * 100,
    ))
}

#[cfg(test)]
mod tests {
    use super::{civil, clock, decimal_text};

    #[test]
    fn decimal_text_keeps_the_exponent() {
        assert_eq!(decimal_text(-123_456_789_012, -4), "-12345678.9012");
        assert_eq!(decimal_text(1, -4), "0.0001");
        assert_eq!(decimal_text(-5, -2), "-0.05");
        assert_eq!(decimal_text(0, -2), "0.00");
        assert_eq!(decimal_text(123, 2), "123E+2");
        assert_eq!(decimal_text(42, 0), "42");
        assert_eq!(decimal_text(i128::MIN, 0), i128::MIN.to_string());
    }

    #[test]
    fn calendar_matches_known_dates_and_bounds() {
        assert_eq!(civil(0).unwrap(), (1858, 11, 17));
        assert_eq!(civil(1 - 678_576).unwrap(), (1, 1, 1));
        assert_eq!(civil(3_652_059 - 678_576).unwrap(), (9999, 12, 31));
        assert_eq!(civil(60_000).unwrap(), (2023, 2, 25));
        assert_eq!(civil(51_544).unwrap(), (2000, 1, 1));
        assert_eq!(civil(51_603).unwrap(), (2000, 2, 29));
        assert!(civil(-678_576).is_none());
        assert!(civil(3_652_060 - 678_576).is_none());
        assert_eq!(clock(863_999_999).unwrap(), (23, 59, 59, 999_900));
        assert!(clock(864_000_000).is_none());
    }
}
