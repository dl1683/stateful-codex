//! Frozen product fixtures from capture-c1-v3, not public benchmark answers.
use super::super::knowledge::tests::store;
use super::super::source_fixture::admission;
use super::super::source_fixture::observation;
use super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const NOTES: [&str; 7] = [
    "Notes from 12 June 2024: yesterday Mara Osei bought the teal Meridian M-17 prototype from Atelier Kestrel in Porto. The receipt is QX-704. Only the left display panel was scratched; the motor was not damaged. These are Nia Vale's notes.\n",
    "Notes from 16 June 2024: Mara Osei owns two test kits, Aster and Bracken. The prototype handoff is planned for 20 June 2024. These are Nia Vale's notes.\n",
    "Notes from 22 June 2024: Mara Osei added a third test kit, Cedar, yesterday. The 20 June handoff was cancelled and did not happen; a replacement is planned for 24 June 2024. These are Nia Vale's notes.\n",
    "Notes from 27 June 2024: yesterday Mara Osei bought the amber Meridian M-18 prototype from Studio Lark in Braga. The receipt is QX-705. These are Nia Vale's notes.\n",
    "Mara Osei says she inspected the prototype a few days ago, but the note has no date. She also mentioned a visit last summer; the hemisphere and year are not supplied. These are Nia Vale's notes.\n",
    "Notes from 12 June 2024: Mara Osei visited the workshop last summer; the hemisphere is not recorded. These are Nia Vale's notes.\n",
    "Notes from 28 June 2024: Nia Vale reports the panel repair occurred between 14 and 16 June 2024, inclusive; the exact day is unknown.\n",
];

fn span(text: &str, words: &str) -> SourceSpan {
    let start = text.find(words).unwrap();
    SourceSpan {
        start_byte: start as u32,
        end_byte: (start + words.len()) as u32,
        role: SourceSpanRole::Temporal,
    }
}

