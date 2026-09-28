use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionDataInit;
use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::PromptCacheAffinity;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEvidenceFreshness;
use codex_project_intelligence::BlackboardHit;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardPremiseLink;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::ProjectRefreshStatus;
use codex_project_intelligence::RootBlackboardProjection;
use codex_project_intelligence::RootPromotion;
use codex_thread_store::StoredProject;
use codex_thread_store::StoredProjectRoot;
use pretty_assertions::assert_eq;

use super::END_MARKER;
use super::MAX_BODY_BYTES;
use super::MAX_ESTIMATED_TOKENS;
use super::ProjectIntelligenceStatus;
use super::START_MARKER;
use super::project_world_state_section;
use super::semantic_fingerprint;
use crate::SelectedProject;
use crate::limits::MAX_MODEL_ITEM_BYTES;
use crate::root_blackboard::ResolvedRootBlackboard;
use crate::root_blackboard::RootBlackboardStatus;
use crate::visible_root::VisibleRoot;
use crate::visible_root::VisibleRootRegistry;

fn section(status: ProjectIntelligenceStatus) -> WorldStateSectionContribution {
    project_world_state_section(status, /*visible_root*/ None)
}

fn assert_fragment_bounded(fragment: &RenderedWorldStateFragment) {
    let (start, end) = fragment.markers();
    assert!(start.len() + fragment.body().len() + end.len() <= MAX_MODEL_ITEM_BYTES);
}

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
    available_at_revision(project, /*revision*/ 0, /*candidate_entries*/ 2)
}

fn available_at_revision(
    project: StoredProject,
    revision: u64,
    candidate_entries: u64,
) -> ProjectIntelligenceStatus {
    let project_id = project.id.clone();
    ProjectIntelligenceStatus::Available {
        project: Box::new(project),
        last_refresh: None,
        root_blackboard: Box::new(RootBlackboardStatus::Available(ResolvedRootBlackboard {
            projection: RootBlackboardProjection {
                project_id,
                revision,
                data: Vec::new(),
                omitted_entries: 0,
                candidate_entries,
            },
            evidence_routes: Default::default(),
            evidence_audit: None,
        })),
    }
}

#[test]
fn incomplete_refresh_health_is_visible_and_changes_project_context() {
    let base_project = project("Research", Vec::new());
    let previous = section(available(base_project.clone()));
    let mut current = available(base_project);
    let ProjectIntelligenceStatus::Available { last_refresh, .. } = &mut current else {
        unreachable!("test status should be available");
    };
    *last_refresh = Some(ProjectRefreshStatus {
        inventory_complete: false,
        region_coverage_complete: true,
        files_indexed: 3,
        regions_indexed: 9,
        files_skipped: 1,
        missing_files: 0,
        truncated: true,
        scan_duration_ms: 12,
        publication_duration_ms: 34,
        completed_at_ms: 56,
    });
    let current = section(current);

    let rendered = current
        .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
        .expect("changed refresh health must be visible");

    // Status lines are replaced as one block, so no stale health line survives.
    assert_eq!(
        rendered.markers(),
        ("<stateful_project_update>", "</stateful_project_update>")
    );
    assert!(
        rendered
            .body()
            .contains("Current status lines (replace every earlier status line):")
    );
    assert!(rendered.body().contains("inventoryComplete=false"));
    assert!(rendered.body().contains("filesSkipped=1"));
    assert!(
        rendered
            .body()
            .contains("Do not infer that an unindexed file is absent")
    );
}

