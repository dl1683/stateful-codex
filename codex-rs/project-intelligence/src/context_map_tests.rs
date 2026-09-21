use pretty_assertions::assert_eq;

use super::*;
use crate::NewHierarchyNode;
use crate::ProjectRelativePath;

fn fingerprint(value: &str) -> SourceFingerprint {
    SourceFingerprint::parse(value).expect("valid fingerprint")
}

fn file_node(lifecycle: NodeLifecycle, source_fingerprint: Option<&str>) -> HierarchyNode {
    HierarchyNode {
        id: HierarchyNodeId::parse("node-file").expect("valid node ID"),
        value: NewHierarchyNode {
            project_id: "project-1".to_string(),
            parent_id: Some(HierarchyNodeId::parse("node-src").expect("valid node ID")),
            kind: NodeKind::File,
            project_root: Some("C:\\workspace".to_string()),
            relative_path: ProjectRelativePath::parse("src/lib.rs").expect("valid path"),
            region_anchor: None,
            source_fingerprint: source_fingerprint.map(fingerprint),
        },
        lifecycle,
        revision: 1,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

fn entry() -> ContextMapEntry {
    ContextMapEntry {
        id: ContextMapEntryId::parse("map-file").expect("valid entry ID"),
        value: NewContextMapEntry {
            project_id: "project-1".to_string(),
            node_id: HierarchyNodeId::parse("node-file").expect("valid node ID"),
            source_fingerprint: fingerprint("sha256:abc"),
            description: "Defines project-intelligence hierarchy types and validation.".to_string(),
            routing_terms: vec!["hierarchy".to_string(), "project intelligence".to_string()],
            coverage: ContextMapCoverage::Complete,
        },
        revision: 1,
        created_at_ms: 1,
        updated_at_ms: 1,
        last_verified_at_ms: None,
    }
}

#[test]
fn entries_are_bounded_routing_metadata() {
    let entry = entry();
    assert_eq!(entry.value.validate(), Ok(()));

    let mut duplicate_term = entry.value.clone();
    duplicate_term.routing_terms.push("hierarchy".to_string());
    assert_eq!(
        duplicate_term.validate(),
        Err(ContextMapError::InvalidRoutingTerm("hierarchy".to_string()))
    );

    let mut source_content = entry.value;
    source_content.description = "x".repeat(MAX_DESCRIPTION_BYTES + 1);
    assert_eq!(
        source_content.validate(),
        Err(ContextMapError::InvalidDescription)
    );
}

#[test]
fn freshness_is_derived_from_live_source_state() {
    let entry = entry();
    assert_eq!(
        entry.freshness_against(&file_node(NodeLifecycle::Active, Some("sha256:abc"))),
        Ok(ContextMapFreshness::Current)
    );
    assert_eq!(
        entry.freshness_against(&file_node(NodeLifecycle::Active, Some("sha256:def"))),
        Ok(ContextMapFreshness::Stale)
    );
    assert_eq!(
        entry.freshness_against(&file_node(NodeLifecycle::Replaced, Some("sha256:abc"))),
        Ok(ContextMapFreshness::Stale)
    );
    assert_eq!(
        entry.freshness_against(&file_node(NodeLifecycle::Missing, Some("sha256:abc"))),
        Ok(ContextMapFreshness::SourceUnavailable)
    );
}

#[test]
fn freshness_rejects_a_different_hierarchy_binding() {
    let entry = entry();
    let mut different_node = file_node(NodeLifecycle::Active, Some("sha256:abc"));
    different_node.id = HierarchyNodeId::parse("node-other").expect("valid node ID");
    assert_eq!(
        entry.freshness_against(&different_node),
        Err(ContextMapError::HierarchyBindingMismatch)
    );
}

#[test]
fn queries_enforce_the_result_bound() {
    let mut query = ContextMapQuery {
        project_id: "project-1".to_string(),
        text: "hierarchy".to_string(),
        max_results: MAX_QUERY_RESULTS,
    };
    assert_eq!(query.validate(), Ok(()));
    query.max_results += 1;
    assert_eq!(query.validate(), Err(ContextMapError::InvalidQuery));
}
