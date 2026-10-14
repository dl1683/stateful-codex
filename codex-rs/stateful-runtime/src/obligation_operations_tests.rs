use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;
use crate::NewStatefulRun;
use crate::RunBudget;
use crate::WorkflowMode;

const PROJECT: &str = "project";
const SOURCE: &str = "call-source";

/// A legacy prefix-shaped blocker has no host receipt, so neither spelling may resolve it.
#[tokio::test]
async fn legacy_prefix_shaped_blockers_refuse_operations_without_writes() {
    let home = TempDir::new().expect("home");
    let store = open_store(&home).await;
    let run_id = new_run(&store, "legacy-prefix").await;
    store
        .append_obligation(
            "legacy".into(),
            NewObligation {
                project_id: PROJECT.into(),
                run_id: run_id.clone(),
                packet: ObligationPacket {
                    blockers: vec![retained("x")],
                    ..Default::default()
                },
                provenance_source_id: SOURCE.into(),
            },
        )
        .await
        .expect("legacy blocker");
    let before = all_obligations(&store, &run_id).await;
    for (index, resolution) in ["x".to_string(), retained("x")].into_iter().enumerate() {
        let refused = update(
            &store,
            &run_id,
            &format!("resolve-{index}"),
            learning("new"),
            &[&resolution],
        )
        .await;
        assert!(matches!(
            refused,
            Err(StatefulRunStoreError::ObligationOperationRefused(_))
        ));
        assert_eq!(all_obligations(&store, &run_id).await, before);
    }
    assert!(retain(&store, &run_id, "new", &["y"]).await.is_err());
    assert_eq!(all_obligations(&store, &run_id).await, before);
}

async fn open_store(home: &TempDir) -> StatefulRunStore {
    StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store opens")
}

async fn new_run(store: &StatefulRunStore, id: &str) -> StatefulRunId {
    store
        .create_run(
            StatefulRunId::parse(id).expect("run id"),
            NewStatefulRun {
                project_id: PROJECT.to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "Fix the parser.".to_string(),
                mode: WorkflowMode::Autonomous,
                budget: RunBudget {
                    max_continuations: 4,
                    max_elapsed_seconds: 600,
                },
            },
        )
        .await
        .expect("run created")
        .id
}

fn key(value: &str) -> OperationReceiptKey {
    OperationReceiptKey::parse(value).expect("receipt key")
}

fn retained(issue: &str) -> String {
    format!("{RETAINED_ISSUE_PREFIX}{issue}")
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

async fn retain(
    store: &StatefulRunStore,
    run_id: &StatefulRunId,
    call: &str,
    issues: &[&str],
) -> Result<StatefulObligation, StatefulRunStoreError> {
    store
        .apply_obligation_operation(
            run_id,
            ObligationOperation::Retain {
                issues: strings(issues),
            },
            &key(call),
            SOURCE,
        )
        .await
}

async fn update(
    store: &StatefulRunStore,
    run_id: &StatefulRunId,
    idempotency_key: &str,
    packet: ObligationPacket,
    resolved: &[&str],
) -> Result<StatefulObligation, StatefulRunStoreError> {
    store
        .apply_obligation_operation(
            run_id,
            ObligationOperation::Update {
                packet: Box::new(packet),
                resolved_issues: strings(resolved),
            },
            &key(idempotency_key),
            SOURCE,
        )
        .await
}

fn learning(text: &str) -> ObligationPacket {
    ObligationPacket {
        learning: vec![text.to_string()],
        ..ObligationPacket::default()
    }
}

async fn all_obligations(
    store: &StatefulRunStore,
    run_id: &StatefulRunId,
) -> Vec<StatefulObligation> {
    store
        .list_obligations(
            run_id, /*after_sequence*/ None, /*max_results*/ 101,
        )
        .await
        .expect("obligations listed")
}

async fn latest_packet(store: &StatefulRunStore, run_id: &StatefulRunId) -> ObligationPacket {
    store
        .latest_obligation(run_id)
        .await
        .expect("latest read")
        .expect("latest exists")
        .value
        .packet
}

/// Completion blockers alone create one receipt; replay after resolution cannot restore them.
#[tokio::test]
async fn completion_blockers_alone_replay_without_resurrecting_resolved_issues() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run_id = new_run(&store, "completion-blockers").await;
    let operation = ObligationOperation::RetainCompletion {
        issues: Vec::new(),
        blockers: strings(&["x"]),
    };
    let receipt = key("completion");
    let original = store
        .apply_obligation_operation(&run_id, operation.clone(), &receipt, SOURCE)
        .await
        .expect("blocker-only retention");
    assert_eq!(
        original.value.packet,
        ObligationPacket {
            blockers: vec![retained("x")],
            ..Default::default()
        }
    );
    assert_eq!(
        store
            .apply_obligation_operation(&run_id, operation.clone(), &receipt, SOURCE)
            .await
            .expect("identical replay"),
        original
    );
    assert_eq!(
        all_obligations(&store, &run_id).await,
        vec![original.clone()]
    );

    let resolved = update(&store, &run_id, "resolve-x", learning("fixed"), &["x"])
        .await
        .expect("resolution");
    let before = all_obligations(&store, &run_id).await;
    assert_eq!(
        store
            .apply_obligation_operation(&run_id, operation, &receipt, SOURCE)
            .await
            .expect("replay after resolution"),
        original
    );
    assert_eq!(all_obligations(&store, &run_id).await, before);
    assert_eq!(latest_packet(&store, &run_id).await, resolved.value.packet);
    let outcome = store
        .end_run_answered(
            &crate::AnsweredRunCommit {
                run_id: run_id.clone(),
                thread_id: "thread-1".into(),
                turn_id: "final".into(),
                answer: "The parser is fixed.".into(),
                result: "The parser is fixed.".into(),
                trailer_issues: vec![],
            },
            &|| true,
        )
        .await
        .expect("terminal answer");
    let run = store.get_run(&run_id).await.unwrap();
    assert_eq!(
        store
            .apply_obligation_operation(
                &run_id,
                ObligationOperation::RetainCompletion {
                    issues: Vec::new(),
                    blockers: strings(&["x"])
                },
                &receipt,
                SOURCE
            )
            .await
            .expect("terminal receipt replay"),
        original
    );
    assert_eq!(all_obligations(&store, &run_id).await, before);
    assert_eq!(store.get_run(&run_id).await.unwrap(), run);
    assert!(matches!(outcome, crate::AnsweredRunOutcome::Answered(..)));
}

