use super::*;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::RequestId;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;

async fn setup() -> Result<(TempDir, TestAppServer, String, String, wiremock::MockServer)> {
    let responses_server = responses::start_mock_server().await;
    let home = TempDir::new()?;
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
                name: "Repair".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "repair-project".to_string(),
            },
        })
        .await?;
    let thread = start_thread(&mut server, &project.project.id).await?;
    Ok((home, server, project.project.id, thread, responses_server))
}

async fn read(
    server: &mut TestAppServer,
    thread: &str,
    project: &str,
) -> Result<StatefulMemoryReadResponse> {
    server
        .request(|request_id| ClientRequest::StatefulMemoryRead {
            request_id,
            params: StatefulMemoryReadParams {
                thread_id: thread.to_string(),
                expected_project_id: project.to_string(),
                cursor: None,
                limit: None,
                background_section: true,
            },
        })
        .await
}

#[tokio::test]
async fn unpersisted_binding_refuses_without_flushing_stale_metadata() -> Result<()> {
    let (home, mut server, project, durable_thread, _responses_server) = setup().await?;
    let fresh = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let request = server.send_request("statefulMemory/add", Some(json!({"threadId":fresh,"expectedProjectId":project,"kind":"note","content":"Do not materialize a staged binding.","clientActionId":"staged-binding"}))).await?;
    let error = server
        .read_stream_until_error_message(RequestId::Integer(request))
        .await?;
    assert_eq!(error.error, JSONRPCErrorError { code: -32602, message: "project binding changed or is not persisted; refresh the thread before using memory".to_string(), data: None });
    // Another metadata writer establishes a different durable binding while this host
    // still has its original staged patch. Memory must neither flush nor overwrite it.
    let other: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Independent writer".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "independent-binding".to_string(),
            },
        })
        .await?;
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let pool = sqlite.open_read_write_pool(&sqlite.state_db_path()).await?;
    // Clone valid metadata into this fresh ID through an independent database writer.
    // This leaves the original host's pending patch intact and exercises admission alone.
    let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('threads')")
        .fetch_all(&pool)
        .await?;
    let selected = columns
        .iter()
        .map(|column| match column.as_str() {
            "id" | "project_id" => "?".to_string(),
            _ => format!("\"{column}\""),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let named = columns
        .iter()
        .map(|column| format!("\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ");
    // Audited identifiers come exclusively from this test database's migrated schema;
    // both replacement values and the source ID remain bound parameters.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO threads ({named}) SELECT {selected} FROM threads WHERE id = ?"
    )))
    .bind(&fresh)
    .bind(&other.project.id)
    .bind(&durable_thread)
    .execute(&pool)
    .await?;
    let request = server
        .send_request(
            "statefulMemory/read",
            Some(json!({"threadId":fresh,"expectedProjectId":project})),
        )
        .await?;
    let error = server
        .read_stream_until_error_message(RequestId::Integer(request))
        .await?;
    assert_eq!(error.error, JSONRPCErrorError { code: -32602, message: "project binding changed or is not persisted; refresh the thread before using memory".to_string(), data: None });
    let binding: String = sqlx::query_scalar("SELECT project_id FROM threads WHERE id = ?")
        .bind(&fresh)
        .fetch_one(&pool)
        .await?;
    assert_eq!(binding, other.project.id);
    assert_eq!(
        read(&mut server, &durable_thread, &project).await?.data,
        Vec::new()
    );
    Ok(())
}


#[tokio::test]
async fn submitted_project_controls_refuse_after_same_thread_rebind_or_unlink() -> Result<()> {
    let (home, mut server, project, thread, _responses_server) = setup().await?;
    let _submitted_binding = read(&mut server, &thread, &project).await?;
    let other: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Other".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "other-project".to_string(),
            },
        })
        .await?;
    for binding in [other.project.id, String::new()] {
        let update = server
            .send_request(
                "thread/metadata/update",
                Some(json!({"threadId":thread, "projectId":binding})),
            )
            .await?;
        let _: codex_app_server_protocol::ThreadMetadataUpdateResponse =
            server.read_response(update).await?;
        for (method, fields) in [
            (
                "add",
                json!({"kind":"rule", "content":"Never push.", "clientActionId":"submitted-on-original"}),
            ),
            ("forget", json!({"entryId":"absent", "expectedRevision":1})),
            (
                "correct",
                json!({"entryId":"absent", "expectedRevision":1, "content":"Replacement"}),
            ),
            ("scope", json!({"action":"leave"})),
            ("read", json!({})),
        ] {
            let mut params = fields;
            params["threadId"] = json!(thread);
            params["expectedProjectId"] = json!(project);
            let request = server
                .send_request(&format!("statefulMemory/{method}"), Some(params))
                .await?;
            let error = server
                .read_stream_until_error_message(RequestId::Integer(request))
                .await?;
            assert_eq!(error.error, JSONRPCErrorError { code: -32602, message: "project binding changed or is not persisted; refresh the thread before using memory".to_string(), data: None });
        }
    }
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let pool = sqlite
        .open_read_write_pool(&home.path().join("project_intelligence_1.sqlite"))
        .await?;
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM blackboard_entries), (SELECT COUNT(*) FROM memory_changes)",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(counts, (0, 0));
    Ok(())
}

