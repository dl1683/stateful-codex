use super::super::context_bounds::tests::snapshot;
use super::super::knowledge::tests::change;
use super::super::knowledge::tests::rule;
use super::super::knowledge::tests::store;
use super::super::source_fixture::admission;
use super::super::source_fixture::observation;
use super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[tokio::test]
async fn c3_three_hundred_proposals_do_not_consume_the_admitted_root_limit() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let text = "Reported rules are not current user instructions.";
    let seal = store
        .observe_source(observation("decoy-source", text), text)
        .await
        .unwrap();
    for start in (0..300).step_by(24) {
        let proposals = (start..(start + 24).min(300))
            .map(|index| SourceProposal {
                source_id: seal.exact_source_locator.clone(),
                source_revision: 1,
                digest: seal.digest.clone(),
                part_index: 0,
                spans: vec![SourceSpan {
                    start_byte: 0,
                    end_byte: text.len() as u32,
                    role: SourceSpanRole::Body,
                }],
                category: ProposalCategory::Rule,
                interpretation: format!("Reported rule decoy {index}"),
                dependency: None,
                temporal: None,
                event_status: None,
                attribution: Some(ProposalAttribution::ReportedThirdParty),
                speaker_span: None,
            })
            .collect();
        let results = store
            .propose_sources(
                &admission,
                HierarchyNodeId::parse("node-project").unwrap(),
                "turn-1",
                proposals,
            )
            .await
            .unwrap();
        assert!(
            results
                .iter()
                .all(|result| result.status == ProposalStatus::Proposed)
        );
    }
    let id = BlackboardEntryId::parse("admitted-control").unwrap();
    store
        .create_entry_with_context(
            id.clone(),
            rule("Always check the decisive source."),
            KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect),
            change(ChangeOperation::Saved, "explicit control"),
        )
        .await
        .unwrap();
    let expected = store.get_entry("project-1", &id).await.unwrap().unwrap();
    for root in [
        store
            .root_projection(RootBlackboardQuery {
                project_id: "project-1".into(),
                max_entries: 1,
            })
            .await
            .unwrap(),
        store
            .root_projection_for_thread(
                RootBlackboardQuery {
                    project_id: "project-1".into(),
                    max_entries: 1,
                },
                "thread-1",
            )
            .await
            .unwrap()
            .0,
    ] {
        assert_eq!(
            root.data
                .into_iter()
                .map(|hit| hit.entry)
                .collect::<Vec<_>>(),
            vec![expected.clone()]
        );
    }
}

#[tokio::test]
async fn c3_new_source_action_never_reuses_or_overwrites_an_existing_proposal() {
    let home = TempDir::new().unwrap();
    let store = store(&home).await;
    let admission = admission(&home).await;
    let texts = [
        "Lena wrote in first-request.md for first-output.md: \"Always use author-year citations.\"",
        "Lena wrote in second-request.md for second-output.md: \"Always use author-year citations.\"",
    ];
    let mut prior = None;
    for (index, text) in texts.iter().enumerate() {
        let seal = store
            .observe_source(observation(&format!("document-{index}"), text), text)
            .await
            .unwrap();
        let before = snapshot(&store).await;
        let result = store
            .propose_sources(
                &admission,
                HierarchyNodeId::parse("node-project").unwrap(),
                "turn-1",
                vec![SourceProposal {
                    source_id: seal.exact_source_locator.clone(),
                    source_revision: 1,
                    digest: seal.digest.clone(),
                    part_index: 0,
                    spans: vec![SourceSpan {
                        start_byte: 0,
                        end_byte: text.len() as u32,
                        role: SourceSpanRole::Body,
                    }],
                    category: ProposalCategory::AttributedContext,
                    interpretation: "Reported author-year request".into(),
                    dependency: Some(ProposalDependency::Scope),
                    temporal: None,
                    event_status: None,
                    attribution: Some(ProposalAttribution::ReportedThirdParty),
                    speaker_span: None,
                }],
            )
            .await;
        if index == 0 {
            let result = result.unwrap();
            prior = Some(BlackboardEntryId::parse(result[0].entry_id.clone().unwrap()).unwrap());
        } else {
            assert!(matches!(
                result,
                Err(BlackboardStoreError::EntryIdentityConflict(_))
            ));
            assert_eq!(snapshot(&store).await, before);
            let context = store
                .proposal_context("project-1", prior.as_ref().unwrap())
                .await
                .unwrap()
                .unwrap();
            assert_ne!(context.source.source_id, seal.exact_source_locator);
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
            *text
        );
    }
}
