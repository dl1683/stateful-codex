use super::super::context_bounds::tests::retire;
use super::super::context_bounds::tests::snapshot;
use super::super::knowledge::tests::change;
use super::super::knowledge::tests::rule;
use super::super::knowledge::tests::store;
use super::super::source_fixture::admission;
use super::super::source_fixture::observation;
use crate::*;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
const PROJECT: &str = "project-1";

#[tokio::test]
async fn c2_exact_source_survives_fault_reopen_redelivery_and_revision_tamper() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let text = "Ground rules for this project:\r\n- Never push.\r\n- ÜBER e\u{301} 👩‍🔬.\r\n";
    let observed = observation("original-event", text);
    sqlx::query("CREATE TRIGGER fail_observation BEFORE INSERT ON capture_sources BEGIN SELECT RAISE(ABORT, 'source fault'); END").execute(&store.pool).await.unwrap();
    let before = snapshot(&store).await;
    assert!(store.observe_source(observed.clone(), text).await.is_err());
    assert_eq!(snapshot(&store).await, before);
    sqlx::query("DROP TRIGGER fail_observation")
        .execute(&store.pool)
        .await
        .unwrap();
    let seal = store.observe_source(observed.clone(), text).await.unwrap();
    let before = snapshot(&store).await;
    let mut wrong = observed.clone();
    wrong.binding_generation += 1;
    assert!(store.observe_source(wrong, text).await.is_err());
    assert!(
        store
            .observe_source(observed.clone(), &text.replace("Never", "Always"))
            .await
            .is_err()
    );
    assert!(
        store
            .read_source_range(
                PROJECT,
                &seal.exact_source_locator,
                &"0".repeat(/*n*/ 64),
                /*start*/ 0,
                text.len() as u32
            )
            .await
            .is_err()
    );
    let offset = text.find('Ü').unwrap() as u32 + 1;
    assert!(
        store
            .read_source_range(
                PROJECT,
                &seal.exact_source_locator,
                &seal.digest,
                offset,
                offset + 2
            )
            .await
            .is_err()
    );
    assert_eq!(snapshot(&store).await, before);
    store.pool.close().await;
    let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .unwrap();
    assert_eq!(reopened.observe_source(observed, text).await.unwrap(), seal);
    assert_eq!(
        reopened
            .read_source_range(
                PROJECT,
                &seal.exact_source_locator,
                &seal.digest,
                /*start*/ 0,
                text.len() as u32
            )
            .await
            .unwrap(),
        SourceRangeRead {
            seal: seal.clone(),
            start_byte: 0,
            end_byte: text.len() as u32,
            exact_text: text.to_string(),
            next_offset: None
        }
    );
    assert!(
        reopened
            .read_source_range(
                "other-project",
                &seal.exact_source_locator,
                &seal.digest,
                /*start*/ 0,
                /*end*/ 2
            )
            .await
            .is_err()
    );
    let mut edited = observation("original-event", text);
    edited.source_revision = 2;
    let revised = reopened.observe_source(edited, text).await.unwrap();
    assert_ne!(revised.exact_source_locator, seal.exact_source_locator);
    assert!(
        revised.immutable_first_observation_sequence > seal.immutable_first_observation_sequence
    );
}

