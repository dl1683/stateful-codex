use super::*;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolName;
use codex_extension_api::ToolPayload;
use codex_project_intelligence::SourceObservation;
use codex_project_intelligence::SourceSpan;
use codex_project_intelligence::SourceSpanRole;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use tempfile::TempDir;

fn call(budget: usize) -> ToolCall<'static> {
    ToolCall {
        turn_id: "turn-1".into(),
        call_id: "read".into(),
        tool_name: ToolName::plain("conversation_read"),
        model: "test".into(),
        codex_turn_metadata: None,
        truncation_policy: codex_utils_output_truncation::TruncationPolicy::Bytes(budget),
        source: ToolCallSource::Direct,
        conversation_history: Default::default(),
        turn_item_emitter: Arc::new(codex_extension_api::NoopTurnItemEmitter),
        environments: Vec::new(),
        payload: ToolPayload::Function {
            arguments: "{}".into(),
        },
    }
}

#[tokio::test]
async fn c3_exact_source_every_tiny_budget_advances_or_terminal_refuses_escaping() {
    let home = TempDir::new().unwrap();
    let store = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .unwrap();
    for (index, prefix) in ["\u{1}", "😀"].iter().enumerate() {
        let text = format!("{prefix}{}", "é".repeat(1000));
        let seal = store
            .observe_source(
                SourceObservation {
                    project_id: "project-1".into(),
                    authoritative_thread_id: "thread-1".into(),
                    binding_generation: 1,
                    original_event_id: format!("event-{index}"),
                    turn_id: "turn-1".into(),
                    part_index: 0,
                    source_revision: 1,
                    complete_envelope: true,
                    incomplete_reason: None,
                    ordered_spans: vec![SourceSpan {
                        start_byte: 0,
                        end_byte: text.len() as u32,
                        role: SourceSpanRole::Body,
                    }],
                },
                &text,
            )
            .await
            .unwrap();
        for budget in 150..=700 {
            let mut offset = 0;
            let mut rebuilt = String::new();
            loop {
                let output = read(
                    &store,
                    "project-1",
                    SourceRead {
                        source_id: seal.exact_source_locator.clone(),
                        digest: seal.digest.clone(),
                        source_revision: 1,
                        offset,
                    },
                    &call(budget),
                )
                .await;
                let output = match output {
                    Ok(output) => output,
                    Err(error) => {
                        assert!(
                            budget < 700 && error.to_string().contains("budget_insufficient"),
                            "{error}"
                        );
                        break;
                    }
                };
                let raw = output.log_output();
                assert!(raw.len() <= call(budget).response_byte_budget(MAX_RESPONSE_BYTES));
                let page: serde_json::Value = serde_json::from_str(&raw).unwrap();
                rebuilt.push_str(page["text"].as_str().unwrap());
                if page["complete"] == true {
                    assert_eq!(rebuilt, text);
                    break;
                }
                let next = page["nextOffset"].as_u64().unwrap() as u32;
                assert!(next > offset && text.is_char_boundary(next as usize));
                offset = next;
                if budget != 700 {
                    break;
                }
            }
        }
        assert!(
            read(
                &store,
                "other-project",
                SourceRead {
                    source_id: seal.exact_source_locator.clone(),
                    digest: seal.digest.clone(),
                    source_revision: 1,
                    offset: 0
                },
                &call(9000)
            )
            .await
            .is_err()
        );
        assert!(
            read(
                &store,
                "project-1",
                SourceRead {
                    source_id: seal.exact_source_locator,
                    digest: seal.digest,
                    source_revision: 2,
                    offset: 0
                },
                &call(9000)
            )
            .await
            .is_err()
        );
    }
}
