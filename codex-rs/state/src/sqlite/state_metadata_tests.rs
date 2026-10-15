use super::*;
use pretty_assertions::assert_eq;
use sqlx::AssertSqlSafe;
use sqlx::Connection;
use sqlx::migrate::Migrate;

async fn snapshot(
    connection: &mut SqliteConnection,
) -> Vec<(String, String, String, i64, Option<String>)> {
    sqlx::query_as(
        "SELECT type, name, tbl_name, rootpage, sql FROM sqlite_schema ORDER BY type, name",
    )
    .fetch_all(connection)
    .await
    .unwrap()
}

#[tokio::test]
async fn sqlx_ledgers_and_renamed_case_variant_tables_are_qualified() {
    for ledger in [Ledger::Canonical, Ledger::Stateful] {
        let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
        let metadata = Metadata::Ledger(ledger);
        assert!(!validate(&mut connection, metadata).await.unwrap());
        connection
            .ensure_migrations_table(ledger.name())
            .await
            .unwrap();
        assert!(validate(&mut connection, metadata).await.unwrap());
        let name = ledger.name();
        let upper = name.to_ascii_uppercase();
        sqlx::raw_sql(AssertSqlSafe(format!(
            "ALTER TABLE {name} RENAME TO bridge; ALTER TABLE bridge RENAME TO {upper}"
        )))
        .execute(&mut connection)
        .await
        .unwrap();
        assert!(validate(&mut connection, metadata).await.unwrap());
        connection.close().await.unwrap();
    }
}

#[tokio::test]
async fn metadata_grammar_refuses_invisible_constraints_without_mutation() {
    for metadata in [
        Metadata::Ledger(Ledger::Canonical),
        Metadata::Ledger(Ledger::Stateful),
        Metadata::Archive,
    ] {
        let definition = metadata.definition();
        for altered in [
            definition.replacen("BIGINT PRIMARY KEY", "BIGINT PRIMARY KEY DESC", 1),
            definition.replacen("TEXT NOT NULL", "TEXT NOT NULL COLLATE NOCASE", 1),
            definition.replacen(
                "TEXT NOT NULL",
                "TEXT NOT NULL CHECK (length(description) < 4)",
                1,
            ),
            definition.replacen("TEXT NOT NULL", "TEXT NOT NULL UNIQUE", 1),
            definition.replacen(
                "BLOB NOT NULL",
                "BLOB NOT NULL REFERENCES proof(checksum)",
                1,
            ),
            format!("{definition} WITHOUT ROWID"),
        ] {
            let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
            sqlx::raw_sql(AssertSqlSafe(altered.clone()))
                .execute(&mut connection)
                .await
                .unwrap();
            let before = snapshot(&mut connection).await;
            assert!(
                validate(&mut connection, metadata).await.is_err(),
                "accepted {altered}"
            );
            assert_eq!(snapshot(&mut connection).await, before);
            connection.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn unexpected_indexes_triggers_and_reserved_name_objects_refuse() {
    for metadata in [
        Metadata::Ledger(Ledger::Canonical),
        Metadata::Ledger(Ledger::Stateful),
        Metadata::Archive,
    ] {
        let name = metadata.name();
        for extra in [
            format!("CREATE INDEX extra ON {name}(description)"),
            format!("CREATE UNIQUE INDEX extra ON {name}(description)"),
            format!(
                "CREATE TRIGGER extra AFTER INSERT ON {} BEGIN SELECT 1; END",
                name.to_ascii_uppercase()
            ),
            format!(
                "CREATE TABLE other(id); CREATE TRIGGER {name} AFTER INSERT ON other BEGIN SELECT 1; END"
            ),
        ] {
            let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
            sqlx::raw_sql(AssertSqlSafe(metadata.definition()))
                .execute(&mut connection)
                .await
                .unwrap();
            sqlx::raw_sql(AssertSqlSafe(extra.clone()))
                .execute(&mut connection)
                .await
                .unwrap();
            let before = snapshot(&mut connection).await;
            assert!(
                validate(&mut connection, metadata).await.is_err(),
                "accepted {extra}"
            );
            assert_eq!(snapshot(&mut connection).await, before);
            connection.close().await.unwrap();
        }
        for replacement in [
            format!("CREATE VIEW {name} AS SELECT 1"),
            format!("CREATE TABLE other(id); CREATE INDEX {name} ON other(id)"),
        ] {
            let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
            sqlx::raw_sql(AssertSqlSafe(replacement))
                .execute(&mut connection)
                .await
                .unwrap();
            let before = snapshot(&mut connection).await;
            assert!(validate(&mut connection, metadata).await.is_err());
            assert_eq!(snapshot(&mut connection).await, before);
            connection.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn ledger_read_rejects_extra_constraints_before_decoding_rows() {
    for ledger in [Ledger::Canonical, Ledger::Stateful] {
        let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
        let definition = Metadata::Ledger(ledger).definition().replacen(
            "BOOLEAN NOT NULL",
            "BOOLEAN NOT NULL CHECK (success = 1)",
            1,
        );
        sqlx::raw_sql(AssertSqlSafe(definition))
            .execute(&mut connection)
            .await
            .unwrap();
        let name = ledger.name();
        sqlx::query(AssertSqlSafe(format!(
            "INSERT INTO {name} VALUES (1, 'original', 'fixed', 1, X'0011', -1)"
        )))
        .execute(&mut connection)
        .await
        .unwrap();
        let before = snapshot(&mut connection).await;
        assert!(
            super::super::state_ledger::read(&mut connection, ledger)
                .await
                .is_err()
        );
        assert_eq!(snapshot(&mut connection).await, before);
        let record: (i64, String, String, i64, Vec<u8>, i64) =
            sqlx::query_as(AssertSqlSafe(format!("SELECT * FROM {name}")))
                .fetch_one(&mut connection)
                .await
                .unwrap();
        assert_eq!(
            record,
            (
                1,
                "original".to_owned(),
                "fixed".to_owned(),
                1,
                vec![0, 17],
                -1
            )
        );
        connection.close().await.unwrap();
    }
}
