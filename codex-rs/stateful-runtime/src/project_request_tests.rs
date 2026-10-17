use std::collections::BTreeMap;

use codex_project_store::PreparedDomains;
use codex_project_store::ProjectStoreLocation;
use codex_project_store::ProjectStoreWriter;
use codex_project_store::StoreIdentity;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::Connection;
use sqlx::Row;
use sqlx::SqlSafeStr;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqliteConnectOptions;

use super::*;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Fixture {
    _directory: tempfile::TempDir,
    location: ProjectStoreLocation,
    identity: StoreIdentity,
    observer: SqliteConnection,
    run: StatefulRunId,
}

impl Fixture {
    #[expect(
        clippy::disallowed_methods,
        reason = "disposable canonical request fixture"
    )]
    async fn new() -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let location = ProjectStoreLocation::select_project(&directory.path().abs())?;
        let identity = codex_project_store::initialize_local_store(&location).await?;
        let mut observer = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(location.database_path().as_path())
                .foreign_keys(/*on*/ true),
        )
        .await?;
        PreparedDomains::prepare(&[crate::project_store_domain()?])
            .await?
            .install(&mut observer)
            .await?;
        sqlx::query("INSERT INTO stateful_runs(id,project_id,goal,mode,status,strategy_revision,revision,created_at_ms,updated_at_ms) VALUES ('run',?,'Goal','autonomous','running',0,1,1,1)")
            .bind(identity.project_uuid.to_string()).execute(&mut observer).await?;
        Ok(Self {
            _directory: directory,
            location,
            identity,
            observer,
            run: StatefulRunId::parse("run")?,
        })
    }

    async fn writer(&self) -> Result<ProjectStoreWriter> {
        Ok(ProjectStoreWriter::open(
            &self.location,
            PreparedDomains::prepare(&[crate::project_store_domain()?]).await?,
            &self.identity,
        )
        .await?)
    }

    async fn seed(&mut self, id: &str, input: &str) -> Result {
        sqlx::query(
            "INSERT INTO stateful_steering VALUES (?,?, 'run',?,'[]','applied',1,NULL,1,1,1)",
        )
        .bind(id)
        .bind(self.identity.project_uuid.to_string())
        .bind(input)
        .execute(&mut self.observer)
        .await?;
        Ok(())
    }

    async fn sql(&mut self, statement: &str) -> Result {
        sqlx::raw_sql(sqlx::AssertSqlSafe(statement).into_sql_str())
            .execute(&mut self.observer)
            .await?;
        Ok(())
    }

    async fn materialize(&self) -> Result<ProjectAcceptanceRequest> {
        let mut writer = self.writer().await?;
        let (guard, request) =
            materialize_project_request(writer.begin_write().await?, &self.run).await?;
        guard.commit().await?;
        writer.close().await?;
        Ok(request)
    }

    async fn refused(&mut self) -> Result {
        let before = self.durable().await?;
        let mut writer = self.writer().await?;
        assert!(
            materialize_project_request(writer.begin_write().await?, &self.run)
                .await
                .is_err()
        );
        assert!(matches!(
            writer.begin_write().await,
            Err(ProjectWriteError::UnfinishedOperation)
        ));
        writer.close().await?;
        assert_eq!(self.durable().await?, before);
        Ok(())
    }

    async fn durable(&mut self) -> Result<BTreeMap<String, Vec<Vec<String>>>> {
        let tables: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
                .fetch_all(&mut self.observer)
                .await?;
        let mut result = BTreeMap::new();
        for table in tables {
            let columns: Vec<String> =
                sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                    .bind(&table)
                    .fetch_all(&mut self.observer)
                    .await?;
            let fields = columns.iter().map(|name| format!("typeof(\"{name}\"),CASE WHEN typeof(\"{name}\") IN ('text','blob') THEN hex(CAST(\"{name}\" AS BLOB)) ELSE \"{name}\" END")).collect::<Vec<_>>().join(",");
            let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT {fields} FROM \"{table}\""
            )))
            .fetch_all(&mut self.observer)
            .await?;
            let mut values = Vec::new();
            for row in rows {
                let mut cells = Vec::new();
                for i in 0..columns.len() {
                    let kind: String = row.try_get(i * 2)?;
                    let value = match kind.as_str() {
                        "integer" => row.try_get::<i64, _>(i * 2 + 1)?.to_string(),
                        "real" => format!("{:016x}", row.try_get::<f64, _>(i * 2 + 1)?.to_bits()),
                        "null" => String::new(),
                        _ => row.try_get::<String, _>(i * 2 + 1)?,
                    };
                    cells.push(format!("{kind}:{value}"));
                }
                values.push(cells);
            }
            values.sort();
            result.insert(table, values);
        }
        Ok(result)
    }
}

#[tokio::test]
async fn append_order_spans_reopen_and_rollback() -> Result {
    let mut f = Fixture::new().await?;
    f.seed("z", "मराठी\0\"x\"").await?;
    let first = f.materialize().await?;
    assert_eq!(
        first,
        ProjectAcceptanceRequest {
            text: "Goal\nमराठी\0\"x\"".into(),
            parts: vec![("z".into(), 5..24)]
        }
    );
    f.sql("UPDATE stateful_acceptance_request_parts SET part=7; UPDATE stateful_runs SET status='cancelled'").await?;
    f.seed("a", "Earlier").await?;
    let before = f.durable().await?;
    let mut writer = f.writer().await?;
    let (guard, second) = materialize_project_request(writer.begin_write().await?, &f.run).await?;
    guard.rollback().await?;
    writer.close().await?;
    assert_eq!(f.durable().await?, before);
    assert_eq!(
        second,
        ProjectAcceptanceRequest {
            text: "Goal\nमराठी\0\"x\"\nEarlier".into(),
            parts: vec![("z".into(), 5..24), ("a".into(), 25..32)]
        }
    );
    assert_eq!(f.materialize().await?, second);
    let stable = f.durable().await?;
    assert_eq!(f.materialize().await?, second);
    assert_eq!(f.durable().await?, stable);
    let ordinals: Vec<(i64, String)> = sqlx::query_as(
        "SELECT part,steering_id FROM stateful_acceptance_request_parts ORDER BY part",
    )
    .fetch_all(&mut f.observer)
    .await?;
    assert_eq!(ordinals, vec![(7, "z".into()), (8, "a".into())]);
    Ok(())
}