#[tokio::test]
async fn c2_long_source_index_rebuild_is_bounded_and_progresses_across_cold_pages() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let mut locators = Vec::new();
    for ordinal in 0..5 {
        let text = format!(
            "{}deepneedle QX-704 {}",
            "é decoy ".repeat(/*n*/ 3800),
            " independent ".repeat(/*n*/ 2200)
        );
        let seal = store
            .observe_source(observation(&format!("long-{ordinal}"), &text), &text)
            .await
            .unwrap();
        assert!(!seal.observation.complete_envelope);
        let offset = text.find("deepneedle").unwrap() as u32;
        assert_eq!(
            store
                .read_source_range(
                    PROJECT,
                    &seal.exact_source_locator,
                    &seal.digest,
                    offset,
                    offset + 18
                )
                .await
                .unwrap()
                .exact_text,
            "deepneedle QX-704 "
        );
        locators.push(seal.exact_source_locator);
    }
    let largest: i64 =
        sqlx::query_scalar("SELECT MAX(octet_length(exact_bytes)) FROM capture_source_chunks")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert!(largest <= 4096);
    let before = store
        .search_source_ranges(PROJECT, "deepneedle", /*after*/ None)
        .await
        .unwrap();
    assert_eq!(
        before
            .ranges
            .iter()
            .map(|range| range.seal.exact_source_locator.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        locators.into_iter().collect()
    );
    store.begin_source_index_rebuild(PROJECT).await.unwrap();
    assert!(matches!(
        store
            .search_source_ranges(PROJECT, "deepneedle", /*after*/ None)
            .await,
        Err(BlackboardStoreError::SourceIndexIncomplete)
    ));
    store.maintain_source_index(PROJECT).await.unwrap();
    store.pool.close().await;
    let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .unwrap();
    for _ in 0..10 {
        if reopened.maintain_source_index(PROJECT).await.unwrap() {
            break;
        }
    }
    assert_eq!(
        reopened
            .search_source_ranges(PROJECT, "deepneedle", /*after*/ None)
            .await
            .unwrap(),
        before
    );
}

#[tokio::test]
async fn c2_forget_fences_linked_copies_overlap_rebuild_and_native_fallbacks() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let text = format!(
        "Mara bought the teal prototype receipt QX-704.{}Independent Cedar kit observation.",
        " padding ".repeat(/*n*/ 1200)
    );
    let seal = store
        .observe_source(observation("purchase", &text), &text)
        .await
        .unwrap();
    let copy = store
        .observe_source(observation("copied-purchase", &text), &text)
        .await
        .unwrap();
    let id = BlackboardEntryId::parse("purchase-entry").unwrap();
    let mut value = rule("Mara bought the teal prototype receipt QX-704.");
    value.kind = BlackboardKind::Note;
    let entry = store
        .create_entry_with_context(
            id.clone(),
            value,
            KnowledgeContext::new(KnowledgeCategory::Note, KnowledgeAuthority::HumanDirect),
            change(ChangeOperation::Saved, "purchase"),
        )
        .await
        .unwrap()
        .0;
    let spans = vec![SourceSpan {
        start_byte: 0,
        end_byte: 44,
        role: SourceSpanRole::Body,
    }];
    let admission = admission(&home).await;
    store
        .link_entry_source(&admission, &id, entry.revision, &seal, &spans)
        .await
        .unwrap();
    assert!(
        !store
            .search_source_ranges(PROJECT, "QX-704", /*after*/ None)
            .await
            .unwrap()
            .ranges
            .is_empty()
    );
    store
        .update_entry(PROJECT, &id, retire(&entry))
        .await
        .unwrap();
    for original in [&seal, &copy] {
        assert!(matches!(
            store
                .read_source_range(
                    PROJECT,
                    &original.exact_source_locator,
                    &original.digest,
                    /*start*/ 0,
                    /*end*/ 60
                )
                .await,
            Err(BlackboardStoreError::SourceExcluded)
        ));
    }
    assert!(!store.source_text_eligible(PROJECT, &text).await.unwrap());
    assert!(
        !store
            .source_text_eligible(PROJECT, "MARA bought the teal prototype receipt QX - 704.")
            .await
            .unwrap()
    );
    store.begin_source_index_rebuild(PROJECT).await.unwrap();
    while !store.maintain_source_index(PROJECT).await.unwrap() {}
    assert_eq!(
        store
            .search_source_ranges(PROJECT, "QX-704", /*after*/ None)
            .await
            .unwrap()
            .ranges,
        Vec::<SourceRangeRead>::new()
    );
    assert!(
        !store
            .search_source_ranges(PROJECT, "Cedar", /*after*/ None)
            .await
            .unwrap()
            .ranges
            .is_empty()
    );
    let before = snapshot(&store).await;
    assert_eq!(
        store
            .observe_source(observation("purchase", &text), &text)
            .await
            .unwrap(),
        seal
    );
    assert_eq!(snapshot(&store).await, before);
    store.pool.close().await;
    let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .unwrap();
    assert!(
        reopened
            .search_source_ranges(PROJECT, "prototype", /*after*/ None)
            .await
            .unwrap()
            .ranges
            .is_empty()
    );
}