#[tokio::test]
async fn scope_review_retains_words_and_reports_open_other_thread_ended_after_restart() -> Result<()>
{
    use codex_app_server_protocol::StatefulMemoryScopeState;
    let (home, mut server, project, thread, responses_server) = setup().await?;
    responses::mount_sse_once(
        &responses_server,
        responses::sse(vec![responses::ev_completed("done")]),
    )
    .await;
    run_turn(
        &mut server,
        &thread,
        "Some ground rules for this whole investigation:\n- Never push.",
    )
    .await?;
    let open = read(&mut server, &thread, &project).await?;
    let other = start_thread(&mut server, &project).await?;
    let elsewhere = read(&mut server, &other, &project).await?;
    let scopes = server
        .send_request(
            "statefulMemory/scope",
            Some(json!({"threadId":thread,"expectedProjectId":project,"action":"list"})),
        )
        .await?;
    let scopes: codex_app_server_protocol::StatefulMemoryScopeResponse =
        server.read_response(scopes).await?;
    let end = server.send_request("statefulMemory/scope", Some(json!({"threadId":thread,"expectedProjectId":project,"action":"end","scopeId":scopes.scopes[0].scope_id}))).await?;
    let _: codex_app_server_protocol::StatefulMemoryScopeResponse =
        server.read_response(end).await?;
    let ended = read(&mut server, &thread, &project).await?;
    assert!(server.shutdown_gracefully().await?.success());
    server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let restarted = read(&mut server, &thread, &project).await?;
    let mut expected_other = open.clone();
    for item in &mut expected_other.data {
        item.scope_state = Some(StatefulMemoryScopeState::NotBoundHere);
    }
    let mut expected_ended = open.clone();
    for item in &mut expected_ended.data {
        item.scope_state = Some(StatefulMemoryScopeState::Ended);
    }
    assert_eq!(
        (open.data[0].scope_state, elsewhere, ended, restarted),
        (
            Some(StatefulMemoryScopeState::Open),
            expected_other,
            expected_ended.clone(),
            expected_ended
        )
    );
    Ok(())
}

#[tokio::test]
async fn shortened_model_entry_has_revision_pinned_exact_text_recovery() -> Result<()> {
    let (_home, mut server, project, thread, responses_server) = setup().await?;
    let words = format!(
        "Decision qualification: {} final reason.",
        "αβ ".repeat(600)
    );
    let log = responses::mount_sse_sequence(&responses_server, vec![
        responses::sse(vec![responses::ev_function_call("record-long", "blackboard_record_batch", &json!({"records":[{
            "idempotencyKey":"long-note", "kind":"note", "content":words, "confidenceBasisPoints":10000,
            "verification":"unverified", "importance":"normal", "rootPromotion":"notPromoted"
        }]}).to_string()), responses::ev_completed("recorded")]),
        responses::sse(vec![responses::ev_completed("done")]),
    ]).await;
    run_turn(
        &mut server,
        &thread,
        "Keep this reported note without applying it.",
    )
    .await?;
    let page = read(&mut server, &thread, &project).await?;
    assert_eq!(page.data.len(), 1);
    let item = &page.data[0];
    assert!(item.content_truncated);
    assert!(words.starts_with(&item.content));
    assert!(item.content.len() <= 2000);
    let exact_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "read-exact",
                    "blackboard_query",
                    &json!({"entryId":item.entry_id,"expectedEntryRevision":item.revision})
                        .to_string(),
                ),
                responses::ev_completed("read"),
            ]),
            responses::sse(vec![responses::ev_completed("done")]),
        ],
    )
    .await;
    run_turn(&mut server, &thread, "Read the exact retained words.").await?;
    let exact: serde_json::Value = serde_json::from_str(
        &exact_log.requests()[1]
            .function_call_output_text("read-exact")
            .expect("exact output"),
    )?;
    assert_eq!(
        (
            exact["content"].clone(),
            exact["complete"].clone(),
            exact["revision"].clone()
        ),
        (json!(words), json!(true), json!(item.revision))
    );
    assert_eq!(log.requests().len(), 2);
    Ok(())
}
