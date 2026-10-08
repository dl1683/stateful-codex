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
async fn c3r3_public_exact_proposal_cut_and_terminal_budget_refusals_fit_cold() -> Result<()> {
    let (home, mut server, project, _, responses_server) = setup().await?;
    assert!(server.shutdown_gracefully().await?.success());
    let config = core_test_support::load_default_config_for_test(&home).await;
    let model = codex_core::test_support::construct_model_info_offline("mock-model", &config);
    let catalog = home.path().join("exact-budget-models.json");
    let budgets = [20, 32, 64, 100, 240, 9000];
    let models: Vec<_> = budgets
        .iter()
        .map(|limit| {
            let mut model = model.clone();
            model.slug = format!("exact-{limit}");
            model.truncation_policy =
                codex_protocol::openai_models::TruncationPolicyConfig::bytes(*limit);
            model
        })
        .collect();
    std::fs::write(&catalog, serde_json::to_vec(&json!({"models":models}))?)?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .with_model("exact-9000")
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

    // Create the real excluded entry through normal model dispatch using the
    // host's same-turn source handle, rather than fabricating an entry ID.
    let bodies = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = bodies.clone();
    Mock::given(method("POST")).and(path("/v1/responses")).respond_with(move |request: &Request| {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let mut bodies = captured.lock().unwrap();
        let first = bodies.is_empty();
        bodies.push(body.clone());
        let events = if first {
            let handles = super::super::source_proposals_tests::source_handles(&body).unwrap();
            let handle = &handles["handles"][0];
            let proposal = json!({"type":"sourceProposal","records":[{
                "sourceId":handle[0],"digest":handle[1],"sourceRevision":handle[2],"partIndex":handle[3],
                "spans":[{"startByte":0,"endByte":handle[4],"role":"body"}],
                "category":"background","interpretation":"excluded teal prototype purchase"
            }]});
            vec![responses::ev_function_call("retain-exact", "blackboard_record_batch", &proposal.to_string()),
                responses::ev_completed("retain")]
        } else { vec![responses::ev_completed("done")] };
        ResponseTemplate::new(/*s*/ 200).insert_header("content-type", "text/event-stream")
            .set_body_string(responses::sse(events))
    }).mount(&responses_server).await;
    run_turn(
        &mut server,
        &thread,
        "Mara bought the teal prototype; motor was not damaged.",
    )
    .await?;
    let bodies = bodies.lock().unwrap().clone();
    assert_eq!(bodies.len(), 2);
    let receipt: Value = serde_json::from_str(
        bodies[1]["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| {
                item["call_id"] == "retain-exact" && item["type"] == "function_call_output"
            })
            .unwrap()["output"]
            .as_str()
            .unwrap(),
    )?;
    assert_eq!(
        (
            receipt["applied"].clone(),
            receipt["results"][0]["status"].clone()
        ),
        (json!(false), json!("proposed"))
    );
    let proposal_id = receipt["results"][0]["entryId"]
        .as_str()
        .unwrap()
        .to_string();
    responses_server.reset().await;
    let words = "ordinary Agent \"exact\" control:\nα";
    let agent = model_call(
        &mut server,
        &responses_server,
        &thread,
        "blackboard_record_batch",
        json!({"records":[record("exact-agent", words)]}),
    )
    .await?;
    assert_eq!(agent["recorded"], json!(1), "{agent}");
    let agent_id = agent["results"][0]["entryId"].as_str().unwrap().to_string();
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = pi::BlackboardStore::open(&sqlite).await?;
    let retained = store
        .get_entry(&project, &pi::BlackboardEntryId::parse(&proposal_id)?)
        .await?
        .unwrap();
    let ordinary = store
        .get_entry(&project, &pi::BlackboardEntryId::parse(&agent_id)?)
        .await?
        .unwrap();
    let before = snapshot(&sqlite).await?;
    for round in 0..2 {
        if round == 1 {
            reopen(&mut server, &home, &thread).await?;
        }
        for budget in budgets {
            for (variant, arguments) in [
                ("proposal", json!({"entryId":proposal_id})),
                ("agent", json!({"entryId":agent_id})),
                (
                    "empty-tail",
                    json!({"entryId":agent_id,"expectedEntryRevision":ordinary.revision,
                    "contentOffset":ordinary.value.content.len()}),
                ),
            ] {
                let call_id = format!("exact-{round}-{budget}-{variant}");
                let mock = responses::mount_sse_sequence(
                    &responses_server,
                    vec![
                        responses::sse(vec![
                            responses::ev_function_call(
                                &call_id,
                                "blackboard_query",
                                &arguments.to_string(),
                            ),
                            responses::ev_completed("query"),
                        ]),
                        responses::sse(vec![responses::ev_completed("done")]),
                    ],
                )
                .await;
                server
                    .start_turn_and_wait_for_completion(TurnStartParams {
                        thread_id: thread.clone(),
                        model: Some(format!("exact-{budget}")),
                        input: vec![UserInput::Text {
                            text: "Inspect the exact retained entry.".into(),
                            text_elements: Vec::new(),
                        }],
                        ..Default::default()
                    })
                    .await?;
                let requests = mock.requests();
                assert_eq!(requests.len(), 2);
                let output = requests[1].function_call_output_text(&call_id).unwrap();
                let allowance = ((budget as usize) * 12 / 10).min(9000);
                assert!(
                    serde_json::to_string(&output)?.len() <= allowance,
                    "{round}/{budget}/{variant}: {output}"
                );
                if variant == "proposal" {
                    assert_eq!(
                        output,
                        if budget <= 32 {
                            "budget_insufficient"
                        } else {
                            "entry not found or automatic evidence unavailable"
                        }
                    );
                } else if budget <= 240 {
                    assert_eq!(
                        output,
                        if budget <= 32 {
                            "budget_insufficient"
                        } else if variant == "agent" {
                            "budget_insufficient: increase the response budget; no continuation issued"
                        } else {
                            "budget_insufficient: increase the response budget"
                        }
                    );
                } else {
                    let output: Value = serde_json::from_str(&output)?;
                    assert_eq!(
                        (
                            output["entryId"].clone(),
                            output["complete"].clone(),
                            output["nextContentOffset"].clone()
                        ),
                        (json!(agent_id), json!(true), Value::Null)
                    );
                    assert_eq!(
                        output["content"],
                        if variant == "agent" {
                            json!(ordinary.value.content)
                        } else {
                            json!("")
                        }
                    );
                }
                assert_eq!(snapshot(&sqlite).await?, before);
                assert_eq!(
                    store
                        .get_entry(&project, &pi::BlackboardEntryId::parse(&proposal_id)?)
                        .await?,
                    Some(retained.clone())
                );
                assert_eq!(
                    store
                        .get_entry(&project, &pi::BlackboardEntryId::parse(&agent_id)?)
                        .await?,
                    Some(ordinary.clone())
                );
            }
        }
    }
    Ok(())
}
