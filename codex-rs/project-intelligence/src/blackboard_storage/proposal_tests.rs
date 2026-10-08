use super::super::context_bounds::tests::retire;
use super::super::context_bounds::tests::snapshot;
use super::super::knowledge::tests::store;
use super::super::source_fixture::admission;
use super::super::source_fixture::observation;
use crate::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const PROJECT: &str = "project-1";

fn node() -> HierarchyNodeId {
    HierarchyNodeId::parse("node-project").unwrap()
}

fn proposal(seal: &SourceSeal, text: &str) -> SourceProposal {
    SourceProposal {
        source_id: seal.exact_source_locator.clone(),
        digest: seal.digest.clone(),
        source_revision: seal.observation.source_revision,
        part_index: seal.observation.part_index,
        spans: vec![SourceSpan {
            start_byte: 0,
            end_byte: text.len().min(512) as u32,
            role: SourceSpanRole::Body,
        }],
        category: ProposalCategory::Decision,
        interpretation: "Model interpretation, not a settled decision".into(),
        dependency: None,
        temporal: None,
        event_status: None,
        attribution: None,
        speaker_span: None,
    }
}

#[tokio::test]
async fn c3r2_durable_proposal_group_keeps_unsupported_context_out_of_automatic_recall() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let text = "Mara bought a prototype; its motor was not damaged.";
    let seal = store
        .observe_source(observation("damaged-context", text), text)
        .await
        .unwrap();
    let result = store
        .propose_sources(&admission, node(), "turn-1", vec![proposal(&seal, text)])
        .await
        .unwrap();
    let id = BlackboardEntryId::parse(result[0].entry_id.clone().unwrap()).unwrap();
    let archived = store.get_entry(PROJECT, &id).await.unwrap();
    for payload in ["x".repeat(/*n*/ 1048576), "not json".into(), "{}".into()] {
        sqlx::query("UPDATE knowledge_context SET payload = ? WHERE entry_id = ?")
            .bind(payload)
            .bind(id.as_str())
            .execute(&store.pool)
            .await
            .unwrap();
        let before = snapshot(&store).await;
        assert_eq!(
            store.get_source_eligible_entry(PROJECT, &id).await.unwrap(),
            None
        );
        assert!(
            store
                .root_projection(RootBlackboardQuery {
                    project_id: PROJECT.into(),
                    max_entries: 1,
                })
                .await
                .unwrap()
                .data
                .is_empty()
        );
        assert_eq!(store.get_entry(PROJECT, &id).await.unwrap(), archived);
        assert_eq!(snapshot(&store).await, before);
    }
}

