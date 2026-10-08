use super::*;
use codex_extension_api::PreviousWorldStateSection;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn c3r1_source_handle_redelivery_preserves_honest_bounded_lower_bound() {
    let home = TempDir::new().unwrap();
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let thread = "00000000-0000-0000-0000-000000000001";
    let project_id = crate::capture_test_support::thread_project(&sqlite, thread).await;
    let extension = StatefulExtension {
        projects: Arc::new(codex_thread_store::InMemoryThreadStore::default()),
        services: Some(crate::services::ProjectIntelligenceServices::new(sqlite)),
        event_sink: None,
        autonomous: None,
        attribution: Default::default(),
        visible_root: Default::default(),
        run_activity: Default::default(),
    };
    let thread_store = ExtensionData::new(thread);
    thread_store.insert(SelectedProject::new(&project_id));
    thread_store.insert(SelectedThread::new(thread));
    let turn_store = ExtensionData::new("turn-1");
    turn_store.insert(SourceTurn {
        turn_id: "turn-1".into(),
    });
    let messages: Vec<UserMessageItem> = (0..10)
        .map(|i| UserMessageItem {
            id: format!("original-{i}"),
            client_id: None,
            content: vec![UserInput::Text {
                text: format!("source-{i}"),
                text_elements: Vec::new(),
            }],
        })
        .collect();
    for message in &messages[..8] {
        extension
            .observe_original_item(&thread_store, &turn_store, message)
            .await;
    }
    let first = handle_section(turn_store.get::<SourceHandles>().as_deref())
        .unwrap()
        .1;
    extension
        .observe_original_item(&thread_store, &turn_store, &messages[0])
        .await;
    let replay = handle_section(turn_store.get::<SourceHandles>().as_deref())
        .unwrap()
        .1;
    assert_eq!(replay.snapshot(), first.snapshot());
    assert!(
        replay
            .render_diff(PreviousWorldStateSection::Known(first.snapshot()))
            .is_none()
    );
    extension
        .observe_original_item(&thread_store, &turn_store, &messages[8])
        .await;
    let (bytes, ninth) = handle_section(turn_store.get::<SourceHandles>().as_deref()).unwrap();
    assert_eq!(ninth.snapshot()["omittedIsExact"], false);
    assert!(
        ninth
            .render_diff(PreviousWorldStateSection::Known(first.snapshot()))
            .is_some()
    );
    assert!(bytes <= 899);
    for message in [&messages[0], &messages[8], &messages[9], &messages[9]] {
        extension
            .observe_original_item(&thread_store, &turn_store, message)
            .await;
        let next = handle_section(turn_store.get::<SourceHandles>().as_deref())
            .unwrap()
            .1;
        // The unknown tail stays a lower bound; replay cannot inflate a false total.
        assert_eq!(next.snapshot(), ninth.snapshot());
        assert_eq!(turn_store.get::<SourceHandles>().unwrap().handles.len(), 8);
    }
    let store = extension
        .services
        .as_ref()
        .unwrap()
        .blackboard()
        .await
        .unwrap();
    assert!(
        store
            .search_source_ranges(&project_id, "source-9", /*after*/ None)
            .await
            .unwrap()
            .ranges
            .iter()
            .any(|range| range.exact_text.contains("source-9"))
    );
}
