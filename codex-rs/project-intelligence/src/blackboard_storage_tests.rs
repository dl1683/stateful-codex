use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;
use crate::BlackboardEntryScope;
use crate::BlackboardEntryUpdate;
use crate::BlackboardEvidenceDependentsQuery;
use crate::BlackboardEvidenceDependentsResult;
use crate::BlackboardEvidenceFreshness;
use crate::BlackboardHit;
use crate::BlackboardQuery;
use crate::BlackboardQueryResult;
use crate::BlackboardRelationId;
use crate::BlackboardRelationKind;
use crate::BlackboardRouteKnowledge;
use crate::BlackboardRouteKnowledgeQuery;
use crate::ContextMapCoverage;
use crate::ContextMapEntryUpdate;
use crate::ContextMapStore;
use crate::EvidenceLineRange;
use crate::HierarchyNodeId;
use crate::HierarchySourceUpdate;
use crate::HierarchyStore;
use crate::NewBlackboardRelation;
use crate::NewContextMapEntry;
use crate::NewHierarchyNode;
use crate::NodeKind;
use crate::NodeLifecycle;
use crate::ProjectRelativePath;
use crate::RootBlackboardQuery;

fn fingerprint(value: &str) -> SourceFingerprint {
    SourceFingerprint::parse(value).expect("valid fingerprint")
}

