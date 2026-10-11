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
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use wiremock::MockServer;

const INSTRUCTION: &str = "Append the sidecar after the summary.";
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
            instruction: INSTRUCTION.to_string(),
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
        vec![SUMMARIZATION_PROMPT.to_string(), INSTRUCTION.to_string()]
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
    assert!(!request.body_contains_text(INSTRUCTION));
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
    let contributor = Arc::new(RecordingExtraction::default());
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions.compaction_extraction_contributor(contributor.clone());
    let provider = local_provider(server);
    let mut builder = test_codex()
        .with_extensions(Arc::new(extensions.build()))
        .with_config(move |config| {
            config.model_provider = provider;
            config.compact_prompt = Some(SUMMARIZATION_PROMPT.to_string());
            configure(config);
        });
    let codex = builder.build(server).await.expect("build codex").codex;
    (codex, contributor)
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
    assert!(!requests[0].body_contains_text(INSTRUCTION));
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
