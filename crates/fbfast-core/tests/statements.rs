use fbfast_core::statements::{tokenize, validate_read_query};

#[test]
fn valid_read_queries_preserve_quoted_content() {
    for sql in [
        "select 1 from rdb$database; -- trailing",
        "/* outer /* nested */ end */ SELECT ? FROM rdb$database",
        "SELECT 'for update; gen_id(x,1)', \"WITH LOCK\" FROM t",
        "SELECT 'it''s ; DELETE FROM t' FROM rdb$database",
        "SELECT q'[next value for seq; -- '' ]' FROM rdb$database",
        "WITH x AS (SELECT 1 AS id FROM rdb$database) SELECT id FROM x",
        "SELECT * FROM t WHERE id IN (SELECT id FROM other_t)",
        "SELECT \"GEN_ID\" FROM t",
        "SELECT \"áé中文\" FROM \"tabela\"",
    ] {
        assert!(validate_read_query(sql).is_ok(), "{sql}");
    }
}

#[test]
fn rejects_statements_locks_and_direct_effects() {
    for sql in [
        "",
        "-- comment",
        "INSERT INTO t VALUES (1) RETURNING id",
        "DELETE FROM t",
        "ALTER SESSION RESET",
        "SET TRANSACTION READ ONLY",
        "EXECUTE BLOCK AS BEGIN END",
        "EXECUTE PROCEDURE p",
        "SELECT 1 FROM rdb$database; SELECT 2 FROM rdb$database",
        "WITH x AS (SELECT 1 FROM rdb$database) DELETE FROM t",
        "SELECT * FROM t FOR UPDATE",
        "SELECT * FROM t WITH LOCK",
        "SELECT NEXT /* comment */ VALUE FOR seq FROM rdb$database",
        "SELECT GEN_ID(seq, 0) FROM rdb$database",
        "SELECT \"GEN_ID\"(seq, 1) FROM rdb$database",
        "SELECT rdb$set_context('USER_SESSION', 'x', 'y') FROM rdb$database",
        "SELECT 'unterminated",
        "SELECT 1 /* unterminated",
        "SELECT ((1)",
        "SELECT 1\0 FROM rdb$database",
        "SELECT q'[bad' FROM rdb$database",
    ] {
        assert!(validate_read_query(sql).is_err(), "{sql}");
    }
}

#[test]
fn token_spans_are_utf8_boundaries_and_increasing() {
    for sql in [
        "SELECT é漢字 FROM t",
        "'á' \"é\" q'[á]' /*é*/ --á\n SELECT 1",
    ] {
        let tokens = tokenize(sql).unwrap();
        let mut previous = 0;
        for token in tokens {
            assert!(token.start >= previous);
            assert!(sql.get(token.start..token.end).is_some());
            previous = token.end;
        }
    }
}
