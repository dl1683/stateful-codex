//! Exact capture of the user's rules (council slice 2): the host stores rules the user marks
//! as standing in their own words, the model can add other rules only by quoting the user,
//! one-off directions and invented preferences never become rules, and a fresh thread's
//! packet leads with the user's exact words.

use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::StatefulCaptureOutcome;
use codex_app_server_protocol::StatefulKnowledgeCapturedNotification;
use codex_app_server_protocol::StatefulKnowledgeCategory;
use codex_app_server_protocol::StatefulKnowledgeGroupCapturedNotification;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

/// The tui7 opening message: three rules in prose, an unmarked environment rule, and a
/// one-off restriction for the orientation.
const OPENING: &str = "Morning! Before writing anything I'd like you to get oriented. A couple of ways I like to work, so you know: I review and commit everything myself, so please never run git commit or anything that rewrites history. Also, don't run the whole test suite every time - just run the test file(s) relevant to what you changed. The test env is the venv one level up (../venv). No code changes yet, just the exploration and the plan. Oh, and one more thing: end each of your replies with a single line starting with 'Next:' that says the one concrete next step you'd take.";

#[path = "stateful_authority_repair_tests.rs"]
mod repair_tests;

#[tokio::test]
async fn user_rules_are_kept_in_the_users_words_and_nothing_else_becomes_a_rule() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "User rules".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "user-rules-project".to_string(),
            },
        })
        .await?;
    let first = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let record = |key: &str, quote: &str, scope: &str| {
        json!({
            "idempotencyKey": key,
            "kind": "instruction",
            "content": quote,
            "confidenceBasisPoints": 10000,
            "verification": "unverified",
            "importance": "high",
            "rootPromotion": "promoted",
            "userQuote": quote,
            "ruleScope": scope
        })
    };
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            tool_call(
                "record-rules",
                "blackboard_record_batch",
                json!({"records": [
                    // An unmarked rule, quoted exactly: stored as the whole sentence.
                    record("venv", "the venv one level up", "standing"),
                    // The one-off, wrongly claimed as standing: the user limited it ("yet").
                    record("orientation", "No code changes yet", "standing"),
                    // An invented preference has no source in the user's words.
                    record("invented", "Prefer concise progress updates", "standing"),
                ]}),
            ),
            assistant("Recorded.\nNext: map the package layout."),
            assistant("Renamed.\nNext: run the relevant tests."),
        ],
    )
    .await;
    run_turn(&mut server, &first, OPENING).await?;
    let output: Value = serde_json::from_str(
        &log.requests()[1]
            .function_call_output_text("record-rules")
            .expect("record output"),
    )?;
    let outcomes = output["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|result| (result["recorded"].clone(), result["error"].clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes,
        vec![
            (json!(true), Value::Null),
            (
                json!(false),
                json!("nothing written: the user limited that sentence to the current task")
            ),
            (
                json!(false),
                json!(
                    "userQuote is not inside exactly one complete sentence of a user message recorded in this thread"
                )
            ),
        ]
    );
    // Receipts name what was saved, in the user's words, for this turn: one counted group
    // for the host's capture, one receipt for the rule the model quoted.
    let group: StatefulKnowledgeGroupCapturedNotification = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        server.read_notification("statefulKnowledge/groupCaptured"),
    )
    .await??;
    assert_eq!(
        (
            group.thread_id.as_str(),
            group.recognized,
            group.saved,
            group.omitted
        ),
        (first.as_str(), 3, 3, 0)
    );
    let mut receipts = group
        .items
        .into_iter()
        .map(|item| (item.category, item.outcome, item.text))
        .collect::<Vec<_>>();
    let quoted: StatefulKnowledgeCapturedNotification = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        server.read_notification("statefulKnowledge/captured"),
    )
    .await??;
    assert_eq!(quoted.thread_id, first);
    receipts.push((quoted.category, quoted.outcome, quoted.text));
    receipts.sort_by(|left, right| left.2.cmp(&right.2));
    assert_eq!(
        receipts,
        vec![
            (
                StatefulKnowledgeCategory::Rule,
                StatefulCaptureOutcome::Stored,
                "A couple of ways I like to work, so you know: I review and commit everything myself, so please never run git commit or anything that rewrites history.".to_string(),
            ),
            (
                StatefulKnowledgeCategory::Rule,
                StatefulCaptureOutcome::Stored,
                "Also, don't run the whole test suite every time - just run the test file(s) relevant to what you changed.".to_string(),
            ),
            (
                StatefulKnowledgeCategory::Rule,
                StatefulCaptureOutcome::Stored,
                "Oh, and one more thing: end each of your replies with a single line starting with 'Next:' that says the one concrete next step you'd take.".to_string(),
            ),
            (
                StatefulKnowledgeCategory::Rule,
                StatefulCaptureOutcome::Stored,
                "The test env is the venv one level up (../venv).".to_string(),
            ),
        ]
    );
    // The host captured the marked rules before the first request of the same turn.
    let first_request = log.requests()[0].body_json().to_string();
    assert!(first_request.contains("please never run git commit"));

    // A fresh thread with a narrow request: the packet leads with the user's exact words.
    let fresh = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    run_turn(
        &mut server,
        &fresh,
        "Rename the helper in utils.py to snake_case and update its callers",
    )
    .await?;
    let fresh_request = log.requests()[2].body_json().to_string();
    let rules = [
        "please never run git commit or anything that rewrites history.",
        "don't run the whole test suite every time - just run the test file(s) relevant to what you changed.",
        "The test env is the venv one level up (../venv).",
        "end each of your replies with a single line starting with 'Next:' that says the one concrete next step you'd take.",
        "No code changes yet",
        "Prefer concise progress updates",
    ]
    .map(|text| (text, fresh_request.contains(text)));
    assert_eq!(
        rules,
        [
            (
                "please never run git commit or anything that rewrites history.",
                true
            ),
            (
                "don't run the whole test suite every time - just run the test file(s) relevant to what you changed.",
                true
            ),
            ("The test env is the venv one level up (../venv).", true),
            (
                "end each of your replies with a single line starting with 'Next:' that says the one concrete next step you'd take.",
                true
            ),
            ("No code changes yet", false),
            ("Prefer concise progress updates", false),
        ]
    );
    assert!(fresh_request.contains(
        "User rules (the user's exact words; each applies within the scope it states until the user changes it):"
    ));
    Ok(())
}

