//! Public qualification of title-free historical memory review.

use super::*;
use codex_app_server_protocol::StatefulMemoryScopeState;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn historical_megabyte_scope_scalars_stay_stored_and_out_of_public_review() -> Result<()> {
    let (home, mut server, project, thread, _responses_server) = setup().await?;
    let added: StatefulMemoryAddResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                thread_id: thread.clone(),
                expected_project_id: project.clone(),
                kind: StatefulMemoryAddKind::Rule,
                content: "Preserve these exact historical words.".to_string(),
                reason: None,
                scope: None,
                client_action_id: "historical-scalar".to_string(),
                background_section: true,
            },
        })
        .await?;
    let pool = SqliteConfig::new_for_testing(home.path().abs())
        .open_read_write_pool(&home.path().join("project_intelligence_1.sqlite"))
        .await?;
    let title = "T".repeat(1024 * 1024);
    let end_condition = "E".repeat(1024 * 1024);
    sqlx::query("INSERT INTO knowledge_scopes (project_id,scope_id,kind,title,state,end_condition,opened_source,ended_source,created_at_ms,updated_at_ms) VALUES (?,'huge-history','investigation',?,'ended',?,'historical-source','historical-end',1,1)")
        .bind(&project).bind(&title).bind(&end_condition).execute(&pool).await?;
    sqlx::query("UPDATE knowledge_context SET scope_id='huge-history' WHERE entry_id=?")
        .bind(&added.item.entry_id)
        .execute(&pool)
        .await?;
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await?
            .is_empty()
    );
    assert!(server.shutdown_gracefully().await?.success());
    server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let wire: serde_json::Value = server
        .request(|request_id| ClientRequest::StatefulMemoryRead {
            request_id,
            params: StatefulMemoryReadParams {
                thread_id: thread.clone(),
                expected_project_id: project.clone(),
                cursor: None,
                limit: Some(1),
                background_section: true,
            },
        })
        .await?;
    let page: StatefulMemoryReadResponse = serde_json::from_value(wire.clone())?;
    let mut expected = added.item;
    expected.scope_state = Some(StatefulMemoryScopeState::Unsupported);
    assert_eq!(page.data, vec![expected]);
    assert!(wire["data"][0].get("scopeTitle").is_none());
    assert!(serde_json::to_vec(&wire)?.len() < 4096);
    let stored: (String, String) = sqlx::query_as("SELECT title,end_condition FROM knowledge_scopes WHERE project_id=? AND scope_id='huge-history'")
        .bind(&project).fetch_one(&pool).await?;
    assert_eq!(stored, (title, end_condition));
    Ok(())
}
