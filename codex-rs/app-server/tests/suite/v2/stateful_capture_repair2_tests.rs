use super::*;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::sync::Mutex;
use wiremock::Mock;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

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
                        ProposalRefusal::Oversized=>{record["interpretation"]=json!("x".repeat(33000));vec![record]}
                    };
                    vec![responses::ev_function_call(&emitted_call_id,"blackboard_record_batch",&json!({"type":"sourceProposal","records":records}).to_string()),responses::ev_completed("request")]
                } else {vec![responses::ev_completed("done")]};
                ResponseTemplate::new(200).insert_header("content-type","text/event-stream").set_body_string(responses::sse(events))
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
