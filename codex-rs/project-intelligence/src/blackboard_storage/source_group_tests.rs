use super::super::context_bounds::tests::retire;
use super::super::context_bounds::tests::snapshot;
use super::super::knowledge::tests::change;
use super::super::knowledge::tests::rule;
use super::super::knowledge::tests::store;
use super::super::source_fixture::admission;
use super::super::source_fixture::observation;
use crate::*;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
const PROJECT: &str = "project-1";

fn request(seal: &SourceSeal, action: &str) -> SourceCaptureGroup {
    SourceCaptureGroup {
        project_id: PROJECT.to_string(),
        action_id: action.to_string(),
        group_id: format!("group-{action}"),
        members: ["Never push.", "Never commit."]
            .into_iter()
            .enumerate()
            .map(|(index, text)| SourceCaptureMember {
                write: CaptureEntryWrite {
                    candidates: vec![BlackboardEntryId::parse(format!("member-{index}")).unwrap()],
                    value: rule(text),
                    context: KnowledgeContext::new(
                        KnowledgeCategory::Rule,
                        KnowledgeAuthority::HumanDirect,
                    ),
                    change: ChangeRecord {
                        origin: ChangeOrigin::DirectControl,
                        ..change(ChangeOperation::Saved, text)
                    },
                },
                seal: seal.clone(),
                spans: vec![SourceSpan {
                    start_byte: 0,
                    end_byte: seal.original_utf8_length,
                    role: SourceSpanRole::Body,
                }],
            })
            .collect(),
    }
}

#[tokio::test]
async fn c2r1_group_refuses_cross_project_member_without_any_mutation() {
    let home = TempDir::new().unwrap();
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = store(&home).await;
    let other_node = HierarchyNodeId::parse("other-project-node").unwrap();
    HierarchyStore::open(&sqlite)
        .await
        .unwrap()
        .create_node(
            other_node.clone(),
            NewHierarchyNode {
                project_id: "project-2".to_string(),
                parent_id: None,
                kind: NodeKind::Project,
                project_root: None,
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await
        .unwrap();
    let admission = admission(&home).await;
    let text = "Never push. Never commit.";
    let seal = store
        .observe_source(observation("original", text), text)
        .await
        .unwrap();
    let before = snapshot(&store).await;
    let mut mismatched = request(&seal, "action");
    mismatched.members[1].write.value.project_id = "project-2".to_string();
    mismatched.members[1].write.value.node_id = other_node;
    assert!(matches!(
        store.write_source_group(&admission, mismatched).await,
        Err(BlackboardStoreError::InvalidSource)
    ));
    assert_eq!(snapshot(&store).await, before);
    store.pool.close().await;
    let reopened = BlackboardStore::open(&sqlite).await.unwrap();
    assert_eq!(snapshot(&reopened).await, before);
    let receipt = reopened
        .write_source_group(&admission, request(&seal, "action"))
        .await
        .unwrap();
    assert_eq!((receipt.saved, receipt.members.len()), (2, 2));
}

#[tokio::test]
async fn c2_group_faults_roll_back_semantics_keep_observation_and_cold_retry() {
    for table in [
        "blackboard_entry_revisions",
        "knowledge_context",
        "capture_identity_aliases",
        "capture_entry_sources",
        "capture_group_members",
        "capture_group_actions",
        "memory_changes",
    ] {
        let home = TempDir::new().unwrap();
        let store = store(&home).await;
        let admission = admission(&home).await;
        let text = "Ground rules for this project:\r\n- Never push.\r\n- Never commit.\r\n";
        let seal = store
            .observe_source(observation("original", text), text)
            .await
            .unwrap();
        let before = snapshot(&store).await;
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE TRIGGER fail_group BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT, 'injected group fault'); END"))).execute(&store.pool).await.unwrap();
        assert!(
            store
                .write_source_group(&admission, request(&seal, "action"))
                .await
                .is_err(),
            "{table}"
        );
        sqlx::query("DROP TRIGGER fail_group")
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(snapshot(&store).await, before, "{table}");
        store.pool.close().await;
        let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
            .await
            .unwrap();
        let receipt = reopened
            .write_source_group(&admission, request(&seal, "action"))
            .await
            .unwrap();
        assert_eq!(
            (receipt.recognized, receipt.saved, receipt.members.len()),
            (2, 2, 2)
        );
        let before_retry = snapshot(&reopened).await;
        assert_eq!(
            reopened
                .write_source_group(&admission, request(&seal, "action"))
                .await
                .unwrap(),
            receipt
        );
        assert_eq!(snapshot(&reopened).await, before_retry);
        let journal = reopened.memory_changes(PROJECT, None, 0, 50).await.unwrap();
        assert_eq!(
            journal
                .iter()
                .filter(|row| row.record.action_id.as_deref() == Some("action"))
                .count(),
            1
        );
        assert_eq!(
            journal
                .iter()
                .filter(|row| row.record.action_id.is_none()
                    && row.record.group_id.as_deref() == Some("group-action"))
                .count(),
            2
        );
    }
}

#[tokio::test]
async fn c2_group_noop_binding_loser_and_lost_response_are_durable() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let text = "Never push. Never commit.";
    let seal = store
        .observe_source(observation("original", text), text)
        .await
        .unwrap();
    let other = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .unwrap();
    let (first, second) = tokio::join!(
        store.write_source_group(&admission, request(&seal, "first")),
        other.write_source_group(&admission, request(&seal, "first"))
    );
    assert_eq!(first.unwrap(), second.unwrap());
    let noop = store
        .write_source_group(&admission, request(&seal, "noop"))
        .await
        .unwrap();
    assert_eq!((noop.saved, noop.already_present), (0, 2));
    let mut mismatch = request(&seal, "noop");
    mismatch.members[1].write.value.content = "Never deploy.".to_string();
    let before = snapshot(&store).await;
    assert!(matches!(
        store.write_source_group(&admission, mismatch).await,
        Err(BlackboardStoreError::ActionAlreadyRecorded(_))
    ));
    assert_eq!(snapshot(&store).await, before);
    let current = store
        .get_entry(PROJECT, &BlackboardEntryId::parse("member-0").unwrap())
        .await
        .unwrap()
        .unwrap();
    store
        .update_entry(PROJECT, &current.id, retire(&current))
        .await
        .unwrap();
    assert!(matches!(
        store
            .write_source_group(&admission, request(&seal, "first"))
            .await,
        Err(BlackboardStoreError::RetiredIdentity)
    ));
    assert!(matches!(
        store
            .write_source_group(&admission, request(&seal, "noop"))
            .await,
        Err(BlackboardStoreError::RetiredIdentity)
    ));
}