#[tokio::test]
async fn c3_reports_scope_negation_and_inner_ranges_keep_whole_enclosure_cold() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let texts = [
        "I wrote last week: \"My preference is tests first.\"",
        "Priya wrote: \"Always test first.\"",
        "\"Always test first.\" That is what Priya wrote.",
        "Priya and I discussed it: \"Always test first.\"",
        "The assistant suggested these decisions:\n- Use SQLite. The reason is smaller deployments.\n",
        "My preferences for this document:\n- Never include arXiv IDs.",
        "Never run git commit and always end each reply with Next: for this task only.",
        "For this project, Always test first today.",
        "For this project, Never push until we agree.",
        "If this works, always test first.",
        "For this project, I choose SQLite. The reason is smaller deployments. It must remain local.",
        "Do not use the option yet; I have not decided.",
        "Lena wrote in lena-request.md for lena-handout.md:\n- Always use author-year citations.\n- Never include arXiv IDs.\nProduce lena-handout.md following her request for that artifact only.\n",
        "Ruled out:\n- DNS; not checked yet, inspect the production host instead.\n",
        "Decision: SQLite; this decision has not been made yet.",
        "Decision: SQLite; it is still undecided.",
        "Decision: SQLite because the defaults look good; we have not decided yet.",
        "Decision: use SQLite; we have not decided whether to provision Redis for caching.",
        "For this project, I decide to use SQLite. The reason is smaller deployments.\nFor this project, keep open whether to provision Redis for caching.\n",
        "```\nGround rules for this project:\n- Always test first.\n- Never push.\n```",
        "Would these be good ground rules?\nGround rules for this project:\n- Always test first.\n- Never push.",
        "Do not follow the rules below:\nGround rules for this project:\n- Always test first.\n- Never push.",
        "Always test first.",
        "For this project, Do not adopt Priya's workflow.",
    ];
    let mut expected = Vec::new();
    for (index, text) in texts.iter().enumerate() {
        let seal = store
            .observe_source(observation(&format!("event-{index}"), text), text)
            .await
            .unwrap();
        let mut request = proposal(&seal, text);
        request.interpretation = format!("Interpretation {index}");
        request.attribution = Some(match index {
            0 => ProposalAttribution::HistoricalUser,
            1 | 2 => ProposalAttribution::ReportedThirdParty,
            4 => ProposalAttribution::ReportedAssistant,
            _ => ProposalAttribution::Unknown,
        });
        request.spans[0].start_byte = text.find("first").unwrap_or(0) as u32;
        if index == 1 || index == 2 {
            let start = text.find("Priya").unwrap() as u32;
            request.spans = vec![SourceSpan {
                start_byte: start,
                end_byte: start + 5,
                role: SourceSpanRole::Attribution,
            }];
            request.speaker_span = Some(0);
        }
        request.dependency = (index >= 3).then_some(ProposalDependency::Scope);
        let result = store
            .propose_sources(&admission, node(), "turn-1", vec![request.clone()])
            .await
            .unwrap();
        let id = BlackboardEntryId::parse(result[0].entry_id.clone().unwrap()).unwrap();
        let entry = store.get_entry(PROJECT, &id).await.unwrap().unwrap();
        assert_eq!(
            (
                entry.value.provenance.kind,
                entry.value.root_promotion,
                entry.value.verification
            ),
            (
                BlackboardProvenanceKind::User,
                RootPromotion::NotPromoted,
                BlackboardVerification::Unverified
            )
        );
        assert_eq!(
            store.proposal_context(PROJECT, &id).await.unwrap().unwrap(),
            ProposalContext {
                version: 1,
                status: result[0].status,
                source: request.clone(),
                enclosure: SourceSpan {
                    start_byte: 0,
                    end_byte: text.len() as u32,
                    role: SourceSpanRole::Body
                },
            }
        );
        assert_eq!(
            store
                .propose_sources(&admission, node(), "turn-1", vec![request])
                .await
                .unwrap(),
            result
        );
        expected.push((seal, id, (*text).to_string()));
    }
    drop(store);
    let sqlite = codex_state::SqliteConfig::new_for_testing(
        codex_utils_absolute_path::AbsolutePathBuf::try_from(home.path()).unwrap(),
    );
    let reopened = BlackboardStore::open(&sqlite).await.unwrap();
    for (seal, id, text) in expected {
        assert_eq!(
            reopened
                .read_source_range(
                    PROJECT,
                    &seal.exact_source_locator,
                    &seal.digest,
                    /*start*/ 0,
                    seal.original_utf8_length
                )
                .await
                .unwrap()
                .exact_text,
            text
        );
        let before = snapshot(&reopened).await;
        let entry = reopened.get_entry(PROJECT, &id).await.unwrap().unwrap();
        for action in ["retire", "promote", "revise"] {
            let mut update = retire(&entry);
            match action {
                "promote" => {
                    update.state = BlackboardEntryState::Active;
                    update.root_promotion = RootPromotion::Promoted;
                }
                "revise" => {
                    update.state = BlackboardEntryState::Active;
                    update.content = "Model changed the source".into();
                    update.provenance.kind = BlackboardProvenanceKind::Agent;
                }
                "retire" => {}
                _ => unreachable!(),
            }
            assert!(matches!(
                reopened.update_entry_from_model(PROJECT, &id, update).await,
                Err(BlackboardStoreError::ModelMutationRefused)
            ));
            assert_eq!(snapshot(&reopened).await, before);
        }
    }
}

#[tokio::test]
async fn c3_tamper_other_turn_and_utf8_boundaries_refuse_without_state_transition() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let text = "😀 café. The motor was not damaged.";
    let seal = store
        .observe_source(observation("event", text), text)
        .await
        .unwrap();
    let base = proposal(&seal, text);
    let before = snapshot(&store).await;
    for index in 0..10 {
        let mut request = base.clone();
        let mut turn = "turn-1";
        match index {
            0 => request.digest = "0".repeat(64),
            1 => request.source_id = "a".repeat(64),
            2 => request.source_revision = 2,
            3 => request.part_index = 1,
            4 => request.spans[0].start_byte = 1,
            5 => request.spans[0].end_byte = text.len() as u32 + 1,
            6 => turn = "other-turn",
            7 => request.spans.push(request.spans[0].clone()),
            8 => request.speaker_span = Some(0),
            9 => request.speaker_span = Some(7),
            _ => unreachable!(),
        }
        assert!(
            store
                .propose_sources(&admission, node(), turn, vec![request])
                .await
                .is_err()
        );
        assert_eq!(snapshot(&store).await, before);
    }
}