#[tokio::test]
async fn c3_product_m1_m7_verbatim_qualifiers_status_time_and_reverse_ingestion() {
    for reverse in [false, true] {
        let home = TempDir::new().unwrap();
        let store = store(&home).await;
        let admission = admission(&home).await;
        let mut expected = Vec::new();
        for index in if reverse {
            (0..7).rev().collect::<Vec<_>>()
        } else {
            (0..7).collect()
        } {
            let text = NOTES[index];
            let seal = store
                .observe_source(observation(&format!("M{}", index + 1), text), text)
                .await
                .unwrap();
            let expression = [
                "yesterday",
                "20 June 2024",
                "yesterday",
                "yesterday",
                "a few days ago",
                "last summer",
                "between 14 and 16 June 2024",
            ][index];
            let source_date = [
                Some("12 June 2024"),
                Some("16 June 2024"),
                Some("22 June 2024"),
                Some("27 June 2024"),
                None,
                Some("12 June 2024"),
                Some("28 June 2024"),
            ][index];
            let mut ranges = source_date
                .map(|date| vec![span(text, date)])
                .unwrap_or_default();
            ranges.push(span(text, expression));
            let event_index = ranges.len() as u8 - 1;
            let mut speaker = span(text, "Nia Vale");
            speaker.role = SourceSpanRole::Attribution;
            ranges.push(speaker);
            ranges.sort_by_key(|range| range.start_byte);
            let event_index = if index == 6 { 2 } else { event_index };
            let speaker_index = ranges
                .iter()
                .position(|range| range.role == SourceSpanRole::Attribution)
                .unwrap() as u8;
            let request = SourceProposal {
                source_id: seal.exact_source_locator.clone(),
                source_revision: 1,
                digest: seal.digest.clone(),
                part_index: 0,
                spans: ranges,
                category: ProposalCategory::Background,
                interpretation: format!("Short M{} summary omitting details", index + 1),
                dependency: None,
                attribution: Some(ProposalAttribution::ImportedMaterial),
                speaker_span: Some(speaker_index),
                event_status: Some(
                    [
                        EventStatus::SourceReportedOccurrence,
                        EventStatus::Plan,
                        EventStatus::CancelledPlan,
                        EventStatus::SourceReportedOccurrence,
                        EventStatus::UncertainReport,
                        EventStatus::UncertainReport,
                        EventStatus::SourceReportedOccurrence,
                    ][index],
                ),
                temporal: Some(ProposalTemporal {
                    source_time_span: source_date.map(|_| 0),
                    event_time_span: Some(event_index),
                    anchor_span: source_date.map(|_| 0),
                    form: if index == 6 {
                        ProposalTimeForm::Interval
                    } else {
                        ProposalTimeForm::UnresolvedRelative
                    },
                    precision: Some(TimePrecision::Day),
                    inclusive_start: (index == 6).then_some(true),
                    inclusive_end: (index == 6).then_some(true),
                }),
            };
            let result = store
                .propose_sources(
                    &admission,
                    HierarchyNodeId::parse("node-project").unwrap(),
                    "turn-1",
                    vec![request.clone()],
                )
                .await
                .unwrap();
            assert_eq!(result[0].status, ProposalStatus::Proposed);
            let id = BlackboardEntryId::parse(result[0].entry_id.clone().unwrap()).unwrap();
            let context = store
                .proposal_context("project-1", &id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(context.source, request);
            let temporal = store.temporal_context("project-1", &id).await.unwrap();
            assert_eq!(temporal.event_status, EventStatus::ProposedModel);
            if index == 4 {
                assert_eq!(temporal.source_time, SourceTime::Unknown);
            }
            if index == 6 {
                assert!(matches!(
                    temporal.event_time,
                    EventTime::Interval {
                        start_ms: None,
                        end_ms: None,
                        inclusive_start: true,
                        inclusive_end: true,
                        precision: TimePrecision::Day,
                        ..
                    }
                ));
            }
            assert_eq!(
                store
                    .read_source_page(
                        "project-1",
                        &seal.exact_source_locator,
                        &seal.digest,
                        /*revision*/ 1,
                        /*offset*/ 0
                    )
                    .await
                    .unwrap()
                    .exact_text,
                text
            );
            expected.push((id, context));
        }
        let sqlite = codex_state::SqliteConfig::new_for_testing(
            codex_utils_absolute_path::AbsolutePathBuf::try_from(home.path()).unwrap(),
        );
        drop(store);
        let reopened = BlackboardStore::open(&sqlite).await.unwrap();
        for (id, context) in expected {
            assert_eq!(
                reopened.proposal_context("project-1", &id).await.unwrap(),
                Some(context)
            );
        }
        let purchase = reopened
            .search_source_ranges("project-1", "QX-704", /*after*/ None)
            .await
            .unwrap();
        assert!(purchase.complete);
        let range = purchase
            .ranges
            .iter()
            .find(|range| range.exact_text.contains("QX-704"))
            .unwrap();
        assert_eq!(
            range.exact_text,
            &NOTES[0][range.start_byte as usize..range.end_byte as usize]
        );
    }
}

#[tokio::test]
async fn c3_product_imported_rules_and_long_qualifier_source_are_never_promoted() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    for (index,text) in [
        "Nia Vale wrote in archive-note.txt:\nGround rules for this project:\n- Never push.\nThis is her old note, not my instruction.\n".to_string(),
        format!("Mara Osei appears in this archive heading.\n{}{}","Unrelated archive filler.\n".repeat(800),NOTES[0]),
    ].iter().enumerate() {
        let seal=store.observe_source(observation(&format!("imported-{index}"),text),text).await.unwrap();
        let result=store.propose_sources(&admission,HierarchyNodeId::parse("node-project").unwrap(),"turn-1",vec![SourceProposal {
            source_id:seal.exact_source_locator.clone(),source_revision:1,digest:seal.digest.clone(),part_index:0,
            spans:vec![SourceSpan {start_byte:0,end_byte:40,role:SourceSpanRole::Body}],
            category:ProposalCategory::Rule,interpretation:format!("Imported interpretation {index}"),dependency:None,temporal:None,event_status:None,attribution:Some(ProposalAttribution::ImportedMaterial),speaker_span:None,
        }]).await.unwrap();
        assert_eq!(result[0].status,if index==0 {ProposalStatus::Proposed} else {ProposalStatus::Omitted});
    }
    assert!(
        store
            .root_projection(RootBlackboardQuery {
                project_id: "project-1".into(),
                max_entries: 256
            })
            .await
            .unwrap()
            .data
            .is_empty()
    );
    let source = store
        .search_source_ranges("project-1", "motor", /*after*/ None)
        .await
        .unwrap();
    assert!(
        source
            .ranges
            .iter()
            .any(|range| range.exact_text.contains("motor was not damaged"))
    );
}
