//! Public qualification of the further reduced memory capabilities.

use super::*;
use codex_app_server_protocol::StatefulMemoryAuthority;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn user_corrected_assistant_memory_refuses_all_model_mutations_after_restart() -> Result<()> {
    let (home, mut server, project, mut thread, responses_server) = setup().await?;
    let pool = SqliteConfig::new_for_testing(home.path().abs())
        .open_read_write_pool(&home.path().join("project_intelligence_1.sqlite"))
        .await?;
    for kind in ["decision", "fact"] {
        for authority in [None, Some("assistant_reported"), Some("human_direct")] {
            let key = format!("corrected-{kind}-{authority:?}");
            let original_words = format!("Use SQLite for {key}");
            let record = json!({"records":[{
                "idempotencyKey":key,"kind":kind,"content":original_words,
                "confidenceBasisPoints":9000,"verification":"unverified",
                "importance":"high","rootPromotion":"promoted"
            }]});
            let log = responses::mount_sse_sequence(
                &responses_server,
                vec![
                    responses::sse(vec![
                        responses::ev_function_call(
                            "original",
                            "blackboard_record_batch",
                            &record.to_string(),
                        ),
                        responses::ev_completed("record"),
                    ]),
                    responses::sse(vec![responses::ev_completed("done")]),
                ],
            )
            .await;
            run_turn(&mut server, &thread, "Record the assistant's decision.").await?;
            assert_eq!(log.requests().len(), 2);
            let original = read(&mut server, &thread, &project)
                .await?
                .data
                .into_iter()
                .find(|item| item.content == original_words)
                .expect("original");
            let corrected: StatefulMemoryCorrectResponse = server
                .request(|request_id| ClientRequest::StatefulMemoryCorrect {
                    request_id,
                    params: StatefulMemoryCorrectParams {
                        thread_id: thread.clone(),
                        expected_project_id: project.clone(),
                        entry_id: original.entry_id.clone(),
                        expected_revision: original.revision,
                        content: format!(
                            "Use PostgreSQL for {key}. Reason: I require local SQL transactions."
                        ),
                        background_section: true,
                    },
                })
                .await?;
            assert_eq!(corrected.item.source, BlackboardProvenanceKind::User);
            if let Some(authority) = authority {
                sqlx::query("INSERT INTO knowledge_context (entry_id,revision,project_id,category,authority,validity) VALUES (?,1,?,'decision',?,'current') ON CONFLICT(entry_id,revision) DO UPDATE SET authority=excluded.authority")
                    .bind(&corrected.item.entry_id).bind(&project).bind(authority).execute(&pool).await?;
            } else {
                sqlx::query("DELETE FROM knowledge_context WHERE entry_id=?")
                    .bind(&corrected.item.entry_id)
                    .execute(&pool)
                    .await?;
            }
            assert!(server.shutdown_gracefully().await?.success());
            server = TestAppServer::builder()
                .with_codex_home(home.path())
                .build_initialized()
                .await?;
            thread = start_thread(&mut server, &project).await?;
            let before = read(&mut server, &thread, &project).await?;
            let mut calls = [
                json!({"action":"revise","content":"Use Redis without the reason."}),
                json!({"action":"setRootPromotion","rootPromotion":"promoted"}),
                json!({"action":"supersede","successorEntryId":original.entry_id}),
            ]
            .into_iter()
            .map(|mut mutation| {
                mutation["entryId"] = json!(corrected.item.entry_id);
                mutation["expectedRevision"] = json!(corrected.item.revision);
                ("blackboard_update_batch", json!({"mutations":[mutation]}))
            })
            .collect::<Vec<_>>();
            calls.push(("blackboard_record_batch", json!({"records":[{
                "idempotencyKey":format!("successor-{key}"),"kind":kind,"content":"Use remote persistence",
                "confidenceBasisPoints":9000,"verification":"unverified",
                "importance":"high","rootPromotion":"promoted",
                "supersedes":[{"entryId":corrected.item.entry_id,"revision":corrected.item.revision}]
            }]})));
            let log = responses::mount_sse_sequence(
                &responses_server,
                calls
                    .iter()
                    .enumerate()
                    .flat_map(|(index, (tool, args))| {
                        vec![
                            responses::sse(vec![
                                responses::ev_function_call(
                                    &format!("refused-{index}"),
                                    tool,
                                    &args.to_string(),
                                ),
                                responses::ev_completed("attempt"),
                            ]),
                            responses::sse(vec![responses::ev_completed("done")]),
                        ]
                    })
                    .collect(),
            )
            .await;
            for index in 0..calls.len() {
                run_turn(&mut server, &thread, "Inspect the recorded memory.").await?;
                let requests = log.requests();
                let output: serde_json::Value = serde_json::from_str(
                    &requests[index * 2 + 1]
                        .function_call_output_text(&format!("refused-{index}"))
                        .expect("refusal"),
                )?;
                assert_eq!(output["failed"], json!(1));
                assert!(
                    output["results"][0]["error"]
                        .as_str()
                        .expect("error")
                        .contains("direct-human")
                );
                assert_eq!(read(&mut server, &thread, &project).await?, before);
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn model_can_still_revise_and_replace_assistant_origin_memory() -> Result<()> {
    let (_home, mut server, project, thread, responses_server) = setup().await?;
    let record = json!({"records":[{
        "idempotencyKey":"assistant-original","kind":"decision","content":"Use SQLite",
        "confidenceBasisPoints":9000,"verification":"unverified",
        "importance":"high","rootPromotion":"promoted"
    }]});
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "assistant-record",
                    "blackboard_record_batch",
                    &record.to_string(),
                ),
                responses::ev_completed("record"),
            ]),
            responses::sse(vec![responses::ev_completed("done")]),
        ],
    )
    .await;
    run_turn(&mut server, &thread, "Record the assistant's decision.").await?;
    assert_eq!(log.requests().len(), 2);
    let original = read(&mut server, &thread, &project).await?.data.remove(0);
    assert_eq!(original.source, BlackboardProvenanceKind::Agent);
    assert_ne!(
        original.authority,
        Some(StatefulMemoryAuthority::HumanDirect)
    );
    let revise = json!({"mutations":[{"action":"revise","entryId":original.entry_id,"expectedRevision":original.revision,"content":"Use Redis"}]});
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "assistant-revise",
                    "blackboard_update_batch",
                    &revise.to_string(),
                ),
                responses::ev_completed("revise"),
            ]),
            responses::sse(vec![responses::ev_completed("done")]),
        ],
    )
    .await;
    run_turn(&mut server, &thread, "Revise the assistant's decision.").await?;
    let output: serde_json::Value = serde_json::from_str(
        &log.requests()[1]
            .function_call_output_text("assistant-revise")
            .expect("updated"),
    )?;
    assert_eq!(output["updated"], json!(1));
    let revised = read(&mut server, &thread, &project).await?.data.remove(0);
    assert_eq!(
        (&revised.content, revised.source),
        (&"Use Redis".to_string(), BlackboardProvenanceKind::Agent)
    );
    let successor = json!({"records":[{
        "idempotencyKey":"assistant-successor","kind":"decision","content":"Use PostgreSQL",
        "confidenceBasisPoints":9000,"verification":"unverified",
        "importance":"high","rootPromotion":"promoted",
        "supersedes":[{"entryId":revised.entry_id,"revision":revised.revision}]
    }]});
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "assistant-succeed",
                    "blackboard_record_batch",
                    &successor.to_string(),
                ),
                responses::ev_completed("succeed"),
            ]),
            responses::sse(vec![responses::ev_completed("done")]),
        ],
    )
    .await;
    run_turn(&mut server, &thread, "Replace the assistant's decision.").await?;
    let output: serde_json::Value = serde_json::from_str(
        &log.requests()[1]
            .function_call_output_text("assistant-succeed")
            .expect("recorded"),
    )?;
    assert_eq!(output["recorded"], json!(1));
    let current = read(&mut server, &thread, &project).await?.data;
    assert_eq!(current.len(), 1);
    assert_eq!(
        (&current[0].content, current[0].source),
        (
            &"Use PostgreSQL".to_string(),
            BlackboardProvenanceKind::Agent
        )
    );
    assert_ne!(
        current[0].authority,
        Some(StatefulMemoryAuthority::HumanDirect)
    );
    assert_eq!(current[0].replaces[0].content, "Use Redis");
    Ok(())
}

