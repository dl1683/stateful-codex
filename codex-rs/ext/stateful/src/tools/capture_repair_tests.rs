use super::*;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolName;
use codex_extension_api::ToolPayload;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::SourceObservation;
use codex_project_intelligence::SourceProposal;
use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use codex_utils_output_truncation::TruncationPolicy;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

const PROJECT: &str = "project-1";
const THREAD: &str = "00000000-0000-0000-0000-000000000001";

pub(super) fn call(
    tool: &str,
    arguments: serde_json::Value,
    budget: usize,
    source: ToolCallSource,
) -> ToolCall<'static> {
    ToolCall {
        turn_id: "turn-1".into(),
        call_id: "repair".into(),
        tool_name: ToolName::plain(tool),
        model: "test".into(),
        codex_turn_metadata: None,
        truncation_policy: TruncationPolicy::Bytes(budget),
        source,
        conversation_history: Default::default(),
        turn_item_emitter: Arc::new(codex_extension_api::NoopTurnItemEmitter),
        environments: Vec::new(),
        payload: ToolPayload::Function {
            arguments: arguments.to_string(),
        },
    }
}

#[tokio::test]
async fn c3r1_decoding_errors_fit_direct_and_code_mode_budgets_without_writes() {
    let home = TempDir::new().unwrap();
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let services = ProjectIntelligenceServices::new(sqlite.clone());
    let store = services.blackboard().await.unwrap();
    let before = store.project_revision(PROJECT).await.unwrap();
    let tool = blackboard_write::BlackboardBatchRecordTool::new(
        PROJECT.into(),
        THREAD.into(),
        services.clone(),
        Arc::new(InMemoryThreadStore::default()),
        /*event_sink*/ None,
        VisibleRootRegistry::default(),
    );
    for source in [
        ToolCallSource::Direct,
        ToolCallSource::CodeMode {
            cell_id: "cell".into(),
            runtime_tool_call_id: "nested".into(),
        },
    ] {
        for key in [
            "!~@#$%^&*()-+=|:;<>?".repeat(1000),
            "\"\\\n".repeat(2000),
            "😀é".repeat(2500),
        ] {
            for budget in [20, 32, 64, 100, 240, 9000, 40000] {
                let mut arguments = json!({"type":"sourceProposal", "records":[]});
                arguments[&key] = json!(1);
                let call = call("blackboard_record_batch", arguments, budget, source.clone());
                assert!(call.function_arguments().unwrap().len() < 32768);
                let Err(FunctionCallError::RespondToModel(message)) =
                    tool.handle(call.clone()).await
                else {
                    panic!("malformed proposal must refuse");
                };
                assert!(
                    serde_json::to_string(&message).unwrap().len()
                        <= call.response_byte_budget(MAX_RESPONSE_BYTES)
                );
                assert!(!message.contains(&key));
                assert!(message.starts_with("invalid tool arguments"), "{message}");
            }
        }
    }
    let Err(FunctionCallError::RespondToModel(message)) = tool
        .handle(call(
            "blackboard_record_batch",
            json!({"records":[{"kind":"note"}]}),
            /*budget*/ 9000,
            ToolCallSource::Direct,
        ))
        .await
    else {
        panic!("incomplete Agent record must refuse")
    };
    assert!(
        message.contains("missing field") && message.contains("idempotencyKey"),
        "{message}"
    );
    assert_eq!(store.project_revision(PROJECT).await.unwrap(), before);
    drop(tool);
    drop(services);
    let reopened = codex_project_intelligence::BlackboardStore::open(&sqlite)
        .await
        .unwrap();
    assert_eq!(reopened.project_revision(PROJECT).await.unwrap(), before);
}