#[tokio::test]
async fn complete_metadata_boundary_includes_new_parts_and_repeated_fields() -> Result {
    // Independent fixture arithmetic: run=3+36+goal; candidate=1+3+36+7
    // +1+1+1; applied input=16+1; final part=3+1+1. Total=111+goal.
    for (size, accepted) in [(65_425, true), (65_426, false)] {
        let mut f = Fixture::new().await?;
        f.seed("s", "\0मराठी").await?;
        sqlx::query("UPDATE stateful_runs SET goal=?")
            .bind("g".repeat(size))
            .execute(&mut f.observer)
            .await?;
        if accepted {
            assert_eq!(f.materialize().await?.text.len(), size + 17);
        } else {
            f.refused().await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn malformed_or_foreign_all_status_rows_and_parts_refuse_without_writes() -> Result {
    for mutation in [
        "UPDATE stateful_runs SET project_id='foreign'",
        "UPDATE stateful_runs SET goal=x'FF'",
        "UPDATE stateful_steering SET project_id='foreign',status='submitted'",
        "UPDATE stateful_steering SET project_id='foreign'",
        "UPDATE stateful_steering SET input=x'FF'",
        "UPDATE stateful_steering SET revision=1.5",
        "UPDATE stateful_steering SET created_at_ms=zeroblob(1000000)",
        "UPDATE stateful_steering SET resulting_strategy_revision=1.5",
        "UPDATE stateful_steering SET id=printf('%0130d',1)",
        "UPDATE stateful_acceptance_request_parts SET steering_id='missing'",
        "UPDATE stateful_steering SET status='submitted'",
        "UPDATE stateful_acceptance_request_parts SET part=1.5",
        "UPDATE stateful_acceptance_request_parts SET part=129",
        "UPDATE stateful_steering SET run_id='other'",
    ] {
        let mut f = Fixture::new().await?;
        f.seed("s", "input").await?;
        f.materialize().await?;
        f.sql("INSERT INTO stateful_runs(id,project_id,goal,mode,status,strategy_revision,revision,created_at_ms,updated_at_ms) SELECT 'other',project_id,goal,mode,status,0,1,1,1 FROM stateful_runs WHERE id='run'").await?;
        f.sql(mutation).await?;
        f.refused().await?;
    }
    Ok(())
}

#[tokio::test]
async fn row_and_sparse_ordinal_caps_apply_before_materialization() -> Result {
    let mut f = Fixture::new().await?;
    f.seed("old", "old").await?;
    f.materialize().await?;
    f.sql("UPDATE stateful_acceptance_request_parts SET part=128")
        .await?;
    f.seed("new", "new").await?;
    f.refused().await?;
    f.sql("DELETE FROM stateful_acceptance_request_parts; DELETE FROM stateful_steering")
        .await?;
    for i in 0..129 {
        f.seed(&format!("s{i}"), "input").await?;
    }
    f.sql("UPDATE stateful_steering SET status='acknowledged'")
        .await?;
    f.refused().await?;
    Ok(())
}

#[tokio::test]
async fn metadata_plans_use_run_and_point_indexes_with_unrelated_population() -> Result {
    let mut f = Fixture::new().await?;
    f.sql("INSERT INTO stateful_runs(id,project_id,goal,mode,status,strategy_revision,revision,created_at_ms,updated_at_ms) SELECT 'other',project_id,goal,mode,status,0,1,1,1 FROM stateful_runs WHERE id='run'").await?;
    f.sql("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<2048) INSERT INTO stateful_steering SELECT 'noise'||x,'unrelated','other','payload','[]','submitted',NULL,NULL,1,1,1 FROM n").await?;
    for (query, index) in [
        (STEERING, "stateful_steering_run_status_revision"),
        (
            PARTS,
            "sqlite_autoindex_stateful_acceptance_request_parts_1",
        ),
    ] {
        let plan: Vec<(i64, i64, i64, String)> =
            sqlx::query_as(sqlx::AssertSqlSafe(format!("EXPLAIN QUERY PLAN {query}")))
                .bind("missing")
                .bind(f.identity.project_uuid.to_string())
                .bind("missing")
                .fetch_all(&mut f.observer)
                .await?;
        assert!(
            plan.iter()
                .any(|row| row.3.contains(index) && row.3.contains("run_id=?")),
            "{plan:?}"
        );
        assert!(!plan.iter().any(|row| row.3.contains("SCAN ")), "{plan:?}");
    }
    let plan: Vec<(i64, i64, i64, String)> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!("EXPLAIN QUERY PLAN {RUN}")))
            .bind(f.identity.project_uuid.to_string())
            .bind("run")
            .fetch_all(&mut f.observer)
            .await?;
    assert!(plan.iter().any(|row| row.3.contains("SEARCH stateful_runs USING INDEX") && row.3.contains("id=?")), "{plan:?}");
    assert_eq!(
        f.materialize().await?,
        ProjectAcceptanceRequest {
            text: "Goal".into(),
            parts: vec![]
        }
    );
    Ok(())
}
