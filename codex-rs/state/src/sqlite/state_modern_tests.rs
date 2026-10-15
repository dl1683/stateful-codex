use super::super::transfer;
use super::*;
use pretty_assertions::assert_eq;
use sqlx::AssertSqlSafe;
use sqlx::SqlitePool;
use sqlx::migrate::Migrator;
use std::collections::BTreeMap;

async fn fixture(canonical: i64, private: i64) -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    Migrator::with_migrations(
        (1..=canonical)
            .map(|version| state_catalog::canonical(version).unwrap())
            .collect(),
    )
    .run(&pool)
    .await
    .unwrap();
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let base = Migrator::with_migrations(
        transfer::FORK_MIGRATOR
            .iter()
            .filter(|migration| migration.version <= private)
            .cloned()
            .collect(),
    );
    transfer::run(&mut transaction, &base, Ledger::Stateful)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    pool
}

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    schema: Vec<(String, String, String, Option<String>)>,
    rows: BTreeMap<String, Vec<String>>,
}

async fn snapshot(connection: &mut SqliteConnection) -> Snapshot {
    let schema: Vec<(String, String, String, Option<String>)> =
        sqlx::query_as("SELECT type, name, tbl_name, sql FROM sqlite_schema ORDER BY type, name")
            .fetch_all(&mut *connection)
            .await
            .unwrap();
    let mut rows = BTreeMap::new();
    for (_, name, _, _) in schema
        .iter()
        .filter(|(kind, name, _, _)| kind == "table" && !name.starts_with("sqlite_"))
    {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_xinfo(?) ORDER BY cid")
                .bind(name)
                .fetch_all(&mut *connection)
                .await
                .unwrap();
        let expressions = columns
            .iter()
            .map(|column| {
                let column = column.replace('"', "\"\"");
                format!("typeof(\"{column}\") || ':' || quote(\"{column}\")")
            })
            .collect::<Vec<_>>()
            .join(" || '|' || ");
        let table = name.replace('"', "\"\"");
        rows.insert(
            name.clone(),
            sqlx::query_scalar(AssertSqlSafe(format!(
                "SELECT {expressions} AS snapshot FROM \"{table}\" ORDER BY snapshot"
            )))
            .fetch_all(&mut *connection)
            .await
            .unwrap(),
        );
    }
    Snapshot { schema, rows }
}

#[tokio::test]
async fn first_application_private_prefixes_qualify_without_archive_and_preserve_all_facts() {
    for (canonical, private) in [(0, 0), (55, 0), (55, 1), (58, 2), (60, 1), (60, 2)] {
        let pool = fixture(canonical, private).await;
        let mut transaction = pool.begin().await.unwrap();
        let expected = PrivateHistory {
            canonical: state_ledger::read(&mut transaction, Ledger::Canonical)
                .await
                .unwrap()
                .unwrap(),
            private: state_ledger::read(&mut transaction, Ledger::Stateful)
                .await
                .unwrap()
                .unwrap(),
            archive: None,
        };
        let before = snapshot(&mut transaction).await;
        assert_eq!(qualify(&mut transaction).await.unwrap(), expected);
        assert_eq!(qualify(&mut transaction).await.unwrap(), expected);
        assert_eq!(snapshot(&mut transaction).await, before);
        transaction.rollback().await.unwrap();
        pool.close().await;
    }
}

#[tokio::test]
async fn inconsistent_private_canonical_and_application_schema_refuse_unchanged() {
    for mutation in [
        "DELETE FROM _stateful_migrations WHERE version = 1",
        "UPDATE _stateful_migrations SET checksum = X'00' WHERE version = 1",
        "UPDATE _stateful_migrations SET version = 3 WHERE version = 2",
        "UPDATE _sqlx_migrations SET checksum = X'00' WHERE version = 59",
        "DELETE FROM _sqlx_migrations WHERE version = 57",
        "DELETE FROM _stateful_migrations",
        "DELETE FROM _stateful_migrations WHERE version = 2",
        "DROP TRIGGER threads_project_binding_generation",
        "ALTER TABLE backfill_state ADD COLUMN unexplained TEXT",
        "CREATE TRIGGER _STATEFUL_MIGRATIONS AFTER INSERT ON threads BEGIN SELECT 1; END",
        "CREATE TRIGGER _STATEFUL_MIGRATION_ARCHIVE AFTER INSERT ON threads BEGIN SELECT 1; END",
        "CREATE TRIGGER unrelated AFTER UPDATE ON threads BEGIN SELECT 1; END",
        "DROP TABLE _sqlx_migrations",
        "INSERT INTO _sqlx_migrations VALUES (61, 'opaque', 'fixed', 1, X'FF', -1)",
    ] {
        let pool = fixture(/*canonical*/ 60, /*private*/ 2).await;
        sqlx::raw_sql(AssertSqlSafe(mutation))
            .execute(&pool)
            .await
            .unwrap();
        let mut transaction = pool.begin().await.unwrap();
        let before = snapshot(&mut transaction).await;
        assert!(
            qualify(&mut transaction).await.is_err(),
            "accepted {mutation}"
        );
        assert_eq!(snapshot(&mut transaction).await, before);
        transaction.rollback().await.unwrap();
        pool.close().await;
    }
}

#[tokio::test]
async fn duplicate_old_owner_and_private_owner_facts_refuse_as_unsupported_mixed_history() {
    for version in [56, 59] {
        let pool = fixture(/*canonical*/ 60, /*private*/ 2).await;
        let identity = state_catalog::fork(/*version*/ 59).unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
            .bind(identity.checksum.as_ref())
            .bind(version)
            .execute(&pool)
            .await
            .unwrap();
        let mut transaction = pool.begin().await.unwrap();
        let before = snapshot(&mut transaction).await;
        assert!(
            qualify(&mut transaction)
                .await
                .unwrap_err()
                .to_string()
                .contains("mixed")
        );
        assert_eq!(snapshot(&mut transaction).await, before);
        transaction.rollback().await.unwrap();
        pool.close().await;
    }
}
