//! Every committed memory change is journaled once and counted from the journal: a commit
//! found in the workspace history is remembered visibly (and only once), direct saves,
//! corrections and forgets are counted per session, the activity list pages, and the return
//! recap is dated. `§` and dashes round-trip through capture, storage and the protocol.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::StatefulCaptureOutcome;
use codex_app_server_protocol::StatefulKnowledgeCategory;
use codex_app_server_protocol::StatefulKnowledgeGroupCapturedNotification;
use codex_app_server_protocol::StatefulMemoryActivityParams;
use codex_app_server_protocol::StatefulMemoryActivityResponse;
use codex_app_server_protocol::StatefulMemoryAddKind;
use codex_app_server_protocol::StatefulMemoryAddParams;
use codex_app_server_protocol::StatefulMemoryAddResponse;
use codex_app_server_protocol::StatefulMemoryChangeCategory;
use codex_app_server_protocol::StatefulMemoryChangeTotals;
use codex_app_server_protocol::StatefulMemoryCorrectParams;
use codex_app_server_protocol::StatefulMemoryCorrectResponse;
use codex_app_server_protocol::StatefulMemoryCounts;
use codex_app_server_protocol::StatefulMemoryForgetParams;
use codex_app_server_protocol::StatefulMemoryForgetResponse;
use codex_app_server_protocol::StatefulMemoryOperation;
use codex_app_server_protocol::StatefulMemoryOrigin;
use codex_app_server_protocol::StatefulMemoryRecapParams;
use codex_app_server_protocol::StatefulMemoryRecapResponse;
use codex_app_server_protocol::StatefulMemorySummaryParams;
use codex_app_server_protocol::StatefulMemorySummaryResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const SUBJECT: &str = "Cite \u{a7} 4.2 \u{2014} not the summary";
const RULE: &str = "Always cite \u{a7} 4.2 \u{2014} never the summary \u{2013} in every answer.";