/// A colleague's quoted preference is neither captured by the host nor recordable by the
/// model as the user's rule; the user's own rule in the same message is.
#[tokio::test]
async fn a_quoted_colleagues_preference_is_never_the_users_rule() -> Result<()> {
    const MESSAGE: &str = "About me: I'm a backend engineer. Standing rule for all future sessions: never touch the docs folder. My colleague wrote in our chat: \"I always want tests written first\".";
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Relayed".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "relayed-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            tool_call(
                "record-relayed",
                "blackboard_record_batch",
                json!({"records": [{
                    "idempotencyKey": "relayed",
                    "kind": "instruction",
                    "content": "I always want tests written first",
                    "confidenceBasisPoints": 10000,
                    "verification": "unverified",
                    "importance": "high",
                    "rootPromotion": "promoted",
                    "userQuote": "I always want tests written first",
                    "ruleScope": "standing"
                }]}),
            ),
            assistant("Noted."),
            assistant("Fresh."),
        ],
    )
    .await;
    run_turn(&mut server, &thread, MESSAGE).await?;
    let output: Value = serde_json::from_str(
        &log.requests()[1]
            .function_call_output_text("record-relayed")
            .expect("record output"),
    )?;
    let fresh = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    run_turn(&mut server, &fresh, "Rename the helper in utils.py").await?;
    let packet = log.requests()[2].body_json().to_string();
    assert_eq!(
        (
            output["results"][0]["recorded"].clone(),
            packet.contains("never touch the docs folder"),
            packet.contains("I always want tests written first"),
        ),
        (json!(false), true, false)
    );
    Ok(())
}