#[tokio::test]
async fn c3_long_enclosure_and_whole_group_bounds_omit_without_partial_authority() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let reason = format!(
        "Use SQLite. Reason: {} Keep it local for this document only.",
        "é".repeat(1400)
    );
    let reason_seal = store
        .observe_source(observation("long-reason", &reason), &reason)
        .await
        .unwrap();
    let mut request = proposal(&reason_seal, &reason);
    request.spans[0].end_byte = reason.len() as u32;
    let retained = store
        .propose_sources(&admission, node(), "turn-1", vec![request])
        .await
        .unwrap();
    assert_eq!(retained[0].status, ProposalStatus::Proposed);
    assert_eq!(
        store
            .read_source_page(
                PROJECT,
                &reason_seal.exact_source_locator,
                &reason_seal.digest,
                /*revision*/ 1,
                /*offset*/ 0
            )
            .await
            .unwrap()
            .exact_text,
        reason
    );
    let text = format!(
        "Priya wrote:\nAlways test first.{} For that task only.",
        "x".repeat(17000)
    );
    let seal = store
        .observe_source(observation("long", &text), &text)
        .await
        .unwrap();
    let result = store
        .propose_sources(&admission, node(), "turn-1", vec![proposal(&seal, &text)])
        .await
        .unwrap();
    assert_eq!(result[0].status, ProposalStatus::Omitted);
    assert_eq!(result[0].entry_id, None);
    assert_eq!(
        store
            .read_source_range(
                PROJECT,
                &seal.exact_source_locator,
                &seal.digest,
                /*start*/ 0,
                /*end*/ 30
            )
            .await
            .unwrap()
            .exact_text,
        &text[..30]
    );
    let before = snapshot(&store).await;
    let mut huge = proposal(&seal, &text);
    huge.interpretation = "é".repeat(257);
    assert!(
        store
            .propose_sources(&admission, node(), "turn-1", vec![huge])
            .await
            .is_err()
    );
    assert!(
        store
            .propose_sources(
                &admission,
                node(),
                "turn-1",
                vec![proposal(&seal, &text); 25]
            )
            .await
            .is_err()
    );
    assert_eq!(snapshot(&store).await, before);
}

#[tokio::test]
async fn c3_imported_details_and_temporal_anchors_remain_proposed_not_ingest_dates() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let text = "Notes from 12 June 2024:\r\nYesterday Mara bought a teal Meridian M-17 from Cedar Workshop, receipt QX-704. The motor was not damaged.";
    let seal = store
        .observe_source(observation("dated", text), text)
        .await
        .unwrap();
    let mut request = proposal(&seal, text);
    request.spans = vec![
        SourceSpan {
            start_byte: 11,
            end_byte: 23,
            role: SourceSpanRole::Temporal,
        },
        SourceSpan {
            start_byte: 26,
            end_byte: 35,
            role: SourceSpanRole::Temporal,
        },
        SourceSpan {
            start_byte: 36,
            end_byte: text.len() as u32,
            role: SourceSpanRole::Body,
        },
    ];
    request.temporal = Some(ProposalTemporal {
        source_time_span: Some(0),
        event_time_span: Some(1),
        anchor_span: Some(0),
        form: ProposalTimeForm::UnresolvedRelative,
        precision: None,
        inclusive_start: None,
        inclusive_end: None,
    });
    let result = store
        .propose_sources(&admission, node(), "turn-1", vec![request.clone()])
        .await
        .unwrap();
    let id = BlackboardEntryId::parse(result[0].entry_id.clone().unwrap()).unwrap();
    let temporal = store.temporal_context(PROJECT, &id).await.unwrap();
    assert_eq!(
        temporal.event_time,
        EventTime::UnresolvedRelative {
            expression: "Yesterday".into(),
            anchor: Some(TemporalAnchor {
                source_id: seal.exact_source_locator.clone(),
                source_revision: 1,
                digest: seal.digest.clone(),
                spans: vec![request.spans[0].clone()]
            }),
        }
    );
    assert_eq!(temporal.event_status, EventStatus::ProposedModel);
    let before = snapshot(&store).await;
    request.temporal.as_mut().unwrap().anchor_span = Some(2);
    assert!(
        store
            .propose_sources(&admission, node(), "turn-1", vec![request])
            .await
            .is_err()
    );
    assert_eq!(snapshot(&store).await, before);
}
