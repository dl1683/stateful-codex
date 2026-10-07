use pretty_assertions::assert_eq;

use super::releases;

#[test]
fn only_the_users_own_words_release_an_investigation() {
    assert_eq!(
        [
            "Great, we've agreed on the root cause. Go ahead and fix it.",
            "End this investigation, please.",
            "My colleague wrote: \"we have agreed on the root cause\".",
            "Have we agreed on the fix yet?",
            "Do NOT change any code until we have agreed on the root cause.",
            "Keep investigating.",
        ]
        .map(releases),
        [true, true, false, false, false, false]
    );
}

/// debug2: the ground rules apply in the thread that stated them and in a thread that
/// continues the investigation, not in another thread, and stop when the user ends it.
#[tokio::test]
async fn investigation_rules_follow_their_threads() {
    use codex_project_intelligence::RootBlackboardQuery;
    use codex_state::SqliteConfig;
    use codex_utils_absolute_path::test_support::PathExt;

    let state_home = tempfile::TempDir::new().expect("state home");
    let services = crate::services::ProjectIntelligenceServices::new(
        SqliteConfig::new_for_testing(state_home.path().abs()),
    );
    let message = "Please investigate. Some ground rules for this whole investigation:\n- Do NOT change any code until we have agreed on the root cause.\n- Never push.";
    let captured = crate::rule_group::capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        message,
    )
    .await;
    let store = services.blackboard().await.expect("store");
    let projection = store
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 16,
        })
        .await
        .expect("root");
    let applies_in = |view: &super::ScopeView| {
        let mut projection = projection.clone();
        view.retain_applicable(&mut projection);
        projection.data.len()
    };
    let opened = super::ScopeView::load(store, &projection, "thread-1").await;
    let other = super::ScopeView::load(store, &projection, "thread-2").await;
    super::observe_turn_start(
        store,
        "project-1",
        "thread-3",
        "turn-1",
        "Remind me what we ruled out.",
        crate::request_scope::RequestScope::Continuity,
    )
    .await;
    let continued = super::ScopeView::load(store, &projection, "thread-3").await;
    super::observe_turn_start(
        store,
        "project-1",
        "thread-3",
        "turn-2",
        "OK, we've agreed on the root cause.",
        crate::request_scope::RequestScope::Continuity,
    )
    .await;
    let ended = super::ScopeView::load(store, &projection, "thread-3").await;
    assert_eq!(
        (
            captured.len(),
            applies_in(&opened),
            applies_in(&other),
            applies_in(&continued),
            applies_in(&ended),
        ),
        (2, 2, 0, 2, 0)
    );
}