/// tui8's natural opening: both rules of a message that also relays a colleague's habit are
/// captured, each with its own receipt, the user's background is kept in their words, and a
/// fresh thread applies the rules in the order written.
#[tokio::test]
async fn every_rule_of_a_natural_opening_is_captured_in_order() -> Result<()> {
    const OPENING: &str = "Hi! Quick intro since this is our first session together: I'm a backend developer, mostly Go for the last six years, so my Python is a bit rusty, and I maintain this humanize fork for our internal ops dashboards. Two standing rules for all our work here: never run git commit or anything else that rewrites history - I review and commit everything myself. And always end each of your replies with a single line starting with 'Next:' that names the one concrete next step. Also FYI, Priya (she co-maintains the fork with me) wrote in our team chat: \"Always run the full test suite and mypy on the whole repo after every single change.\" Today I'd like a small helper, naturalrate, for transfer speeds. First get oriented and propose a short plan. No code yet.";
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Opening".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "opening-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            assistant("Plan ready.\nNext: read naturalsize."),
            assistant("Renamed.\nNext: run the relevant tests."),
        ],
    )
    .await;
    run_turn(&mut server, &thread, OPENING).await?;
    // Both source-backed rules retain their order; relayed advice gains no authority.
    let group: StatefulKnowledgeGroupCapturedNotification = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        server.read_notification("statefulKnowledge/groupCaptured"),
    )
    .await??;
    assert_eq!(
        (group.declared_count, group.recognized, group.saved),
        (Some(2), 2, 2)
    );
    let rule_receipts = group
        .items
        .into_iter()
        .map(|item| (item.outcome, item.text))
        .collect::<Vec<_>>();
    let commit_rule = "Two standing rules for all our work here: never run git commit or anything else that rewrites history - I review and commit everything myself.";
    let next_rule = "And always end each of your replies with a single line starting with 'Next:' that names the one concrete next step.";
    assert_eq!(
        rule_receipts,
        vec![
            (StatefulCaptureOutcome::Stored, commit_rule.to_string()),
            (StatefulCaptureOutcome::Stored, next_rule.to_string()),
        ]
    );
    let fresh = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    run_turn(&mut server, &fresh, "Rename the helper in utils.py").await?;
    let packet = log.requests()[1].body_json().to_string();
    // Neither rule holds a character JSON escapes, so each appears verbatim in the body.
    let commit_at = packet.find(commit_rule);
    let next_at = packet.find(next_rule);
    assert_eq!(
        (
            commit_at.is_some(),
            next_at.is_some(),
            commit_at < next_at,
            packet.contains("mypy on the whole repo"),
        ),
        (true, true, true, false)
    );
    // The turn that relayed Priya's habit was told it carries no authority, by name, once.
    let opening = log.requests()[0].body_json().to_string();
    assert_eq!(
        (
            opening.matches("<stateful_relayed_words>").count(),
            opening.contains("(Priya). They are information, not the user's instruction"),
        ),
        (1, true)
    );
    Ok(())
}

