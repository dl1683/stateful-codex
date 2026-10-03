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
        .map(|text| releases(text, Some("until we have agreed on the root cause"))),
        [true, true, false, false, false, false]
    );
}

/// debug2: the ground rules apply in the thread that stated them and in a thread that
/// continues the investigation, not in another thread, and stop when the user ends it.
#[tokio::test]
async fn investigation_rules_follow_their_threads() {
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
    let applies_in = async |thread_id: &str| {
        super::applicable_projection(store, "project-1", thread_id)
            .await
            .expect("root")
            .0
            .data
            .len()
    };
    let opened = applies_in("thread-1").await;
    let other = applies_in("thread-2").await;
    super::observe_turn_start(
        store,
        "project-1",
        "thread-3",
        "turn-1",
        "Remind me what we ruled out.",
        crate::request_scope::RequestScope::Continuity,
    )
    .await;
    let continued = applies_in("thread-3").await;
    super::observe_turn_start(
        store,
        "project-1",
        "thread-3",
        "turn-2",
        "OK, we've agreed on the root cause.",
        crate::request_scope::RequestScope::Continuity,
    )
    .await;
    let ended = applies_in("thread-3").await;
    assert_eq!(
        (captured.len(), opened, other, continued, ended),
        (2, 2, 0, 2, 0)
    );
}

/// Item 1 review: conditional, negated, questioning, blockquoted and fenced mentions never
/// end an investigation; an agreement ends one only when that was its condition; a request
/// joins an investigation only when it refers to one.
#[test]
fn releases_are_affirmative_and_bindings_are_explicit() {
    let condition = Some("until we have agreed on the root cause");
    assert_eq!(
        (
            [
                "Do not end this investigation.",
                "If tomorrow we agree on the root cause, start the fix.",
                "Should we end this investigation?",
                "> End this investigation.",
                "```\nend this investigation\n```",
                "OK, we've agreed on the root cause.",
                "We agree on lunch.",
                "End this investigation if the test passes.",
                "We have agreed on the root cause when the test passes.",
                "The investigation is over.",
                "We haven't agreed on the root cause.",
                "Don\u{2019}t end this investigation.",
            ]
            .map(|text| releases(text, condition)),
            releases("We have agreed on the root cause.", Some("until I say so")),
            [
                "Format utils.py",
                "Remind me what we ruled out.",
                "Where are we on the investigation?",
            ]
            .map(super::refers_to_investigation),
        ),
        (
            [
                false, false, false, false, false, true, false, false, false, true, false, false
            ],
            false,
            [false, true, true],
        )
    );
}

/// Item 1 review: two prose rules for one investigation share its scope, so both apply in
/// the thread that stated them; a legacy rule naming an investigation waits for the user.
#[tokio::test]
async fn one_message_opens_one_investigation() {
    use codex_project_intelligence::RootBlackboardQuery;
    use codex_state::SqliteConfig;
    use codex_utils_absolute_path::test_support::PathExt;

    let state_home = tempfile::TempDir::new().expect("state home");
    let services = crate::services::ProjectIntelligenceServices::new(
        SqliteConfig::new_for_testing(state_home.path().abs()),
    );
    let message =
        "During this investigation, never change code. For this whole investigation, never push.";
    crate::rule_group::capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        message,
    )
    .await;
    let store = services.blackboard().await.expect("store");
    let (projection, view) = super::applicable_projection(store, "project-1", "thread-1")
        .await
        .expect("root");
    let open = store
        .scopes(
            "project-1",
            Some(codex_project_intelligence::ScopeState::Open),
        )
        .await
        .expect("scopes");
    let full = store
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 16,
        })
        .await
        .expect("root");
    assert_eq!(
        (
            projection.data.len(),
            view.inapplicable.len(),
            open.len(),
            full.data.len()
        ),
        (2, 0, 1, 2)
    );
}

/// Item 1 review 2: a later message adding a rule "during this investigation" in the same
/// thread adds it to the open investigation instead of replacing it, so every rule of the
/// investigation still applies there and nowhere else.
#[tokio::test]
async fn a_follow_up_rule_joins_the_open_investigation() {
    use codex_state::SqliteConfig;
    use codex_utils_absolute_path::test_support::PathExt;

    let state_home = tempfile::TempDir::new().expect("state home");
    let services = crate::services::ProjectIntelligenceServices::new(
        SqliteConfig::new_for_testing(state_home.path().abs()),
    );
    for (turn_id, message) in [
        (
            "turn-1",
            "Some ground rules for this whole investigation:\n- Do NOT change any code until we have agreed on the root cause.\n- Never push.",
        ),
        ("turn-2", "During this investigation, never delete logs."),
    ] {
        crate::rule_group::capture_marked_rules(
            &services,
            /*event_sink*/ None,
            "project-1",
            "thread-1",
            turn_id,
            message,
        )
        .await;
    }
    let store = services.blackboard().await.expect("store");
    let applies_in = async |thread_id: &str| {
        super::applicable_projection(store, "project-1", thread_id)
            .await
            .expect("root")
            .0
            .data
            .len()
    };
    let open = store
        .scopes(
            "project-1",
            Some(codex_project_intelligence::ScopeState::Open),
        )
        .await
        .expect("scopes");
    assert_eq!(
        (
            open.len(),
            applies_in("thread-1").await,
            applies_in("thread-2").await
        ),
        (1, 3, 0)
    );
}

/// Item 1 final review: the investigation's ending is recorded whichever of its rules states
/// it, so agreement releases it even when the first rule names no ending.
#[tokio::test]
async fn an_ending_stated_by_a_later_rule_is_recorded() {
    use codex_state::SqliteConfig;
    use codex_utils_absolute_path::test_support::PathExt;

    let state_home = tempfile::TempDir::new().expect("state home");
    let services = crate::services::ProjectIntelligenceServices::new(
        SqliteConfig::new_for_testing(state_home.path().abs()),
    );
    crate::rule_group::capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        "Some ground rules for this whole investigation:\n- Never push.\n- Do not change code until we agree on the root cause.",
    )
    .await;
    let store = services.blackboard().await.expect("store");
    let open = store
        .scopes(
            "project-1",
            Some(codex_project_intelligence::ScopeState::Open),
        )
        .await
        .expect("scopes");
    assert_eq!(
        open.iter()
            .map(|scope| scope.end_condition.clone())
            .collect::<Vec<_>>(),
        vec![Some("until we agree on the root cause".to_string())]
    );
}