async fn fixture(temp_dir: &TempDir) -> (HierarchyStore, BlackboardStore, BlackboardEntry, u64) {
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    let hierarchy = HierarchyStore::open(&sqlite)
        .await
        .expect("hierarchy opens");
    let context_map = ContextMapStore::open(&sqlite)
        .await
        .expect("context map opens");
    let project_node = HierarchyNodeId::parse("node-project").expect("valid node ID");
    hierarchy
        .create_node(
            project_node.clone(),
            NewHierarchyNode {
                project_id: "project-1".to_string(),
                parent_id: None,
                kind: NodeKind::Project,
                project_root: None,
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await
        .expect("project node inserts");
    let root_node = HierarchyNodeId::parse("node-root").expect("valid node ID");
    hierarchy
        .create_node(
            root_node.clone(),
            NewHierarchyNode {
                project_id: "project-1".to_string(),
                parent_id: Some(project_node.clone()),
                kind: NodeKind::Directory,
                project_root: Some("C:\\workspace".to_string()),
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: Some(fingerprint("sha256:root")),
            },
        )
        .await
        .expect("root directory inserts");
    let file_id = HierarchyNodeId::parse("node-file").expect("valid node ID");
    let file = hierarchy
        .create_node(
            file_id.clone(),
            NewHierarchyNode {
                project_id: "project-1".to_string(),
                parent_id: Some(root_node),
                kind: NodeKind::File,
                project_root: Some("C:\\workspace".to_string()),
                relative_path: ProjectRelativePath::parse("README.md").expect("valid path"),
                region_anchor: None,
                source_fingerprint: Some(fingerprint("sha256:abc")),
            },
        )
        .await
        .expect("file node inserts");
    let map_id = ContextMapEntryId::parse("map-readme").expect("valid map ID");
    context_map
        .create_entry(
            map_id.clone(),
            NewContextMapEntry {
                project_id: "project-1".to_string(),
                node_id: file_id,
                source_fingerprint: fingerprint("sha256:abc"),
                description: "Project purpose and constraints.".to_string(),
                routing_terms: vec!["purpose".to_string()],
                coverage: ContextMapCoverage::Complete,
            },
        )
        .await
        .expect("context-map entry inserts");
    let blackboard = BlackboardStore::open(&sqlite)
        .await
        .expect("blackboard opens");
    let entry = blackboard
        .create_entry(
            BlackboardEntryId::parse("fact-purpose").expect("valid entry ID"),
            NewBlackboardEntry {
                project_id: "project-1".to_string(),
                node_id: project_node,
                kind: BlackboardKind::Fact,
                content: "The README defines the project purpose.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(9_000).expect("valid confidence"),
                verification: BlackboardVerification::SourceVerified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Promoted,
                evidence: vec![BlackboardEvidenceLink {
                    context_map_entry_id: map_id,
                    source_fingerprint: fingerprint("sha256:abc"),
                    line_range: Some(EvidenceLineRange { start: 2, end: 4 }),
                }],
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "turn-1".to_string(),
                },
            },
        )
        .await
        .expect("blackboard entry inserts");
    (hierarchy, blackboard, entry, file.revision)
}

#[tokio::test]
async fn blackboard_persistence_is_idempotent_and_rejects_stale_evidence() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let (hierarchy, blackboard, created, file_revision) = fixture(&temp_dir).await;
    let mut tail_value = created.value.clone();
    tail_value.content = "A second finding depends on the same source.".to_string();
    let tail = blackboard
        .create_entry(
            BlackboardEntryId::parse("fact-purpose-tail").expect("valid entry ID"),
            tail_value,
        )
        .await
        .expect("second source-linked entry inserts");
    assert_eq!(
        blackboard
            .create_entry(created.id.clone(), created.value.clone())
            .await
            .expect("identical retry returns existing entry"),
        created
    );
    let mut conflict = created.value.clone();
    conflict.content = "Conflicting reuse of a stable identity.".to_string();
    assert!(matches!(
        blackboard.create_entry(created.id.clone(), conflict).await,
        Err(BlackboardStoreError::EntryIdentityConflict(id)) if id == created.id.as_str()
    ));
    assert_eq!(
        blackboard
            .route_knowledge(BlackboardRouteKnowledgeQuery {
                project_id: "project-1".to_string(),
                context_map_entry_ids: vec![
                    ContextMapEntryId::parse("map-readme").expect("valid map ID"),
                ],
            })
            .await
            .expect("route knowledge loads"),
        vec![BlackboardRouteKnowledge {
            context_map_entry_id: ContextMapEntryId::parse("map-readme").expect("valid map ID"),
            active_entries: 2,
            root_entries: 2,
        }]
    );
    drop(blackboard);
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    let reopened = BlackboardStore::open(&sqlite)
        .await
        .expect("blackboard reopens");
    assert_eq!(
        reopened
            .get_entry("project-1", &created.id)
            .await
            .expect("entry loads"),
        Some(created.clone())
    );

    let file_id = HierarchyNodeId::parse("node-file").expect("valid node ID");
    hierarchy
        .update_source_state(
            "project-1",
            &file_id,
            HierarchySourceUpdate {
                expected_revision: file_revision,
                lifecycle: NodeLifecycle::Active,
                source_fingerprint: Some(fingerprint("sha256:def")),
            },
        )
        .await
        .expect("source fingerprint changes");
    let project_revision = hierarchy
        .project_intelligence_status("project-1")
        .await
        .expect("project intelligence status loads")
        .revision;
    assert_eq!(
        reopened
            .evidence_dependents(BlackboardEvidenceDependentsQuery {
                project_id: "project-1".to_string(),
                context_map_entry_ids: vec![
                    ContextMapEntryId::parse("map-readme").expect("valid map ID"),
                ],
                entry_scope: BlackboardEntryScope::Active,
                expected_project_revision: None,
                after_entry_id: None,
                max_results: 1,
            })
            .await
            .expect("changed-route dependents load"),
        BlackboardEvidenceDependentsResult {
            project_revision,
            data: vec![BlackboardHit::new(
                created.clone(),
                BlackboardEvidenceFreshness::Stale,
            )],
            truncated: true,
        }
    );
    assert_eq!(
        reopened
            .evidence_dependents(BlackboardEvidenceDependentsQuery {
                project_id: "project-1".to_string(),
                context_map_entry_ids: vec![
                    ContextMapEntryId::parse("map-readme").expect("valid map ID"),
                ],
                entry_scope: BlackboardEntryScope::Active,
                expected_project_revision: Some(project_revision),
                after_entry_id: Some(created.id.clone()),
                max_results: 1,
            })
            .await
            .expect("next changed-route dependent page loads"),
        BlackboardEvidenceDependentsResult {
            project_revision,
            data: vec![BlackboardHit::new(tail, BlackboardEvidenceFreshness::Stale,)],
            truncated: false,
        }
    );
    assert!(matches!(
        reopened
            .evidence_dependents(BlackboardEvidenceDependentsQuery {
                project_id: "project-1".to_string(),
                context_map_entry_ids: vec![
                    ContextMapEntryId::parse("missing-route").expect("valid map ID"),
                ],
                entry_scope: BlackboardEntryScope::Active,
                expected_project_revision: None,
                after_entry_id: None,
                max_results: 1,
            })
            .await,
        Err(BlackboardStoreError::EvidenceNotFound(id)) if id == "missing-route"
    ));
    assert!(matches!(
        reopened
            .evidence_dependents(BlackboardEvidenceDependentsQuery {
                project_id: "project-1".to_string(),
                context_map_entry_ids: vec![
                    ContextMapEntryId::parse("map-readme").expect("valid map ID"),
                ],
                entry_scope: BlackboardEntryScope::Active,
                expected_project_revision: Some(project_revision + 1),
                after_entry_id: Some(created.id.clone()),
                max_results: 1,
            })
            .await,
        Err(BlackboardStoreError::ProjectRevisionConflict { expected, actual })
            if expected == project_revision + 1 && actual == project_revision
    ));
    let mut stale = created.value;
    stale.content = "This claim still points to the old source revision.".to_string();
    assert!(matches!(
        reopened
            .create_entry(
                BlackboardEntryId::parse("fact-stale").expect("valid entry ID"),
                stale,
            )
            .await,
        Err(BlackboardStoreError::EvidenceNotCurrent)
    ));
}