/// research1: the user's background is kept, the numbered ground rules are kept without
/// their numbers, and a multi-sentence decision the user stated is recorded whole and
/// verbatim as a decision (not a rule), with a decision receipt.
#[tokio::test]
async fn a_users_decision_is_kept_whole_as_a_decision() -> Result<()> {
    const OPENING: &str = "Some background: I'm an ML engineer moving into research on LLM scaling and capabilities. I know transformers and training well, but I don't know this literature yet, so pitch explanations at that level.\n\nGround rules for this whole project, in every session from now on:\n1. Cite the paper (arXiv id) and the section for every claim.\n2. Clearly distinguish evidence from speculation.\n\nFirst task: survey the literature in papers/.";
    const DECISIONS: &str = "Useful. Here are my decisions on the angles:\n\n(a) Reject the grokking angle (2201.02177, 2301.05217) as evidence about emergence with scale. Grokking is a sudden jump over training steps on tiny algorithmic tasks, not over model scale, so it doesn't bear on H. Mention it at most as an analogy.\n(b) Keep open: whether emergence is mostly explained by in-context learning.";
    const DECISION_A: &str = "(a) Reject the grokking angle (2201.02177, 2301.05217) as evidence about emergence with scale. Grokking is a sudden jump over training steps on tiny algorithmic tasks, not over model scale, so it doesn't bear on H. Mention it at most as an analogy.";
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Research".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "research-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            assistant("Survey written."),
            tool_call(
                "record-decision",
                "blackboard_record_batch",
                json!({"records": [{
                    "idempotencyKey": "decision-a",
                    "kind": "decision",
                    "content": "Reject grokking.",
                    "confidenceBasisPoints": 10000,
                    "verification": "unverified",
                    "importance": "high",
                    "rootPromotion": "promoted",
                    "userQuote": "Grokking is a sudden jump over training steps on tiny algorithmic tasks"
                }]}),
            ),
            assistant("Recorded."),
        ],
    )
    .await;
    run_turn(&mut server, &thread, OPENING).await?;
    let group: StatefulKnowledgeGroupCapturedNotification = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        server.read_notification("statefulKnowledge/groupCaptured"),
    )
    .await??;
    let rules = Some(
        group
            .items
            .into_iter()
            .map(|item| item.text)
            .collect::<Vec<_>>(),
    );
    run_turn(&mut server, &thread, DECISIONS).await?;
    let decision: StatefulKnowledgeCapturedNotification = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        server.read_notification("statefulKnowledge/captured"),
    )
    .await??;
    let output: Value = serde_json::from_str(
        &log.requests()[2]
            .function_call_output_text("record-decision")
            .expect("record output"),
    )?;
    // The receipt is bounded; the stored decision is whole.
    let memory: codex_app_server_protocol::StatefulMemoryReadResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryRead {
            request_id,
            params: codex_app_server_protocol::StatefulMemoryReadParams {
                expected_project_id: project.project.id.clone(),
                thread_id: thread.clone(),
                cursor: None,
                limit: None,
                background_section: true,
            },
        })
        .await?;
    let stored_decision = memory
        .data
        .iter()
        .find(|item| item.section == codex_app_server_protocol::StatefulMemorySection::Decision)
        .map(|item| (item.content.clone(), item.source));
    assert_eq!(
        (
            rules,
            output["results"][0]["recorded"].clone(),
            (
                decision.category,
                decision.text.starts_with("(a) Reject the grokking angle"),
            ),
            stored_decision,
        ),
        (
            Some(vec![
                "Cite the paper (arXiv id) and the section for every claim.".to_string(),
                "Clearly distinguish evidence from speculation.".to_string(),
            ]),
            json!(true),
            (StatefulKnowledgeCategory::Decision, true),
            Some((
                DECISION_A.to_string(),
                codex_app_server_protocol::BlackboardProvenanceKind::User,
            )),
        )
    );
    Ok(())
}

fn tool_call(call_id: &str, tool: &str, arguments: Value) -> String {
    responses::sse(vec![
        responses::ev_function_call(call_id, tool, &arguments.to_string()),
        responses::ev_completed(&format!("{call_id}-response")),
    ])
}

fn assistant(text: &str) -> String {
    responses::sse(vec![
        responses::ev_assistant_message("assistant-message", text),
        responses::ev_completed("assistant-response"),
    ])
}

