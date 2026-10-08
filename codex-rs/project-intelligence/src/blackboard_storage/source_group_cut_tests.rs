//! New source-group actions cannot reuse or promote existing typed members.
use super::*;
use pretty_assertions::assert_eq;

#[derive(Clone, Copy)]
enum Collision {
    CanonicalAlias,
    SuppliedCandidate,
}

#[derive(Clone, Copy)]
enum Divergence {
    TypedValueAndPromotion,
    PayloadOnly,
}

fn kit_request(seal: &SourceSeal, action: &str) -> SourceCaptureGroup {
    let mut group = request(seal, action);
    group.members.truncate(/*len*/ 1);
    let member = &mut group.members[0].write;
    member.value.kind = BlackboardKind::Note;
    member.value.content = "Kit count".to_string();
    member.value.structured_value = Some(BlackboardStructuredValue {
        value: "2".to_string(),
        unit: Some("kits".to_string()),
    });
    member.value.root_promotion = RootPromotion::NotPromoted;
    member.value.provenance.kind = BlackboardProvenanceKind::Agent;
    member.context = KnowledgeContext {
        payload: Some("{\"count\":2}".to_string()),
        ..KnowledgeContext::new(
            KnowledgeCategory::Note,
            KnowledgeAuthority::AssistantReported,
        )
    };
    member.change.origin = ChangeOrigin::ModelTool;
    member.change.category = KnowledgeCategory::Note;
    group
}

async fn refuse_collision(divergence: Divergence) {
    for collision in [Collision::CanonicalAlias, Collision::SuppliedCandidate] {
        let home = TempDir::new().unwrap();
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let store = store(&home).await;
        let admission = admission(&home).await;
        let original = "Kit count: 2 kits.";
        let seal = store
            .observe_source(observation("two-kits", original), original)
            .await
            .unwrap();
        let committed = store
            .write_source_group(&admission, kit_request(&seal, "first"))
            .await
            .unwrap();
        assert_eq!(
            (
                committed.saved,
                committed.already_present,
                committed.pending
            ),
            (0, 0, 1)
        );
        let id = BlackboardEntryId::parse(committed.members[0].entry_id.clone().unwrap()).unwrap();
        let entry = store.get_entry(PROJECT, &id).await.unwrap().unwrap();
        let context = store
            .knowledge_context(PROJECT, &id)
            .await
            .unwrap()
            .unwrap();
        let independent = "Independent Cedar note. Kit count: 3 kits.";
        let newer = store
            .observe_source(observation("three-kits", independent), independent)
            .await
            .unwrap();
        let make_request = || {
            let mut group = kit_request(&newer, "new-action");
            let member = &mut group.members[0].write;
            member.context.payload = Some("{\"count\":3}".to_string());
            match divergence {
                Divergence::TypedValueAndPromotion => {
                    member.value.structured_value.as_mut().unwrap().value = "3".to_string();
                    member.value.root_promotion = RootPromotion::Promoted;
                }
                Divergence::PayloadOnly => (),
            }
            match collision {
                Collision::CanonicalAlias => {
                    member.candidates = vec![BlackboardEntryId::parse("fresh-candidate").unwrap()];
                }
                Collision::SuppliedCandidate => {
                    // A different body bypasses canonical matching. Even a later
                    // supplied collision must refuse before a fresh candidate inserts.
                    member.value.content = "Updated kit tally".to_string();
                    member.candidates = vec![
                        BlackboardEntryId::parse("fresh-candidate").unwrap(),
                        id.clone(),
                    ];
                }
            }
            let mut earlier = kit_request(&newer, "new-action").members.remove(0);
            earlier.write.candidates =
                vec![BlackboardEntryId::parse("earlier-new-member").unwrap()];
            earlier.write.value.content = "Independent Cedar note.".to_string();
            group.members.insert(0, earlier);
            group
        };
        let before = snapshot(&store).await;
        assert!(matches!(
            store.write_source_group(&admission, make_request()).await,
            Err(BlackboardStoreError::EntryIdentityConflict(_))
        ));
        assert_eq!(snapshot(&store).await, before);
        store.pool.close().await;
        let reopened = BlackboardStore::open(&sqlite).await.unwrap();
        assert!(matches!(
            reopened
                .write_source_group(&admission, make_request())
                .await,
            Err(BlackboardStoreError::EntryIdentityConflict(_))
        ));
        assert_eq!(snapshot(&reopened).await, before);
        assert_eq!(
            reopened.get_entry(PROJECT, &id).await.unwrap().unwrap(),
            entry
        );
        assert_eq!(
            reopened
                .knowledge_context(PROJECT, &id)
                .await
                .unwrap()
                .unwrap(),
            context
        );
        assert_eq!(
            reopened
                .capture_group(PROJECT, "group-first")
                .await
                .unwrap(),
            Some(committed.clone())
        );
        assert_eq!(
            reopened
                .write_source_group(&admission, kit_request(&seal, "first"))
                .await
                .unwrap(),
            committed
        );
        assert_eq!(snapshot(&reopened).await, before);

        let mut independent_group = kit_request(&newer, "independent");
        independent_group.members[0].write.candidates =
            vec![BlackboardEntryId::parse("independent").unwrap()];
        independent_group.members[0].write.value.content = "Independent Cedar note.".to_string();
        independent_group.members[0].write.value.root_promotion = RootPromotion::Promoted;
        let saved = reopened
            .write_source_group(&admission, independent_group)
            .await
            .unwrap();
        assert_eq!((saved.saved, saved.already_present), (1, 0));
        // Existing direct controls remain available, including retirement and archives.
        reopened
            .update_entry(PROJECT, &id, retire(&entry))
            .await
            .unwrap();
        assert_eq!(
            reopened
                .get_entry(PROJECT, &id)
                .await
                .unwrap()
                .unwrap()
                .state,
            BlackboardEntryState::Tombstoned
        );
        assert_eq!(
            reopened
                .capture_group(PROJECT, "group-first")
                .await
                .unwrap(),
            Some(committed)
        );
    }
}

#[tokio::test]
async fn c2cut_group_requested_three_stored_two_refuses_atomically_after_reopen() {
    refuse_collision(Divergence::TypedValueAndPromotion).await;
}

#[tokio::test]
async fn c2cut_group_payload_only_collision_refuses_atomically_after_reopen() {
    refuse_collision(Divergence::PayloadOnly).await;
}
