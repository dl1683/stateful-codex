//! Public normal-generation tests: no extractor, authority control or memory-only turn.
use super::*;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::RootBlackboardQuery;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;
use wiremock::Mock;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

pub(super) fn source_handles(value: &Value) -> Option<Value> {
    match value {
        Value::String(text) => text
            .split_once("<capture_sources>")
            .and_then(|(_, rest)| rest.split_once("</capture_sources>"))
            .and_then(|(json, _)| serde_json::from_str(json.trim()).ok()),
        Value::Array(values) => values.iter().rev().find_map(source_handles),
        Value::Object(fields) => fields.values().find_map(source_handles),
        Value::Null | Value::Bool(_) | Value::Number(_) => None,
    }
}

#[tokio::test]
async fn c3_public_proposals_recall_cut_source_routes_forget_and_cold_retry() -> Result<()> {
    let (home, mut server, project, thread, responses_server) =
        super::capture_sources_tests::setup().await?;
    let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = calls.clone();
    Mock::given(method("POST")).and(path("/v1/responses"))
        .respond_with(move |request: &Request| {
            let body:Value = serde_json::from_slice(&request.body).unwrap();
            let mut calls = captured.lock().unwrap();
            let sequence = calls.len();
            calls.push(body.clone());
            let response = match sequence {
                0 => {
                    let handles = source_handles(&body).expect("host issued source handles in first normal request");
                    let handle = &handles["handles"][0];
                    let args = json!({"type":"sourceProposal","records":[{
                        "sourceId":handle[0],"digest":handle[1],"sourceRevision":handle[2],"partIndex":handle[3],
                        "spans":[{"startByte":26,"endByte":35,"role":"temporal"},
                            {"startByte":36,"endByte":handle[4],"role":"body"}],
                        "category":"background","interpretation":"Mara bought a prototype; seller and receipt omitted.",
                        "temporal":{"eventTimeSpan":0,"form":"unresolvedRelative"}
                    }]});
                    responses::sse(vec![responses::ev_function_call("proposal", "blackboard_record_batch", &args.to_string()), responses::ev_completed("propose")])
                }
                1 => responses::sse(vec![responses::ev_assistant_message("answer", "The note records the purchase."), responses::ev_completed("answer")]),
                2 => responses::sse(vec![responses::ev_function_call("sources", "conversation_read", &json!({"sourceQuery":"QX-704"}).to_string()), responses::ev_completed("search")]),
                3 => {
                    let handles = source_handles(&calls[0]).unwrap();
                    let seal = &handles["handles"][0];
                    responses::sse(vec![responses::ev_function_call("exact", "conversation_read", &json!({
                        "sourceId":seal[0],"digest":seal[1],"sourceRevision":seal[2]
                    }).to_string()),responses::ev_completed("read")])
                }
                4 => responses::sse(vec![responses::ev_function_call("recall", "memory_read", &json!({"question":"Mara prototype"}).to_string()),responses::ev_completed("recall")]),
                6 => responses::sse(vec![responses::ev_function_call("excluded-search", "conversation_read", &json!({"sourceQuery":"QX-704"}).to_string()), responses::ev_completed("excluded-search")]),
                7 => {
                    let handles = source_handles(&calls[0]).unwrap();
                    let seal = &handles["handles"][0];
                    responses::sse(vec![responses::ev_function_call("excluded-exact", "conversation_read", &json!({
                        "sourceId":seal[0],"digest":seal[1],"sourceRevision":seal[2]
                    }).to_string()),responses::ev_completed("excluded-exact")])
                }
                _ => responses::sse(vec![responses::ev_assistant_message("answer", "The attributed note names QX-704."), responses::ev_completed("answer")]),
            };
            ResponseTemplate::new(200).insert_header("content-type","text/event-stream").set_body_string(response)
        }).mount(&responses_server).await;
    let text = "Notes from 12 June 2024:\r\nYesterday Mara Osei bought the teal Meridian M-17 prototype from Atelier Kestrel in Porto. The receipt is QX-704. Only the left display panel was scratched; the motor was not damaged. These are Nia Vale's notes.";
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.clone(),
            input: vec![UserInput::Text {
                text: text.into(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    assert_eq!(
        calls.lock().unwrap().len(),
        2,
        "only the normal tool call and final answer, no extraction or closing turn"
    );
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = BlackboardStore::open(&sqlite).await?;
    let bodies = calls.lock().unwrap().clone();
    let result = tool_output(&bodies[1], "proposal").unwrap();
    assert_eq!(
        (
            result["applied"].clone(),
            result["results"][0]["status"].clone()
        ),
        (json!(false), json!("proposed"))
    );
    let id = BlackboardEntryId::parse(
        result["results"][0]["entryId"]
            .as_str()
            .unwrap()
            .to_string(),
    )?;
    let proposal = store.proposal_context(&project, &id).await?.unwrap();
    assert_eq!(
        store
            .root_projection(RootBlackboardQuery {
                project_id: project.clone(),
                max_entries: 256
            })
            .await?
            .data
            .len(),
        0
    );
    // Native source storage survives the model-recall cut, including summary-omitted details.
    assert_eq!(
        store
            .read_source_page(
                &project,
                &proposal.source.source_id,
                &proposal.source.digest,
                proposal.source.source_revision,
                /*offset*/ 0
            )
            .await?
            .exact_text,
        text
    );
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.clone(),
            input: vec![UserInput::Text {
                text: "What receipt and damage did the note report?".into(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let bodies = calls.lock().unwrap().clone();
    for (body, call, field) in [
        (&bodies[3], "sources", "sourceQuery"),
        (&bodies[4], "exact", "digest"),
    ] {
        let output = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["call_id"] == call && item["type"] == "function_call_output")
            .unwrap()["output"]
            .as_str()
            .unwrap();
        assert!(
            output.contains("unknown field") && output.contains(field),
            "{output}"
        );
        assert!(!output.contains("QX-704") && !output.contains("Meridian M-17"));
    }
    let recall = tool_output(&bodies[5], "recall").unwrap();
    assert_eq!(recall["entries"][0]["applied"], json!(false));
    assert_eq!(
        recall["entries"][0]["proposal"]["status"],
        json!("proposed")
    );
    let forget = server.send_request("statefulMemory/forget",Some(json!({"threadId":thread,"expectedProjectId":project,"entryId":id.as_str(),"expectedRevision":1,"clientActionId":"forget-proposal"}))).await?;
    let _: StatefulMemoryForgetResponse = server.read_response(forget).await?;
    let before = super::model_retirement_tests::snapshot(&sqlite).await?;
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.clone(),
            input: vec![UserInput::Text {
                text: "independent unrelated control".into(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let bodies = calls.lock().unwrap().clone();
    for (body, call) in [
        (&bodies[7], "excluded-search"),
        (&bodies[8], "excluded-exact"),
    ] {
        let output = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["call_id"] == call && item["type"] == "function_call_output")
            .unwrap()["output"]
            .as_str()
            .unwrap();
        assert!(output.contains("unknown field"));
        assert!(!output.contains("QX-704") && !output.contains("Meridian M-17"));
    }
    assert_eq!(
        super::model_retirement_tests::snapshot(&sqlite).await?,
        before
    );
    assert!(
        store
            .read_source_page(
                &project,
                &proposal.source.source_id,
                &proposal.source.digest,
                proposal.source.source_revision,
                /*offset*/ 0
            )
            .await
            .is_err()
    );
    assert!(server.shutdown_gracefully().await?.success());
    let reopened = BlackboardStore::open(&sqlite).await?;
    reopened.begin_source_index_rebuild(&project).await?;
    while !reopened.maintain_source_index(&project).await? {}
    assert!(
        reopened
            .search_source_ranges(&project, "QX-704", /*after*/ None)
            .await?
            .ranges
            .is_empty()
    );
    assert!(
        reopened
            .search_source_ranges(
                &project,
                "independent unrelated control",
                /*after*/ None
            )
            .await?
            .ranges
            .iter()
            .any(|range| range.exact_text.contains("independent unrelated control"))
    );
    let admission = codex_state::ThreadProjectAdmission::acquire(
        &sqlite,
        codex_protocol::ThreadId::from_string(&thread)?,
        &project,
    )
    .await?
    .unwrap();
    let node = store.get_entry(&project, &id).await?.unwrap().value.node_id;
    let pool = sqlite
        .open_read_only_pool(
            &home.path().join("project_intelligence_1.sqlite"),
            /*busy_timeout*/ None,
        )
        .await?;
    let turn: String =
        sqlx::query_scalar("SELECT turn_id FROM capture_sources WHERE source_id = ?")
            .bind(&proposal.source.source_id)
            .fetch_one(&pool)
            .await?;
    assert!(
        reopened
            .propose_sources(&admission, node, &turn, vec![proposal.source])
            .await
            .is_err()
    );
    Ok(())
}

fn tool_output(body: &Value, call: &str) -> Option<Value> {
    body["input"]
        .as_array()?
        .iter()
        .find(|item| item["call_id"] == call && item["type"] == "function_call_output")
        .and_then(|item| item["output"].as_str())
        .and_then(|output| serde_json::from_str(output).ok())
}

#[tokio::test]
async fn c3_public_all_factual_parts_retained_without_model_writes_or_extra_calls() -> Result<()> {
    let (home, mut server, project, thread, responses_server) =
        super::capture_sources_tests::setup().await?;
    let notes = [
        "Notes from 12 June 2024: yesterday Mara Osei bought the teal Meridian M-17 prototype from Atelier Kestrel in Porto. The receipt is QX-704. Only the left display panel was scratched; the motor was not damaged. These are Nia Vale's notes.\n",
        "Notes from 16 June 2024: Mara Osei owns two test kits, Aster and Bracken. The prototype handoff is planned for 20 June 2024. These are Nia Vale's notes.\n",
        "Notes from 22 June 2024: Mara Osei added a third test kit, Cedar, yesterday. The 20 June handoff was cancelled and did not happen; a replacement is planned for 24 June 2024. These are Nia Vale's notes.\n",
        "Notes from 27 June 2024: yesterday Mara Osei bought the amber Meridian M-18 prototype from Studio Lark in Braga. The receipt is QX-705. These are Nia Vale's notes.\n",
        "Mara Osei says she inspected the prototype a few days ago, but the note has no date. She also mentioned a visit last summer; the hemisphere and year are not supplied. These are Nia Vale's notes.\n",
        "Notes from 12 June 2024: Mara Osei visited the workshop last summer; the hemisphere is not recorded. These are Nia Vale's notes.\n",
        "Notes from 28 June 2024: Nia Vale reports the panel repair occurred between 14 and 16 June 2024, inclusive; the exact day is unknown.\n",
    ];
    for text in notes {
        let mock = responses::mount_sse_once(
            &responses_server,
            responses::sse(vec![
                responses::ev_assistant_message(
                    "answer",
                    "Noted; brief summary deliberately omits names, qualifiers and times.",
                ),
                responses::ev_completed("done"),
            ]),
        )
        .await;
        server
            .start_turn_and_wait_for_completion(TurnStartParams {
                thread_id: thread.clone(),
                input: vec![UserInput::Text {
                    text: text.into(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            })
            .await?;
        assert_eq!(
            mock.requests().len(),
            1,
            "no model extraction, memory-only turn or promotion needed"
        );
    }
    assert!(server.shutdown_gracefully().await?.success());
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let pool = sqlite
        .open_read_only_pool(
            &home.path().join("project_intelligence_1.sqlite"),
            /*busy_timeout*/ None,
        )
        .await?;
    let seals:Vec<String>=sqlx::query_scalar("SELECT metadata FROM capture_sources WHERE project_id = ? ORDER BY observed_sequence LIMIT 8").bind(&project).fetch_all(&pool).await?;
    assert_eq!(seals.len(), notes.len());
    let store = BlackboardStore::open(&sqlite).await?;
    for (metadata, text) in seals.iter().zip(notes) {
        let seal: codex_project_intelligence::SourceSeal = serde_json::from_str(metadata)?;
        assert_eq!(
            store
                .read_source_page(
                    &project,
                    &seal.exact_source_locator,
                    &seal.digest,
                    /*revision*/ 1,
                    /*offset*/ 0
                )
                .await?
                .exact_text,
            text
        );
    }
    assert!(
        store
            .search_source_ranges(&project, "QX-704", /*after*/ None)
            .await?
            .ranges
            .iter()
            .any(|range| range.exact_text.contains("QX-704"))
    );
    assert!(
        store
            .root_projection(RootBlackboardQuery {
                project_id: project,
                max_entries: 256
            })
            .await?
            .data
            .is_empty()
    );
    Ok(())
}
