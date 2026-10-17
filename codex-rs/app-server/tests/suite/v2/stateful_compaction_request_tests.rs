//! First ordinary requests after remote manual and automatic post-turn checkpoints.

use super::*;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::RootBlackboardQuery;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_ordinary_requests_after_remote_compaction_carry_bounded_current_root() -> Result<()> {
    let server = responses::start_mock_server().await;
    let reply = |id: &str, total_tokens| {
        responses::sse(vec![
            responses::ev_assistant_message(&format!("{id}-message"), "Done"),
            responses::ev_completed_with_tokens(id, total_tokens),
        ])
    };
    let compact = |id: &str| {
        responses::sse(vec![
            json!({
                "type": "response.output_item.done",
                "item": {"type": "compaction", "encrypted_content": id},
            }),
            responses::ev_completed(id),
        ])
    };
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            reply("initial", /*total_tokens*/ 120),
            compact("manual-opaque-summary"),
            reply("after-manual", /*total_tokens*/ 7_000),
            compact("post-turn-opaque-summary"),
            reply("after-post-turn", /*total_tokens*/ 120),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        // The provider capability selects remote compaction v2; all traffic stays mocked.
        .with_provider_name("OpenAI")
        .with_model("gpt-5.5")
        .enable_feature(Feature::Sqlite)
        .disable_feature(Feature::TokenBudget)
        .with_root_config(
            "model_context_window = 12000\nmodel_auto_compact_token_limit = 11000\nmodel_post_turn_compact_threshold_percent = 50",
        )
        .with_provider_config("supports_websockets = false\nrequires_openai_auth = false")
        .write(codex_home.path())?;
    let mut app = start_app(codex_home.path()).await?;
    let mut projects = Vec::new();
    for name in ["Selected boundary project", "Foreign boundary project"] {
        let created: ProjectCreateResponse = app
            .request(|request_id| ClientRequest::ProjectCreate {
                request_id,
                params: ProjectCreateParams {
                    name: name.to_string(),
                    roots: Vec::new(),
                    metadata: Some(BTreeMap::new()),
                    idempotency_key: name.to_string(),
                },
            })
            .await?;
        seed_root_blackboard(codex_home.path(), &created.project.id).await?;
        projects.push(created.project.id);
    }
    let project_id = &projects[0];
    let foreign_id = &projects[1];
    const FOREIGN_FACT: &str = "Foreign project knowledge must stay outside this root.";
    promote_root_fact(codex_home.path(), foreign_id, "foreign-only", FOREIGN_FACT).await?;
    // Whole long entries force real omissions; lexical IDs keep the decisive facts first.
    let mut bulky_facts = Vec::new();
    for index in 0..10 {
        let content = format!("Bounded root fact {index}: {}", "x".repeat(1_750));
        promote_root_fact(
            codex_home.path(),
            project_id,
            &format!("z-bounded-{index}"),
            &content,
        )
        .await?;
        bulky_facts.push(content);
    }
    let thread = app
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.clone()),
            ..Default::default()
        })
        .await?
        .thread;
    run_turn(&mut app, &thread.id).await?;
    let compact_request = app
        .send_thread_compact_start_request(ThreadCompactStartParams {
            thread_id: thread.id.clone(),
        })
        .await?;
    let _: ThreadCompactStartResponse = app.read_response(compact_request).await?;
    let _: TurnCompletedNotification = app.read_notification("turn/completed").await?;
    // Change the root after the checkpoint: a stale retained root or isolated delta cannot pass.
    promote_root_fact(codex_home.path(), project_id, "later-boundary-fact", LATER_FACT).await?;
    let projection = BlackboardStore::open(&SqliteConfig::new_for_testing(codex_home.path().abs()))
        .await?
        .root_projection(RootBlackboardQuery {
            project_id: project_id.clone(),
            max_entries: 256,
        })
        .await?;
    run_turn(&mut app, &thread.id).await?;
    run_turn(&mut app, &thread.id).await?;

    let requests = mock.requests();
    let [_, manual, after_manual, post_turn, after_post_turn] = requests.as_slice() else {
        panic!("expected exactly five requests, got {}", requests.len());
    };
    for (request, trigger, phase, reason) in [
        (manual, "manual", "standalone_turn", "user_requested"),
        (post_turn, "auto", "post_turn", "context_limit"),
    ] {
        let metadata: Value = serde_json::from_str(
            &request.header("x-codex-turn-metadata").expect("turn metadata"),
        )?;
        assert_eq!(metadata["request_kind"], "compaction");
        assert_eq!(
            metadata["compaction"],
            json!({
                "trigger": trigger,
                "reason": reason,
                "implementation": "responses_compaction_v2",
                "phase": phase,
                "strategy": "memento",
            })
        );
    }
    for (request, summary) in [
        (after_manual, "manual-opaque-summary"),
        (after_post_turn, "post-turn-opaque-summary"),
    ] {
        let metadata: Value = serde_json::from_str(
            &request.header("x-codex-turn-metadata").expect("turn metadata"),
        )?;
        assert_eq!(metadata["request_kind"], "turn");
        assert_eq!(metadata["compaction"], Value::Null);
        assert_eq!(
            RootView::of(request, project_id),
            RootView::revised(Some(projection.revision))
        );
        let body = request.body_json();
        // A failed attempt that merely retained the previous root is not a checkpoint.
        assert_eq!(
            body["input"]
                .as_array()
                .expect("inference input")
                .iter()
                .filter(|item| item["type"] == "compaction")
                .map(|item| item["encrypted_content"].clone())
                .collect::<Vec<_>>(),
            vec![json!(summary)]
        );
        let roots = body["input"]
            .as_array()
            .expect("inference input")
            .iter()
            .filter(|item| item["role"] == "developer")
            .filter_map(|item| item["content"].as_array())
            .flatten()
            .filter_map(|content| content["text"].as_str())
            .filter(|text| text.trim_start().starts_with(ROOT.0))
            .collect::<Vec<_>>();
        let [root] = roots.as_slice() else {
            panic!("expected exactly one full root");
        };
        assert!(root.len() <= 9_000, "root exceeded the existing item byte cap");
        let shown = bulky_facts
            .iter()
            .filter(|fact| root.contains(fact.as_str()))
            .count();
        let omitted = bulky_facts.len() - shown;
        assert!(
            shown > 0 && omitted > 0,
            "fixture must exercise partial coverage"
        );
        assert!(root.contains(&format!(
            "Partial coverage: {omitted} applicable root entries omitted"
        )));
        let wire = body.to_string();
        for excluded in [
            foreign_id.as_str(),
            FOREIGN_FACT,
            "conversation_packet",
            "conversation_origin",
        ] {
            assert!(!wire.contains(excluded), "unexpected provider input: {excluded}");
        }
    }
    Ok(())
}
