use codex_project_store::PreparedDomains;
use pretty_assertions::assert_eq;
use sqlx::Connection;
use sqlx::SqlSafeStr;
use sqlx::SqliteConnection;

use crate::acceptance_request::APPLIED_DISCOVERY_SQL;
use crate::project_completion::FOREIGN_DEPENDENTS_SQL;

#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "isolated real canonical query-plan fixture")]
async fn actual_queries_preserve_complete_order_and_all_status_foreign_detection() -> Result<(), Box<dyn std::error::Error>> {
    let mut connection = SqliteConnection::connect("sqlite::memory:").await?;
    codex_project_store::initialize_connection(&mut connection).await?;
    PreparedDomains::prepare(&[crate::project_store_domain()?]).await?.install(&mut connection).await?;
    for (run,project) in [("target","project"),("unrelated","other-project")] {
        sqlx::query("INSERT INTO stateful_runs(id,project_id,goal,mode,status,strategy_revision,revision,created_at_ms,updated_at_ms) VALUES (?,?,'goal','collaborative','running',0,1,1,1)")
            .bind(run).bind(project).execute(&mut connection).await?;
    }
    let run = crate::StatefulRunId::parse("target")?;
    crate::acceptance_storage::ensure_ledger(&mut connection, &run).await?;
    for index in 0..2048 {
        let id = format!("noise-{index}");
        sqlx::query("INSERT INTO stateful_steering(id,project_id,run_id,input,affected_obligation_ids_json,status,resulting_strategy_revision,reason,revision,created_at_ms,updated_at_ms) VALUES (?,'other-project','unrelated','noise','[]','applied',1,NULL,1,1,1)")
            .bind(id).execute(&mut connection).await?;
    }
    for (id,revision,time) in [("tie-b",1,2),("recorded",0,0),("tie-a",1,2),("earlier",1,1),("later-revision",2,0)] {
        sqlx::query("INSERT INTO stateful_steering(id,project_id,run_id,input,affected_obligation_ids_json,status,resulting_strategy_revision,reason,revision,created_at_ms,updated_at_ms) VALUES (?,'project','target',?,'[]','applied',?,NULL,1,?,1)")
            .bind(id).bind(id).bind(revision).bind(time).execute(&mut connection).await?;
    }
    sqlx::query("INSERT INTO stateful_acceptance_request_parts VALUES ('target',7,'recorded')").execute(&mut connection).await?;
    let expected = vec!["earlier".to_string(),"tie-a".to_string(),"tie-b".to_string(),"later-revision".to_string()];
    assert_eq!(sqlx::query_scalar::<_,String>(APPLIED_DISCOVERY_SQL).bind("target").bind("target").fetch_all(&mut connection).await?,expected);
    let plan: Vec<(i64,i64,i64,String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!("EXPLAIN QUERY PLAN {APPLIED_DISCOVERY_SQL}")).into_sql_str())
        .bind("target").bind("target").fetch_all(&mut connection).await?;
    assert!(plan.iter().any(|row|row.3.contains("SEARCH stateful_steering USING COVERING INDEX stateful_steering_run_status_revision (run_id=? AND status=?)")), "{plan:?}");
    assert!(!plan.iter().any(|row|row.3.contains("USE TEMP B-TREE")), "{plan:?}");
    let plan: Vec<(i64,i64,i64,String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!("EXPLAIN QUERY PLAN {FOREIGN_DEPENDENTS_SQL}")).into_sql_str())
        .bind("target").bind("project").bind("target").bind("project").bind("target").bind("project").fetch_all(&mut connection).await?;
    assert!(plan.iter().any(|row|row.3.contains("SEARCH stateful_steering USING INDEX stateful_steering_run_status_revision (run_id=?)")), "{plan:?}");
    for status in ["submitted","acknowledged","applied","rejected"] {
        sqlx::query("INSERT INTO stateful_steering(id,project_id,run_id,input,affected_obligation_ids_json,status,resulting_strategy_revision,reason,revision,created_at_ms,updated_at_ms) VALUES ('foreign','foreign-project','target','must not be hidden','[]',?,1,'fixture',1,1,1)")
            .bind(status).execute(&mut connection).await?;
        let foreign: bool = sqlx::query_scalar(FOREIGN_DEPENDENTS_SQL)
            .bind("target").bind("project").bind("target").bind("project").bind("target").bind("project").fetch_one(&mut connection).await?;
        assert!(foreign, "{status}");
        let mut discovered = expected.clone();
        if status == "applied" { discovered.insert(/*index*/ 1, "foreign".into()); }
        assert_eq!(sqlx::query_scalar::<_,String>(APPLIED_DISCOVERY_SQL).bind("target").bind("target").fetch_all(&mut connection).await?,discovered);
        sqlx::query("DELETE FROM stateful_steering WHERE id='foreign'").execute(&mut connection).await?;
    }
    let foreign: bool = sqlx::query_scalar(FOREIGN_DEPENDENTS_SQL)
        .bind("target").bind("project").bind("target").bind("project").bind("target").bind("project").fetch_one(&mut connection).await?;
    assert!(!foreign);
    assert_eq!(sqlx::query_scalar::<_,String>(APPLIED_DISCOVERY_SQL).bind("target").bind("target").fetch_all(&mut connection).await?,expected);
    connection.close().await?;
    Ok(())
}
