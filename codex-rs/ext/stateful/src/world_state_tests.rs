use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionDataInit;
use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::PromptCacheAffinity;
use codex_project_intelligence::RootBlackboardProjection;
use codex_thread_store::StoredProject;
use codex_thread_store::StoredProjectRoot;
use pretty_assertions::assert_eq;

use super::MAX_BODY_BYTES;
use super::MAX_ESTIMATED_TOKENS;
use super::ProjectIntelligenceStatus;
use super::project_world_state_section;
use crate::SelectedProject;
use crate::root_blackboard::ResolvedRootBlackboard;
use crate::root_blackboard::RootBlackboardStatus;

fn project(name: &str, roots: Vec<StoredProjectRoot>) -> StoredProject {
    StoredProject {
        id: "project-1".to_string(),
        name: name.to_string(),
        roots,
        metadata: Default::default(),
        position: 0,
        created_at_ms: 1,
        updated_at_ms: 2,
        recency_at_ms: None,
    }
}

fn available(project: StoredProject) -> ProjectIntelligenceStatus {
    let project_id = project.id.clone();
    ProjectIntelligenceStatus::Available {
        project: Box::new(project),
        root_blackboard: Box::new(RootBlackboardStatus::Available(ResolvedRootBlackboard {
            projection: RootBlackboardProjection {
                project_id,
                revision: 0,
                data: Vec::new(),
                omitted_entries: 0,
            },
            evidence_routes: Default::default(),
        })),
    }
}

#[test]
fn renders_selected_project_as_bounded_typed_world_state() {
    let section = project_world_state_section(available(project(
        "Research\nProject",
        vec![StoredProjectRoot {
            path: "C:\\work\\research".to_string(),
        }],
    )));

    let rendered = section
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("new project state should render");

    assert_eq!(rendered.role(), "developer");
    assert_eq!(
        rendered.markers(),
        ("<stateful_project>", "</stateful_project>")
    );
    assert!(rendered.body().contains("Project ID: project-1"));
    assert!(rendered.body().contains("Project name: Research Project"));
    assert!(rendered.body().contains("C:\\work\\research"));
    assert!(rendered.body().contains("Project intelligence revision: 0"));
    assert!(
        rendered
            .body()
            .contains("verify only the smallest decisive source set")
    );
    assert!(rendered.body().contains("use evidence_read"));
    assert!(
        rendered
            .body()
            .contains("Do not query deeper state, search by every known filename")
    );
    assert!(
        rendered
            .body()
            .contains("do not persist cheap-to-recompute inventories")
    );
    assert!(rendered.body().contains("bounded batch tool"));
    assert!(
        rendered
            .body()
            .contains("No knowledge has been promoted to the root blackboard yet")
    );
    assert!(rendered.body().len() <= MAX_BODY_BYTES);
}

#[test]
fn unchanged_snapshot_does_not_repeat_project_context() {
    let section = project_world_state_section(available(project("Research", Vec::new())));
    let snapshot = section.snapshot().clone();

    assert!(
        section
            .render_diff(PreviousWorldStateSection::Known(&snapshot))
            .is_none()
    );
}

#[test]
fn long_project_metadata_is_truncated_on_utf8_boundaries() {
    let section = project_world_state_section(available(project(
        &"🙂".repeat(500),
        vec![StoredProjectRoot {
            path: "x".repeat(4_000),
        }],
    )));
    let rendered = section
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("new project state should render");

    assert!(rendered.body().len() <= MAX_BODY_BYTES);
    assert!(codex_utils_string::approx_token_count(rendered.body()) <= MAX_ESTIMATED_TOKENS);
    assert!(std::str::from_utf8(rendered.body().as_bytes()).is_ok());
}

#[test]
fn selected_project_keeps_cache_affinity_in_sync() {
    let mut init = ExtensionDataInit::new();
    SelectedProject::insert_initial(&mut init, "project-1");
    let data = ExtensionData::new_with_init("thread-1", init);

    assert_eq!(
        data.get::<PromptCacheAffinity>()
            .and_then(|affinity| affinity.key()),
        Some("stateful-project:project-1".to_string())
    );
    SelectedProject::insert(&data, "project-2");
    assert_eq!(
        data.get::<SelectedProject>()
            .map(|selected| selected.project_id().to_string()),
        Some("project-2".to_string())
    );
    assert_eq!(
        data.get::<PromptCacheAffinity>()
            .and_then(|affinity| affinity.key()),
        Some("stateful-project:project-2".to_string())
    );
    SelectedProject::remove(&data);
    assert_eq!(data.get::<SelectedProject>(), None);
    assert_eq!(
        data.get::<PromptCacheAffinity>()
            .and_then(|affinity| affinity.key()),
        None
    );
}
