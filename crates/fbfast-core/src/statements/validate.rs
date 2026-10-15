use super::{tokenize, tokenizer::invalid, Token};
use crate::error::{Error, ErrorKind};

/// This is the client policy gate. Execution must ALSO check that the prepared
/// statement is isc_info_sql_stmt_select and use a read-only TPB.
pub fn validate_read_query(sql: &str) -> Result<Vec<Token>, Error> {
    let mut tokens = tokenize(sql)?;
    if tokens.last().is_some_and(|token| token.symbol(b';')) {
        tokens.pop();
    }
    if tokens.iter().any(|token| token.symbol(b';')) {
        return Err(invalid("Multiple statements are unsupported"));
    }
    let first = tokens.first().ok_or_else(|| invalid("SQL is empty"))?;
    if !first.word("SELECT") && !first.word("WITH") {
        return Err(invalid(
            "Only SELECT and read-only SELECT CTEs are supported",
        ));
    }
    check_balanced_and_select(&tokens)?;
    for (index, token) in tokens.iter().enumerate() {
        let tail = &tokens[index..];
        if sequence(tail, &["FOR", "UPDATE"]) || sequence(tail, &["WITH", "LOCK"]) {
            return Err(read_only("Locking SELECT is unsupported"));
        }
        if sequence(tail, &["NEXT", "VALUE", "FOR"]) {
            return Err(read_only("Direct sequence mutation is unsupported"));
        }
        if ["GEN_ID", "RDB$SET_CONTEXT"]
            .iter()
            .any(|name| token.identifier(name))
            && tail.get(1).is_some_and(|token| token.symbol(b'('))
        {
            return Err(read_only(
                "Sequence/context mutation functions are unsupported",
            ));
        }
    }
    Ok(tokens)
}

fn check_balanced_and_select(tokens: &[Token]) -> Result<(), Error> {
    let mut depth = 0usize;
    let mut select = false;
    for token in tokens {
        if token.symbol(b'(') {
            depth += 1;
        } else if token.symbol(b')') {
            depth = depth
                .checked_sub(1)
                .ok_or_else(|| invalid("Unbalanced SQL parentheses"))?;
        } else if depth == 0 && token.word("SELECT") {
            select = true;
        } else if depth == 0
            && [
                "INSERT", "UPDATE", "DELETE", "MERGE", "EXECUTE", "ALTER", "DROP", "CREATE",
            ]
            .iter()
            .any(|word| token.word(word))
        {
            return Err(invalid("A CTE must end in a read-only SELECT"));
        }
    }
    if depth != 0 {
        return Err(invalid("Unbalanced SQL parentheses"));
    }
    if !select {
        return Err(invalid("A CTE must end in a read-only SELECT"));
    }
    Ok(())
}

fn sequence(tokens: &[Token], words: &[&str]) -> bool {
    tokens.len() >= words.len()
        && tokens
            .iter()
            .zip(words)
            .all(|(token, word)| token.word(word))
}

fn read_only(message: &str) -> Error {
    Error::new(ErrorKind::ReadOnlyViolation, "validate", message)
}