/// Empty retention operations refuse atomically; an issue-free redirect skips the operation.
#[tokio::test]
async fn empty_retention_operations_refuse_without_writes() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run_id = new_run(&store, "empty-retention").await;
    for operation in [
        ObligationOperation::Retain { issues: Vec::new() },
        ObligationOperation::RetainCompletion {
            issues: Vec::new(),
            blockers: Vec::new(),
        },
    ] {
        assert_eq!(
            store
                .apply_obligation_operation(&run_id, operation, &key("empty"), SOURCE)
                .await
                .map_err(|error| error.to_string()),
            Err(format!(
                "nothing changed: retain 1-{MAX_OPERATION_ISSUES} open issues per call"
            ))
        );
        assert_eq!(all_obligations(&store, &run_id).await, Vec::new());
    }
}

/// B1: a replayed Retain returns its original row unchanged after an intervening write and
/// appends nothing; the same call with other issues conflicts; retained issues accumulate
/// without duplicates and the rest of the latest packet is copied verbatim.
#[tokio::test]
async fn a_retain_replay_returns_the_original_row_after_intervening_writes() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run_id = new_run(&store, "run-1").await;

    let first = retain(&store, &run_id, "call-a", &["x"]).await.expect("A");
    assert_eq!(
        first.value.packet,
        ObligationPacket {
            blockers: vec![retained("x")],
            ..ObligationPacket::default()
        }
    );
    let progress = ObligationPacket {
        learning: strings(&["the lexer drops tabs"]),
        next: strings(&["fix the lexer"]),
        ..ObligationPacket::default()
    };
    update(&store, &run_id, "progress-1", progress.clone(), &[])
        .await
        .expect("update");

    let rows = all_obligations(&store, &run_id).await;
    assert_eq!(
        retain(&store, &run_id, "call-a", &["x"])
            .await
            .expect("replay A"),
        first
    );
    assert_eq!(all_obligations(&store, &run_id).await, rows);

    retain(&store, &run_id, "call-b", &["x", "z"])
        .await
        .expect("B");
    assert!(matches!(
        retain(&store, &run_id, "call-b", &["z"]).await,
        Err(StatefulRunStoreError::ObligationIdentityConflict(_))
    ));
    assert_eq!(
        latest_packet(&store, &run_id).await,
        ObligationPacket {
            blockers: vec![retained("x"), retained("z")],
            ..progress
        }
    );
}

