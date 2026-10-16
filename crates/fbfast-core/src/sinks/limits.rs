use crate::error::{Error, ErrorKind};

pub(crate) fn add_materialized_bytes(
    current: usize,
    added: usize,
    limit: Option<u64>,
) -> Result<usize, Error> {
    let observed = current.checked_add(added).ok_or_else(counter_overflow)?;
    check_materialized_bytes(observed, limit)?;
    Ok(observed)
}

pub(crate) fn check_materialized_bytes(observed: usize, limit: Option<u64>) -> Result<(), Error> {
    let observed = u64::try_from(observed).map_err(|_| counter_overflow())?;
    if let Some(limit) = limit.filter(|limit| observed > *limit) {
        return Err(Error::new(
            ErrorKind::ResultLimit,
            "materialize",
            format!(
                "Materialized result exceeds max_result_bytes (limit={limit}, observed={observed})"
            ),
        ));
    }
    Ok(())
}

fn counter_overflow() -> Error {
    Error::new(
        ErrorKind::ResultLimit,
        "materialize",
        "Materialized result byte counter overflow",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_limit_passes_and_first_byte_above_fails() {
        assert_eq!(add_materialized_bytes(10, 5, Some(15)).unwrap(), 15);
        let error = add_materialized_bytes(10, 6, Some(15)).unwrap_err();
        assert_eq!(error.kind, ErrorKind::ResultLimit);
        assert!(error.message.contains("limit=15"));
        assert!(error.message.contains("observed=16"));
        assert_eq!(
            add_materialized_bytes(usize::MAX, 1, None)
                .unwrap_err()
                .kind,
            ErrorKind::ResultLimit
        );
    }
}