#[tokio::test]
async fn c2_unknown_temporal_context_and_interval_round_trip_preserve_record_clock() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let id = BlackboardEntryId::parse("dated-report").unwrap();
    let text =
        "Notes from 28 June 2024: repair between 14 and 16 June, inclusive; exact day unknown.";
    let seal = store
        .observe_source(observation("note", text), text)
        .await
        .unwrap();
    let anchor = TemporalAnchor {
        source_id: seal.exact_source_locator.clone(),
        source_revision: 1,
        digest: seal.digest.clone(),
        spans: vec![SourceSpan {
            start_byte: 0,
            end_byte: text.len() as u32,
            role: SourceSpanRole::Temporal,
        }],
    };
    let temporal = TemporalContext {
        version: 1,
        recorded_at_ms: 9000000000000,
        source_time: SourceTime::Attributed {
            expression: "28 June 2024".to_string(),
            anchor: anchor.clone(),
            precision: TimePrecision::Day,
            timezone: TimeZone::Unknown,
        },
        event_time: EventTime::Interval {
            expression: "between 14 and 16 June, inclusive".to_string(),
            anchor,
            start_ms: None,
            end_ms: None,
            inclusive_start: true,
            inclusive_end: true,
            precision: TimePrecision::Day,
            timezone: TimeZone::Unknown,
            derivation: TemporalDerivation::SourceExplicit,
        },
        event_status: EventStatus::SourceReportedOccurrence,
    };
    temporal.validate().unwrap();
    let mut context = KnowledgeContext::new(
        KnowledgeCategory::Note,
        KnowledgeAuthority::AssistantReported,
    );
    context.payload = Some(serde_json::json!({"temporal": temporal}).to_string());
    let mut value = rule("repair interval, exact day unknown");
    value.kind = BlackboardKind::Note;
    value.provenance.kind = BlackboardProvenanceKind::Agent;
    store
        .create_entry_with_context(
            id.clone(),
            value.clone(),
            context.clone(),
            change(ChangeOperation::Saved, "repair"),
        )
        .await
        .unwrap();
    store.pool.close().await;
    let reopened = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .unwrap();
    assert_eq!(
        reopened.knowledge_context(PROJECT, &id).await.unwrap(),
        Some(context)
    );
    let stored = reopened.get_entry(PROJECT, &id).await.unwrap().unwrap();
    let mut expected = temporal.clone();
    expected.recorded_at_ms = stored.created_at_ms;
    assert_eq!(
        reopened.temporal_context(PROJECT, &id).await.unwrap(),
        expected
    );
    let legacy = BlackboardEntryId::parse("unknown-source").unwrap();
    reopened.create_entry(legacy.clone(), value).await.unwrap();
    assert_eq!(
        reopened.knowledge_context(PROJECT, &legacy).await.unwrap(),
        None
    );
    let stored = reopened.get_entry(PROJECT, &legacy).await.unwrap().unwrap();
    assert_eq!(
        reopened.temporal_context(PROJECT, &legacy).await.unwrap(),
        TemporalContext {
            version: 1,
            recorded_at_ms: stored.created_at_ms,
            source_time: SourceTime::Unknown,
            event_time: EventTime::Unknown,
            event_status: EventStatus::Unknown
        }
    );
    let unknown = TemporalContext {
        version: 1,
        recorded_at_ms: 123,
        source_time: SourceTime::Unknown,
        event_time: EventTime::UnresolvedRelative {
            expression: "a few days ago".to_string(),
            anchor: None,
        },
        event_status: EventStatus::UncertainReport,
    };
    unknown.validate().unwrap();
    assert_eq!(
        serde_json::from_str::<TemporalContext>(&serde_json::to_string(&unknown).unwrap()).unwrap(),
        unknown
    );
}

