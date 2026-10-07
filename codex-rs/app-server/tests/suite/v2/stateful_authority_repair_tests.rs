use super::*;
use codex_app_server_protocol::StatefulMemoryAuthority;
use codex_app_server_protocol::StatefulMemoryReadParams;
use codex_app_server_protocol::StatefulMemoryReadResponse;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn historical_self_speech_has_no_third_party_row_or_annotation() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Self speech".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "self-speech".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![assistant("Historical."), assistant("Attributed.")],
    )
    .await;
    let self_speech = "I wrote last week: \"My preference is tests first.\"";
    run_turn(&mut server, &thread, self_speech).await?;
    let read = |request_id| ClientRequest::StatefulMemoryRead {
        request_id,
        params: StatefulMemoryReadParams {
            thread_id: thread.clone(),
            expected_project_id: project.project.id.clone(),
            cursor: None,
            limit: None,
            background_section: true,
        },
    };
    let empty: StatefulMemoryReadResponse = server.request(read).await?;
    assert_eq!(empty.data, Vec::new());
    let packet = log.requests()[0].body_json();
    assert!(!packet.to_string().contains("<stateful_relayed_words>"));
    assert!(packet.to_string().contains("I wrote last week"));
    run_turn(
        &mut server,
        &thread,
        "Priya wrote: \"Always write tests first.\"",
    )
    .await?;
    let attributed: StatefulMemoryReadResponse = server.request(read).await?;
    assert_eq!(
        attributed
            .data
            .iter()
            .map(|item| (item.authority, item.attributed_to.as_deref()))
            .collect::<Vec<_>>(),
        vec![(
            Some(StatefulMemoryAuthority::ReportedThirdParty),
            Some("Priya")
        )]
    );
    assert!(
        log.requests()[1]
            .body_json()
            .to_string()
            .contains("<stateful_relayed_words>")
    );
    assert!(server.shutdown_gracefully().await?.success());
    server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let persisted: StatefulMemoryReadResponse = server.request(read).await?;
    assert_eq!(persisted, attributed);
    Ok(())
}

/// Existing explicit relaying and authority guards qualify retention and non-promotion.
/// Automatic document preference capture/application remains assigned to Step 3.
#[tokio::test]
async fn delegated_document_retains_source_and_target_without_general_rule_promotion() -> Result<()>
{
    const GENERAL: &str =
        "Standing rule for all future work: cite arXiv IDs and sections for every factual claim.";
    const DOCUMENT: &str = "Always use author-year citations and plain language in lena-handout.md. Never include arXiv IDs or section locators. Apply this format to every future artifact.";
    const RELAY: &str = "Lena wrote in lena-request.md for lena-handout.md: \"Always use author-year citations and plain language in lena-handout.md. Never include arXiv IDs or section locators. Apply this format to every future artifact.\" Produce lena-handout.md following Lena's document for that artifact only.";
    let responses_server = responses::start_mock_server().await;
    let home = TempDir::new()?;
    let root = TempDir::new()?;
    std::fs::write(root.path().join("lena-request.md"), DOCUMENT)?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Delegated artifact".to_string(),
                roots: vec![codex_app_server_protocol::ProjectRoot {
                    path: root.path().abs(),
                }],
                metadata: None,
                idempotency_key: "delegated-document".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let _: codex_app_server_protocol::StatefulMemoryAddResponse = server.request(|request_id| ClientRequest::StatefulMemoryAdd {
        request_id,
        params: codex_app_server_protocol::StatefulMemoryAddParams {
            thread_id: thread.clone(), expected_project_id: project.project.id.clone(),
            kind: codex_app_server_protocol::StatefulMemoryAddKind::Rule,
            content: GENERAL.to_string(), scope: None, reason: None,
            client_action_id: "direct-general-rule".to_string(), background_section: true,
        },
    }).await?;
    let log = responses::mount_sse_sequence(&responses_server, vec![
        tool_call("read-document", "evidence_read", json!({"relativePath":"lena-request.md"})),
        assistant("Read Lena's request."),
        assistant("Only the named artifact is delegated."),
        assistant("Unrelated review."),
    ]).await;
    run_turn(
        &mut server,
        &thread,
        &format!("{GENERAL} Read lena-request.md."),
    )
    .await?;
    let read_output: Value = serde_json::from_str(
        &log.requests()[1]
            .function_call_output_text("read-document")
            .expect("document output"),
    )?;
    assert!(
        read_output["content"]
            .as_str()
            .expect("source words")
            .contains(DOCUMENT)
    );
    assert!(
        read_output["blackboardEvidence"]["readReceiptId"]
            .as_str()
            .is_some()
    );
    run_turn(&mut server, &thread, RELAY).await?;
    let memory: StatefulMemoryReadResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryRead {
            request_id,
            params: StatefulMemoryReadParams {
                thread_id: thread.clone(),
                expected_project_id: project.project.id.clone(),
                cursor: None,
                limit: None,
                background_section: true,
            },
        })
        .await?;
    let note = memory
        .data
        .iter()
        .find(|item| item.attributed_to.as_deref() == Some("Lena"))
        .expect("retained colleague");
    assert_eq!(
        note.authority,
        Some(StatefulMemoryAuthority::ReportedThirdParty)
    );
    let store = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs())).await?;
    let id = BlackboardEntryId::parse(note.entry_id.clone())?;
    let entry = store
        .get_entry(&project.project.id, &id)
        .await?
        .expect("entry");
    let context = store
        .knowledge_context(&project.project.id, &id)
        .await?
        .expect("context");
    let payload: Value = serde_json::from_str(context.payload.as_deref().expect("source facets"))?;
    assert_eq!(
        (
            context.category,
            context.authority,
            entry.value.root_promotion,
            payload["speaker"].clone(),
            payload["sentence"].clone()
        ),
        (
            KnowledgeCategory::AttributedContext,
            KnowledgeAuthority::ReportedThirdParty,
            RootPromotion::NotPromoted,
            json!("Lena"),
            json!(RELAY.split(" Produce").next().expect("source sentence"))
        )
    );
    assert!(entry.value.content.contains(DOCUMENT));
    assert!(
        entry
            .value
            .provenance
            .source_id
            .starts_with(&format!("user-message:{thread}/"))
    );
    let fresh = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    run_turn(&mut server, &fresh, "Prepare an unrelated review.md.").await?;
    let packet = log.requests()[3].body_json().to_string();
    assert!(packet.contains(GENERAL));
    assert!(!packet.contains(DOCUMENT));
    assert_eq!(
        memory
            .data
            .iter()
            .filter(
                |item| item.section == codex_app_server_protocol::StatefulMemorySection::UserRule
            )
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec![GENERAL]
    );
    Ok(())
}