#[tokio::test]
async fn c3r2_automatic_proposal_recall_cut_keeps_storage_and_agent_control_cold() {
    let home = TempDir::new().unwrap();
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let project_id = crate::capture_test_support::thread_project(&sqlite, THREAD).await;
    let services = ProjectIntelligenceServices::new(sqlite.clone());
    let node = services.project_node_id(&project_id).await.unwrap();
    let store = services.blackboard().await.unwrap();
    let text = "Mara bought the teal prototype; motor was not damaged.";
    let admission = codex_state::ThreadProjectAdmission::acquire(
        &sqlite,
        codex_protocol::ThreadId::from_string(THREAD).unwrap(),
        &project_id,
    )
    .await
    .unwrap()
    .unwrap();
    let seal = store
        .observe_source(
            SourceObservation {
                project_id: project_id.clone(),
                authoritative_thread_id: THREAD.into(),
                binding_generation: admission.binding_generation(),
                original_event_id: "original".into(),
                turn_id: "turn-1".into(),
                part_index: 0,
                source_revision: 1,
                complete_envelope: true,
                incomplete_reason: None,
                ordered_spans: vec![codex_project_intelligence::SourceSpan {
                    start_byte: 0,
                    end_byte: text.len() as u32,
                    role: codex_project_intelligence::SourceSpanRole::Body,
                }],
            },
            text,
        )
        .await
        .unwrap();
    let proposal: SourceProposal = serde_json::from_value(json!({
        "sourceId":seal.exact_source_locator, "digest":seal.digest, "sourceRevision":1, "partIndex":0,
        "spans":[{"startByte":0,"endByte":text.len(),"role":"body"}],
        "category":"background", "interpretation":"needle prototype purchase"
    })).unwrap();
    let result = store
        .propose_sources(&admission, node.clone(), "turn-1", vec![proposal.clone()])
        .await
        .unwrap();
    let id = BlackboardEntryId::parse(result[0].entry_id.clone().unwrap()).unwrap();
    let before = store.get_entry(&project_id, &id).await.unwrap();
    let agent = codex_project_intelligence::NewBlackboardEntry {
        project_id: project_id.clone(),
        node_id: before.as_ref().unwrap().value.node_id.clone(),
        kind: codex_project_intelligence::BlackboardKind::Note,
        content: "independent Agent positive control".into(),
        structured_value: None,
        confidence: codex_project_intelligence::ConfidenceScore::from_basis_points(
            /*value*/ 9000,
        )
        .unwrap(),
        verification: codex_project_intelligence::BlackboardVerification::Unverified,
        importance: codex_project_intelligence::BlackboardImportance::Normal,
        root_promotion: codex_project_intelligence::RootPromotion::NotPromoted,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: codex_project_intelligence::BlackboardProvenance {
            kind: codex_project_intelligence::BlackboardProvenanceKind::Agent,
            source_id: "turn-1".into(),
        },
    };
    store
        .create_entry(BlackboardEntryId::parse("agent-control").unwrap(), agent)
        .await
        .unwrap();
    store
        .create_relation(
            codex_project_intelligence::BlackboardRelationId::parse("proposal-link").unwrap(),
            codex_project_intelligence::NewBlackboardRelation {
                project_id: project_id.clone(),
                from_entry_id: BlackboardEntryId::parse("agent-control").unwrap(),
                to_entry_id: id.clone(),
                kind: codex_project_intelligence::BlackboardRelationKind::RelatedTo,
                note: Some("needle prototype purchase".into()),
                confidence: codex_project_intelligence::ConfidenceScore::from_basis_points(
                    /*value*/ 0,
                )
                .unwrap(),
                provenance: codex_project_intelligence::BlackboardProvenance {
                    kind: codex_project_intelligence::BlackboardProvenanceKind::Agent,
                    source_id: "turn-1".into(),
                },
            },
        )
        .await
        .unwrap();
    drop(admission);
    drop(services);
    for _ in 0..2 {
        let services = ProjectIntelligenceServices::new(sqlite.clone());
        let tool = blackboard::BlackboardQueryTool::new(
            project_id.clone(),
            THREAD.into(),
            services.clone(),
            Arc::new(InMemoryThreadStore::default()),
            VisibleRootRegistry::default(),
        );
        for budget in [400, 600, 1000, 2000, 9000] {
            let output = tool
                .handle(call(
                    "blackboard_query",
                    json!({"text":"needle prototype"}),
                    budget,
                    ToolCallSource::Direct,
                ))
                .await
                .unwrap();
            let value: serde_json::Value = serde_json::from_str(&output.log_output()).unwrap();
            assert_eq!(
                (value["data"].clone(), value["truncated"].clone()),
                (json!([]), json!(false))
            );
        }
        for source in [
            ToolCallSource::Direct,
            ToolCallSource::CodeMode {
                cell_id: "cell".into(),
                runtime_tool_call_id: "nested".into(),
            },
        ] {
            for budget in [2, 4, 8, 20, 32, 64, 100, 240, 9000] {
                for arguments in [
                    json!({"entryId":id.as_str()}),
                    json!({"entryId":"agent-control"}),
                    json!({"entryId":"agent-control","expectedEntryRevision":1,
                        "contentOffset":"independent Agent positive control".len()}),
                    json!({"entryId":"agent-control","expectedEntryRevision":2}),
                    json!({"entryId":id.as_str(),"text":"invalid combination"}),
                ] {
                    let call = call(
                        "blackboard_query",
                        arguments.clone(),
                        budget,
                        source.clone(),
                    );
                    let allowance = call.response_byte_budget(MAX_RESPONSE_BYTES);
                    match tool.handle(call).await {
                        Err(FunctionCallError::RespondToModel(message)) => {
                            assert!(
                                serde_json::to_string(&message).unwrap().len() <= allowance,
                                "{budget}: {message}"
                            );
                            if arguments == json!({"entryId":id.as_str()}) && allowance >= 51 {
                                assert_eq!(
                                    message,
                                    "entry not found or automatic evidence unavailable"
                                );
                            } else if allowance == 24 {
                                assert_eq!(message, "budget_insufficient");
                            }
                        }
                        Ok(output) => {
                            assert_eq!(arguments["entryId"], json!("agent-control"));
                            assert!(output.log_output().len() <= allowance);
                            let result: serde_json::Value =
                                serde_json::from_str(&output.log_output()).unwrap();
                            assert_eq!(result["entryId"], json!("agent-control"));
                            if allowance == MAX_RESPONSE_BYTES {
                                assert_eq!(result["complete"], json!(true));
                            } else if result["complete"] == false {
                                assert!(result["nextContentOffset"].as_u64().unwrap() > 0);
                                assert!(
                                    "independent Agent positive control"
                                        .starts_with(result["content"].as_str().unwrap())
                                );
                            }
                        }
                        Err(FunctionCallError::Fatal(message)) => panic!("{message}"),
                    }
                }
            }
        }
        let memory = memory_read::MemoryReadTool::new(project_id.clone(), services.clone());
        for arguments in [
            json!({"question":"needle", "includeHistory":false}),
            json!({"question":"needle", "since":"2020-01-01"}),
        ] {
            let output = memory
                .handle(call(
                    "memory_read",
                    arguments,
                    /*budget*/ 9000,
                    ToolCallSource::Direct,
                ))
                .await
                .unwrap();
            let value: serde_json::Value = serde_json::from_str(&output.log_output()).unwrap();
            assert_eq!(value["entries"], json!([]));
            assert!(!output.log_output().contains("needle prototype purchase"));
        }
        let store = services.blackboard().await.unwrap();
        assert_eq!(store.get_entry(&project_id, &id).await.unwrap(), before);
        assert_eq!(
            store
                .get_source_eligible_entry(&project_id, &id)
                .await
                .unwrap(),
            None
        );
        assert!(
            store
                .entry_source_links(&project_id, &id, /*after*/ None)
                .await
                .is_err()
        );
        let admission = codex_state::ThreadProjectAdmission::acquire(
            &sqlite,
            codex_protocol::ThreadId::from_string(THREAD).unwrap(),
            &project_id,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            store
                .propose_sources(&admission, node.clone(), "turn-1", vec![proposal.clone()])
                .await
                .unwrap(),
            result
        );
        drop(admission);
        let output = tool
            .handle(call(
                "blackboard_query",
                json!({"text":"missing"}),
                /*budget*/ 9000,
                ToolCallSource::Direct,
            ))
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&output.log_output()).unwrap();
        assert_eq!(
            (value["data"].clone(), value["truncated"].clone()),
            (json!([]), json!(false))
        );
        let output = tool
            .handle(call(
                "blackboard_query",
                json!({"text":"independent Agent"}),
                /*budget*/ 9000,
                ToolCallSource::Direct,
            ))
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&output.log_output()).unwrap();
        assert_eq!(
            (
                value["data"][0]["entryId"].clone(),
                value["data"][0]["content"].clone()
            ),
            (
                json!("agent-control"),
                json!("independent Agent positive control")
            )
        );
        assert_eq!(value["data"][0]["relations"], json!([]));
        assert!(!output.log_output().contains("needle prototype purchase"));
    }
}