#[tokio::test]
async fn guarded_updates_supersede_entries_without_rewriting_identity() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let (_hierarchy, blackboard, created, _file_revision) = fixture(&temp_dir).await;
    let successor_id = BlackboardEntryId::parse("fact-purpose-v2").expect("valid entry ID");
    let mut successor_value = created.value.clone();
    successor_value.content = "The project purpose has a newer interpretation.".to_string();
    successor_value.verification = BlackboardVerification::Unverified;
    successor_value.evidence.clear();
    successor_value.provenance.source_id = "turn-2".to_string();
    blackboard
        .create_entry(successor_id.clone(), successor_value)
        .await
        .expect("successor inserts");
    let update = BlackboardEntryUpdate {
        expected_revision: created.revision,
        kind: created.value.kind,
        content: created.value.content.clone(),
        structured_value: created.value.structured_value.clone(),
        confidence: created.value.confidence,
        verification: created.value.verification,
        importance: created.value.importance,
        root_promotion: RootPromotion::NotPromoted,
        evidence: created.value.evidence.clone(),
        state: BlackboardEntryState::Superseded,
        superseded_by: Some(successor_id.clone()),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::Agent,
            source_id: "turn-2".to_string(),
        },
    };
    let superseded = blackboard
        .update_entry("project-1", &created.id, update.clone())
        .await
        .expect("entry supersedes");
    assert_eq!(
        blackboard
            .query(BlackboardQuery {
                project_id: "project-1".to_string(),
                text: Some("README defines project purpose".to_string()),
                within_node: None,
                root_promotion: None,
                entry_scope: BlackboardEntryScope::Historical,
                max_results: 10,
            })
            .await
            .expect("historical query succeeds"),
        BlackboardQueryResult {
            data: vec![BlackboardHit::new(
                superseded.clone(),
                BlackboardEvidenceFreshness::Current,
            )],
            truncated: false,
        }
    );
    assert_eq!(
        (
            superseded.revision,
            superseded.state,
            superseded.superseded_by
        ),
        (
            created.revision + 1,
            BlackboardEntryState::Superseded,
            Some(successor_id)
        )
    );
    assert!(matches!(
        blackboard
            .update_entry("project-1", &created.id, update.clone())
            .await,
        Err(BlackboardStoreError::RevisionConflict { expected, actual })
            if expected == created.revision && actual == created.revision + 1
    ));
    let current_revision_update = BlackboardEntryUpdate {
        expected_revision: superseded.revision,
        ..update
    };
    assert!(matches!(
        blackboard
            .update_entry("project-1", &created.id, current_revision_update)
            .await,
        Err(BlackboardStoreError::EntryNotActive(id)) if id == created.id.as_str()
    ));
}

