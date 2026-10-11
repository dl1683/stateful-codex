//! Contributor extraction inside the native LOCAL compaction request: one request per native
//! attempt, the trailer separated before history, and no repair request on failure.

use std::sync::Arc;
use std::sync::Mutex;

use codex_core::TurnInputRequest;
use codex_core::compact::SUMMARIZATION_PROMPT;
use codex_core::compact::SUMMARY_PREFIX;
use codex_extension_api::CompactionExtractionContributor;
use codex_extension_api::CompactionExtractionPreparation;
use codex_extension_api::CompactionExtractionResult;
use codex_extension_api::CompactionRequestSnapshot;
use codex_extension_api::CompactionSourceClass;
use codex_extension_api::CompactionTrailer;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::built_in_model_providers;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ResponsesRequest;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed_with_tokens;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::sse_failed;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodexBuilder;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::MockServer;

const INSTRUCTION_LEAD: &str = "After the ordinary summary, append exactly one capsule trailer:";
const OPENING: &str = "<capsule_sidecar_v1 request=\"host-marker-1\">";
const USER_MESSAGE: &str = "Use SQLite because the tool runs offline.";
const SUMMARY: &str = "Native summary prose.";

#[derive(Default)]
struct RecordingExtraction {
    prepared: Mutex<Vec<u32>>,
    completed: Mutex<Vec<CompactionExtractionResult>>,
}

impl CompactionExtractionContributor for RecordingExtraction {
    fn prepare(
        &self,
        request: &CompactionRequestSnapshot,
        _thread_store: &ExtensionData,
    ) -> Option<CompactionExtractionPreparation> {
        self.prepared
            .lock()
            .expect("prepared lock")
            .push(request.attempt);
        Some(CompactionExtractionPreparation {
            instruction: instruction(),
            trailer_opening: OPENING.to_string(),
        })
    }

    fn complete(&self, result: CompactionExtractionResult, _thread_store: &ExtensionData) {
        self.completed.lock().expect("completed lock").push(result);
    }
}

/// (attempt, native attempts, trailer, user source texts with byte-identical origins)
type Completed = (u32, u32, CompactionTrailer, Vec<(String, bool)>);

#[derive(Debug, PartialEq)]
struct Observed {
    prepared: Vec<u32>,
    completed: Vec<Completed>,
}

impl RecordingExtraction {
    fn observed(&self) -> Observed {
        let completed = self
            .completed
            .lock()
            .expect("completed lock")
            .iter()
            .map(|result| {
                let users = result
                    .request
                    .parts
                    .iter()
                    .filter(|part| part.class == CompactionSourceClass::User)
                    .map(|part| {
                        (
                            part.text.clone(),
                            part.origin
                                .as_ref()
                                .is_some_and(|origin| origin.byte_identical),
                        )
                    })
                    .collect();
                (
                    result.request.attempt,
                    result.native_attempts,
                    result.trailer.clone(),
                    users,
                )
            })
            .collect();
        Observed {
            prepared: self.prepared.lock().expect("prepared lock").clone(),
            completed,
        }
    }
}

fn local_provider(server: &MockServer) -> ModelProviderInfo {
    let mut provider = built_in_model_providers(/*openai_base_url*/ None)["openai"].clone();
    provider.name = "OpenAI (test)".into();
    provider.base_url = Some(format!("{}/v1", server.uri()));
    provider.supports_websockets = false;
    provider
}

