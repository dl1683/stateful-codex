use super::super::state_catalog;
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
async fn actual_trusted_migrations_expose_exact_owned_declarations() {
    let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    for version in 1..=60 {
        sqlx::raw_sql(state_catalog::canonical(version).unwrap().sql)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    for version in [59, 60] {
        sqlx::raw_sql(state_catalog::fork(version).unwrap().sql)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    for (table, column, declaration) in [
        ("backfill_state", "owner_token", "owner_token TEXT"),
        (
            "threads",
            "project_binding_generation",
            "project_binding_generation INTEGER NOT NULL DEFAULT 1",
        ),
    ] {
        let sql: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?")
                .bind(table)
                .fetch_one(&mut connection)
                .await
                .unwrap();
        let parts = split(&sql, table, &limits()).unwrap();
        let expected = projection_tokens::scan(declaration, &limits()).unwrap();
        assert_eq!(
            (parts.storage, parts.column(column).unwrap()),
            (RowStorage::Rowid, Some(expected.as_slice()))
        );
    }
    connection.close().await.unwrap();
}

#[tokio::test]
async fn unrelated_entries_nested_commas_and_without_rowid_remain_separate() {
    let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    sqlx::raw_sql("CREATE TABLE [THREADS] (id TEXT PRIMARY KEY, extra TEXT DEFAULT (printf('a,b', 7)), \"project_binding_generation\" INTEGER NOT NULL DEFAULT 1, 顧客 TEXT, generated TEXT AS (substr(extra, 1, 2)) VIRTUAL, CHECK (length(extra) > 0)) WITHOUT ROWID")
        .execute(&mut connection).await.unwrap();
    let sql: String = sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name = 'THREADS'")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    let parts = split(&sql, "threads", &limits()).unwrap();
    let actual: Vec<_> = parts.entries().map(<[Token<'_>]>::to_vec).collect();
    let expected: Vec<_> = [
        "id TEXT PRIMARY KEY",
        "extra TEXT DEFAULT (printf('a,b', 7))",
        "\"project_binding_generation\" INTEGER NOT NULL DEFAULT 1",
        "顧客 TEXT",
        "generated TEXT AS (substr(extra, 1, 2)) VIRTUAL",
        "CHECK (length(extra) > 0)",
    ]
    .into_iter()
    .map(|entry| projection_tokens::scan(entry, &limits()).unwrap())
    .collect();
    assert_eq!(
        (parts.storage, actual),
        (RowStorage::WithoutRowid, expected)
    );
    assert_eq!(
        parts.column("PROJECT_BINDING_GENERATION").unwrap().unwrap()[0]
            .identifier()
            .unwrap(),
        "project_binding_generation"
    );
    connection.close().await.unwrap();
}

#[tokio::test]
async fn name_positions_support_quotes_but_literals_and_constraint_names_are_not_columns() {
    for (sql, expected) in [
        (
            "CREATE TABLE 'backfill_state' ('owner_token' TEXT, other TEXT DEFAULT 'owner_token')",
            Some(vec!["'owner_token'", "TEXT"]),
        ),
        (
            "CREATE TABLE backfill_state (other TEXT DEFAULT 'owner_token', CONSTRAINT owner_token CHECK (other IS NULL))",
            None,
        ),
        (
            "CREATE TABLE `backfill_state` (other TEXT, [OWNER_TOKEN] TEXT)",
            Some(vec!["[OWNER_TOKEN]", "TEXT"]),
        ),
    ] {
        let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
        sqlx::raw_sql(sql).execute(&mut connection).await.unwrap();
        let stored: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE type = 'table'")
                .fetch_one(&mut connection)
                .await
                .unwrap();
        let parts = split(&stored, "backfill_state", &limits()).unwrap();
        let actual = parts
            .column("owner_token")
            .unwrap()
            .map(|tokens| tokens.iter().map(|token| token.raw).collect::<Vec<_>>());
        assert_eq!(actual, expected);
        connection.close().await.unwrap();
    }
}

#[test]
fn malformed_ambiguous_and_unproved_table_shapes_refuse() {
    for sql in [
        "CREATE VIEW threads AS SELECT 1",
        "CREATE VIRTUAL TABLE threads USING fts5(id)",
        "CREATE TABLE other(id)",
        "CREATE TABLE threads()",
        "CREATE TABLE threads(id,)",
        "CREATE TABLE threads(,id)",
        "CREATE TABLE threads(id) STRICT",
        "CREATE TABLE threads(id) WITHOUT ROWID, STRICT",
        "CREATE TABLE threads(id) unknown",
        "CREATE TABLE threads(id); CREATE TABLE other(id)",
        "CREATE TABLE IF EXISTS threads(id)",
    ] {
        assert!(split(sql, "threads", &limits()).is_err(), "accepted {sql}");
    }
    let duplicate = split(
        "CREATE TABLE threads(owner_token TEXT, [OWNER_TOKEN] TEXT)",
        "threads",
        &limits(),
    )
    .unwrap();
    assert!(duplicate.column("owner_token").is_err());
    let optional = split(
        "CREATE TABLE IF NOT EXISTS threads(id)",
        "threads",
        &limits(),
    )
    .unwrap();
    assert_eq!(optional.storage, RowStorage::Rowid);
    let sql = "CREATE TABLE threads(owner_token TEXT)";
    assert!(
        split(
            sql,
            "threads",
            &Limits {
                bytes: sql.len() - 1,
                tokens: 8192,
                depth: 64
            }
        )
        .is_err()
    );
}
