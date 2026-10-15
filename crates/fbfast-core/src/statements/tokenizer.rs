use crate::error::{Error, ErrorKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Word(String),
    Identifier(String),
    Literal,
    Symbol(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
}

impl Token {
    pub fn word(&self, expected: &str) -> bool {
        matches!(&self.kind, TokenKind::Word(w) if w == expected)
    }

    pub fn identifier(&self, expected: &str) -> bool {
        match &self.kind {
            TokenKind::Word(w) | TokenKind::Identifier(w) => w == expected,
            _ => false,
        }
    }

    pub fn symbol(&self, expected: u8) -> bool {
        self.kind == TokenKind::Symbol(expected)
    }
}

pub(super) fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::UnsupportedStatement, "validate", message)
}

pub fn tokenize(sql: &str) -> Result<Vec<Token>, Error> {
    let bytes = sql.as_bytes();
    if bytes.contains(&0) {
        return Err(invalid("SQL contains NUL"));
    }
    let mut tokens = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        if bytes[pos].is_ascii_whitespace() {
            pos += 1;
            continue;
        }
        if bytes[pos..].starts_with(b"--") {
            while pos < bytes.len() && bytes[pos] != b'\n' && bytes[pos] != b'\r' {
                pos += 1;
            }
            continue;
        }
        if bytes[pos..].starts_with(b"/*") {
            pos = comment_end(bytes, pos)?;
            continue;
        }
        let start = pos;
        let kind = if matches!(bytes[pos], b'q' | b'Q') && bytes.get(pos + 1) == Some(&b'\'') {
            pos = alternative_literal_end(bytes, pos)?;
            TokenKind::Literal
        } else if matches!(bytes[pos], b'\'' | b'"') {
            let quote = bytes[pos];
            pos = quoted_end(bytes, pos, quote)?;
            if quote == b'"' {
                TokenKind::Identifier(sql[start + 1..pos - 1].replace("\"\"", "\""))
            } else {
                TokenKind::Literal
            }
        } else if is_word(bytes[pos]) {
            pos += 1;
            while pos < bytes.len() && is_word(bytes[pos]) {
                pos += 1;
            }
            TokenKind::Word(sql[start..pos].to_ascii_uppercase())
        } else {
            pos += 1;
            TokenKind::Symbol(bytes[start])
        };
        tokens.push(Token {
            kind,
            start,
            end: pos,
        });
    }
    Ok(tokens)
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$') || byte >= 128
}

fn quoted_end(bytes: &[u8], start: usize, quote: u8) -> Result<usize, Error> {
    let mut pos = start + 1;
    while pos < bytes.len() {
        if bytes[pos] == quote {
            if bytes.get(pos + 1) == Some(&quote) {
                pos += 2;
            } else {
                return Ok(pos + 1);
            }
        } else {
            pos += 1;
        }
    }
    Err(invalid("Unterminated SQL string or identifier"))
}

fn alternative_literal_end(bytes: &[u8], start: usize) -> Result<usize, Error> {
    let opener = *bytes
        .get(start + 2)
        .ok_or_else(|| invalid("Unterminated alternative string"))?;
    if !opener.is_ascii() || opener.is_ascii_whitespace() || opener == b'\'' {
        return Err(invalid("Unsupported alternative string delimiter"));
    }
    let closer = match opener {
        b'[' => b']',
        b'(' => b')',
        b'{' => b'}',
        b'<' => b'>',
        c => c,
    };
    let mut pos = start + 3;
    while pos + 1 < bytes.len() {
        if bytes[pos] == closer && bytes[pos + 1] == b'\'' {
            return Ok(pos + 2);
        }
        pos += 1;
    }
    Err(invalid("Unterminated alternative string"))
}

fn comment_end(bytes: &[u8], start: usize) -> Result<usize, Error> {
    let mut depth = 1usize;
    let mut pos = start + 2;
    while pos + 1 < bytes.len() {
        if bytes[pos..].starts_with(b"/*") {
            depth += 1;
            pos += 2;
        } else if bytes[pos..].starts_with(b"*/") {
            depth -= 1;
            pos += 2;
            if depth == 0 {
                return Ok(pos);
            }
        } else {
            pos += 1;
        }
    }
    Err(invalid("Unterminated SQL comment"))
}
