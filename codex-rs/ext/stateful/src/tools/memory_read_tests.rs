use pretty_assertions::assert_eq;

use super::excerpt;
use super::mentions_any;
use super::parse_utc_day;
use super::question_terms;

#[test]
fn question_terms_keep_content_words_only() {
    assert_eq!(
        question_terms(
            "Are typo hints on or off by default, how does an app turn them on, and why?"
        ),
        vec![
            "typo".to_string(),
            "hints".to_string(),
            "default".to_string(),
            "app".to_string(),
            "turn".to_string(),
        ]
    );
    assert_eq!(question_terms("why is it on?"), Vec::<String>::new());
}

/// The horizon2 S17 question keeps every topic, including the short symbols it asks about.
#[test]
fn a_long_multi_topic_question_keeps_its_later_topics() {
    let terms = question_terms(
        "A reviewer asked two things and I want to answer from what we actually decided, not from scratch: why is the overdue sign opt-in instead of always on, and why do months and years use \"mth\" and \"yr\"? Also, a new teammate is joining: list the standing rules I gave you when we started working on this fork, word for word if you can.",
    );
    assert_eq!(
        [
            "overdue", "sign", "months", "years", "mth", "yr", "standing", "rules"
        ]
        .map(|term| terms.contains(&term.to_string())),
        [true; 8]
    );
}

#[test]
fn matching_and_excerpts_follow_the_question() {
    let text = format!(
        "{}Typo hints are opt-in because downstream snapshot tests were breaking.{}",
        "Background. ".repeat(40),
        " More text.".repeat(40)
    );
    let terms = question_terms("why are typo hints opt-in");
    let cut = excerpt(&text, &terms, 160);
    assert!(cut.len() <= 160, "{}", cut.len());
    assert!(cut.starts_with("...") && cut.ends_with("..."));
    assert!(cut.contains("Typo hints are opt-in"));
    assert_eq!(
        (
            mentions_any("Made typo_hints opt-in", &terms),
            mentions_any("Unrelated change", &terms),
            excerpt("short", &terms, 160),
        ),
        (true, false, "short".to_string())
    );
}

#[test]
fn since_is_a_utc_day() {
    assert_eq!(
        (
            parse_utc_day("1970-01-02"),
            parse_utc_day("2026-10-02"),
            parse_utc_day("2026-13-01").is_err(),
            parse_utc_day("yesterday").is_err(),
        ),
        (Ok(86_400_000), Ok(1_790_899_200_000), true, true)
    );
}

#[test]
fn since_rejects_impossible_dates_and_years() {
    assert_eq!(
        (
            parse_utc_day("2026-02-31").is_err(),
            parse_utc_day("2024-02-29").is_ok(),
            parse_utc_day("2023-02-29").is_err(),
            parse_utc_day("99999999999-01-01").is_err(),
            parse_utc_day("1969-12-31").is_err(),
        ),
        (true, true, true, true, true)
    );
}

#[test]
fn excerpts_stay_centred_when_lowercasing_changes_byte_lengths() {
    let text = format!(
        "\u{130}{}The decision: hints are opt-in.{}",
        " background".repeat(60),
        " tail".repeat(60)
    );
    let cut = excerpt(&text, &["decision".to_string()], 120);
    assert!(cut.contains("The decision: hints are opt-in."), "{cut}");
}

fn decision(
    node_id: &codex_project_intelligence::HierarchyNodeId,
    content: &str,
) -> codex_project_intelligence::NewBlackboardEntry {
    codex_project_intelligence::NewBlackboardEntry {
        project_id: "project-1".to_string(),
        node_id: node_id.clone(),
        kind: codex_project_intelligence::BlackboardKind::Decision,
        content: content.to_string(),
        structured_value: None,
        confidence: codex_project_intelligence::ConfidenceScore::from_basis_points(9_000)
            .expect("confidence"),
        verification: codex_project_intelligence::BlackboardVerification::Unverified,
        importance: codex_project_intelligence::BlackboardImportance::High,
        root_promotion: codex_project_intelligence::RootPromotion::Promoted,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: codex_project_intelligence::BlackboardProvenance {
            kind: codex_project_intelligence::BlackboardProvenanceKind::Agent,
            source_id: "turn-1".to_string(),
        },
    }
}

