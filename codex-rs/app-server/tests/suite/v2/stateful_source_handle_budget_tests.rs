use super::source_proposals_tests::source_handles;
use super::*;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::SourceSeal;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

#[tokio::test]
async fn c3r1_public_eight_parts_then_ninth_steering_discloses_unknown_omissions_cold() -> Result<()>
{
    let (release, gate) = tokio::sync::oneshot::channel();
    let (responses_server, _completions) = start_streaming_sse_server(vec![
        vec![
            StreamingSseChunk {
                gate: None,
                body: responses::sse(vec![responses::ev_response_created("first")]),
            },
            StreamingSseChunk {
                gate: Some(gate),
                body: responses::sse(vec![responses::ev_completed("first")]),
            },
        ],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![responses::ev_completed("steered")]),
        }],
    ])
    .await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(responses_server.uri())
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
                name: "Handle coverage".into(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "handle-coverage".into(),
            },
        })
        .await?;
    let thread = start_thread(&mut server, &project.project.id).await?;
    let parts: Vec<String> = (0..8).map(|i| format!("original-{i}")).collect();
    let input: Vec<Value> = parts
        .iter()
        .map(|text| json!({"type":"text", "text":text}))
        .collect();
    let id = server
        .send_request("turn/start", Some(json!({"threadId":thread,"input":input})))
        .await?;
    let started: codex_app_server_protocol::TurnStartResponse = server.read_response(id).await?;
    tokio::time::timeout(
        std::time::Duration::from_secs(/*secs*/ 10),
        responses_server.wait_for_request_count(/*count*/ 1),
    )
    .await?;
    let id = server
        .send_request(
            "turn/steer",
            Some(json!({"threadId":thread,"expectedTurnId":started.turn.id,
        "input":[{"type":"text","text":"ninth-source-independent"}]})),
        )
        .await?;
    let _: codex_app_server_protocol::TurnSteerResponse = server.read_response(id).await?;
    release.send(()).unwrap();
    let _: codex_app_server_protocol::TurnCompletedNotification =
        server.read_notification("turn/completed").await?;
    let requests = responses_server.requests().await;
    assert_eq!(requests.len(), 2);
    let first = source_handles(&serde_json::from_slice::<Value>(&requests[0])?).unwrap();
    let ninth = source_handles(&serde_json::from_slice::<Value>(&requests[1])?).unwrap();
    assert_eq!(first["omittedIsExact"], json!(true));
    assert_eq!(ninth["omittedIsExact"], json!(false));
    assert_eq!(first["handles"], ninth["handles"]);
    let offered = ninth["handles"].as_array().unwrap().len();
    assert_eq!(
        (first["omitted"].as_u64(), ninth["omitted"].as_u64()),
        (Some((8 - offered) as u64), Some((9 - offered) as u64))
    );
    assert!(first.to_string().len() <= 850 && ninth.to_string().len() <= 850);
    assert!(server.shutdown_gracefully().await?.success());
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let pool = sqlite
        .open_read_only_pool(
            &home.path().join("project_intelligence_1.sqlite"),
            /*busy_timeout*/ None,
        )
        .await?;
    let metadata: Vec<String> = sqlx::query_scalar(
        "SELECT metadata FROM capture_sources WHERE project_id = ? ORDER BY observed_sequence",
    )
    .bind(&project.project.id)
    .fetch_all(&pool)
    .await?;
    let seals = metadata
        .iter()
        .map(|json| serde_json::from_str::<SourceSeal>(json))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert_eq!(seals.len(), 9);
    let reopened = BlackboardStore::open(&sqlite).await?;
    for (seal, text) in seals.iter().zip(
        parts
            .iter()
            .map(String::as_str)
            .chain(["ninth-source-independent"]),
    ) {
        assert_eq!(seal.observation.turn_id, started.turn.id);
        assert_eq!(
            reopened
                .read_source_page(
                    &project.project.id,
                    &seal.exact_source_locator,
                    &seal.digest,
                    seal.observation.source_revision,
                    /*offset*/ 0
                )
                .await?
                .exact_text,
            text
        );
    }
    assert!(
        reopened
            .search_source_ranges(
                &project.project.id,
                "ninth-source-independent",
                /*after*/ None
            )
            .await?
            .ranges
            .iter()
            .any(|range| range.exact_text.contains("ninth-source-independent"))
    );
    Ok(())
}
