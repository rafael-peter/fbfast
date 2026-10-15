//! Lexical policy supplements, and never replaces, the server's prepare classification.
pub(crate) mod cache;
mod tokenizer;
mod validate;

pub use tokenizer::{tokenize, Token, TokenKind};
pub use validate::validate_read_query;