/// B4: a resolution replay (also after more writes and a restart) returns the original row
/// and never resurrects the issue; omitted issues are carried until resolved, and an unknown
/// resolution refuses the whole call.
#[tokio::test]
async fn a_resolution_replay_never_resurrects_the_issue() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run_id = new_run(&store, "run-1").await;
    retain(&store, &run_id, "call-r", &["x"])
        .await
        .expect("retain");

    // An update that omits x without resolving it carries x forward.
    let carried = update(&store, &run_id, "k0", learning("tried a fix"), &[])
        .await
        .expect("omitting update");
    assert_eq!(
        carried.value.packet,
        ObligationPacket {
            blockers: vec![retained("x")],
            ..learning("tried a fix")
        }
    );

    let resolution = update(&store, &run_id, "k", learning("x fixed"), &["x"])
        .await
        .expect("resolution");
    assert_eq!(
        resolution.value.packet,
        ObligationPacket {
            changed: strings(&["Resolved: x"]),
            ..learning("x fixed")
        }
    );
    // After resolution, x is no longer carried.
    let after = update(&store, &run_id, "k2", learning("moved on"), &[])
        .await
        .expect("later update");
    assert_eq!(after.value.packet, learning("moved on"));

    let rows = all_obligations(&store, &run_id).await;
    assert_eq!(
        update(&store, &run_id, "k", learning("x fixed"), &["x"])
            .await
            .expect("replay"),
        resolution
    );
    store.pool.close().await;
    let store = open_store(&home).await;
    assert_eq!(
        update(&store, &run_id, "k", learning("x fixed"), &["x"])
            .await
            .expect("replay after restart"),
        resolution
    );
    assert_eq!(all_obligations(&store, &run_id).await, rows);
    assert_eq!(latest_packet(&store, &run_id).await, learning("moved on"));

    let refused = update(&store, &run_id, "k3", learning("other"), &["nope"]).await;
    assert!(
        matches!(
            refused,
            Err(StatefulRunStoreError::ObligationOperationRefused(_))
        ),
        "{refused:?}"
    );
    assert_eq!(all_obligations(&store, &run_id).await, rows);
}

/// B6: bounds hold on the final packet, after carry-forward and resolution history; an
/// overflow refuses the whole call and no obligation row of the run changes.
#[tokio::test]
async fn an_augmented_overflow_refuses_the_whole_call() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;

    let full = new_run(&store, "full-blockers").await;
    let thirty = ObligationPacket {
        blockers: (0..30).map(|index| format!("blocker {index}")).collect(),
        ..ObligationPacket::default()
    };
    update(&store, &full, "thirty", thirty, &[])
        .await
        .expect("30 blockers");
    let rows = all_obligations(&store, &full).await;
    assert_eq!(
        retain(&store, &full, "call-3", &["a", "b", "c"])
            .await
            .map_err(|error| error.to_string()),
        Err(format!("nothing changed: {OVERFLOW}"))
    );
    assert_eq!(all_obligations(&store, &full).await, rows);
    // Exactly at the bound is accepted.
    assert_eq!(
        retain(&store, &full, "call-2", &["a", "b"])
            .await
            .expect("32 blockers")
            .value
            .packet
            .blockers
            .len(),
        32
    );

    let history = new_run(&store, "full-changed").await;
    retain(&store, &history, "call-pq", &["p", "q"])
        .await
        .expect("retain p q");
    let rows = all_obligations(&store, &history).await;
    let crowded = ObligationPacket {
        changed: (0..31).map(|index| format!("change {index}")).collect(),
        ..learning("both fixed")
    };
    assert_eq!(
        update(&store, &history, "u1", crowded, &["p", "q"])
            .await
            .map_err(|error| error.to_string()),
        Err(format!("nothing changed: {OVERFLOW}"))
    );
    assert_eq!(all_obligations(&store, &history).await, rows);
}

fn resolved_with_y() -> ObligationPacket {
    ObligationPacket {
        changed: strings(&["Resolved: x"]),
        blockers: vec![retained("y")],
        ..learning("x fixed")
    }
}

/// B7 (orders): a Retain of y and an Update resolving x give the same complete packet in
/// either order.
#[tokio::test]
async fn retention_and_resolution_commute() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    for retain_first in [true, false] {
        let run_id = new_run(&store, &format!("run-{retain_first}")).await;
        retain(&store, &run_id, "call-x", &["x"]).await.expect("x");
        if retain_first {
            retain(&store, &run_id, "call-y", &["y"]).await.expect("y");
        }
        update(&store, &run_id, "resolve-x", learning("x fixed"), &["x"])
            .await
            .expect("x resolved");
        if !retain_first {
            retain(&store, &run_id, "call-y", &["y"]).await.expect("y");
        }
        assert_eq!(latest_packet(&store, &run_id).await, resolved_with_y());
        assert_eq!(all_obligations(&store, &run_id).await.len(), 3);
    }
}

