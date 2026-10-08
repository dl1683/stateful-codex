use super::super::context_bounds::tests::snapshot;
use super::super::knowledge::tests::change;
use super::super::knowledge::tests::rule;
use super::super::knowledge::tests::store;
use super::super::source_fixture::admission;
use super::super::source_fixture::observation;
use super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[tokio::test]
async fn c3_proposal_reader_quarantines_megabyte_legacy_metadata_before_materialization() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let id = BlackboardEntryId::parse("eligible-unscoped-note").unwrap();
    let mut value = rule("Short ordinary note.");
    value.kind = BlackboardKind::Note;
    value.provenance.kind = BlackboardProvenanceKind::Agent;
    store
        .create_entry_with_context(
            id.clone(),
            value,
            KnowledgeContext::new(
                KnowledgeCategory::Note,
                KnowledgeAuthority::AssistantReported,
            ),
            change(ChangeOperation::Saved, "note"),
        )
        .await
        .unwrap();
    for field in ["payload", "end_condition", "group_id", "scope_id"] {
        let large = if field == "payload" {
            serde_json::json!({"speaker":"x".repeat(1048576)}).to_string()
        } else {
            "x".repeat(1048576)
        };
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE knowledge_context SET {field} = ? WHERE entry_id = ?"
        )))
        .bind(&large)
        .bind(id.as_str())
        .execute(&store.pool)
        .await
        .unwrap();
        let before = snapshot(&store).await;
        assert!(matches!(
            store.proposal_context("project-1", &id).await,
            Err(BlackboardStoreError::UnsupportedContext)
        ));
        assert_eq!(snapshot(&store).await, before);
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE knowledge_context SET {field} = NULL WHERE entry_id = ?"
        )))
        .bind(id.as_str())
        .execute(&store.pool)
        .await
        .unwrap();
    }
    for (version, interpretation) in [(2, "summary".to_string()), (1, "x".repeat(513))] {
        let payload=serde_json::json!({"proposal":ProposalContext {
            version,status:ProposalStatus::Proposed,
            source:SourceProposal {
                source_id:"a".repeat(64),source_revision:1,digest:"b".repeat(64),part_index:0,
                spans:vec![SourceSpan {start_byte:0,end_byte:1,role:SourceSpanRole::Body}],
                category:ProposalCategory::Note,interpretation,dependency:None,temporal:None,event_status:None,attribution:None,speaker_span:None,
            },enclosure:SourceSpan {start_byte:0,end_byte:1,role:SourceSpanRole::Body},
        }}).to_string();
        sqlx::query("UPDATE knowledge_context SET payload = ? WHERE entry_id = ?")
            .bind(payload)
            .bind(id.as_str())
            .execute(&store.pool)
            .await
            .unwrap();
        let before = snapshot(&store).await;
        assert!(matches!(
            store.proposal_context("project-1", &id).await,
            Err(BlackboardStoreError::UnsupportedContext)
        ));
        assert_eq!(snapshot(&store).await, before);
    }
}

#[tokio::test]
async fn c3_group_fault_rolls_back_proposals_links_actions_and_journal_keeps_observation() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let text = "Mara bought a prototype. It was teal; the motor was not damaged.";
    let seal = store
        .observe_source(observation("fault", text), text)
        .await
        .unwrap();
    let request = vec![SourceProposal {
        source_id: seal.exact_source_locator.clone(),
        source_revision: 1,
        digest: seal.digest.clone(),
        part_index: 0,
        spans: vec![SourceSpan {
            start_byte: 0,
            end_byte: text.len() as u32,
            role: SourceSpanRole::Body,
        }],
        category: ProposalCategory::Background,
        interpretation: "Mara bought a prototype".into(),
        dependency: None,
        temporal: None,
        event_status: None,
        attribution: None,
        speaker_span: None,
    }];
    sqlx::query("CREATE TRIGGER fail_proposal_member BEFORE INSERT ON capture_group_members BEGIN SELECT RAISE(ABORT, 'injected member fault'); END").execute(&store.pool).await.unwrap();
    let before = snapshot(&store).await;
    assert!(
        store
            .propose_sources(
                &admission,
                HierarchyNodeId::parse("node-project").unwrap(),
                "turn-1",
                request.clone()
            )
            .await
            .is_err()
    );
    assert_eq!(snapshot(&store).await, before);
    assert_eq!(
        store
            .read_source_page(
                "project-1",
                &seal.exact_source_locator,
                &seal.digest,
                /*revision*/ 1,
                /*offset*/ 0
            )
            .await
            .unwrap()
            .exact_text,
        text
    );
    sqlx::query("DROP TRIGGER fail_proposal_member")
        .execute(&store.pool)
        .await
        .unwrap();
    let result = store
        .propose_sources(
            &admission,
            HierarchyNodeId::parse("node-project").unwrap(),
            "turn-1",
            request.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .propose_sources(
                &admission,
                HierarchyNodeId::parse("node-project").unwrap(),
                "turn-1",
                request
            )
            .await
            .unwrap(),
        result
    );
}