/// The real addition shape: it names the request's own delimiter and schema.
fn instruction() -> String {
    format!(
        "{INSTRUCTION_LEAD}
{OPENING}
{{\"version\":1,\"records\":[],\"supersessions\":[]}}
</capsule_sidecar_v1>"
    )
}

fn trailer() -> String {
    format!(
        "{OPENING}\n{{\"version\":1,\"records\":[],\"supersessions\":[]}}\n</capsule_sidecar_v1>"
    )
}

fn summary_with_trailer() -> String {
    format!("{SUMMARY}\n{}", trailer())
}

/// The compaction request ends with the native instruction followed by the addition.
fn assert_augmented(request: &ResponsesRequest) {
    let user_texts = request.message_input_texts("user");
    assert_eq!(
        user_texts[user_texts.len() - 2..].to_vec(),
        vec![SUMMARIZATION_PROMPT.to_string(), instruction()]
    );
}

/// The follow-up request carries the native summary and never the trailer.
fn assert_summary_only(request: &ResponsesRequest) {
    let summary = format!("{SUMMARY_PREFIX}\n{SUMMARY}");
    assert!(
        request.message_input_texts("user").contains(&summary),
        "summary must be installed"
    );
    assert!(!request.body_contains_text("capsule_sidecar_v1"));
    assert!(!request.body_contains_text(INSTRUCTION_LEAD));
}

async fn submit(codex: &codex_core::CodexThread, text: &str) {
    codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("submit turn");
    wait_for_event(codex, |event| matches!(event, EventMsg::TurnComplete(_))).await;
}

async fn build(
    server: &MockServer,
    configure: impl FnOnce(&mut codex_core::config::Config) + Send + 'static,
) -> (Arc<codex_core::CodexThread>, Arc<RecordingExtraction>) {
    build_from(server, test_codex().with_config(configure)).await
}

async fn build_from(
    server: &MockServer,
    builder: TestCodexBuilder,
) -> (Arc<codex_core::CodexThread>, Arc<RecordingExtraction>) {
    let contributor = Arc::new(RecordingExtraction::default());
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions.compaction_extraction_contributor(contributor.clone());
    let provider = local_provider(server);
    let mut builder = builder
        .with_extensions(Arc::new(extensions.build()))
        .with_config(move |config| {
            config.model_provider = provider;
            config.compact_prompt = Some(SUMMARIZATION_PROMPT.to_string());
        });
    let codex = builder
        .build_with_auto_env(server)
        .await
        .expect("build codex")
        .codex;
    (codex, contributor)
}

/// Every frozen source part names the text at that position of the captured wire request.
fn assert_positions_match_wire(contributor: &RecordingExtraction, request: &ResponsesRequest) {
    let completed = contributor.completed.lock().expect("completed lock");
    let input = request.input();
    let mismatched = completed[0]
        .request
        .parts
        .iter()
        .filter(|part| {
            input[part.item_index]["content"][part.part_index]["text"].as_str()
                != Some(part.text.as_str())
        })
        .map(|part| (part.item_index, part.part_index))
        .collect::<Vec<_>>();
    assert_eq!(mismatched, Vec::<(usize, usize)>::new());
}

/// Assistant source texts with whether the host proved their published identity.
fn assistant_origins(contributor: &RecordingExtraction) -> Vec<(String, bool)> {
    contributor.completed.lock().expect("completed lock")[0]
        .request
        .parts
        .iter()
        .filter(|part| part.class == CompactionSourceClass::Assistant)
        .map(|part| (part.text.clone(), part.origin.is_some()))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_compaction_separates_trailer_and_reports_the_frozen_request() {
    skip_if_no_network!();
    let server = start_mock_server().await;
    let log = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_assistant_message("m0", "FIRST_REPLY"),
                ev_completed_with_tokens("r0", /*total_tokens*/ 80),
            ]),
            sse(vec![
                ev_assistant_message("m1", &summary_with_trailer()),
                ev_completed_with_tokens("r1", /*total_tokens*/ 100),
            ]),
            sse(vec![
                ev_assistant_message("m2", "AFTER"),
                ev_completed_with_tokens("r2", /*total_tokens*/ 120),
            ]),
        ],
    )
    .await;
    let (codex, contributor) = build(&server, |_| {}).await;

    submit(&codex, USER_MESSAGE).await;
    codex.submit(Op::Compact).await.expect("compact");
    wait_for_event(&codex, |event| matches!(event, EventMsg::TurnComplete(_))).await;
    submit(&codex, "next").await;

    let requests = log.requests();
    assert_eq!(requests.len(), 3);
    assert!(!requests[0].body_contains_text(INSTRUCTION_LEAD));
    assert_augmented(&requests[1]);
    assert_summary_only(&requests[2]);
    assert_positions_match_wire(&contributor, &requests[1]);
    assert_eq!(
        contributor.observed(),
        Observed {
            prepared: vec![1],
            completed: vec![(
                1,
                1,
                CompactionTrailer::Present(trailer()),
                vec![(USER_MESSAGE.to_string(), true)],
            )],
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupted_attempt_is_counted_and_never_reported() {
    skip_if_no_network!();
    let server = start_mock_server().await;
    let log = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_assistant_message("m0", "FIRST_REPLY"),
                ev_completed_with_tokens("r0", /*total_tokens*/ 80),
            ]),
            // Closed before completion: buffered output must be discarded.
            sse(vec![ev_assistant_message("m1", "PARTIAL")]),
            sse(vec![
                ev_assistant_message("m2", &summary_with_trailer()),
                ev_completed_with_tokens("r2", /*total_tokens*/ 100),
            ]),
            sse(vec![
                ev_assistant_message("m3", "AFTER"),
                ev_completed_with_tokens("r3", /*total_tokens*/ 120),
            ]),
        ],
    )
    .await;
    let (codex, contributor) = build(&server, |_| {}).await;

    submit(&codex, USER_MESSAGE).await;
    codex.submit(Op::Compact).await.expect("compact");
    wait_for_event(&codex, |event| matches!(event, EventMsg::TurnComplete(_))).await;
    submit(&codex, "next").await;

    let requests = log.requests();
    assert_eq!(requests.len(), 4);
    assert_augmented(&requests[1]);
    assert_augmented(&requests[2]);
    assert_summary_only(&requests[3]);
    assert!(!requests[3].body_contains_text("PARTIAL"));
    assert_eq!(
        contributor.observed(),
        Observed {
            prepared: vec![1, 2],
            completed: vec![(
                2,
                2,
                CompactionTrailer::Present(trailer()),
                vec![(USER_MESSAGE.to_string(), true)],
            )],
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ambiguous_trailer_fails_compaction_without_a_repair_request() {
    skip_if_no_network!();
    let server = start_mock_server().await;
    let repeated = format!("{SUMMARY}\n{}\n{}", trailer(), trailer());
    let log = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_assistant_message("m0", "FIRST_REPLY"),
                ev_completed_with_tokens("r0", /*total_tokens*/ 80),
            ]),
            sse(vec![
                ev_assistant_message("m1", &repeated),
                ev_completed_with_tokens("r1", /*total_tokens*/ 100),
            ]),
            sse(vec![
                ev_assistant_message("m2", "AFTER"),
                ev_completed_with_tokens("r2", /*total_tokens*/ 120),
            ]),
        ],
    )
    .await;
    let (codex, contributor) = build(&server, |_| {}).await;

    submit(&codex, USER_MESSAGE).await;
    codex.submit(Op::Compact).await.expect("compact");
    wait_for_event(&codex, |event| matches!(event, EventMsg::TurnComplete(_))).await;
    submit(&codex, "next").await;

    let requests = log.requests();
    assert_eq!(requests.len(), 3);
    // Old history is preserved and neither summary nor trailer was recorded.
    assert!(
        requests[2]
            .message_input_texts("user")
            .contains(&USER_MESSAGE.to_string())
    );
    assert!(!requests[2].body_contains_text(SUMMARY));
    assert!(!requests[2].body_contains_text("capsule_sidecar_v1"));
    assert_eq!(
        contributor.observed(),
        Observed {
            prepared: vec![1],
            completed: Vec::new(),
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mid_turn_auto_compaction_separates_trailer() {
    skip_if_no_network!();
    let server = start_mock_server().await;
    let log = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_function_call("call-1", "unsupported_tool", "{}"),
                ev_completed_with_tokens("r1", /*total_tokens*/ 96),
            ]),
            sse(vec![
                ev_assistant_message("m2", &summary_with_trailer()),
                ev_completed_with_tokens("r2", /*total_tokens*/ 10),
            ]),
            sse(vec![
                ev_assistant_message("m3", "FINAL"),
                ev_completed_with_tokens("r3", /*total_tokens*/ 10),
            ]),
        ],
    )
    .await;
    let (codex, contributor) = build(&server, |config| {
        config.model_context_window = Some(100);
        config.model_auto_compact_token_limit = Some(90);
    })
    .await;

    submit(&codex, USER_MESSAGE).await;

    let requests = log.requests();
    assert_eq!(requests.len(), 3);
    assert_augmented(&requests[1]);
    assert_summary_only(&requests[2]);
    assert_eq!(
        contributor.observed(),
        Observed {
            prepared: vec![1],
            completed: vec![(
                1,
                1,
                CompactionTrailer::Present(trailer()),
                vec![(USER_MESSAGE.to_string(), true)],
            )],
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_turn_auto_compaction_separates_trailer() {
    skip_if_no_network!();
    let server = start_mock_server().await;
    let log = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_assistant_message("m1", "FIRST_REPLY"),
                ev_completed_with_tokens("r1", /*total_tokens*/ 60_000),
            ]),
            sse(vec![
                ev_assistant_message("m2", &summary_with_trailer()),
                ev_completed_with_tokens("r2", /*total_tokens*/ 10),
            ]),
            sse(vec![
                ev_assistant_message("m3", "AFTER"),
                ev_completed_with_tokens("r3", /*total_tokens*/ 10),
            ]),
        ],
    )
    .await;
    let (codex, contributor) = build(&server, |config| {
        config.model_context_window = Some(100_000);
        config.model_post_turn_compact_threshold_percent = 50;
    })
    .await;

    submit(&codex, USER_MESSAGE).await;
    submit(&codex, "next").await;

    let requests = log.requests();
    assert_eq!(requests.len(), 3);
    assert_augmented(&requests[1]);
    assert_summary_only(&requests[2]);
    assert_eq!(contributor.observed().completed.len(), 1);
}

