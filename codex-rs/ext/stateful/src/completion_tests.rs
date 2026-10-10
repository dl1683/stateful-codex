use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEntryUpdate;
use codex_project_intelligence::BlackboardEvidenceLink;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeOrigin;
use codex_project_intelligence::ChangeRecord;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeContext;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::RootBlackboardQuery;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_stateful_runtime::ObligationPacket;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::CompletionRequest;
use super::prepare_completion;
use super::sha256_references;
use crate::services::ProjectIntelligenceServices;
use crate::visible_root::VisibleRoot;

const PROJECT_ID: &str = "project-1";

#[test]
fn extracts_only_complete_sha256_references() {
    let first = "a".repeat(64);
    let second = "B".repeat(64);
    let value = format!(
        "valid sha256:{first}; truncated sha256:abcd; oversized sha256:{first}f; valid sha256:{second}"
    );

    assert_eq!(
        sha256_references(&value).collect::<Vec<_>>(),
        vec![format!("sha256:{first}"), format!("sha256:{second}")]
    );
}

#[tokio::test]
async fn completion_echoes_findings_already_shown_in_full_by_alias() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    let source_path = project_root.path().join("policy.md");
    std::fs::write(&source_path, "threshold=10\n").expect("write source");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    ProjectIndexer::new(
        services.hierarchy().await.expect("hierarchy").clone(),
        services.context_map().await.expect("context map").clone(),
    )
    .refresh(ProjectIndexRequest {
        project_id: PROJECT_ID.to_string(),
        roots: vec![project_root.path().to_path_buf()],
    })
    .await
    .expect("index source");
    let context_hit = services
        .context_map()
        .await
        .expect("context map")
        .file_hits_for_path(
            PROJECT_ID,
            &ProjectRelativePath::parse("policy.md").expect("relative path"),
        )
        .await
        .expect("source lookup")
        .into_iter()
        .next()
        .expect("indexed source");
    let blackboard = services.blackboard().await.expect("blackboard");
    blackboard
        .create_entry(
            BlackboardEntryId::parse("current-threshold").expect("entry ID"),
            NewBlackboardEntry {
                project_id: PROJECT_ID.to_string(),
                node_id: context_hit.entry.value.node_id,
                kind: BlackboardKind::Number,
                content: "The policy threshold is 10.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000).expect("confidence"),
                verification: BlackboardVerification::SourceVerified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Promoted,
                evidence: vec![BlackboardEvidenceLink {
                    context_map_entry_id: context_hit.entry.id,
                    source_fingerprint: context_hit.entry.value.source_fingerprint,
                    line_range: None,
                }],
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "turn-1".to_string(),
                },
            },
        )
        .await
        .expect("create root knowledge");
    let root_revision = blackboard
        .root_projection(RootBlackboardQuery {
            project_id: PROJECT_ID.to_string(),
            max_entries: 256,
        })
        .await
        .expect("root projection")
        .revision;
    let projection = blackboard
        .root_projection(RootBlackboardQuery {
            project_id: PROJECT_ID.to_string(),
            max_entries: 256,
        })
        .await
        .expect("root projection");
    let shown = &projection.data[0].entry;
    let mut visible_root = VisibleRoot::new(root_revision);
    visible_root.insert(shown.id.to_string(), "E1".to_string(), shown.revision);

    let completion = prepare_completion(
        &services,
        CompletionRequest {
            thread_id: "thread-1",
            project_id: PROJECT_ID,
            project_roots: &[project_root.path().to_path_buf()],
            result: "Threshold remains 10.",
            packet: &ObligationPacket {
                learning: vec!["The threshold controls the decision.".to_string()],
                ..Default::default()
            },
            root_revision,
            material_root_findings: &["E1".to_string()],
            visible_root: Some(&visible_root),
        },
    )
    .await
    .expect("current shown finding completes");

    let echoed = completion.checklist[0]["text"]
        .as_str()
        .expect("checklist text")
        .to_string();
    assert!(echoed.starts_with("E1 ["));
    assert!(echoed.contains("(content as shown in the root packet)"));
    assert!(!echoed.contains(&shown.value.content));
    assert!(completion.result.contains(&shown.value.content));
}