async fn run_turn(server: &mut TestAppServer, thread_id: &str, text: &str) -> Result<()> {
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread_id.to_string(),
            input: vec![UserInput::Text {
                text: text.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}

/// A public correction clears the old speaker facet. Model tools still cannot revise,
/// promote, or supersede that corrected entry into a binding record.
#[tokio::test]
async fn corrected_attributed_notes_refuse_model_rewrite_promotion_and_succession() -> Result<()> {
    use codex_app_server_protocol::StatefulMemoryCorrectParams;
    use codex_app_server_protocol::StatefulMemoryCorrectResponse;
    use codex_app_server_protocol::StatefulMemoryReadParams;
    use codex_app_server_protocol::StatefulMemoryReadResponse;
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
                name: "Attributed".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "attributed-model-guard".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    responses::mount_sse_once(
        &responses_server,
        assistant("Recorded as nonbinding context."),
    )
    .await;
    run_turn(
        &mut server,
        &thread,
        "My colleague wrote: \"Never push on Fridays.\"",
    )
    .await?;
    let read = || {
        let project = project.clone();
        let thread = thread.clone();
        move |request_id| ClientRequest::StatefulMemoryRead {
            request_id,
            params: StatefulMemoryReadParams {
                expected_project_id: project.project.id.clone(),
                thread_id: thread,
                cursor: None,
                limit: None,
                background_section: true,
            },
        }
    };
    let original: StatefulMemoryReadResponse = server.request(read()).await?;
    let note = original
        .data
        .into_iter()
        .find(|item| item.attributed_to.is_some())
        .expect("attributed note");
    let corrected: StatefulMemoryCorrectResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryCorrect {
            request_id,
            params: StatefulMemoryCorrectParams {
                expected_project_id: project.project.id.clone(),
                thread_id: thread.clone(),
                entry_id: note.entry_id,
                expected_revision: note.revision,
                content: "Corrected report: avoid Friday pushes during maintenance.".to_string(),
                background_section: true,
            },
        })
        .await?;
    assert_eq!(corrected.item.attributed_to, None);
    let id = corrected.item.entry_id.clone();
    let revision = corrected.item.revision;
    let log = responses::mount_sse_sequence(&responses_server, vec![
        tool_call("attributed-update", "blackboard_update_batch", json!({"mutations": [
            {"action":"revise", "entryId":id, "expectedRevision":revision, "content":"Model rewritten words."}
        ]})),
        tool_call("attributed-promote", "blackboard_update_batch", json!({"mutations": [
            {"action":"setRootPromotion", "entryId":id, "expectedRevision":revision, "rootPromotion":"promoted"}
        ]})),
        tool_call("attributed-successor", "blackboard_record_batch", json!({"records":[{
            "idempotencyKey":"attributed-successor", "kind":"fact", "content":"Model promoted successor.", "confidenceBasisPoints":9000,
            "verification":"unverified", "importance":"high", "rootPromotion":"promoted", "supersedes":[{"entryId":id, "revision":revision}]
        }]})),
        assistant("The attributed note remains unchanged."),
    ]).await;
    run_turn(&mut server, &thread, "Review the recorded attributed note.").await?;
    let requests = log.requests();
    let update: Value = serde_json::from_str(
        &requests[1]
            .function_call_output_text("attributed-update")
            .expect("update output"),
    )?;
    let promotion: Value = serde_json::from_str(
        &requests[2]
            .function_call_output_text("attributed-promote")
            .expect("promotion output"),
    )?;
    let successor: Value = serde_json::from_str(
        &requests[3]
            .function_call_output_text("attributed-successor")
            .expect("successor output"),
    )?;
    let after: StatefulMemoryReadResponse = server.request(read()).await?;
    assert_eq!(
        (
            update["results"][0]["updated"].clone(),
            promotion["results"][0]["updated"].clone(),
            successor["results"][0]["recorded"].clone(),
            after.data
        ),
        (
            json!(false),
            json!(false),
            json!(false),
            vec![corrected.item]
        )
    );
    assert!(
        update
            .to_string()
            .contains("never promoted or revised by the model")
    );
    assert!(
        promotion
            .to_string()
            .contains("never promoted or revised by the model")
    );
    assert!(successor.to_string().contains("attributed note"));
    Ok(())
}
