use codex_extension_api::PreviousWorldStateSection;
use codex_thread_store::StoredProject;
use codex_thread_store::StoredProjectRoot;
use pretty_assertions::assert_eq;

use super::MAX_BODY_BYTES;
use super::ProjectIntelligenceStatus;
use super::project_world_state_section;

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

#[test]
fn renders_selected_project_as_bounded_typed_world_state() {
    let section = project_world_state_section(ProjectIntelligenceStatus::Available(project(
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
    assert!(rendered.body().len() <= MAX_BODY_BYTES);
}

#[test]
fn unchanged_snapshot_does_not_repeat_project_context() {
    let section = project_world_state_section(ProjectIntelligenceStatus::Available(project(
        "Research",
        Vec::new(),
    )));
    let snapshot = section.snapshot().clone();

    assert!(
        section
            .render_diff(PreviousWorldStateSection::Known(&snapshot))
            .is_none()
    );
}

#[test]
fn long_project_metadata_is_truncated_on_utf8_boundaries() {
    let section = project_world_state_section(ProjectIntelligenceStatus::Available(project(
        &"🙂".repeat(500),
        vec![StoredProjectRoot {
            path: "x".repeat(4_000),
        }],
    )));
    let rendered = section
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("new project state should render");

    assert!(rendered.body().len() <= MAX_BODY_BYTES);
    assert!(std::str::from_utf8(rendered.body().as_bytes()).is_ok());
}
