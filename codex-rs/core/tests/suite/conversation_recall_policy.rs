//! Every request carries the host recall policy once, with or without a conversation packet.

use anyhow::Result;
use codex_login::CodexAuth;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use core_test_support::responses;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;

const POLICY_OPENING: &str =
    "When asked what was said or reported earlier, use original conversation deliveries";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recall_policy_reaches_inference_and_both_compaction_paths() -> Result<()> {
    skip_if_no_network!(Ok(()));

    for (auth, remote_triggers, compaction) in [
        // A custom provider summarizes locally; ChatGPT sessions use remote-v2 compaction.
        (
            CodexAuth::from_api_key("dummy"),
            0,
            sse(vec![
                responses::ev_assistant_message("summary", "LOCAL_SUMMARY"),
                responses::ev_completed("compact-response"),
            ]),
        ),
        (
            CodexAuth::create_dummy_chatgpt_auth_for_testing(),
            1,
            sse(vec![
                json!({
                    "type": "response.output_item.done",
                    "item": {"type": "compaction", "encrypted_content": "REMOTE_SUMMARY"},
                }),
                responses::ev_completed("compact-response"),
            ]),
        ),
    ] {
        let server = wiremock::MockServer::start().await;
        let response_mock = responses::mount_sse_sequence(
            &server,
            vec![
                sse(vec![
                    responses::ev_assistant_message("answer", "The answer is 42."),
                    responses::ev_completed("response-1"),
                ]),
                compaction,
            ],
        )
        .await;
        let mut builder = test_codex().with_auth(auth);
        if remote_triggers == 0 {
            // Only non-OpenAI providers summarize locally.
            builder = builder.with_config(|config| {
                config.model_provider.name = "Local compaction test provider".to_string();
            });
        }
        let test = builder.build(&server).await?;

        // The first compaction has no earlier packet, so the policy cannot depend on one.
        test.submit_turn("What is the answer?").await?;
        test.codex.submit(Op::Compact).await?;
        wait_for_event(&test.codex, |event| {
            matches!(event, EventMsg::TurnComplete(_))
        })
        .await;
        test.codex.shutdown_and_wait().await?;

        let requests = response_mock.requests();
        assert_eq!(requests.len(), 2);
        let [inference, compaction] = requests.as_slice() else {
            unreachable!("two requests checked above");
        };
        for request in [inference, compaction] {
            // The helper asserts that the policy closes the instructions exactly once.
            let base = request.instructions_text();
            let instructions = request.body_json()["instructions"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            assert!(instructions[base.len()..].contains(POLICY_OPENING));
        }
        assert_eq!(
            compaction.inputs_of_type("compaction_trigger").len(),
            remote_triggers
        );
        // A request-only decoration is not a model change.
        assert!(
            !inference.body_contains_text("<model_switch>"),
            "the recall policy was mistaken for a model change"
        );
        assert_eq!(
            inference.instructions_text(),
            compaction.instructions_text()
        );
    }
    Ok(())
}
