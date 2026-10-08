use super::*;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::sync::Mutex;
use wiremock::Mock;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test]
async fn c3r2_public_ordinary_agent_batch_above_32k_commits_and_replays_cold() -> Result<()> {
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let records: Vec<_> = (0..9).map(|index| json!({
        "idempotencyKey":format!("compat-{index}"), "kind":"note", "content":format!("{index}{}","a".repeat(/*n*/ 3999)),
        "confidenceBasisPoints":0,"verification":"unverified","importance":"normal","rootPromotion":"notPromoted"
    })).collect();
    let request = json!({"records":records,"relations":[]});
    assert!(request.to_string().len() > 32768);
    let result: Value = serde_json::from_str(
        &model_output_with_id(
            &mut server,
            &responses_server,
            &thread,
            "blackboard_record_batch",
            request.clone(),
            "compat-agent",
        )
        .await?,
    )?;
    assert_eq!(
        (
            result["recorded"].clone(),
            result["failed"].clone(),
            result["relationsRecorded"].clone(),
            result["relationsFailed"].clone()
        ),
        (json!(9), json!(0), json!(0), json!(0))
    );
    assert!(result.to_string().len() <= 9000);
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = pi::BlackboardStore::open(&sqlite).await?;
    let node = pi::HierarchyStore::open(&sqlite)
        .await?
        .project_node(&project)
        .await?
        .unwrap()
        .id;
    let mut committed = Vec::new();
    for (index, receipt) in result["results"].as_array().unwrap().iter().enumerate() {
        let id = pi::BlackboardEntryId::parse(receipt["entryId"].as_str().unwrap())?;
        let entry = store.get_entry(&project, &id).await?.unwrap();
        let expected = pi::NewBlackboardEntry {
            project_id: project.clone(),
            node_id: node.clone(),
            kind: pi::BlackboardKind::Note,
            content: records[index]["content"].as_str().unwrap().into(),
            structured_value: None,
            confidence: pi::ConfidenceScore::from_basis_points(/*value*/ 0)?,
            verification: pi::BlackboardVerification::Unverified,
            importance: pi::BlackboardImportance::Normal,
            root_promotion: pi::RootPromotion::NotPromoted,
            evidence: Vec::new(),
            premises: Vec::new(),
            provenance: pi::BlackboardProvenance {
                kind: pi::BlackboardProvenanceKind::Agent,
                source_id: "compat-agent".into(),
            },
        };
        assert_eq!(entry.value, expected);
        assert_eq!(
            (entry.revision, entry.state, entry.superseded_by.clone()),
            (1, pi::BlackboardEntryState::Active, None)
        );
        committed.push(entry);
    }
    assert_eq!(committed.len(), 9);
    let before = snapshot(&sqlite).await?;
    reopen(&mut server, &home, &thread).await?;
    let replay: Value = serde_json::from_str(
        &model_output_with_id(
            &mut server,
            &responses_server,
            &thread,
            "blackboard_record_batch",
            request,
            "compat-agent",
        )
        .await?,
    )?;
    assert_eq!(replay, result);
    assert_eq!(snapshot(&sqlite).await?, before);
    let reopened = pi::BlackboardStore::open(&sqlite).await?;
    for entry in committed {
        assert_eq!(reopened.get_entry(&project, &entry.id).await?, Some(entry));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ProposalRefusal {
    Valid,
    Empty,
    TooMany,
    Oversized,
}

#[tokio::test]
async fn c3r2_public_decoded_proposal_refusals_fit_tiny_budgets_without_writes() -> Result<()> {
    let (home, mut server, project, _, responses_server) = setup().await?;
    assert!(server.shutdown_gracefully().await?.success());
    let config = core_test_support::load_default_config_for_test(&home).await;
    let model = codex_core::test_support::construct_model_info_offline("mock-model", &config);
    let catalog = home.path().join("proposal-budget-models.json");
    let budgets = [20, 32, 64, 100, 240];
    let models: Vec<_> = budgets
        .iter()
        .map(|limit| {
            let mut model = model.clone();
            model.slug = format!("proposal-{limit}");
            model.truncation_policy =
                codex_protocol::openai_models::TruncationPolicyConfig::bytes(*limit);
            model
        })
        .collect();
    std::fs::write(&catalog, serde_json::to_vec(&json!({"models":models}))?)?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .with_model("proposal-20")
        .with_root_config(&format!(
            "model_catalog_json = {}",
            serde_json::to_string(&catalog)?
        ))
        .write(home.path())?;
    server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let thread = start_thread(&mut server, &project).await?;
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    for budget in budgets {
        for variant in [
            ProposalRefusal::Valid,
            ProposalRefusal::Empty,
            ProposalRefusal::TooMany,
            ProposalRefusal::Oversized,
        ] {
            let before = snapshot(&sqlite).await?;
            let bodies = Arc::new(Mutex::new(Vec::<Value>::new()));
            let captured = bodies.clone();
            let call_id = format!("refusal-{budget}-{}", uuid::Uuid::new_v4());
            let emitted_call_id = call_id.clone();
            responses_server.reset().await;
            Mock::given(method("POST")).and(path("/v1/responses")).respond_with(move |request:&Request| {
                let body:Value=serde_json::from_slice(&request.body).unwrap();
                let mut bodies=captured.lock().unwrap();
                let first=bodies.is_empty();bodies.push(body.clone());
                let events=if first {
                    let handles=super::super::source_proposals_tests::source_handles(&body).unwrap();
                    let handle=&handles["handles"][0];
                    let mut record=json!({"sourceId":handle[0],"digest":handle[1],"sourceRevision":handle[2],"partIndex":handle[3],
                        "spans":[{"startByte":0,"endByte":handle[4],"role":"body"}],"category":"background","interpretation":"Cedar equipment is teal."});
                    let records=match variant {
                        ProposalRefusal::Valid=>vec![record],ProposalRefusal::Empty=>vec![],ProposalRefusal::TooMany=>vec![record;25],
                        ProposalRefusal::Oversized=>{record["interpretation"]=json!("x".repeat(/*n*/ 33000));vec![record]}
                    };
                    vec![responses::ev_function_call(&emitted_call_id,"blackboard_record_batch",&json!({"type":"sourceProposal","records":records}).to_string()),responses::ev_completed("request")]
                } else {vec![responses::ev_completed("done")]};
                ResponseTemplate::new(/*s*/ 200).insert_header("content-type","text/event-stream").set_body_string(responses::sse(events))
            }).mount(&responses_server).await;
            server
                .start_turn_and_wait_for_completion(TurnStartParams {
                    thread_id: thread.clone(),
                    model: Some(format!("proposal-{budget}")),
                    input: vec![UserInput::Text {
                        text: "Cedar valve is teal.".into(),
                        text_elements: Vec::new(),
                    }],
                    ..Default::default()
                })
                .await?;
            let bodies = bodies.lock().unwrap().clone();
            assert_eq!(bodies.len(), 2);
            let output = bodies[1]["input"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["call_id"] == call_id && item["type"] == "function_call_output")
                .unwrap()["output"]
                .as_str()
                .unwrap();
            let allowance = (budget as usize) * 12 / 10;
            assert!(
                serde_json::to_string(output)?.len() <= allowance,
                "{budget}: {output}"
            );
            if budget == 20 {
                assert_eq!(output, "budget_insufficient");
            }
            assert_eq!(snapshot(&sqlite).await?, before);
        }
    }
    Ok(())
}

#[tokio::test]
async fn c3r2_public_memory_read_matching_entry_advances_or_terminally_refuses_cold() -> Result<()>
{
    let (home, mut server, project, _, responses_server) = setup().await?;
    assert!(server.shutdown_gracefully().await?.success());
    let config = core_test_support::load_default_config_for_test(&home).await;
    let model = codex_core::test_support::construct_model_info_offline("mock-model", &config);
    let catalog = home.path().join("memory-budget-models.json");
    let models: Vec<_> = [("memory-small", 1200), ("memory-large", 9000)]
        .into_iter()
        .map(|(slug, limit)| {
            let mut model = model.clone();
            model.slug = slug.into();
            model.truncation_policy =
                codex_protocol::openai_models::TruncationPolicyConfig::bytes(limit);
            model
        })
        .collect();
    std::fs::write(&catalog, serde_json::to_vec(&json!({"models":models}))?)?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .with_model("memory-large")
        .with_root_config(&format!(
            "model_catalog_json = {}",
            serde_json::to_string(&catalog)?
        ))
        .write(home.path())?;
    server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let thread = start_thread(&mut server, &project).await?;
    let written = model_call(
        &mut server,
        &responses_server,
        &thread,
        "blackboard_record_batch",
        json!({"records":[record("memory-budget",&format!("equipment {}","a".repeat(/*n*/ 3990)))]}),
    )
    .await?;
    assert_eq!(written["recorded"], json!(1));
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let before = snapshot(&sqlite).await?;
    for round in 0..2 {
        if round == 1 {
            reopen(&mut server, &home, &thread).await?;
        }
        for (model, budget) in [("memory-small", 1440), ("memory-large", 9000)] {
            let call_id = format!("memory-{round}-{model}");
            let mock = responses::mount_sse_sequence(
                &responses_server,
                vec![
                    responses::sse(vec![
                        responses::ev_function_call(
                            &call_id,
                            "memory_read",
                            &json!({"question":"equipment","includeHistory":false}).to_string(),
                        ),
                        responses::ev_completed("read"),
                    ]),
                    responses::sse(vec![responses::ev_completed("done")]),
                ],
            )
            .await;
            server
                .start_turn_and_wait_for_completion(TurnStartParams {
                    thread_id: thread.clone(),
                    model: Some(model.into()),
                    input: vec![UserInput::Text {
                        text: "Inspect the retained project memory.".into(),
                        text_elements: Vec::new(),
                    }],
                    ..Default::default()
                })
                .await?;
            let requests = mock.requests();
            assert_eq!(requests.len(), 2);
            let output = requests[1].function_call_output_text(&call_id).unwrap();
            assert!(output.len() <= budget);
            if model == "memory-small" {
                assert_eq!(output, "budget_insufficient");
            } else {
                let output: Value = serde_json::from_str(&output)
                    .map_err(|error| anyhow::anyhow!("{error}: delivered {output}"))?;
                assert_eq!(
                    (
                        output["entries"].as_array().unwrap().len(),
                        output["entries"][0]["entryId"].clone(),
                        output["turns"].clone()
                    ),
                    (1, written["results"][0]["entryId"].clone(), json!([]))
                );
            }
            assert_eq!(snapshot(&sqlite).await?, before);
        }
    }
    Ok(())
}
