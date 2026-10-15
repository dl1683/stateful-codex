use super::*;
use pretty_assertions::assert_eq;
use sqlx::Connection;
use sqlx::migrate::Migrate;

#[tokio::test]
async fn both_ledgers_preserve_complete_typed_metadata() {
    for ledger in [Ledger::Canonical, Ledger::Stateful] {
        let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
        assert_eq!(read(&mut connection, ledger).await.unwrap(), None);
        connection
            .ensure_migrations_table(ledger.name())
            .await
            .unwrap();
        assert_eq!(
            read(&mut connection, ledger).await.unwrap(),
            Some(Vec::new())
        );
        let expected = AppliedRecord {
            version: 1,
            description: "original description λ\n".to_owned(),
            installed_on: "2000-01-01 00:00:00".to_owned(),
            success: 1,
            checksum: vec![0, 13, 10, 255],
            execution_time: -1,
        };
        let name = ledger.name();
        sqlx::query(AssertSqlSafe(format!(
            "INSERT INTO {name} VALUES (?, ?, ?, ?, ?, ?)"
        )))
        .bind(expected.version)
        .bind(&expected.description)
        .bind(&expected.installed_on)
        .bind(expected.success)
        .bind(&expected.checksum)
        .bind(expected.execution_time)
        .execute(&mut connection)
        .await
        .unwrap();
        assert_eq!(
            read(&mut connection, ledger).await.unwrap(),
            Some(vec![expected])
        );
        connection.close().await.unwrap();
    }
}

#[tokio::test]
async fn private_ledger_refuses_coercion_failed_rows_and_case_variant_triggers() {
    for mutation in [
        "UPDATE _stateful_migrations SET version = NULL",
        "UPDATE _stateful_migrations SET success = 2",
        "UPDATE _stateful_migrations SET installed_on = 123",
        "UPDATE _stateful_migrations SET checksum = 'not a blob'",
    ] {
        let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
        connection
            .ensure_migrations_table(Ledger::Stateful.name())
            .await
            .unwrap();
        sqlx::query("INSERT INTO _stateful_migrations VALUES (1, 'source', 'fixed', 1, X'00', -1)")
            .execute(&mut connection)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(mutation))
            .execute(&mut connection)
            .await
            .unwrap();
        assert!(
            read(&mut connection, Ledger::Stateful).await.is_err(),
            "accepted {mutation}"
        );
        connection.close().await.unwrap();
    }
    let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    connection
        .ensure_migrations_table(Ledger::Stateful.name())
        .await
        .unwrap();
    sqlx::query("ALTER TABLE _stateful_migrations RENAME TO ledger_bridge; ALTER TABLE ledger_bridge RENAME TO _STATEFUL_MIGRATIONS; CREATE TRIGGER arbitrary_name AFTER UPDATE ON _STATEFUL_MIGRATIONS BEGIN SELECT 1; END")
        .execute(&mut connection).await.unwrap();
    assert!(
        read(&mut connection, Ledger::Stateful)
            .await
            .unwrap_err()
            .to_string()
            .contains("trigger")
    );
    connection.close().await.unwrap();
}