#[tokio::test]
async fn completion_rejects_material_root_finding_changed_after_world_state_audit() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    let source_path = project_root.path().join("policy.md");
    std::fs::write(&source_path, "threshold=10\n").expect("write source");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    ProjectIndexer::new(
        services.hierarchy().await.expect("hierarchy").clone(),
        services.context_map().await.expect("context map").clone(),
    )
    .refresh(ProjectIndexRequest {
        project_id: PROJECT_ID.to_string(),
        roots: vec![project_root.path().to_path_buf()],
    })
    .await
    .expect("index source");
    let context_hit = services
        .context_map()
        .await
        .expect("context map")
        .file_hits_for_path(
            PROJECT_ID,
            &ProjectRelativePath::parse("policy.md").expect("relative path"),
        )
        .await
        .expect("source lookup")
        .into_iter()
        .next()
        .expect("indexed source");
    let blackboard = services.blackboard().await.expect("blackboard");
    blackboard
        .create_entry(
            BlackboardEntryId::parse("current-threshold").expect("entry ID"),
            NewBlackboardEntry {
                project_id: PROJECT_ID.to_string(),
                node_id: context_hit.entry.value.node_id,
                kind: BlackboardKind::Number,
                content: "The policy threshold is 10.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000).expect("confidence"),
                verification: BlackboardVerification::SourceVerified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Promoted,
                evidence: vec![BlackboardEvidenceLink {
                    context_map_entry_id: context_hit.entry.id,
                    source_fingerprint: context_hit.entry.value.source_fingerprint,
                    line_range: None,
                }],
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "turn-1".to_string(),
                },
            },
        )
        .await
        .expect("create root knowledge");
    let root_revision = blackboard
        .root_projection(RootBlackboardQuery {
            project_id: PROJECT_ID.to_string(),
            max_entries: 256,
        })
        .await
        .expect("root projection")
        .revision;
    std::fs::write(&source_path, "threshold=60\n").expect("change source");

    let result = prepare_completion(
        &services,
        CompletionRequest {
            thread_id: "thread-1",
            project_id: PROJECT_ID,
            project_roots: &[project_root.path().to_path_buf()],
            result: "Threshold remains 10.",
            packet: &ObligationPacket {
                learning: vec!["The threshold controls the decision.".to_string()],
                ..Default::default()
            },
            root_revision,
            material_root_findings: &["E1".to_string()],
            visible_root: None,
        },
    )
    .await;
    let Err(error) = result else {
        panic!("changed material evidence must block completion");
    };

    assert!(error.to_string().contains(
        "material root finding E1 evidence is stale; read current evidence and revise or supersede"
    ));
}