#[test]
fn renders_selected_project_as_bounded_typed_world_state() {
    let section = section(available(project(
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
            .contains("the host rechecked that the cited source bytes still match")
    );
    assert!(
        rendered
            .body()
            .contains("It did not prove that those bytes entail the entry")
    );
    assert!(
        rendered
            .body()
            .contains("Reuse these entries without routine rereading")
    );
    assert!(rendered.body().contains(
        "compare only those candidates against the requested scope and evidence endpoint"
    ));
    assert!(
        rendered
            .body()
            .contains("do not turn one criterion or decision dimension")
    );
    assert!(
        rendered
            .body()
            .contains("search by every known filename, or reread the corpus")
    );
    assert!(
        rendered
            .body()
            .contains("do not persist cheap-to-recompute inventories")
    );
    assert!(
        rendered
            .body()
            .contains("Preserve decision-changing contrasts, exact values, qualifiers")
    );
    assert!(!rendered.body().contains("established premises"));
    assert!(rendered.body().contains("bounded batch tool"));
    assert!(
        rendered
            .body()
            .contains("No knowledge has been promoted to the root blackboard yet")
    );
    assert!(
        rendered
            .body()
            .contains("2 active candidate entries await an explicit project-relevance decision")
    );
    assert!(
        rendered
            .body()
            .contains("select at most 8 highest-priority E aliases")
    );
    assert!(
        rendered
            .body()
            .contains("rootRevision is not expectedRevision")
    );
    assert!(rendered.body().len() <= MAX_BODY_BYTES);
    assert_fragment_bounded(&rendered);
}

#[test]
fn unchanged_snapshot_does_not_repeat_project_context() {
    let section = section(available(project("Research", Vec::new())));
    let snapshot = section.snapshot().clone();

    assert!(
        section
            .render_diff(PreviousWorldStateSection::Known(&snapshot))
            .is_none()
    );
}

#[test]
fn invisible_project_metadata_change_does_not_repeat_project_context() {
    let previous_project = project("Research", Vec::new());
    let mut current_project = previous_project.clone();
    current_project.updated_at_ms += 1;
    let previous = section(available(previous_project));
    let current = section(available(current_project));

    assert!(
        current
            .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
            .is_none()
    );
}

#[test]
fn unknown_legacy_snapshot_renders_the_full_current_packet() {
    let current = section(available(project("Research", Vec::new())));

    let rendered = current
        .render_diff(PreviousWorldStateSection::Unknown)
        .expect("unknown legacy state must be replaced with current project intelligence");

    assert_eq!(
        rendered.markers(),
        ("<stateful_project>", "</stateful_project>")
    );
    assert!(rendered.body().contains("Project intelligence revision: 0"));
}

#[test]
fn unchecked_audit_label_changes_semantic_fingerprint() {
    let audited = "verification=userConfirmed evidence=current S1 (current)";
    let unaudited = "verification=userConfirmed evidence=uncheckedThisTurn S1 (uncheckedThisTurn)";
    let stale = "verification=stale evidence=stale S1 (stale)";

    assert_ne!(
        semantic_fingerprint(audited),
        semantic_fingerprint(unaudited)
    );
    assert_ne!(semantic_fingerprint(audited), semantic_fingerprint(stale));
}

#[test]
fn revision_only_change_renders_a_compact_update() {
    let previous = section(available_at_revision(
        project("Research", Vec::new()),
        /*revision*/ 7,
        /*candidate_entries*/ 2,
    ));
    let current = section(available_at_revision(
        project("Research", Vec::new()),
        /*revision*/ 11,
        /*candidate_entries*/ 2,
    ));

    let rendered = current
        .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
        .expect("the completion revision must be updated");

    assert_eq!(
        rendered.markers(),
        ("<stateful_project_update>", "</stateful_project_update>")
    );
    assert_eq!(
        rendered.body(),
        "Project intelligence revision advanced from 7 to 11. The model-visible root blackboard knowledge and source routes are unchanged. Use rootRevision 11 for completion; retain the existing root packet for reasoning and routing."
    );
    assert!(!rendered.body().contains("Project roots:"));
    assert_fragment_bounded(&rendered);
}

#[test]
fn hard_bound_uses_whole_lines_and_truthful_omission() {
    let dense_line = "\x01\x02\x03\t".repeat(MAX_BODY_BYTES);
    let mut output = String::new();
    super::append_line(&mut output, &dense_line);
    assert_eq!(output, super::OMISSION_MARKER);

    let mut output = String::new();
    let first_line = "A".repeat(MAX_BODY_BYTES - super::OMISSION_MARKER.len() - 2);
    assert!(super::try_append_line(
        &mut output,
        &first_line,
        /*reserved_bytes*/ 0
    ));
    assert!(!super::try_append_line(
        &mut output,
        "tail",
        /*reserved_bytes*/ 0
    ));
    assert!(output.len() < MAX_BODY_BYTES);

    let entries = (0..8)
        .map(|index| hit(&format!("entry-dense-{index}"), &dense_line))
        .collect();
    let rendered = section(with_entries(/*revision*/ 3, entries))
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("dense project state should render");
    assert_fragment_bounded(&rendered);
    assert!(rendered.body().contains("omitted by the context bound"));
}

#[test]
fn unexpected_fragment_overflow_clears_visible_root() {
    let registry = VisibleRootRegistry::default();
    registry.record("thread-1", VisibleRoot::new(/*project_revision*/ 3));
    let visible_root = (registry.clone(), "thread-1".to_string());
    let rendered = super::final_fragment(
        "developer",
        (START_MARKER, END_MARKER),
        "x".repeat(MAX_MODEL_ITEM_BYTES),
        "project-1",
        Some(&visible_root),
        super::RegistryUpdate::Record(None),
    );
    assert_fragment_bounded(&rendered);
    assert!(rendered.body().contains("exceeded its hard byte bound"));
    assert_eq!(registry.get("thread-1"), None);
}

#[test]
fn rewritten_status_line_replaces_the_status_block() {
    let previous = section(available_at_revision(
        project("Research", Vec::new()),
        /*revision*/ 7,
        /*candidate_entries*/ 2,
    ));
    let current = section(available_at_revision(
        project("Research", Vec::new()),
        /*revision*/ 11,
        /*candidate_entries*/ 3,
    ));

    let rendered = current
        .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
        .expect("changed root knowledge must render");

    assert_eq!(
        rendered.markers(),
        ("<stateful_project_update>", "</stateful_project_update>")
    );
    assert!(
        rendered
            .body()
            .contains("Current status lines (replace every earlier status line):")
    );
    assert!(!rendered.body().contains("Project roots:"));
    assert!(
        rendered
            .body()
            .contains("3 active candidate entries await an explicit project-relevance decision")
    );
}

#[test]
fn promoted_entry_change_sends_only_new_lines_and_alias_moves() {
    let unchanged = hit("entry-b", "The approval threshold is 10.");
    let previous = section(with_entries(
        /*revision*/ 3,
        vec![
            unchanged.clone(),
            hit("entry-c", "Rollback runs on staging."),
        ],
    ));
    let current = section(with_entries(
        /*revision*/ 4,
        vec![
            hit("entry-a", "The permit UTH-0441 was never transferred."),
            unchanged,
        ],
    ));

    let rendered = current
        .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
        .expect("root change must render");

    assert_eq!(
        rendered.markers(),
        ("<stateful_project_update>", "</stateful_project_update>")
    );
    let body = rendered.body();
    assert_fragment_bounded(&rendered);
    assert!(body.contains("revision advanced from 3 to 4"));
    assert!(body.contains("The permit UTH-0441 was never transferred."));
    assert!(body.contains("E alias changes (same content, renumbered): E1->E2"));
    assert!(body.contains("former E2"));
    assert!(!body.contains("The approval threshold is 10."));
}

#[test]
fn snapshot_without_root_layout_falls_back_to_the_full_packet() {
    let previous = section(with_entries(
        /*revision*/ 3,
        vec![hit("entry-b", "The approval threshold is 10.")],
    ));
    let mut legacy = previous.snapshot().clone();
    let legacy_object = legacy.as_object_mut().expect("snapshot is an object");
    legacy_object.remove("rootEntries");
    let current = section(with_entries(
        /*revision*/ 4,
        vec![hit("entry-a", "A new decisive fact.")],
    ));

    let rendered = current
        .render_diff(PreviousWorldStateSection::Known(&legacy))
        .expect("root change must render");

    assert_eq!(
        rendered.markers(),
        ("<stateful_project>", "</stateful_project>")
    );
}

#[test]
fn wholesale_root_change_renders_the_full_packet() {
    let previous = section(with_entries(
        /*revision*/ 3,
        vec![hit("entry-a", "Old fact.")],
    ));
    let replacement = (0..6)
        .map(|index| {
            hit(
                &format!("entry-new-{index}"),
                &"decisive detail ".repeat(60),
            )
        })
        .collect();
    let current = section(with_entries(/*revision*/ 4, replacement));

    let rendered = current
        .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
        .expect("root change must render");

    assert_eq!(
        rendered.markers(),
        ("<stateful_project>", "</stateful_project>")
    );
}

fn hit(id: &str, content: &str) -> BlackboardHit {
    BlackboardHit::new(
        BlackboardEntry {
            id: BlackboardEntryId::parse(id).expect("valid entry ID"),
            value: NewBlackboardEntry {
                project_id: "project-1".to_string(),
                node_id: HierarchyNodeId::parse("node-root").expect("valid node ID"),
                kind: BlackboardKind::Fact,
                content: content.to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(9_000).expect("valid confidence"),
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Promoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "test".to_string(),
                },
            },
            state: BlackboardEntryState::Active,
            superseded_by: None,
            revision: 1,
            created_at_ms: 1,
            updated_at_ms: 1,
        },
        BlackboardEvidenceFreshness::NotApplicable,
    )
}

fn with_entries(revision: u64, data: Vec<BlackboardHit>) -> ProjectIntelligenceStatus {
    let mut status = available_at_revision(
        project("Research", Vec::new()),
        revision,
        /*candidate_entries*/ 0,
    );
    let ProjectIntelligenceStatus::Available {
        root_blackboard, ..
    } = &mut status
    else {
        unreachable!("test status should be available");
    };
    let RootBlackboardStatus::Available(root) = root_blackboard.as_mut() else {
        unreachable!("test root should be available");
    };
    root.projection.data = data;
    status
}

#[test]
fn long_project_metadata_is_truncated_on_utf8_boundaries() {
    let section = section(available(project(
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

#[test]
fn visible_root_tracks_only_what_the_model_holds_in_full() {
    let registry = VisibleRootRegistry::default();
    let tracked = |status| {
        project_world_state_section(status, Some((registry.clone(), "thread-1".to_string())))
    };
    let mut derived = hit("entry-derived", "The launch is blocked by the permit.");
    derived.entry.value.premises = vec![BlackboardPremiseLink {
        entry_id: BlackboardEntryId::parse("entry-deeper").expect("valid entry ID"),
        revision: 1,
    }];
    let first = tracked(with_entries(
        /*revision*/ 3,
        vec![hit("entry-a", "The permit was never transferred."), derived],
    ));
    first.render_diff(PreviousWorldStateSection::Absent);
    let shown = registry
        .get("thread-1")
        .expect("full render records the root");
    assert_eq!(
        (
            shown.alias_for("entry-a", /*revision*/ 1),
            shown.alias_for("entry-derived", /*revision*/ 1),
        ),
        (Some("E1"), None)
    );

    let same = tracked(with_entries(
        /*revision*/ 4,
        vec![hit("entry-a", "The permit was never transferred.")],
    ));
    same.render_diff(PreviousWorldStateSection::Known(first.snapshot()));
    assert_eq!(registry.get("thread-1"), None);

    let full = tracked(with_entries(
        /*revision*/ 5,
        vec![hit("entry-a", "The permit was never transferred.")],
    ));
    full.render_diff(PreviousWorldStateSection::Absent);
    let advanced = tracked(with_entries(
        /*revision*/ 6,
        vec![hit("entry-a", "The permit was never transferred.")],
    ));
    advanced.render_diff(PreviousWorldStateSection::Known(full.snapshot()));
    assert_eq!(
        registry.get("thread-1").map(|shown| shown.project_revision),
        Some(6)
    );
}

#[test]
fn premises_certify_only_when_rendered_at_their_pinned_revision() {
    let registry = VisibleRootRegistry::default();
    let pinned = |id: &str, entry_id: &str, revision: u64| {
        let mut derived = hit(id, "The launch depends on an earlier finding.");
        derived.entry.value.premises = vec![BlackboardPremiseLink {
            entry_id: BlackboardEntryId::parse(entry_id).expect("valid entry ID"),
            revision,
        }];
        derived
    };
    let mut current = hit("entry-a", "The permit was never transferred.");
    current.entry.revision = 2;
    let fillers = (0..12)
        .map(|index| hit(&format!("entry-filler-{index}"), &"x".repeat(2_800)))
        .collect::<Vec<_>>();
    let mut data = vec![
        current,
        pinned("entry-exact", "entry-a", /*revision*/ 2),
        pinned("entry-stale", "entry-a", /*revision*/ 1),
        pinned("entry-over-budget", "entry-late", /*revision*/ 1),
    ];
    data.extend(fillers);
    // Same size as the fillers, so once one filler no longer fits neither does this.
    data.push(hit("entry-late", &"x".repeat(2_800)));
    project_world_state_section(
        with_entries(/*revision*/ 3, data),
        Some((registry.clone(), "thread-1".to_string())),
    )
    .render_diff(PreviousWorldStateSection::Absent);

    let shown = registry
        .get("thread-1")
        .expect("full render records the root");
    assert_eq!(
        [
            shown.alias_for("entry-exact", /*revision*/ 1),
            shown.alias_for("entry-stale", /*revision*/ 1),
            shown.alias_for("entry-over-budget", /*revision*/ 1),
            shown.alias_for("entry-late", /*revision*/ 1),
        ],
        [Some("E2"), None, None, None]
    );
}

#[test]
fn edited_tail_of_a_long_entry_is_sent_as_a_change() {
    let long_id = format!("entry-{}", "x".repeat(400));
    let entry = |tail: &str, revision: u64| {
        let mut hit = hit(&long_id, &format!("{}{tail}", "A".repeat(2_790)));
        hit.entry.revision = revision;
        hit
    };
    let previous = section(with_entries(
        /*revision*/ 3,
        vec![entry("TAIL-OLD", 1)],
    ));
    let current = section(with_entries(
        /*revision*/ 4,
        vec![entry("TAIL-NEW", 2)],
    ));

    let rendered = current
        .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
        .expect("an edited entry must render");

    assert!(rendered.body().contains("TAIL-NEW"));
}

#[test]
fn non_content_semantic_changes_reach_the_model_as_changes() {
    let base = || hit("entry-a", "The approval threshold applies.");
    let mut verified = base();
    verified.entry.value.verification = BlackboardVerification::Disputed;
    let mut valued = base();
    valued.entry.value.structured_value =
        Some(codex_project_intelligence::BlackboardStructuredValue {
            value: "6".to_string(),
            unit: Some("members".to_string()),
        });
    let pinned = |revision: u64| {
        let mut pinned = base();
        pinned.entry.value.premises = vec![BlackboardPremiseLink {
            entry_id: BlackboardEntryId::parse("entry-deeper").expect("valid entry ID"),
            revision,
        }];
        pinned
    };
    let previous = section(with_entries(/*revision*/ 3, vec![base()]));
    let previous_pin = section(with_entries(/*revision*/ 3, vec![pinned(1)]));

    for (label, previous, changed, expected) in [
        ("verification", &previous, verified, "verification=disputed"),
        ("structured value", &previous, valued, "value=6 members"),
        ("premise pin", &previous_pin, pinned(2), "@r2"),
    ] {
        let rendered = section(with_entries(/*revision*/ 4, vec![changed]))
            .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
            .unwrap_or_else(|| panic!("a {label} change must render"));
        assert!(
            rendered.body().contains(expected),
            "{label}: {}",
            rendered.body()
        );
    }
}