#[tokio::test]
async fn model_routes_refuse_direct_human_meaning_and_leave_whole_items_unchanged() -> Result<()> {
    let (_home, mut server, project, thread, responses_server) = setup().await?;
    let record = json!({"records":[{
        "idempotencyKey":"assistant-replacement","kind":"decision","content":"Use Redis",
        "confidenceBasisPoints":9000,"verification":"unverified",
        "importance":"high","rootPromotion":"promoted"
    }]});
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "replacement",
                    "blackboard_record_batch",
                    &record.to_string(),
                ),
                responses::ev_completed("record"),
            ]),
            responses::sse(vec![responses::ev_completed("done")]),
        ],
    )
    .await;
    run_turn(
        &mut server,
        &thread,
        "Record the assistant's replacement candidate.",
    )
    .await?;
    assert_eq!(log.requests().len(), 2);
    let assistant = read(&mut server, &thread, &project).await?.data.remove(0);
    for (index, kind) in [
        StatefulMemoryAddKind::Decision,
        StatefulMemoryAddKind::Background,
        StatefulMemoryAddKind::Note,
        StatefulMemoryAddKind::Rule,
    ]
    .into_iter()
    .enumerate()
    {
        let added: StatefulMemoryAddResponse = server
            .request(|request_id| ClientRequest::StatefulMemoryAdd {
                request_id,
                params: StatefulMemoryAddParams {
                    thread_id: thread.clone(),
                    expected_project_id: project.clone(),
                    kind,
                    content: "Use SQLite".to_string(),
                    reason: (kind == StatefulMemoryAddKind::Decision)
                        .then(|| "smaller deployments".to_string()),
                    scope: None,
                    client_action_id: format!("direct-{index}"),
                    background_section: true,
                },
            })
            .await?;
        assert_eq!(
            added.item.authority,
            Some(StatefulMemoryAuthority::HumanDirect)
        );
        if kind == StatefulMemoryAddKind::Decision {
            assert_eq!(added.item.content, "Use SQLite Reason: smaller deployments");
        }
        let reference = json!({"entryId":added.item.entry_id,"revision":added.item.revision});
        let mutations = [
            json!({"action":"revise","content":"Use Redis"}),
            json!({"action":"revise","kind":"decision"}),
            json!({"action":"revise","evidence":[],"premises":[]}),
            json!({"action":"setRootPromotion","rootPromotion":"promoted"}),
            json!({"action":"supersede","successorEntryId":assistant.entry_id}),
        ];
        let mut calls = mutations
            .into_iter()
            .map(|mut mutation| {
                mutation["entryId"] = json!(added.item.entry_id);
                mutation["expectedRevision"] = json!(added.item.revision);
                ("blackboard_update_batch", json!({"mutations":[mutation]}))
            })
            .collect::<Vec<_>>();
        calls.push((
            "blackboard_record_batch",
            json!({"records":[{
                "idempotencyKey":format!("model-successor-{index}"),
                "kind":"decision","content":"Use remote persistence",
                "confidenceBasisPoints":9000,"verification":"unverified",
                "importance":"high","rootPromotion":"promoted","supersedes":[reference]
            }]}),
        ));
        let log = responses::mount_sse_sequence(
            &responses_server,
            calls
                .iter()
                .enumerate()
                .flat_map(|(call_index, (tool, arguments))| {
                    vec![
                        responses::sse(vec![
                            responses::ev_function_call(
                                &format!("direct-{index}-{call_index}"),
                                tool,
                                &arguments.to_string(),
                            ),
                            responses::ev_completed("attempt"),
                        ]),
                        responses::sse(vec![responses::ev_completed("done")]),
                    ]
                })
                .collect(),
        )
        .await;
        let before = read(&mut server, &thread, &project).await?;
        for call_index in 0..calls.len() {
            run_turn(&mut server, &thread, "Inspect the recorded decision.").await?;
            let requests = log.requests();
            let output = requests[call_index * 2 + 1]
                .function_call_output_text(&format!("direct-{index}-{call_index}"))
                .expect("model refusal");
            let refusal: serde_json::Value = serde_json::from_str(&output)?;
            assert_eq!(refusal["failed"], json!(1));
            assert!(
                refusal["results"][0]["error"]
                    .as_str()
                    .expect("error")
                    .contains("direct-human")
            );
            // Includes content, reason, source, authority, revision, timestamp and history;
            // also proves that record succession created no additional public item.
            assert_eq!(read(&mut server, &thread, &project).await?, before);
        }
        let corrected: StatefulMemoryCorrectResponse = server
            .request(|request_id| ClientRequest::StatefulMemoryCorrect {
                request_id,
                params: StatefulMemoryCorrectParams {
                    thread_id: thread.clone(),
                    expected_project_id: project.clone(),
                    entry_id: added.item.entry_id.clone(),
                    expected_revision: added.item.revision,
                    content: "Use PostgreSQL by my explicit correction.".to_string(),
                    background_section: true,
                },
            })
            .await?;
        assert_eq!(
            corrected.item.content,
            "Use PostgreSQL by my explicit correction."
        );
        let _: StatefulMemoryForgetResponse = server
            .request(|request_id| ClientRequest::StatefulMemoryForget {
                request_id,
                params: StatefulMemoryForgetParams {
                    thread_id: thread.clone(),
                    expected_project_id: project.clone(),
                    entry_id: corrected.item.entry_id.clone(),
                    expected_revision: corrected.item.revision,
                },
            })
            .await?;
        assert_eq!(
            read(&mut server, &thread, &project).await?.data,
            vec![assistant.clone()]
        );
    }
    Ok(())
}