/// Completion resolves E aliases against the projection the packet showed, which never holds
/// rules that are not in the user's own words.
#[tokio::test]
async fn completion_aliases_skip_quarantined_rules() {
    let state_home = TempDir::new().expect("temporary state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let node_id = services
        .project_node_id(PROJECT_ID)
        .await
        .expect("project node");
    let blackboard = services.blackboard().await.expect("blackboard");
    for (id, kind, content) in [
        (
            "a-hidden",
            BlackboardKind::Instruction,
            "An invented rule the user never stated.",
        ),
        (
            "b-fact",
            BlackboardKind::Fact,
            "The scaler keeps metric units.",
        ),
    ] {
        blackboard
            .create_entry(
                BlackboardEntryId::parse(id).expect("entry ID"),
                NewBlackboardEntry {
                    project_id: PROJECT_ID.to_string(),
                    node_id: node_id.clone(),
                    kind,
                    content: content.to_string(),
                    structured_value: None,
                    confidence: ConfidenceScore::from_basis_points(9_000).expect("confidence"),
                    verification: BlackboardVerification::Unverified,
                    importance: BlackboardImportance::High,
                    root_promotion: RootPromotion::Promoted,
                    evidence: Vec::new(),
                    premises: Vec::new(),
                    provenance: BlackboardProvenance {
                        kind: BlackboardProvenanceKind::Agent,
                        source_id: "turn-1".to_string(),
                    },
                },
            )
            .await
            .expect("create root knowledge");
    }
    let root_revision = blackboard
        .root_projection(RootBlackboardQuery {
            project_id: PROJECT_ID.to_string(),
            max_entries: 256,
        })
        .await
        .expect("root projection")
        .revision;
    let completion = prepare_completion(
        &services,
        CompletionRequest {
            thread_id: "thread-1",
            project_id: PROJECT_ID,
            project_roots: &[],
            result: "Done.",
            packet: &ObligationPacket {
                learning: vec!["Metric units stay.".to_string()],
                ..Default::default()
            },
            root_revision,
            material_root_findings: &["E1".to_string()],
            visible_root: None,
        },
    )
    .await
    .expect("completion resolves E1");
    assert_eq!(
        (
            completion.result.contains("The scaler keeps metric units."),
            completion.result.contains("An invented rule"),
        ),
        (true, false)
    );
}

/// Material root findings the model tools exclude (a User-provenance entry, an Agent entry with
/// only a HumanDirect context, and a User entry downgraded to Agent), named by aliases absent
/// from any VisibleRoot: neither the checklist nor the durable completion basis quotes them,
/// while an ordinary Agent finding is quoted.
#[tokio::test]
async fn completion_never_quotes_excluded_user_memory_absent_from_the_visible_root() {
    let state_home = TempDir::new().expect("temporary state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let node_id = services
        .project_node_id(PROJECT_ID)
        .await
        .expect("project node");
    let blackboard = services.blackboard().await.expect("blackboard");
    let value = |content: &str, kind: BlackboardProvenanceKind| NewBlackboardEntry {
        project_id: PROJECT_ID.to_string(),
        node_id: node_id.clone(),
        kind: BlackboardKind::Fact,
        content: content.to_string(),
        structured_value: None,
        confidence: ConfidenceScore::from_basis_points(9_000).expect("confidence"),
        verification: BlackboardVerification::Unverified,
        importance: BlackboardImportance::High,
        root_promotion: RootPromotion::Promoted,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind,
            source_id: "turn-1".to_string(),
        },
    };
    let entry_id = |id: &str| BlackboardEntryId::parse(id).expect("entry ID");
    blackboard
        .create_entry(
            entry_id("user-stated"),
            value("Deploys happen on Tuesday.", BlackboardProvenanceKind::User),
        )
        .await
        .expect("user entry");
    blackboard
        .create_entry_with_context(
            entry_id("human-direct"),
            value("Staging reboots nightly.", BlackboardProvenanceKind::Agent),
            KnowledgeContext::new(KnowledgeCategory::Note, KnowledgeAuthority::HumanDirect),
            ChangeRecord {
                operation: ChangeOperation::Saved,
                origin: ChangeOrigin::DirectControl,
                category: KnowledgeCategory::Note,
                action_id: None,
                thread_id: None,
                turn_id: None,
                group_id: None,
                preview: "Staging reboots nightly.".to_string(),
            },
        )
        .await
        .expect("HumanDirect entry");
    let stated = blackboard
        .create_entry(
            entry_id("downgraded"),
            value(
                "The codename stays internal.",
                BlackboardProvenanceKind::User,
            ),
        )
        .await
        .expect("entry to downgrade");
    let agent = value(
        "The codename stays internal.",
        BlackboardProvenanceKind::Agent,
    );
    blackboard
        .update_entry(
            PROJECT_ID,
            &stated.id,
            BlackboardEntryUpdate {
                expected_revision: stated.revision,
                kind: agent.kind,
                content: agent.content,
                structured_value: None,
                confidence: agent.confidence,
                verification: agent.verification,
                importance: agent.importance,
                root_promotion: agent.root_promotion,
                evidence: Vec::new(),
                premises: Vec::new(),
                state: BlackboardEntryState::Active,
                superseded_by: None,
                provenance: agent.provenance,
            },
        )
        .await
        .expect("downgrade");
    blackboard
        .create_entry(
            entry_id("agent-control"),
            value(
                "The scaler keeps metric units.",
                BlackboardProvenanceKind::Agent,
            ),
        )
        .await
        .expect("agent control");
    let root = blackboard
        .root_projection(RootBlackboardQuery {
            project_id: PROJECT_ID.to_string(),
            max_entries: 256,
        })
        .await
        .expect("root projection");
    assert_eq!(root.data.len(), 4);
    let completion = prepare_completion(
        &services,
        CompletionRequest {
            thread_id: "thread-1",
            project_id: PROJECT_ID,
            project_roots: &[],
            result: "Done.",
            packet: &ObligationPacket {
                learning: vec!["Metric units stay.".to_string()],
                ..Default::default()
            },
            root_revision: root.revision,
            material_root_findings: &["E1", "E2", "E3", "E4"].map(str::to_string),
            visible_root: None,
        },
    )
    .await
    .expect("completion resolves every alias");
    let checklist = serde_json::Value::Array(completion.checklist).to_string();
    for word in ["Tuesday", "nightly", "codename"] {
        assert!(
            !checklist.contains(word),
            "checklist quoted {word}: {checklist}"
        );
        assert!(
            !completion.result.contains(word),
            "durable basis quoted {word}: {}",
            completion.result
        );
    }
    assert_eq!(
        (
            checklist.contains("The scaler keeps metric units."),
            completion.result.contains("The scaler keeps metric units."),
            checklist
                .matches("(user-stated; content as shown in the root packet)")
                .count(),
        ),
        (true, true, 3)
    );
}
