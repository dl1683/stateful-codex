//! Every capsule-relevant eligibility writer advances the generation in its own transaction.
use super::context_bounds::tests::retire;
use super::knowledge::tests::change;
use super::knowledge::tests::rule;
use super::knowledge::tests::store;
use super::source_fixture::admission;
use super::source_fixture::observation;
use crate::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
const PROJECT: &str = "project-1";

async fn generation(store: &BlackboardStore) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(MAX(generation), 0) FROM capsule_eligibility_generations WHERE project_id = ?")
        .bind(PROJECT).fetch_one(&store.pool).await.unwrap()
}

fn advance(seen: &mut i64, label: &str, now: i64) {
    assert!(now > *seen, "{label}: {now} <= {seen}");
    *seen = now;
}

fn human_write(id: &str, text: &str) -> CaptureEntryWrite {
    CaptureEntryWrite {
        candidates: vec![BlackboardEntryId::parse(id).unwrap()],
        value: rule(text),
        context: KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect),
        change: ChangeRecord {
            origin: ChangeOrigin::DirectControl,
            ..change(ChangeOperation::Saved, text)
        },
    }
}

#[tokio::test]
async fn capsule_generation_advances_for_every_eligibility_writer() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let mut seen = generation(&store).await;

    let user = store
        .create_entry(
            BlackboardEntryId::parse("user").unwrap(),
            rule("Keep tabs."),
        )
        .await
        .unwrap();
    advance(&mut seen, "create_entry", generation(&store).await);
    store
        .record_context(
            &user,
            &KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect),
            /*change*/ None,
        )
        .await
        .unwrap();
    advance(&mut seen, "record_context", generation(&store).await);
    store
        .create_entry_with_context(
            BlackboardEntryId::parse("contextual").unwrap(),
            rule("Prefer small commits."),
            KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect),
            ChangeRecord {
                origin: ChangeOrigin::DirectControl,
                ..change(ChangeOperation::Saved, "Prefer small commits.")
            },
        )
        .await
        .unwrap();
    advance(
        &mut seen,
        "create_entry_with_context",
        generation(&store).await,
    );

    // Agent-only revision without a change record: the gap the journal cannot see.
    let mut note = rule("Agent cedar note.");
    note.kind = BlackboardKind::Note;
    note.provenance.kind = BlackboardProvenanceKind::Agent;
    let agent_id = BlackboardEntryId::parse("agent").unwrap();
    let agent = store
        .create_entry(agent_id.clone(), note.clone())
        .await
        .unwrap();
    advance(&mut seen, "create_entry agent", generation(&store).await);
    let mut revise = retire(&agent);
    revise.state = BlackboardEntryState::Active;
    revise.content = "Agent revised cedar note.".to_string();
    let agent = store
        .update_entry_from_model(PROJECT, &agent_id, revise)
        .await
        .unwrap();
    advance(
        &mut seen,
        "update_entry_from_model",
        generation(&store).await,
    );
    store
        .create_relation(
            BlackboardRelationId::parse("depends").unwrap(),
            NewBlackboardRelation {
                project_id: PROJECT.to_string(),
                from_entry_id: agent_id.clone(),
                to_entry_id: user.id.clone(),
                kind: BlackboardRelationKind::DependsOn,
                note: None,
                confidence: ConfidenceScore::from_basis_points(9000).unwrap(),
                provenance: note.provenance.clone(),
            },
        )
        .await
        .unwrap();
    advance(&mut seen, "create_relation", generation(&store).await);
    note.content = "Agent successor note.".to_string();
    store
        .create_successor_from_model(
            BlackboardEntryId::parse("successor").unwrap(),
            note,
            vec![SupersededEntry {
                id: agent_id,
                expected_revision: agent.revision,
            }],
        )
        .await
        .unwrap();
    advance(
        &mut seen,
        "create_successor_from_model",
        generation(&store).await,
    );

    // Observing a source is not an eligibility change; linking and forgetting it are.
    let text = "Keep tabs.";
    let seal = store
        .observe_source(observation("linked", text), text)
        .await
        .unwrap();
    assert_eq!(generation(&store).await, seen, "observe_source");
    let user = store.get_entry(PROJECT, &user.id).await.unwrap().unwrap();
    store
        .link_entry_source(
            &admission,
            &user.id,
            user.revision,
            &seal,
            &seal.observation.ordered_spans,
        )
        .await
        .unwrap();
    advance(&mut seen, "link_entry_source", generation(&store).await);
    store
        .update_entry_recorded(
            PROJECT,
            &user.id,
            retire(&user),
            Some(&change(ChangeOperation::Forgotten, "Keep tabs.")),
        )
        .await
        .unwrap();
    advance(
        &mut seen,
        "forget with source exclusion",
        generation(&store).await,
    );
    let excluded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM capture_source_exclusions")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(excluded, 1);

    store
        .write_capture(CaptureWrite {
            project_id: PROJECT.to_string(),
            units: vec![human_write("captured", "Run tests first.")],
        })
        .await
        .unwrap();
    advance(&mut seen, "write_capture", generation(&store).await);
    let text = "Never push.";
    let seal = store
        .observe_source(observation("grouped", text), text)
        .await
        .unwrap();
    let receipt = store
        .write_source_group(
            &admission,
            SourceCaptureGroup {
                project_id: PROJECT.to_string(),
                action_id: "group-action".to_string(),
                group_id: "group-1".to_string(),
                members: vec![SourceCaptureMember {
                    write: human_write("grouped", text),
                    spans: seal.observation.ordered_spans.clone(),
                    seal,
                }],
                existing: ExistingWording::Refuse,
            },
        )
        .await
        .unwrap();
    advance(&mut seen, "write_source_group", generation(&store).await);
    store
        .undo_capture_group(&admission, &receipt.group_id, "undo-group")
        .await
        .unwrap();
    advance(&mut seen, "undo_capture_group", generation(&store).await);

    // Scopes and identity coverage have no Rust writer here; host/maintenance SQL is covered.
    for statement in [
        "INSERT INTO knowledge_scopes VALUES ('project-1', 'scope-1', 'task', 'Task', 'open', NULL, 'source', NULL, 1, 1)",
        "INSERT INTO knowledge_scope_bindings VALUES ('project-1', 'thread-1', 'scope-1', 1)",
        "UPDATE knowledge_scopes SET state = 'ended', ended_source = 'source' WHERE scope_id = 'scope-1'",
        "DELETE FROM knowledge_scope_bindings WHERE thread_id = 'thread-1'",
        "INSERT INTO capture_identity_coverage(project_id, watermark) VALUES ('project-1', 1)",
    ] {
        sqlx::query(statement).execute(&store.pool).await.unwrap();
        advance(&mut seen, statement, generation(&store).await);
    }
    store.pool.close().await;
}