/// A -> B -> C where the question matches A and C: one group, C first, with B and A as
/// history; and a since filter applies before matches are ranked.
#[tokio::test]
async fn matches_of_one_chain_share_a_group_and_since_filters_first() {
    let state_home = tempfile::TempDir::new().expect("state home");
    let services = crate::services::ProjectIntelligenceServices::new(
        codex_state::SqliteConfig::new_for_testing(
            codex_utils_absolute_path::test_support::PathExt::abs(state_home.path()),
        ),
    );
    let node_id = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    let id = |value: &str| codex_project_intelligence::BlackboardEntryId::parse(value).expect("id");
    store
        .create_entry(
            id("a"),
            decision(&node_id, "Rounding uses ONE decimal place."),
        )
        .await
        .expect("A");
    store
        .create_successor(
            id("b"),
            decision(&node_id, "Formatting now uses two places."),
            vec![codex_project_intelligence::SupersededEntry {
                id: id("a"),
                expected_revision: 1,
            }],
        )
        .await
        .expect("B");
    store
        .create_successor(
            id("c"),
            decision(&node_id, "Rounding uses TWO decimal places, final."),
            vec![codex_project_intelligence::SupersededEntry {
                id: id("b"),
                expected_revision: 1,
            }],
        )
        .await
        .expect("C");
    let tool = super::MemoryReadTool::new(
        "project-1".to_string(),
        "thread-1".to_string(),
        services.clone(),
        std::sync::Arc::new(codex_thread_store::InMemoryThreadStore::default()),
    );
    let terms = question_terms("rounding decimal places");
    let (groups, _) = tool
        .knowledge(
            store, &terms, /*since_ms*/ None, /*include_history*/ true,
        )
        .await
        .expect("knowledge");
    let ids = groups
        .iter()
        .map(|group| {
            group
                .iter()
                .map(|row| row["entryId"].as_str().unwrap_or_default().to_string())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let (future_only, _) = tool
        .knowledge(
            store,
            &terms,
            Some(i64::MAX / 2),
            /*include_history*/ true,
        )
        .await
        .expect("knowledge");
    assert_eq!(
        (
            ids.len(),
            ids[0].first().cloned(),
            ids[0].len(),
            future_only.len()
        ),
        (1, Some("c".to_string()), 3, 0)
    );
}

/// A -> B -> C -> D -> E -> F with matches on F and A: the walk from A stops at the
/// four-hop bound on E, which the group of F already shows, so A joins that group and no
/// entry repeats.
#[tokio::test]
async fn a_long_chain_joins_the_group_that_already_shows_it() {
    let state_home = tempfile::TempDir::new().expect("state home");
    let services = crate::services::ProjectIntelligenceServices::new(
        codex_state::SqliteConfig::new_for_testing(
            codex_utils_absolute_path::test_support::PathExt::abs(state_home.path()),
        ),
    );
    let node_id = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    let id = |value: &str| codex_project_intelligence::BlackboardEntryId::parse(value).expect("id");
    store
        .create_entry(id("a"), decision(&node_id, "Rounding starts at one place."))
        .await
        .expect("A");
    for (previous, next, content) in [
        ("a", "b", "Second value."),
        ("b", "c", "Third value."),
        ("c", "d", "Fourth value."),
        ("d", "e", "Fifth value."),
        ("e", "f", "Rounding ends at six places."),
    ] {
        store
            .create_successor(
                id(next),
                decision(&node_id, content),
                vec![codex_project_intelligence::SupersededEntry {
                    id: id(previous),
                    expected_revision: 1,
                }],
            )
            .await
            .expect("successor");
    }
    let tool = super::MemoryReadTool::new(
        "project-1".to_string(),
        "thread-1".to_string(),
        services.clone(),
        std::sync::Arc::new(codex_thread_store::InMemoryThreadStore::default()),
    );
    let (groups, truncated) = tool
        .knowledge(
            store,
            &question_terms("rounding"),
            Some(0),
            /*include_history*/ true,
        )
        .await
        .expect("knowledge");
    let mut ids = groups
        .iter()
        .flatten()
        .map(|row| row["entryId"].as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>();
    let first = ids.first().cloned();
    ids.sort();
    assert_eq!(
        (groups.len(), first, ids, truncated),
        (
            1,
            Some("f".to_string()),
            ["a", "b", "c", "d", "e", "f"].map(String::from).to_vec(),
            false
        )
    );
}

/// More dated matches than the hit cap are reported as more matching knowledge.
#[tokio::test]
async fn since_matches_beyond_the_hit_cap_are_reported() {
    let state_home = tempfile::TempDir::new().expect("state home");
    let services = crate::services::ProjectIntelligenceServices::new(
        codex_state::SqliteConfig::new_for_testing(
            codex_utils_absolute_path::test_support::PathExt::abs(state_home.path()),
        ),
    );
    let node_id = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    for number in 0..=super::MAX_SEARCH_HITS {
        store
            .create_entry(
                codex_project_intelligence::BlackboardEntryId::parse(format!("entry-{number}"))
                    .expect("id"),
                decision(&node_id, &format!("Rounding rule number {number}.")),
            )
            .await
            .expect("entry");
    }
    let tool = super::MemoryReadTool::new(
        "project-1".to_string(),
        "thread-1".to_string(),
        services.clone(),
        std::sync::Arc::new(codex_thread_store::InMemoryThreadStore::default()),
    );
    let (groups, truncated) = tool
        .knowledge(
            store,
            &question_terms("rounding"),
            Some(0),
            /*include_history*/ false,
        )
        .await
        .expect("knowledge");
    assert_eq!(
        (groups.len(), truncated),
        (super::MAX_SEARCH_HITS as usize, true)
    );
}

/// With A -> ... -> F, older matches arriving first (A, then B) still resolve to F: B is not
/// stopped at an entry that only A's walk had reached.
#[tokio::test]
async fn older_matches_first_still_reach_the_current_entry() {
    let state_home = tempfile::TempDir::new().expect("state home");
    let services = crate::services::ProjectIntelligenceServices::new(
        codex_state::SqliteConfig::new_for_testing(
            codex_utils_absolute_path::test_support::PathExt::abs(state_home.path()),
        ),
    );
    let node_id = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    let id = |value: &str| codex_project_intelligence::BlackboardEntryId::parse(value).expect("id");
    store
        .create_entry(
            id("a"),
            decision(&node_id, "Rounding rounding starts at one place."),
        )
        .await
        .expect("A");
    for (previous, next, content) in [
        ("a", "b", "Rounding is now two places."),
        ("b", "c", "Third value."),
        ("c", "d", "Fourth value."),
        ("d", "e", "Fifth value."),
        ("e", "f", "Sixth value."),
    ] {
        store
            .create_successor(
                id(next),
                decision(&node_id, content),
                vec![codex_project_intelligence::SupersededEntry {
                    id: id(previous),
                    expected_revision: 1,
                }],
            )
            .await
            .expect("successor");
    }
    let tool = super::MemoryReadTool::new(
        "project-1".to_string(),
        "thread-1".to_string(),
        services.clone(),
        std::sync::Arc::new(codex_thread_store::InMemoryThreadStore::default()),
    );
    let mut heads = Vec::new();
    for include_history in [false, true] {
        let (groups, _) = tool
            .knowledge(store, &question_terms("rounding"), Some(0), include_history)
            .await
            .expect("knowledge");
        heads.push(
            groups
                .iter()
                .map(|group| group[0]["entryId"].as_str().unwrap_or_default().to_string())
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(heads, vec![vec!["f".to_string()], vec!["f".to_string()]]);
}

/// A replaced entry keeps the authorship of its own words: a user-authored entry replaced by
/// an agent record is still labelled as the user's.
#[tokio::test]
async fn replaced_entries_keep_their_own_authorship() {
    let state_home = tempfile::TempDir::new().expect("state home");
    let services = crate::services::ProjectIntelligenceServices::new(
        codex_state::SqliteConfig::new_for_testing(
            codex_utils_absolute_path::test_support::PathExt::abs(state_home.path()),
        ),
    );
    let node_id = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    let id = |value: &str| codex_project_intelligence::BlackboardEntryId::parse(value).expect("id");
    let mut by_user = decision(&node_id, "Rounding uses one place.");
    by_user.provenance.kind = codex_project_intelligence::BlackboardProvenanceKind::User;
    store.create_entry(id("a"), by_user).await.expect("A");
    store
        .create_successor(
            id("b"),
            decision(&node_id, "Rounding uses two places."),
            vec![codex_project_intelligence::SupersededEntry {
                id: id("a"),
                expected_revision: 1,
            }],
        )
        .await
        .expect("B");
    let tool = super::MemoryReadTool::new(
        "project-1".to_string(),
        "thread-1".to_string(),
        services.clone(),
        std::sync::Arc::new(codex_thread_store::InMemoryThreadStore::default()),
    );
    let (groups, _) = tool
        .knowledge(
            store,
            &question_terms("rounding"),
            /*since_ms*/ None,
            /*include_history*/ true,
        )
        .await
        .expect("knowledge");
    let sources = groups[0]
        .iter()
        .map(|row| {
            (
                row["entryId"].as_str().unwrap_or_default().to_string(),
                row["source"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sources,
        vec![
            ("b".to_string(), "agent record".to_string()),
            ("a".to_string(), "user".to_string()),
        ]
    );
}