#[tokio::test]
async fn c2_group_before_commit_cancellation_rolls_back_then_committed_recovery() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let text = "Never push. Never commit.";
    let seal = store
        .observe_source(observation("original", text), text)
        .await
        .unwrap();
    let before = snapshot(&store).await;
    let lock = store.pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let waiting_store = store.clone();
    let waiting_seal = seal.clone();
    let waiting_admission = admission.clone();
    let task = tokio::spawn(async move {
        waiting_store
            .write_source_group(&waiting_admission, request(&waiting_seal, "cancelled"))
            .await
    });
    tokio::task::yield_now().await;
    // Abort only the task this test started, while it is waiting for writer admission.
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    drop(lock);
    assert_eq!(snapshot(&store).await, before);
    let committed = store
        .write_source_group(&admission, request(&seal, "committed"))
        .await
        .unwrap();
    store.pool.close().await;
    let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .unwrap();
    assert_eq!(
        reopened
            .write_source_group(&admission, request(&seal, "committed"))
            .await
            .unwrap(),
        committed
    );
}

#[tokio::test]
async fn c2_original_source_cold_retry_after_failed_group_and_dual_writer_forget() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let text = "Never push. Never commit.";
    let seal = store
        .observe_source(observation("original", text), text)
        .await
        .unwrap();
    let initial = store
        .write_source_group(&admission, request(&seal, "direct"))
        .await
        .unwrap();
    let id = BlackboardEntryId::parse(initial.members[0].entry_id.clone().unwrap()).unwrap();
    let entry = store.get_entry(PROJECT, &id).await.unwrap().unwrap();
    let other = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .unwrap();
    let mut automatic = request(&seal, "automatic");
    for member in &mut automatic.members {
        member.write.change.origin = ChangeOrigin::ModelTool;
        member.write.value.kind = BlackboardKind::Note;
        member.write.value.provenance.kind = BlackboardProvenanceKind::Agent;
        member.write.context.authority = KnowledgeAuthority::AssistantReported;
        member.write.context.category = KnowledgeCategory::Note;
    }
    let (capture, forgotten) = tokio::join!(
        other.write_source_group(&admission, automatic),
        store.update_entry(PROJECT, &id, retire(&entry))
    );
    forgotten.unwrap();
    assert!(capture.is_err());
    let mut retry = request(&seal, "fresh-automatic");
    for member in &mut retry.members {
        member.write.change.origin = ChangeOrigin::ModelTool;
        member.write.value.provenance.kind = BlackboardProvenanceKind::Agent;
    }
    assert!(store.write_source_group(&admission, retry).await.is_err());
    store.pool.close().await;
    other.pool.close().await;
    let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .unwrap();
    assert_eq!(
        reopened
            .observe_source(observation("original", text), text)
            .await
            .unwrap(),
        seal
    );
    assert!(
        reopened
            .read_source_range(
                PROJECT,
                &seal.exact_source_locator,
                &seal.digest,
                /*start*/ 0,
                text.len() as u32
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn c2_group_bounds_noop_fault_and_model_authority_refusal_leave_whole_store_unchanged() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let seal = store
        .observe_source(
            observation("bounds", "Never push. Never commit."),
            "Never push. Never commit.",
        )
        .await
        .unwrap();
    let before = snapshot(&store).await;
    let mut empty = request(&seal, "empty");
    empty.members.clear();
    assert!(store.write_source_group(&admission, empty).await.is_err());
    let mut oversized = request(&seal, "oversized");
    while oversized.members.len() < 25 {
        oversized
            .members
            .push(request(&seal, "unused").members.remove(/*index*/ 0));
    }
    assert!(
        store
            .write_source_group(&admission, oversized)
            .await
            .is_err()
    );
    let mut large_context = request(&seal, "legacy-sized-context");
    large_context.members[0].write.context.payload = Some("x".repeat(/*n*/ 1048576));
    assert!(matches!(
        store.write_source_group(&admission, large_context).await,
        Err(BlackboardStoreError::UnsupportedContext)
    ));
    let mut model = request(&seal, "model-authority");
    for member in &mut model.members {
        member.write.change.origin = ChangeOrigin::ModelTool;
    }
    assert!(matches!(
        store.write_source_group(&admission, model).await,
        Err(BlackboardStoreError::ModelMutationRefused)
    ));
    assert_eq!(snapshot(&store).await, before);
    store
        .write_source_group(&admission, request(&seal, "first"))
        .await
        .unwrap();
    let before = snapshot(&store).await;
    sqlx::query("CREATE TRIGGER noop_fault BEFORE INSERT ON capture_group_actions BEGIN SELECT RAISE(ABORT, 'noop fault'); END").execute(&store.pool).await.unwrap();
    assert!(
        store
            .write_source_group(&admission, request(&seal, "noop"))
            .await
            .is_err()
    );
    sqlx::query("DROP TRIGGER noop_fault")
        .execute(&store.pool)
        .await
        .unwrap();
    assert_eq!(snapshot(&store).await, before);
    let receipt = store
        .write_source_group(&admission, request(&seal, "noop"))
        .await
        .unwrap();
    assert_eq!((receipt.saved, receipt.already_present), (0, 2));
}

#[tokio::test]
async fn c2_agent_group_races_forget_without_active_identity_after_retirement() {
    for forget_first in [false, true] {
        let home = TempDir::new().unwrap();
        let store = store(&home).await;
        let admission = admission(&home).await;
        let text = "Never push. Never commit.";
        let seal = store
            .observe_source(observation("race", text), text)
            .await
            .unwrap();
        let model_request = |action: &str| {
            let mut group = request(&seal, action);
            for member in &mut group.members {
                member.write.value.provenance.kind = BlackboardProvenanceKind::Agent;
                member.write.context.authority = KnowledgeAuthority::AssistantReported;
                member.write.change.origin = ChangeOrigin::ModelTool;
            }
            group
        };
        let first = store
            .write_source_group(&admission, model_request("first"))
            .await
            .unwrap();
        let id = BlackboardEntryId::parse(first.members[0].entry_id.clone().unwrap()).unwrap();
        let entry = store.get_entry(PROJECT, &id).await.unwrap().unwrap();
        let other = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
            .await
            .unwrap();
        let result = if forget_first {
            let (retirement, capture) = tokio::join!(
                store.update_entry(PROJECT, &id, retire(&entry)),
                other.write_source_group(&admission, model_request("racer"))
            );
            retirement.unwrap();
            capture
        } else {
            let (capture, retirement) = tokio::join!(
                other.write_source_group(&admission, model_request("racer")),
                store.update_entry(PROJECT, &id, retire(&entry))
            );
            retirement.unwrap();
            capture
        };
        if let Ok(receipt) = result {
            assert_eq!((receipt.saved, receipt.already_present), (0, 2));
        }
        assert_eq!(
            store.get_entry(PROJECT, &id).await.unwrap().unwrap().state,
            BlackboardEntryState::Tombstoned
        );
        assert!(
            store
                .write_source_group(&admission, model_request("after-forget"))
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM blackboard_entries")
                .fetch_one(&store.pool)
                .await
                .unwrap(),
            2
        );
    }
}

#[tokio::test]
async fn c2_failed_original_group_then_forget_and_cold_replay_never_restores() {
    for seeded_before_observation in [false, true] {
        let home = TempDir::new().unwrap();
        let store = store(&home).await;
        let admission = admission(&home).await;
        let id = BlackboardEntryId::parse("generation-zero").unwrap();
        if seeded_before_observation {
            store
                .create_entry(id.clone(), rule("Never push."))
                .await
                .unwrap();
        }
        let text = "Ground rules for this project:\n- Never push.\n";
        let observed = observation("original-failed-message", text);
        let seal = store.observe_source(observed.clone(), text).await.unwrap();
        let original_request = |action: &str| {
            let mut group = request(&seal, action);
            group.members.truncate(/*len*/ 1);
            group
        };
        let before_failure = snapshot(&store).await;
        sqlx::query("CREATE TRIGGER fail_original BEFORE INSERT ON capture_group_actions BEGIN SELECT RAISE(ABORT, 'before group outcome'); END")
            .execute(&store.pool).await.unwrap();
        assert!(
            store
                .write_source_group(&admission, original_request("failed-original-action"))
                .await
                .is_err()
        );
        sqlx::query("DROP TRIGGER fail_original")
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(snapshot(&store).await, before_failure);
        if !seeded_before_observation {
            store
                .create_entry(id.clone(), rule("Never push."))
                .await
                .unwrap();
        }
        let entry = store.get_entry(PROJECT, &id).await.unwrap().unwrap();
        store
            .update_entry(PROJECT, &id, retire(&entry))
            .await
            .unwrap();
        let forgotten = snapshot(&store).await;
        store.pool.close().await;
        let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
            .await
            .unwrap();
        assert_eq!(reopened.observe_source(observed, text).await.unwrap(), seal);
        // Transport IDs are deliberately not part of the native observation identity.
        for action in ["failed-original-action", "new-transport-action"] {
            let mut automatic = original_request(action);
            for member in &mut automatic.members {
                member.write.value.provenance.kind = BlackboardProvenanceKind::Agent;
                member.write.context.authority = KnowledgeAuthority::AssistantReported;
                member.write.change.origin = ChangeOrigin::ModelTool;
            }
            assert!(matches!(
                reopened.write_source_group(&admission, automatic).await,
                Err(BlackboardStoreError::RetiredIdentity | BlackboardStoreError::SourceExcluded)
            ));
            assert_eq!(snapshot(&reopened).await, forgotten);
        }
    }
}

#[tokio::test]
async fn c2_source_group_and_forget_serialize_across_actual_processes() {
    for (order, failed_original) in [
        ("race", false),
        ("capture-first", false),
        ("forget-first", false),
        ("race", true),
        ("capture-first", true),
        ("forget-first", true),
    ] {
        let home = TempDir::new().unwrap();
        let store = store(&home).await;
        let native_admission = admission(&home).await;
        if failed_original {
            store
                .create_entry_with_context(
                    BlackboardEntryId::parse("member-0").unwrap(),
                    rule("Never push."),
                    KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect),
                    ChangeRecord {
                        origin: ChangeOrigin::DirectControl,
                        ..change(ChangeOperation::Saved, "Never push.")
                    },
                )
                .await
                .unwrap();
        }
        let text = "Ground rules for this project:\n- Never push.\n";
        let seal = store
            .observe_source(observation("cross-process-original", text), text)
            .await
            .unwrap();
        let mut original = request(&seal, "process-original");
        original.members.truncate(/*len*/ 1);
        if failed_original {
            let before = snapshot(&store).await;
            sqlx::query("CREATE TRIGGER fail_process_original BEFORE INSERT ON capture_group_actions BEGIN SELECT RAISE(ABORT, 'failed first capture'); END").execute(&store.pool).await.unwrap();
            assert!(
                store
                    .write_source_group(&native_admission, original)
                    .await
                    .is_err()
            );
            sqlx::query("DROP TRIGGER fail_process_original")
                .execute(&store.pool)
                .await
                .unwrap();
            assert_eq!(snapshot(&store).await, before);
        } else {
            original.members[0].write.value.provenance.kind = BlackboardProvenanceKind::Agent;
            original.members[0].write.context.authority = KnowledgeAuthority::AssistantReported;
            original.members[0].write.change.origin = ChangeOrigin::ModelTool;
            store
                .write_source_group(&native_admission, original)
                .await
                .unwrap();
        }
        drop(native_admission);

        // Run this library's own test binary: each worker owns a separate OS process
        // and SQLite pool. No workspace executable lookup or shell interpolation.
        let executable = std::env::current_exe().unwrap();
        let worker = format!(
            "{}::source_group_process_worker",
            module_path!().split_once("::").unwrap().1
        );
        let mut capture = tokio::process::Command::new(&executable);
        capture
            .args(["--ignored", "--exact", &worker, "--nocapture"])
            .env("CODEX_C2_PROCESS_HOME", home.path())
            .env("CODEX_C2_PROCESS_ROLE", "capture")
            .env("CODEX_C2_PROCESS_ORDER", order)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let capture = capture.spawn().unwrap();
        let mut forget = tokio::process::Command::new(&executable);
        forget
            .args(["--ignored", "--exact", &worker, "--nocapture"])
            .env("CODEX_C2_PROCESS_HOME", home.path())
            .env("CODEX_C2_PROCESS_ROLE", "forget")
            .env("CODEX_C2_PROCESS_ORDER", order)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let forget = forget.spawn().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 10), async {
            while !home.path().join("capture-ready").exists()
                || !home.path().join("forget-ready").exists()
            {
                tokio::time::sleep(std::time::Duration::from_millis(/*millis*/ 10)).await;
            }
        })
        .await
        .unwrap();
        tokio::fs::write(home.path().join("start"), b"start")
            .await
            .unwrap();
        let (capture, forget) = tokio::join!(capture.wait_with_output(), forget.wait_with_output());
        for output in [capture.unwrap(), forget.unwrap()] {
            assert!(
                output.status.success(),
                "{order}: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let committed = snapshot(&store).await;
        store.pool.close().await;
        let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
            .await
            .unwrap();
        assert_eq!(snapshot(&reopened).await, committed);
        let entry = reopened
            .get_entry(PROJECT, &BlackboardEntryId::parse("member-0").unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.state, BlackboardEntryState::Tombstoned);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM blackboard_entries")
                .fetch_one(&reopened.pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            reopened
                .observe_source(observation("cross-process-original", text), text)
                .await
                .unwrap(),
            seal
        );
        assert!(
            !reopened
                .entry_source_eligible(PROJECT, &entry.id)
                .await
                .unwrap()
        );
        assert!(
            reopened
                .get_source_eligible_entry(PROJECT, &entry.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            reopened
                .read_source_range(
                    PROJECT,
                    &seal.exact_source_locator,
                    &seal.digest,
                    /*start*/ 0,
                    text.len() as u32
                )
                .await,
            Err(BlackboardStoreError::SourceExcluded)
        ));
        let admission = admission(&home).await;
        let mut retry = request(&seal, "process-original");
        retry.members.truncate(/*len*/ 1);
        retry.members[0].write.value.provenance.kind = BlackboardProvenanceKind::Agent;
        retry.members[0].write.context.authority = KnowledgeAuthority::AssistantReported;
        retry.members[0].write.change.origin = ChangeOrigin::ModelTool;
        let before = snapshot(&reopened).await;
        assert!(matches!(
            reopened.write_source_group(&admission, retry).await,
            Err(BlackboardStoreError::RetiredIdentity | BlackboardStoreError::SourceExcluded)
        ));
        assert_eq!(snapshot(&reopened).await, before);
    }
}

