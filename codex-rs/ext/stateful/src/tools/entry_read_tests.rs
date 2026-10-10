use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[tokio::test]
async fn exact_words_continue_within_serialized_budget_and_refuse_revision_drift() {
    let home = TempDir::new().expect("home");
    let services = crate::services::ProjectIntelligenceServices::new(
        SqliteConfig::new_for_testing(home.path().abs()),
    );
    let node = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    let words = "Whole reason: §3.2–§4 — α.\n\t".repeat(40);
    let words = words.trim();
    // An agent finding: the user's own memory never reaches exact model reads.
    let entry = store
        .create_entry(
            codex_project_intelligence::BlackboardEntryId::parse("exact").expect("id"),
            codex_project_intelligence::NewBlackboardEntry {
                project_id: "project-1".to_string(),
                node_id: node,
                kind: codex_project_intelligence::BlackboardKind::Decision,
                content: words.to_string(),
                structured_value: None,
                confidence: codex_project_intelligence::ConfidenceScore::from_basis_points(9_000)
                    .expect("confidence"),
                verification: codex_project_intelligence::BlackboardVerification::Unverified,
                importance: codex_project_intelligence::BlackboardImportance::High,
                root_promotion: codex_project_intelligence::RootPromotion::Promoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: codex_project_intelligence::BlackboardProvenance {
                    kind: codex_project_intelligence::BlackboardProvenanceKind::Agent,
                    source_id: "turn-1".to_string(),
                },
            },
        )
        .await
        .expect("record");
    for budget in [700, 1000, 2000] {
        let mut offset = 0;
        let mut recovered = String::new();
        loop {
            let output = super::read(
                store,
                "project-1",
                entry.id.to_string(),
                Some(entry.revision),
                offset,
                budget,
            )
            .await
            .expect("read");
            let raw = output.log_output();
            assert!(raw.len() <= budget);
            let page: serde_json::Value = serde_json::from_str(&raw).expect("json");
            recovered.push_str(page["content"].as_str().expect("words"));
            if page["complete"] == true {
                break;
            }
            let next = page["nextContentOffset"].as_u64().expect("next") as usize;
            assert!(next > offset && words.is_char_boundary(next));
            offset = next;
        }
        assert_eq!(recovered, words);
    }
    assert!(
        super::read(
            store,
            "project-1",
            entry.id.to_string(),
            Some(entry.revision),
            /*offset*/ 0,
            /*budget*/ 100
        )
        .await
        .is_err()
    );
    crate::memory_controls::forget_entry(
        store,
        &crate::memory_controls::MemoryActor::default(),
        "project-1",
        &entry.id,
        entry.revision,
    )
    .await
    .expect("forget");
    assert!(
        super::read(
            store,
            "project-1",
            entry.id.to_string(),
            Some(entry.revision),
            /*offset*/ 20,
            /*budget*/ 700
        )
        .await
        .is_err()
    );
}
