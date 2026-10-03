use pretty_assertions::assert_eq;

/// debug2: the ground rules apply in the thread that stated them and in a thread the user
/// joins to the investigation, not in another thread, and stop when the user ends it; words
/// alone ("we agreed on the root cause") neither join nor end it.
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
    let scope = store
        .thread_scope("project-1", "thread-1")
        .await
        .expect("binding")
        .expect("bound");
    store
        .bind_thread_scope("project-1", "thread-3", &scope.scope_id)
        .await
        .expect("join");
    let continued = applies_in("thread-3").await;
    store
        .end_scope(
            "project-1",
            &scope.scope_id,
            "direct-control:thread-3",
            &codex_project_intelligence::ChangeRecord {
                operation: codex_project_intelligence::ChangeOperation::ScopeEnded,
                origin: codex_project_intelligence::ChangeOrigin::DirectControl,
                category: codex_project_intelligence::KnowledgeCategory::Rule,
                action_id: None,
                thread_id: Some("thread-3".to_string()),
                turn_id: None,
                group_id: None,
                preview: scope.title.clone(),
            },
        )
        .await
        .expect("end");
    let ended = applies_in("thread-3").await;
    assert_eq!(
        (captured.len(), opened, other, continued, ended),
        (2, 2, 0, 2, 0)
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
            view.unscoped_legacy,
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