#[tokio::test]
#[ignore = "support worker executed by the cross-process qualification test"]
async fn source_group_process_worker() {
    let path = std::path::PathBuf::from(std::env::var_os("CODEX_C2_PROCESS_HOME").unwrap());
    let role = std::env::var("CODEX_C2_PROCESS_ROLE").unwrap();
    let order = std::env::var("CODEX_C2_PROCESS_ORDER").unwrap();
    let sqlite = SqliteConfig::new_for_testing(path.abs());
    let store = BlackboardStore::open(&sqlite).await.unwrap();
    tokio::fs::write(path.join(format!("{role}-ready")), b"ready")
        .await
        .unwrap();
    let dependency = match (order.as_str(), role.as_str()) {
        ("capture-first", "forget") => Some("capture-done"),
        ("forget-first", "capture") => Some("forget-done"),
        ("race" | "capture-first" | "forget-first", "capture" | "forget") => None,
        _ => panic!("invalid worker role/order"),
    };
    tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 10), async {
        while !path.join("start").exists()
            || dependency.is_some_and(|name| !path.join(name).exists())
        {
            tokio::time::sleep(std::time::Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await
    .unwrap();
    if role == "forget" {
        let id = BlackboardEntryId::parse("member-0").unwrap();
        let entry = store.get_entry(PROJECT, &id).await.unwrap().unwrap();
        store
            .update_entry(PROJECT, &id, retire(&entry))
            .await
            .unwrap();
    } else {
        let thread = serde_json::from_str("\"00000000-0000-0000-0000-000000000001\"").unwrap();
        let admission = codex_state::ThreadProjectAdmission::acquire(&sqlite, thread, PROJECT)
            .await
            .unwrap()
            .unwrap();
        let metadata: String = sqlx::query_scalar("SELECT metadata FROM capture_sources WHERE project_id = 'project-1' AND original_event_id = 'cross-process-original'").fetch_one(&store.pool).await.unwrap();
        let seal: SourceSeal = serde_json::from_str(&metadata).unwrap();
        let mut capture = request(&seal, "process-racer");
        capture.members.truncate(/*len*/ 1);
        capture.members[0].write.value.provenance.kind = BlackboardProvenanceKind::Agent;
        capture.members[0].write.context.authority = KnowledgeAuthority::AssistantReported;
        capture.members[0].write.change.origin = ChangeOrigin::ModelTool;
        match store.write_source_group(&admission, capture).await {
            Ok(receipt) => assert_eq!((receipt.saved, receipt.already_present), (0, 1)),
            Err(
                BlackboardStoreError::RetiredIdentity
                | BlackboardStoreError::SourceExcluded
                | BlackboardStoreError::ModelMutationRefused,
            ) => (),
            Err(error) => panic!("unexpected process outcome: {error}"),
        }
    }
    store.pool.close().await;
    tokio::fs::write(path.join(format!("{role}-done")), b"done")
        .await
        .unwrap();
}

#[tokio::test]
async fn c2_host_origin_group_replay_rechecks_current_human_authority() {
    for origin in [ChangeOrigin::HostCapture, ChangeOrigin::HostObserved] {
        let home = TempDir::new().unwrap();
        let store = store(&home).await;
        let admission = admission(&home).await;
        let text = "Ground rules for this project:\n- Never push.\n";
        let seal = store
            .observe_source(observation("host-replay-original", text), text)
            .await
            .unwrap();
        let request = || {
            let mut group = request(&seal, "host-origin-action");
            group.members.truncate(/*len*/ 1);
            group.members[0].write.value.provenance.kind = BlackboardProvenanceKind::Agent;
            group.members[0].write.context.authority = KnowledgeAuthority::AssistantReported;
            group.members[0].write.change.origin = origin;
            group
        };
        store
            .write_source_group(&admission, request())
            .await
            .unwrap();
        // A schema-valid legacy authority adjustment must be read from current
        // context, not inferred from the unchanged group receipt or alias projection.
        sqlx::query(
            "UPDATE knowledge_context SET authority = 'human_direct' WHERE entry_id = 'member-0'",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        let before = snapshot(&store).await;
        store.pool.close().await;
        let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
            .await
            .unwrap();
        assert!(matches!(
            reopened.write_source_group(&admission, request()).await,
            Err(BlackboardStoreError::ModelMutationRefused)
        ));
        assert_eq!(snapshot(&reopened).await, before);
    }
}
