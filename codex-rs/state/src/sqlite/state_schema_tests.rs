use super::tokens;
use pretty_assertions::assert_eq;

#[test]
fn schema_tokens_ignore_only_unquoted_sqlite_whitespace_and_comments() {
    let sql = "CREATE TABLE sample (value TEXT DEFAULT 'two  words--/*');";
    let decorated = sql
        .replace("TABLE", "/* comment */ TABLE")
        .replace(" (", "\n( ");
    assert_eq!(tokens(&decorated).unwrap(), tokens(sql).unwrap());
    assert_ne!(
        tokens(&sql.replace("two  words", "two words")).unwrap(),
        tokens(sql).unwrap()
    );
    assert!(tokens("CREATE TABLE sample (owner_token\u{a0} TEXT)").is_err());
    assert!(tokens("CREATE\u{b}TABLE sample (value TEXT)").is_err());
}

#[test]
fn schema_tokens_preserve_escaped_quotes_and_delimited_identifiers() {
    let sql = "CREATE TABLE \"a\"\"b\" ([c d] TEXT DEFAULT 'can''t  --x', `e``f` TEXT);";
    assert_eq!(
        tokens(&sql.replace(" TEXT ", "\nTEXT\n")).unwrap(),
        tokens(sql).unwrap()
    );
    assert_ne!(
        tokens(&sql.replace("[c d]", "[c  d]")).unwrap(),
        tokens(sql).unwrap()
    );
    assert_ne!(
        tokens(&sql.replace("t  --x", "t --x")).unwrap(),
        tokens(sql).unwrap()
    );
    for input in [
        "'unterminated",
        "\"unterminated",
        "[unterminated",
        "`unterminated",
        "/* unterminated",
        "--nul\0\nSELECT 1",
        "/*\0*/ SELECT 1",
    ] {
        assert!(tokens(input).is_err(), "accepted {input}");
    }
}

#[test]
fn numeric_blob_and_compound_operator_boundaries_are_preserved() {
    assert_eq!(
        tokens("a>=1.2e-3 AND b<>X'00ff'").unwrap(),
        tokens("a >= 1.2e-3 AND b <> X'00ff'").unwrap()
    );
    assert_ne!(tokens("a>=1").unwrap(), tokens("a > =1").unwrap());
    assert_ne!(tokens("a->>'x'").unwrap(), tokens("a-> >'x'").unwrap());
    assert_ne!(tokens("X'00ff'").unwrap(), tokens("X '00ff'").unwrap());
    for input in ["X'0'", "X'gg'", "1e+", "0x"] {
        assert!(tokens(input).is_err());
    }
}
