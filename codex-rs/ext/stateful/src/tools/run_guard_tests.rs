use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolCallSource;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::bounded_rejection;
use crate::tools::MAX_RESPONSE_BYTES;
use crate::tools::capture_repair_tests::call;

fn serialized(error: FunctionCallError) -> usize {
    let FunctionCallError::RespondToModel(message) = error else {
        panic!("rejections respond to the model");
    };
    serde_json::to_string(&message)
        .expect("message serializes")
        .len()
}

fn sized(budget: usize) -> ToolCall<'static> {
    call(
        "stateful_run_update",
        json!({}),
        budget,
        ToolCallSource::Direct,
    )
}

/// An echoed oversized reference (two identical 50,000-byte aliases) and the short
/// diagnostics both stay within every call allowance, from tiny to the tool cap.
#[test]
fn every_rejection_fits_the_call_allowance() {
    let echoed = format!(
        "materialRootFindings contains duplicate reference {}",
        "E".repeat(/*n*/ 50_000)
    );
    let guide = "Valid completion calls (replace the <...> text): ...";
    let mut violations = Vec::new();
    for budget in [1, 20, 64, 100, 240, 1_000, 9_000, 40_000] {
        let call = sized(budget);
        let allowance = call.response_byte_budget(MAX_RESPONSE_BYTES);
        for (message, guide) in [
            (echoed.as_str(), Some(guide)),
            (echoed.as_str(), None),
            ("run revision conflict: expected 2, found 3", Some(guide)),
        ] {
            let size = serialized(bounded_rejection(&call, message, guide));
            if size > allowance || size > MAX_RESPONSE_BYTES {
                violations.push((budget, size, allowance));
            }
        }
    }
    assert_eq!(violations, Vec::new());
}

#[test]
fn guidance_is_dropped_before_the_rejection_itself() {
    let message = "run revision conflict: expected 2, found 3";
    let guide = "g".repeat(/*n*/ 20_000);
    assert_eq!(
        [
            bounded_rejection(&sized(/*budget*/ 40_000), message, Some("use revision 3")),
            bounded_rejection(&sized(/*budget*/ 40_000), message, Some(&guide)),
            bounded_rejection(&sized(/*budget*/ 40_000), &"x".repeat(/*n*/ 20_000), None),
        ]
        .map(|error| error.to_string()),
        [
            format!("{message}. use revision 3"),
            message.to_string(),
            super::OVERSIZED_REJECTION.to_string(),
        ]
    );
}