#[tokio::test]
async fn stale_evidence_can_be_demoted_superseded_and_retired_without_rewriting_history() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let (hierarchy, blackboard, created, file_revision) = fixture(&temp_dir).await;
    let retired_id = BlackboardEntryId::parse("fact-purpose-retired").expect("valid entry ID");
    let mut retired_value = created.value.clone();
    retired_value.content = "The old README also defined a retired constraint.".to_string();
    let retired = blackboard
        .create_entry(retired_id, retired_value)
        .await
        .expect("second historical entry inserts");

    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    let context_map = ContextMapStore::open(&sqlite)
        .await
        .expect("context map opens");
    let map_id = ContextMapEntryId::parse("map-readme").expect("valid map ID");
    let old_map = context_map
        .get_entry("project-1", &map_id)
        .await
        .expect("context map loads")
        .expect("context map exists");
    hierarchy
        .update_source_state(
            "project-1",
            &HierarchyNodeId::parse("node-file").expect("valid node ID"),
            HierarchySourceUpdate {
                expected_revision: file_revision,
                lifecycle: NodeLifecycle::Active,
                source_fingerprint: Some(fingerprint("sha256:def")),
            },
        )
        .await
        .expect("source fingerprint changes");
    context_map
        .update_entry(
            "project-1",
            &map_id,
            ContextMapEntryUpdate {
                expected_revision: old_map.revision,
                source_fingerprint: fingerprint("sha256:def"),
                description: "Revised project purpose and constraints.".to_string(),
                routing_terms: vec!["purpose".to_string()],
                coverage: ContextMapCoverage::Complete,
            },
        )
        .await
        .expect("context map refreshes");

    let historical_evidence = created.value.evidence.clone();
    let demoted = blackboard
        .update_entry(
            "project-1",
            &created.id,
            BlackboardEntryUpdate {
                expected_revision: created.revision,
                kind: created.value.kind,
                content: created.value.content.clone(),
                structured_value: created.value.structured_value.clone(),
                confidence: created.value.confidence,
                verification: BlackboardVerification::Stale,
                importance: created.value.importance,
                root_promotion: RootPromotion::NotPromoted,
                evidence: historical_evidence.clone(),
                state: BlackboardEntryState::Active,
                superseded_by: None,
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Maintenance,
                    source_id: "source-refresh".to_string(),
                },
            },
        )
        .await
        .expect("stale claim demotes without rewriting evidence");
    assert_eq!(demoted.value.evidence, historical_evidence);

    let successor_id = BlackboardEntryId::parse("fact-purpose-current").expect("valid entry ID");
    let mut successor_value = created.value.clone();
    successor_value.content = "The revised README defines the current purpose.".to_string();
    successor_value.evidence = vec![BlackboardEvidenceLink {
        context_map_entry_id: map_id,
        source_fingerprint: fingerprint("sha256:def"),
        line_range: Some(EvidenceLineRange { start: 3, end: 5 }),
    }];
    successor_value.provenance.source_id = "turn-current".to_string();
    blackboard
        .create_entry(successor_id.clone(), successor_value)
        .await
        .expect("current successor inserts");

    let superseded = blackboard
        .update_entry(
            "project-1",
            &created.id,
            BlackboardEntryUpdate {
                expected_revision: demoted.revision,
                kind: demoted.value.kind,
                content: demoted.value.content.clone(),
                structured_value: demoted.value.structured_value.clone(),
                confidence: demoted.value.confidence,
                verification: demoted.value.verification,
                importance: demoted.value.importance,
                root_promotion: demoted.value.root_promotion,
                evidence: demoted.value.evidence.clone(),
                state: BlackboardEntryState::Superseded,
                superseded_by: Some(successor_id.clone()),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "turn-current".to_string(),
                },
            },
        )
        .await
        .expect("stale claim supersedes");
    assert_eq!(
        (
            superseded.state,
            superseded.superseded_by,
            superseded.value.evidence
        ),
        (
            BlackboardEntryState::Superseded,
            Some(successor_id),
            historical_evidence.clone()
        )
    );

    let retired = blackboard
        .update_entry(
            "project-1",
            &retired.id,
            BlackboardEntryUpdate {
                expected_revision: retired.revision,
                kind: retired.value.kind,
                content: retired.value.content,
                structured_value: retired.value.structured_value,
                confidence: retired.value.confidence,
                verification: retired.value.verification,
                importance: retired.value.importance,
                root_promotion: RootPromotion::NotPromoted,
                evidence: retired.value.evidence,
                state: BlackboardEntryState::Tombstoned,
                superseded_by: None,
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Maintenance,
                    source_id: "source-refresh".to_string(),
                },
            },
        )
        .await
        .expect("stale claim retires");
    assert_eq!(
        (retired.state, retired.value.evidence),
        (BlackboardEntryState::Tombstoned, historical_evidence)
    );
}