/// B7 (contention): while a third pool holds the write lock, a Retain and a resolving
/// Update are both started on two other pools, so both wait on the lock; once it is
/// released they commit serialized, each reading the packet the other wrote, and neither
/// write is lost.
#[tokio::test]
async fn contended_retention_and_resolution_serialize_on_one_connection() {
    let home = TempDir::new().expect("tempdir");
    let first = open_store(&home).await;
    let second = open_store(&home).await;
    let holder = open_store(&home).await;
    let run_id = new_run(&first, "run-1").await;
    retain(&first, &run_id, "call-x", &["x"]).await.expect("x");

    let lock = holder
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .expect("write lock held");
    let retain_y = tokio::spawn({
        let (store, run_id) = (first.clone(), run_id.clone());
        async move { retain(&store, &run_id, "call-y", &["y"]).await }
    });
    let resolve_x = tokio::spawn({
        let (store, run_id) = (second.clone(), run_id.clone());
        async move { update(&store, &run_id, "resolve-x", learning("x fixed"), &["x"]).await }
    });
    // Both operations reach the lock and wait (busy timeout 5 s) before it is released.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(!retain_y.is_finished() && !resolve_x.is_finished());
    lock.rollback().await.expect("lock released");

    let retained_y = retain_y.await.expect("task").expect("y");
    let resolved_x = resolve_x.await.expect("task").expect("x resolved");
    assert_ne!(retained_y.sequence, resolved_x.sequence);
    assert_eq!(latest_packet(&first, &run_id).await, resolved_with_y());
    assert_eq!(all_obligations(&first, &run_id).await.len(), 3);
}

/// A raw issue shaped like a retained entry is refused whole, so each retained issue has
/// one identity and both supported resolution spellings (raw and prefixed) name it.
#[tokio::test]
async fn a_prefix_shaped_issue_is_refused_and_both_spellings_resolve() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run_id = new_run(&store, "run-1").await;
    let prefixed_x = retained("x");
    let refused = retain(&store, &run_id, "call-ambiguous", &["x", &prefixed_x]).await;
    assert!(
        matches!(
            refused,
            Err(StatefulRunStoreError::ObligationOperationRefused(_))
        ),
        "{refused:?}"
    );
    assert_eq!(all_obligations(&store, &run_id).await, Vec::new());

    retain(&store, &run_id, "call-xy", &["x", "y"])
        .await
        .expect("x y");
    let raw = update(&store, &run_id, "resolve-raw", learning("x fixed"), &["x"])
        .await
        .expect("raw spelling");
    assert_eq!(
        raw.value.packet,
        ObligationPacket {
            changed: strings(&["Resolved: x"]),
            blockers: vec![retained("y")],
            ..learning("x fixed")
        }
    );
    let prefixed_y = retained("y");
    let prefixed = update(
        &store,
        &run_id,
        "resolve-prefixed",
        learning("y fixed"),
        &[&prefixed_y],
    )
    .await
    .expect("prefixed spelling");
    assert_eq!(
        prefixed.value.packet,
        ObligationPacket {
            changed: strings(&["Resolved: y"]),
            ..learning("y fixed")
        }
    );
}

/// B8: narrative "Resolved:" text never erases a retained issue, and only the host may write
/// the reserved prefix.
#[tokio::test]
async fn only_resolved_issues_erase_and_the_prefix_is_reserved() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run_id = new_run(&store, "run-1").await;
    retain(&store, &run_id, "call-x", &["x"]).await.expect("x");

    let narrative = ObligationPacket {
        changed: strings(&["Resolved: x"]),
        ..ObligationPacket::default()
    };
    assert_eq!(
        update(&store, &run_id, "narrative", narrative.clone(), &[])
            .await
            .expect("narrative update")
            .value
            .packet,
        ObligationPacket {
            blockers: vec![retained("x")],
            ..narrative
        }
    );

    let rows = all_obligations(&store, &run_id).await;
    for packet in [
        ObligationPacket {
            blockers: vec![retained("z")],
            ..ObligationPacket::default()
        },
        ObligationPacket {
            changed: vec![retained("z")],
            ..learning("forged")
        },
    ] {
        let refused = update(&store, &run_id, "forged", packet, &[]).await;
        assert!(
            matches!(
                refused,
                Err(StatefulRunStoreError::ObligationOperationRefused(_))
            ),
            "{refused:?}"
        );
    }
    assert_eq!(all_obligations(&store, &run_id).await, rows);
}

/// An update written before receipts existed is found by its legacy id: the same packet
/// replays it, another packet conflicts.
#[tokio::test]
async fn a_legacy_update_is_replayed_by_its_key() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run_id = new_run(&store, "run-1").await;
    let legacy = store
        .append_obligation(
            stable_id("obligation", PROJECT, "progress-1"),
            NewObligation {
                project_id: PROJECT.to_string(),
                run_id: run_id.clone(),
                packet: learning("old"),
                provenance_source_id: SOURCE.to_string(),
            },
        )
        .await
        .expect("legacy append");
    assert_eq!(
        update(&store, &run_id, "progress-1", learning("old"), &[])
            .await
            .expect("legacy replay"),
        legacy
    );
    assert!(matches!(
        update(&store, &run_id, "progress-1", learning("new"), &[]).await,
        Err(StatefulRunStoreError::ObligationIdentityConflict(_))
    ));
}
