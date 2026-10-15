use super::*;
use pretty_assertions::assert_eq;
use sqlx::Connection;
use sqlx::SqliteConnection;

fn limits() -> Limits {
    Limits {
        bytes: 65_536,
        tokens: 8192,
        depth: 64,
    }
}

#[tokio::test]
async fn stored_sqlite_unicode_and_numeric_separators_preserve_lexical_identity() {
    let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    sqlx::raw_sql("CREATE TABLE threads (id TEXT PRIMARY KEY, project_binding_generation INTEGER NOT NULL DEFAULT 0, 顧客_名 TEXT DEFAULT 'owner_token', amount REAL DEFAULT 1_000.2_5e+0_2, hex_value INTEGER DEFAULT 0xAB_CD, a b TEXT, \u{feff}extra TEXT, payload BLOB DEFAULT X'CAFE') WITHOUT ROWID")
        .execute(&mut connection).await.unwrap();
    let sql: String = sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name = 'threads'")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    let tokens = scan(&sql, &limits()).unwrap();
    let literals: Vec<_> = tokens
        .iter()
        .filter(|token| token.kind == Kind::Literal)
        .map(|token| token.raw)
        .collect();
    assert_eq!(
        literals,
        ["0", "'owner_token'", "1_000.2_5e+0_2", "0xAB_CD", "X'CAFE'"]
    );
    let names: Vec<String> = tokens
        .iter()
        .filter_map(|token| token.identifier())
        .map(Cow::into_owned)
        .collect();
    assert!(names.contains(&"顧客_名".to_owned()));
    assert!(names.contains(&"a\u{a0}b".to_owned()));
    assert!(names.contains(&"extra".to_owned()));
    assert!(!names.contains(&"owner_token".to_owned()));
    let invalid = "CREATE TABLE rejected(\u{7f}edge TEXT)";
    assert!(
        sqlx::raw_sql(invalid)
            .execute(&mut connection)
            .await
            .is_err()
    );
    assert!(scan(invalid, &limits()).is_err());
    connection.close().await.unwrap();
}

#[test]
fn quotes_literals_comments_and_operators_remain_distinct() {
    let sql = "\"ow\"\"ner\", `ow``ner`, [owner], 'ow''ner', X'ABCD' -- owner\n /* '(' */ value ->> '$.owner' || .1_2e-3";
    let actual = scan(sql, &limits()).unwrap();
    assert_eq!(
        actual,
        vec![
            Token {
                raw: "\"ow\"\"ner\"",
                kind: Kind::Identifier
            },
            Token {
                raw: ",",
                kind: Kind::Symbol
            },
            Token {
                raw: "`ow``ner`",
                kind: Kind::Identifier
            },
            Token {
                raw: ",",
                kind: Kind::Symbol
            },
            Token {
                raw: "[owner]",
                kind: Kind::Identifier
            },
            Token {
                raw: ",",
                kind: Kind::Symbol
            },
            Token {
                raw: "'ow''ner'",
                kind: Kind::Literal
            },
            Token {
                raw: ",",
                kind: Kind::Symbol
            },
            Token {
                raw: "X'ABCD'",
                kind: Kind::Literal
            },
            Token {
                raw: "value",
                kind: Kind::Identifier
            },
            Token {
                raw: "->>",
                kind: Kind::Symbol
            },
            Token {
                raw: "'$.owner'",
                kind: Kind::Literal
            },
            Token {
                raw: "||",
                kind: Kind::Symbol
            },
            Token {
                raw: ".1_2e-3",
                kind: Kind::Literal
            },
        ]
    );
    let names: Vec<_> = actual
        .iter()
        .filter_map(|token| token.identifier())
        .map(Cow::into_owned)
        .collect();
    assert_eq!(names, ["ow\"ner", "ow`ner", "owner", "value"]);
    assert_ne!(
        scan("1_000", &limits()).unwrap(),
        scan("1 _000", &limits()).unwrap()
    );
    assert_ne!(
        scan("1e+2", &limits()).unwrap(),
        scan("1 e + 2", &limits()).unwrap()
    );
}

#[test]
fn malformed_lexemes_and_explicit_allocation_budgets_refuse() {
    for sql in [
        "'open", "\"open", "[open", "/* open", "X'0'", "X'GG'", "1__0", "1_", "0x", "1e+", "1abc",
        "1$x", "(a", "a)", "a\0b", "a\x01b", "a\x7fb",
    ] {
        assert!(scan(sql, &limits()).is_err(), "accepted {sql:?}");
    }
    let sql = "(((a)))";
    assert!(
        scan(
            sql,
            &Limits {
                bytes: sql.len(),
                tokens: 7,
                depth: 3
            }
        )
        .is_ok()
    );
    for bound in [
        Limits {
            bytes: sql.len() - 1,
            tokens: 7,
            depth: 3,
        },
        Limits {
            bytes: sql.len(),
            tokens: 6,
            depth: 3,
        },
        Limits {
            bytes: sql.len(),
            tokens: 7,
            depth: 2,
        },
    ] {
        assert!(scan(sql, &bound).is_err());
    }
}
