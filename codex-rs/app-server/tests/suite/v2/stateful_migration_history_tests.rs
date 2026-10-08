use std::borrow::Cow;

use super::*;
use codex_app_server_protocol::RequestId;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;
use sqlx::AssertSqlSafe;
use sqlx::SqlSafeStr;
use sqlx::migrate::Migration;
use sqlx::migrate::MigrationType;
use sqlx::migrate::Migrator;

static PI: Migrator = sqlx::migrate!("../project-intelligence/migrations");
static RT: Migrator = sqlx::migrate!("../stateful-runtime/migrations");

#[tokio::test]
async fn public_unsupported_migration_history_refuses_reads_and_adds_without_empty_success()
-> Result<()> {
    let variants = [
        (
            "project_intelligence_1.sqlite",
            "capture group members",
            include_str!(
                "../../../../project-intelligence/tests/data/migration_history/item4_members_0018.sql"
            ),
        ),
        (
            "project_intelligence_1.sqlite",
            "capture group members",
            include_str!(
                "../../../../project-intelligence/tests/data/migration_history/item4_identity_0018.sql"
            ),
        ),
        (
            "project_intelligence_1.sqlite",
            "qualification jobs",
            include_str!(
                "../../../../project-intelligence/tests/data/migration_history/item5_jobs_0018.sql"
            ),
        ),
        (
            "stateful_runtime_1.sqlite",
            "context windows",
            include_str!(
                "../../../../stateful-runtime/tests/data/migration_history/early_0005.sql"
            ),
        ),
        ("project_intelligence_1.sqlite", "untracked legacy", ""),
        ("stateful_runtime_1.sqlite", "untracked legacy", ""),
    ];
    for (database, description, sql) in variants {
        let home = TempDir::new()?;
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let (base, version) = if database.starts_with("project") {
            (&PI, 18)
        } else {
            (&RT, 5)
        };
        let mut migrations: Vec<_> = base
            .iter()
            .filter(|m| m.version < version)
            .cloned()
            .collect();
        migrations.push(Migration::new(
            version,
            Cow::Borrowed(description),
            MigrationType::Simple,
            AssertSqlSafe(sql).into_sql_str(),
            /*no_tx*/ false,
        ));
        let historical = Migrator {
            migrations: Cow::Owned(migrations),
            ..Migrator::DEFAULT
        };
        let path = home.path().join(database);
        let pool = sqlite.open_read_write_pool(&path).await?;
        let tracked = description != "untracked legacy";
        if tracked {
            sqlite.run_migrations(&pool, &historical).await?;
        } else {
            sqlx::query("CREATE TABLE legacy_words (words TEXT); INSERT INTO legacy_words VALUES ('Keep exact words: α.')").execute(&pool).await?;
        }
        pool.close().await;
        let before = Sha256::digest(std::fs::read(&path)?);
        let checksum: String = historical
            .iter()
            .last()
            .unwrap()
            .checksum
            .iter()
            .take(/*n*/ 12)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let responses_server = responses::start_mock_server().await;
        MockResponsesConfig::new(&responses_server.uri())
            .enable_feature(Feature::Sqlite)
            .write(home.path())?;
        let mut server = TestAppServer::builder()
            .with_codex_home(home.path())
            .build_initialized()
            .await?;
        let project: ProjectCreateResponse = server
            .request(|request_id| ClientRequest::ProjectCreate {
                request_id,
                params: ProjectCreateParams {
                    name: "History".to_string(),
                    roots: Vec::new(),
                    metadata: None,
                    idempotency_key: "history-project".to_string(),
                },
            })
            .await?;
        let thread = start_thread(&mut server, &project.project.id).await?;
        let requests = if version == 18 {
            vec![
                (
                    "statefulMemory/read",
                    json!({"threadId":thread,"expectedProjectId":project.project.id}),
                ),
                (
                    "statefulMemory/add",
                    json!({"threadId":thread,"expectedProjectId":project.project.id,"kind":"note","content":"Do not pretend this was saved.","clientActionId":"unsupported-add"}),
                ),
                ("blackboard/query", json!({"projectId":project.project.id})),
            ]
        } else {
            vec![("statefulRun/read", json!({"runId":"legacy"}))]
        };
        for (method, params) in requests {
            let id = server.send_request(method, Some(params)).await?;
            let error = server
                .read_stream_until_error_message(RequestId::Integer(id))
                .await?
                .error;
            assert_eq!((error.code, error.data), (-32603, None));
            assert!(
                error.message.contains("unsupported migration history"),
                "{}",
                error.message
            );
            let identity = if tracked {
                vec![
                    format!("version={version:04}"),
                    format!("description={description:?}"),
                    format!("checksum={checksum}"),
                ]
            } else {
                vec![
                    "version=unknown".to_string(),
                    "missing or empty migration ledger".to_string(),
                    "checksum=unavailable".to_string(),
                ]
            };
            for expected in identity.into_iter().chain([
                database.to_string(),
                "automatic conversion is unavailable".to_string(),
            ]) {
                assert!(error.message.contains(&expected), "{}", error.message);
            }
            assert_eq!(Sha256::digest(std::fs::read(&path)?), before);
        }
        assert!(server.shutdown_gracefully().await?.success());
        assert_eq!(Sha256::digest(std::fs::read(&path)?), before);
    }
    Ok(())
}