fn git(cwd: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Someone Else",
            "-c",
            "user.email=someone@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "i18n.commitEncoding=utf-8",
        ])
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn memory_changes_are_journaled_once_and_counted() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let repo = TempDir::new()?;
    git(repo.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(repo.path().join("essay.md"), "draft\n")?;
    git(repo.path(), &["add", "essay.md"]);
    git(repo.path(), &["commit", "-q", "-m", "initial"]);
    MockResponsesConfig::new(&responses.uri())
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
                name: "Activity".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(repo.path().to_path_buf())
                        .expect("absolute repository root"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "activity-project".to_string(),
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
    let summary = |since: Option<u64>| {
        let params = StatefulMemorySummaryParams {
            thread_id: thread.clone(),
            since_sequence: since,
            thread_ids: since.map(|_| vec![thread.clone()]),
        };
        move |request_id| ClientRequest::StatefulMemorySummary { request_id, params }
    };
    let start: StatefulMemorySummaryResponse = server.request(summary(None)).await?;
    assert_eq!(
        (start.counts, start.latest_sequence, start.since),
        (StatefulMemoryCounts::default(), 0, None)
    );

    run_turn(&mut server, &thread).await?;
    // Someone commits between turns; the next turn remembers it, visibly, once.
    std::fs::write(repo.path().join("essay.md"), "draft two\n")?;
    git(repo.path(), &["commit", "-q", "-am", SUBJECT]);
    run_turn(&mut server, &thread).await?;
    let group: StatefulKnowledgeGroupCapturedNotification = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        server.read_notification("statefulKnowledge/groupCaptured"),
    )
    .await??;
    assert_eq!(
        (
            group.category,
            group.saved,
            group.items.len(),
            group.items[0].outcome,
            group.items[0].text.ends_with(SUBJECT),
        ),
        (
            StatefulKnowledgeCategory::Commit,
            1,
            1,
            StatefulCaptureOutcome::Stored,
            true
        )
    );
    run_turn(&mut server, &thread).await?;

    // The user's own save, correction and forget, with a retried save.
    let added: StatefulMemoryAddResponse = server.request(add(&thread, RULE, "add-1")).await?;
    let _: StatefulMemoryAddResponse = server.request(add(&thread, RULE, "add-1")).await?;
    let decision: StatefulMemoryAddResponse = server
        .request(add_decision(&thread, "Use SQLite.", "add-2"))
        .await?;
    let _: StatefulMemoryCorrectResponse = server
        .request({
            let params = StatefulMemoryCorrectParams {
                thread_id: thread.clone(),
                entry_id: decision.item.entry_id.clone(),
                expected_revision: decision.item.revision,
                content: "Use SQLite \u{2014} one file per project.".to_string(),
                background_section: true,
            };
            move |request_id| ClientRequest::StatefulMemoryCorrect { request_id, params }
        })
        .await?;
    let note: StatefulMemoryAddResponse = server
        .request({
            let params = StatefulMemoryAddParams {
                thread_id: thread.clone(),
                kind: StatefulMemoryAddKind::Note,
                content: "Temporary note.".to_string(),
                scope: None,
                reason: None,
                client_action_id: "add-3".to_string(),
                background_section: true,
            };
            move |request_id| ClientRequest::StatefulMemoryAdd { request_id, params }
        })
        .await?;
    let _: StatefulMemoryForgetResponse = server
        .request({
            let params = StatefulMemoryForgetParams {
                thread_id: thread.clone(),
                entry_id: note.item.entry_id.clone(),
                expected_revision: note.item.revision,
            };
            move |request_id| ClientRequest::StatefulMemoryForget { request_id, params }
        })
        .await?;

    let after: StatefulMemorySummaryResponse =
        server.request(summary(Some(start.latest_sequence))).await?;
    assert_eq!(
        (after.counts, after.since),
        (
            StatefulMemoryCounts {
                rules: 1,
                decisions: 1,
                commits: 1,
                ..StatefulMemoryCounts::default()
            },
            Some(StatefulMemoryChangeTotals {
                saved: 3,
                commits_remembered: 1,
                corrected: 1,
                forgotten: 1,
                ..StatefulMemoryChangeTotals::default()
            }),
        )
    );
    assert_eq!(added.item.content, RULE);

    // The activity list pages through the same journal, oldest first.
    let mut changes = Vec::new();
    let mut cursor = None;
    loop {
        let params = StatefulMemoryActivityParams {
            thread_id: thread.clone(),
            cursor: cursor.clone(),
            after_sequence: None,
            thread_ids: None,
            limit: Some(2),
        };
        let page: StatefulMemoryActivityResponse = server
            .request(move |request_id| ClientRequest::StatefulMemoryActivity { request_id, params })
            .await?;
        changes.extend(page.data);
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(
        changes
            .iter()
            .map(|change| (change.operation, change.origin, change.category))
            .collect::<Vec<_>>(),
        vec![
            (
                StatefulMemoryOperation::Saved,
                StatefulMemoryOrigin::HostObserved,
                StatefulMemoryChangeCategory::CommitObservation
            ),
            (
                StatefulMemoryOperation::Saved,
                StatefulMemoryOrigin::DirectControl,
                StatefulMemoryChangeCategory::Rule
            ),
            (
                StatefulMemoryOperation::Saved,
                StatefulMemoryOrigin::DirectControl,
                StatefulMemoryChangeCategory::Decision
            ),
            (
                StatefulMemoryOperation::Corrected,
                StatefulMemoryOrigin::DirectControl,
                StatefulMemoryChangeCategory::Decision
            ),
            (
                StatefulMemoryOperation::Saved,
                StatefulMemoryOrigin::DirectControl,
                StatefulMemoryChangeCategory::Note
            ),
            (
                StatefulMemoryOperation::Forgotten,
                StatefulMemoryOrigin::DirectControl,
                StatefulMemoryChangeCategory::Note
            ),
        ]
    );
    assert_eq!(
        (
            changes[0].preview.starts_with("Remembered commit "),
            changes[0].preview.ends_with(SUBJECT),
            changes[1].preview.as_str(),
        ),
        (true, true, RULE)
    );

    // The return card is dated and carries the rule and the corrected decision.
    let recap: StatefulMemoryRecapResponse = server
        .request({
            let params = StatefulMemoryRecapParams {
                thread_id: thread.clone(),
            };
            move |request_id| ClientRequest::StatefulMemoryRecap { request_id, params }
        })
        .await?;
    assert_eq!(
        (
            recap.last_work.as_ref().map(|work| work.thread_id.as_str()),
            recap
                .last_work
                .as_ref()
                .is_some_and(|work| work.finished_at > 0 && work.finished_at <= recap.as_of),
            recap.rules.clone(),
            recap
                .decisions
                .iter()
                .map(|decision| decision.text.clone())
                .collect::<Vec<_>>(),
            recap.commits.len(),
        ),
        (
            Some(thread.as_str()),
            true,
            vec![RULE.to_string()],
            vec!["Use SQLite \u{2014} one file per project.".to_string()],
            1,
        )
    );
    Ok(())
}

fn add(
    thread: &str,
    content: &str,
    action: &str,
) -> impl FnOnce(codex_app_server_protocol::RequestId) -> ClientRequest {
    let params = StatefulMemoryAddParams {
        thread_id: thread.to_string(),
        kind: StatefulMemoryAddKind::Rule,
        content: content.to_string(),
        scope: None,
        reason: None,
        client_action_id: action.to_string(),
        background_section: true,
    };
    move |request_id| ClientRequest::StatefulMemoryAdd { request_id, params }
}

fn add_decision(
    thread: &str,
    content: &str,
    action: &str,
) -> impl FnOnce(codex_app_server_protocol::RequestId) -> ClientRequest {
    let params = StatefulMemoryAddParams {
        thread_id: thread.to_string(),
        kind: StatefulMemoryAddKind::Decision,
        content: content.to_string(),
        scope: None,
        reason: None,
        client_action_id: action.to_string(),
        background_section: true,
    };
    move |request_id| ClientRequest::StatefulMemoryAdd { request_id, params }
}

async fn run_turn(server: &mut TestAppServer, thread_id: &str) -> Result<()> {
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread_id.to_string(),
            input: vec![UserInput::Text {
                text: "Continue the essay.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}