#[tokio::test]
async fn c2_source_pages_bound_escaped_metadata_progress_and_reject_cursor_drift() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let text = format!(
        "{}needle exact Unicode é 👩‍🔬 {}",
        "padding ".repeat(/*n*/ 300),
        "\"\t\n".repeat(/*n*/ 500)
    );
    for index in 0..12 {
        let mut obs = observation(&format!("{index}-{}", "event".repeat(/*n*/ 95)), &text);
        obs.turn_id = "turn".repeat(/*n*/ 128);
        store.observe_source(obs, &text).await.unwrap();
    }
    let mut cursor = None;
    let mut seen = std::collections::BTreeSet::new();
    let mut first_cursor = None;
    loop {
        let page = store
            .search_source_ranges(PROJECT, "needle", cursor.as_ref())
            .await
            .unwrap();
        assert!(serde_json::to_string(&page).unwrap().len() <= 9000);
        assert!(!page.ranges.is_empty());
        for range in &page.ranges {
            assert!(range.exact_text.contains("needle"));
            assert!(seen.insert(range.seal.exact_source_locator.clone()));
            let exact = store
                .read_source_range(
                    PROJECT,
                    &range.seal.exact_source_locator,
                    &range.seal.digest,
                    range.start_byte,
                    range.end_byte,
                )
                .await
                .unwrap();
            assert_eq!(exact.exact_text, range.exact_text);
        }
        if first_cursor.is_none() {
            first_cursor = page.after.clone();
        }
        if page.complete {
            assert!(page.after.is_none());
            break;
        }
        assert_ne!(page.after, cursor);
        cursor = page.after;
    }
    assert_eq!(seen.len(), 12);
    let cursor = first_cursor.unwrap();
    assert!(matches!(
        store
            .search_source_ranges(PROJECT, "different", Some(&cursor))
            .await,
        Err(BlackboardStoreError::SourceCursorDrift)
    ));
    store
        .observe_source(
            observation("later", "needle later event"),
            "needle later event",
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .search_source_ranges(PROJECT, "needle", Some(&cursor))
            .await,
        Err(BlackboardStoreError::SourceCursorDrift)
    ));
}

#[tokio::test]
async fn c2_source_links_page_exact_ranges_and_omission_redelivery_stays_unknown() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let text = "abcdefghijklmno";
    let seal = store
        .observe_source(observation("linked", text), text)
        .await
        .unwrap();
    let id = BlackboardEntryId::parse("linked-evidence").unwrap();
    let entry = store
        .create_entry(id.clone(), rule("Explicit independent entry."))
        .await
        .unwrap();
    let spans = (0..9)
        .map(|start| SourceSpan {
            start_byte: start,
            end_byte: start + 1,
            role: SourceSpanRole::Body,
        })
        .collect::<Vec<_>>();
    store
        .link_entry_source(&admission, &id, entry.revision, &seal, &spans[..8])
        .await
        .unwrap();
    store
        .link_entry_source(&admission, &id, entry.revision, &seal, &spans[8..])
        .await
        .unwrap();
    let page = store
        .entry_source_links(PROJECT, &id, /*after*/ None)
        .await
        .unwrap();
    assert_eq!(
        page.links.iter().map(|link| &link.span).collect::<Vec<_>>(),
        spans[..8].iter().collect::<Vec<_>>()
    );
    assert!(!page.complete);
    let next = store
        .entry_source_links(PROJECT, &id, page.after.as_ref())
        .await
        .unwrap();
    assert_eq!(
        next.links.iter().map(|link| &link.span).collect::<Vec<_>>(),
        spans[8..].iter().collect::<Vec<_>>()
    );
    assert!(next.complete);
    let extra = SourceSpan {
        start_byte: 9,
        end_byte: 10,
        role: SourceSpanRole::Body,
    };
    store
        .link_entry_source(&admission, &id, entry.revision, &seal, &[extra])
        .await
        .unwrap();
    assert!(matches!(
        store
            .entry_source_links(PROJECT, &id, page.after.as_ref())
            .await,
        Err(BlackboardStoreError::SourceCursorDrift)
    ));
    let mut omitted = observation("omitted", text);
    omitted.complete_envelope = false;
    omitted.ordered_spans.clear();
    omitted.incomplete_reason =
        Some("original inspection exceeds budget; native locator only".to_string());
    store.record_source_omission(omitted.clone()).await.unwrap();
    let before = snapshot(&store).await;
    store.record_source_omission(omitted).await.unwrap();
    assert!(
        store
            .observe_source(observation("omitted", text), text)
            .await
            .is_err()
    );
    assert_eq!(snapshot(&store).await, before);
}