#[tokio::test]
async fn root_projection_and_deeper_query_derive_live_evidence_state() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let (hierarchy, blackboard, created, file_revision) = fixture(&temp_dir).await;
    let initial = blackboard
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 10,
        })
        .await
        .expect("root projection loads");
    assert_eq!(
        initial.data,
        vec![BlackboardHit::new(
            created.clone(),
            BlackboardEvidenceFreshness::Current,
        )]
    );
    assert_eq!(initial.omitted_entries, 0);
    assert_eq!(initial.candidate_entries, 0);

    let mut deeper_value = created.value.clone();
    deeper_value.node_id = HierarchyNodeId::parse("node-file").expect("valid node ID");
    deeper_value.content = "A hidden deployment constraint affects the strategy.".to_string();
    deeper_value.verification = BlackboardVerification::Unverified;
    deeper_value.root_promotion = RootPromotion::Candidate;
    deeper_value.evidence.clear();
    deeper_value.provenance.source_id = "turn-3".to_string();
    let deeper = blackboard
        .create_entry(
            BlackboardEntryId::parse("note-deployment").expect("valid entry ID"),
            deeper_value,
        )
        .await
        .expect("deeper entry inserts");
    assert_eq!(
        blackboard
            .query(BlackboardQuery {
                project_id: "project-1".to_string(),
                text: Some("deployment constraint".to_string()),
                within_node: Some(HierarchyNodeId::parse("node-file").expect("valid node ID"),),
                root_promotion: Some(RootPromotion::Candidate),
                entry_scope: BlackboardEntryScope::Active,
                max_results: 10,
            })
            .await
            .expect("deeper query succeeds"),
        BlackboardQueryResult {
            data: vec![BlackboardHit::new(
                deeper,
                BlackboardEvidenceFreshness::NotApplicable,
            )],
            truncated: false,
        }
    );
    let before_source_change = blackboard
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 10,
        })
        .await
        .expect("root projection reloads");
    assert_eq!(before_source_change.data, initial.data);
    assert_eq!(before_source_change.candidate_entries, 1);
    assert!(before_source_change.revision > initial.revision);

    hierarchy
        .update_source_state(
            "project-1",
            &HierarchyNodeId::parse("node-file").expect("valid node ID"),
            HierarchySourceUpdate {
                expected_revision: file_revision,
                lifecycle: NodeLifecycle::Active,
                source_fingerprint: Some(fingerprint("sha256:def")),
            },
        )
        .await
        .expect("source changes");
    let stale = blackboard
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 10,
        })
        .await
        .expect("stale projection loads");
    assert_eq!(stale.data.len(), 1);
    assert_eq!(
        (
            stale.data[0].evidence_freshness,
            stale.data[0].effective_verification,
        ),
        (
            BlackboardEvidenceFreshness::Stale,
            BlackboardVerification::Stale,
        )
    );
    assert!(stale.revision > before_source_change.revision);
}

#[tokio::test]
async fn blackboard_relations_are_project_scoped_idempotent_and_queryable() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let (_hierarchy, blackboard, source, _file_revision) = fixture(&temp_dir).await;
    let mut target_value = source.value.clone();
    target_value.content =
        "A deployment constraint changes the implementation strategy.".to_string();
    target_value.verification = BlackboardVerification::Unverified;
    target_value.evidence.clear();
    target_value.provenance.source_id = "turn-2".to_string();
    let target = blackboard
        .create_entry(
            BlackboardEntryId::parse("strategy-constraint").expect("valid entry ID"),
            target_value,
        )
        .await
        .expect("target entry inserts");
    let relation = NewBlackboardRelation {
        project_id: "project-1".to_string(),
        from_entry_id: source.id.clone(),
        to_entry_id: target.id.clone(),
        kind: BlackboardRelationKind::Supports,
        note: Some("The documented purpose makes this strategy necessary.".to_string()),
        confidence: ConfidenceScore::from_basis_points(8_500).expect("valid confidence"),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::Maintenance,
            source_id: "maintenance-1".to_string(),
        },
    };
    let created = blackboard
        .create_relation(
            BlackboardRelationId::parse("purpose-supports-strategy").expect("valid relation ID"),
            relation.clone(),
        )
        .await
        .expect("relation inserts");
    assert_eq!(
        blackboard
            .create_relation(created.id.clone(), relation)
            .await
            .expect("identical relation is idempotent"),
        created
    );
    assert_eq!(
        blackboard
            .list_relations("project-1", &source.id, 10)
            .await
            .expect("relations load"),
        vec![created.clone()]
    );
    let hit = blackboard
        .query(BlackboardQuery {
            project_id: "project-1".to_string(),
            text: Some("implementation strategy".to_string()),
            within_node: None,
            root_promotion: None,
            entry_scope: BlackboardEntryScope::Active,
            max_results: 10,
        })
        .await
        .expect("related entry query succeeds");
    assert_eq!(hit.data[0].relations, vec![created]);
}