/// Runs one turn, then a manual compaction answered by `compaction`, then one more turn.
async fn manual_compaction_after_reply(
    reply: &str,
    compaction: Vec<String>,
) -> (Vec<ResponsesRequest>, Arc<RecordingExtraction>) {
    let server = start_mock_server().await;
    let mut responses = vec![sse(vec![
        ev_assistant_message("m0", reply),
        ev_completed_with_tokens("r0", /*total_tokens*/ 80),
    ])];
    responses.extend(compaction);
    responses.push(sse(vec![
        ev_assistant_message("m9", "AFTER"),
        ev_completed_with_tokens("r9", /*total_tokens*/ 120),
    ]));
    let log = mount_sse_sequence(&server, responses).await;
    let (codex, contributor) = build(&server, |_| {}).await;

    submit(&codex, USER_MESSAGE).await;
    codex.submit(Op::Compact).await.expect("compact");
    wait_for_event(&codex, |event| matches!(event, EventMsg::TurnComplete(_))).await;
    submit(&codex, "next").await;
    (log.requests(), contributor)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn augmented_compaction_without_a_current_summary_keeps_history() {
    skip_if_no_network!();
    // The earlier answer must never stand in for a missing summary.
    let reply = "FIRST_REPLY";
    let cases = vec![
        (
            "no assistant text",
            sse(vec![ev_completed_with_tokens(
                "r1", /*total_tokens*/ 100,
            )]),
        ),
        (
            "whitespace only",
            sse(vec![
                ev_assistant_message("m1", "  \n\t "),
                ev_completed_with_tokens("r1", /*total_tokens*/ 100),
            ]),
        ),
        (
            "empty after stripping hidden markup",
            sse(vec![
                ev_assistant_message(
                    "m1",
                    &format!("<oai-mem-citation>doc</oai-mem-citation>\n{}", trailer()),
                ),
                ev_completed_with_tokens("r1", /*total_tokens*/ 100),
            ]),
        ),
    ];
    for (name, compaction) in cases {
        let (requests, contributor) = manual_compaction_after_reply(reply, vec![compaction]).await;

        assert_eq!(requests.len(), 3, "{name}: no repair request");
        assert!(
            requests[2]
                .message_input_texts("user")
                .contains(&USER_MESSAGE.to_string()),
            "{name}: history preserved"
        );
        assert!(
            !requests[2].body_contains_text(SUMMARY_PREFIX),
            "{name}: no replacement summary"
        );
        assert_eq!(
            contributor.observed(),
            Observed {
                prepared: vec![1],
                completed: Vec::new(),
            },
            "{name}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hidden_assistant_text_is_not_a_published_source() {
    skip_if_no_network!();
    let hidden = "Visible answer.<oai-mem-citation>doc-7</oai-mem-citation>";
    let (requests, contributor) = manual_compaction_after_reply(
        hidden,
        vec![sse(vec![
            ev_assistant_message("m1", &summary_with_trailer()),
            ev_completed_with_tokens("r1", /*total_tokens*/ 100),
        ])],
    )
    .await;

    assert_eq!(requests.len(), 3);
    assert_summary_only(&requests[2]);
    // Recorded provider bytes differ from what the user saw: no source identity.
    assert_eq!(
        assistant_origins(&contributor),
        vec![(hidden.to_string(), false)]
    );

    let (_, exact) = manual_compaction_after_reply(
        "FIRST_REPLY",
        vec![sse(vec![
            ev_assistant_message("m1", &summary_with_trailer()),
            ev_completed_with_tokens("r1", /*total_tokens*/ 100),
        ])],
    )
    .await;
    assert_eq!(
        assistant_origins(&exact),
        vec![("FIRST_REPLY".to_string(), true)]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn augmentation_overflow_retries_natively_without_trimming_sources() {
    skip_if_no_network!();
    let (requests, contributor) = manual_compaction_after_reply(
        "FIRST_REPLY",
        vec![
            sse_failed("r1", "context_length_exceeded", "context window exceeded"),
            sse(vec![
                ev_assistant_message("m2", SUMMARY),
                ev_completed_with_tokens("r2", /*total_tokens*/ 100),
            ]),
        ],
    )
    .await;

    assert_eq!(requests.len(), 4);
    assert_augmented(&requests[1]);
    // Same sources, minus only the addition.
    assert_eq!(
        requests[2].input(),
        requests[1].input()[..requests[1].input().len() - 1]
    );
    assert_summary_only(&requests[3]);
    assert_eq!(
        contributor.observed(),
        Observed {
            prepared: vec![1],
            completed: Vec::new(),
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn responses_lite_withholds_extraction() {
    skip_if_no_network!();
    let server = start_mock_server().await;
    let log = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_assistant_message("m0", "FIRST_REPLY"),
                ev_completed_with_tokens("r0", /*total_tokens*/ 80),
            ]),
            sse(vec![
                ev_assistant_message("m1", SUMMARY),
                ev_completed_with_tokens("r1", /*total_tokens*/ 100),
            ]),
            sse(vec![
                ev_assistant_message("m2", "AFTER"),
                ev_completed_with_tokens("r2", /*total_tokens*/ 120),
            ]),
        ],
    )
    .await;
    let (codex, contributor) = build_from(
        &server,
        test_codex().with_model_info_override("gpt-5.5", |model| {
            model.use_responses_lite = true;
        }),
    )
    .await;

    submit(&codex, USER_MESSAGE).await;
    codex.submit(Op::Compact).await.expect("compact");
    wait_for_event(&codex, |event| matches!(event, EventMsg::TurnComplete(_))).await;
    submit(&codex, "next").await;

    let requests = log.requests();
    assert_eq!(requests.len(), 3);
    assert!(!requests[1].body_contains_text(INSTRUCTION_LEAD));
    assert_summary_only(&requests[2]);
    assert_eq!(
        contributor.observed(),
        Observed {
            prepared: Vec::new(),
            completed: Vec::new(),
        }
    );
}

/// Waits for turn completion, keeping a debug rendering of every event on the way.
async fn complete_turn(codex: &codex_core::CodexThread, events: &mut Vec<String>) {
    wait_for_event(codex, |event| {
        events.push(format!("{event:?}"));
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn split_or_malformed_trailer_never_reaches_history_rollout_or_events() {
    skip_if_no_network!();
    let (head, tail) = OPENING.split_at(12);
    let payload = "{\"version\":1,\"records\":[],\"supersessions\":[]}";
    let cases = [
        (
            "opening split across parts",
            vec![
                format!("{SUMMARY}\n{head}"),
                format!("{tail}\n{payload}\n</capsule_sidecar_v1>"),
            ],
            format!("{OPENING}\n{payload}\n</capsule_sidecar_v1>"),
        ),
        (
            "no newline after the opening",
            vec![format!(
                "{SUMMARY}\n{OPENING}{payload}\n</capsule_sidecar_v1>"
            )],
            format!("{OPENING}{payload}\n</capsule_sidecar_v1>"),
        ),
    ];
    for (name, parts, expected_trailer) in cases {
        let server = start_mock_server().await;
        let content = parts
            .iter()
            .map(|text| json!({"type": "output_text", "text": text}))
            .collect::<Vec<_>>();
        let log = mount_sse_sequence(
            &server,
            vec![
                sse(vec![
                    ev_assistant_message("m0", "FIRST_REPLY"),
                    ev_completed_with_tokens("r0", /*total_tokens*/ 80),
                ]),
                sse(vec![
                    json!({
                        "type": "response.output_item.done",
                        "item": {"type": "message", "role": "assistant", "id": "m1", "content": content}
                    }),
                    ev_completed_with_tokens("r1", /*total_tokens*/ 100),
                ]),
                sse(vec![
                    ev_assistant_message("m2", "AFTER"),
                    ev_completed_with_tokens("r2", /*total_tokens*/ 120),
                ]),
            ],
        )
        .await;
        let (codex, contributor) = build(&server, |_| {}).await;
        let mut events = Vec::new();

        submit(&codex, USER_MESSAGE).await;
        codex.submit(Op::Compact).await.expect("compact");
        complete_turn(&codex, &mut events).await;
        codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "next".to_string(),
                text_elements: Vec::new(),
            }]))
            .await
            .expect("submit turn");
        complete_turn(&codex, &mut events).await;
        codex.flush_rollout().await.expect("flush rollout");
        let rollout = std::fs::read_to_string(codex.rollout_path().expect("rollout path"))
            .expect("read rollout");

        let requests = log.requests();
        assert_eq!(requests.len(), 3, "{name}");
        assert_summary_only(&requests[2]);
        let leaked = |text: &str| text.contains("capsule_sidecar") || text.contains("marker-1");
        // The installed summary is persisted and its events were observed, without the trailer.
        assert!(rollout.contains(SUMMARY), "{name}: summary persisted");
        assert!(!leaked(&rollout), "{name}: rollout");
        assert!(
            events
                .iter()
                .any(|event| event.contains("RawResponseItem") && event.contains(SUMMARY)),
            "{name}: summary raw event"
        );
        assert_eq!(
            events.iter().filter(|event| leaked(event)).count(),
            0,
            "{name}: events"
        );
        assert_eq!(
            contributor.observed().completed[0].2,
            CompactionTrailer::Present(expected_trailer),
            "{name}"
        );
    }
}
