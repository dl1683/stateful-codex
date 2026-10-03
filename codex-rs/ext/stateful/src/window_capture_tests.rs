use std::collections::HashMap;
use std::path::PathBuf;

use codex_protocol::items::FileChangeItem;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::PatchApplyStatus;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::MAX_OBSERVATION_BYTES;
use super::clean_head;
use super::clean_tail;
use super::observation;
use super::plan_payload;
use super::serialized_len;

const TOKEN: &str = "abcdefghijklmnopqrstuvwxyz012345";

#[test]
fn a_cut_never_separates_a_secret_from_what_identifies_it() {
    // The tail cut falls just after "Bearer ": the token alone must not survive.
    let output = format!("Bearer {TOKEN}\n{}", "x".repeat(2_015));
    let kept = clean_tail(&output, 2_048);
    assert!(!kept.contains(TOKEN), "{kept}");
    // The head cut falls inside an API key: the kept prefix must not be a usable key.
    let command = format!("{} sk-{}", "a".repeat(1_000), "Z".repeat(40));
    let kept = clean_head(&command, 1_024);
    assert!(!kept.contains(&"Z".repeat(16)), "{kept}");
    assert_eq!(clean_head("plain text", 1_024), "plain text");
}

#[test]
fn a_patch_with_many_long_paths_is_kept_with_an_omitted_count() {
    let changes = (0..32)
        .map(|index| {
            (
                PathBuf::from(format!("/work/{index:02}/{}", "p".repeat(500))),
                FileChange::Add {
                    content: String::new(),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let (_, _, payload) = observation(&TurnItem::FileChange(FileChangeItem {
        id: "patch-1".to_string(),
        changes,
        status: Some(PatchApplyStatus::Failed),
        auto_approved: None,
        stdout: None,
        stderr: None,
    }))
    .expect("observed");
    assert!(serialized_len(&payload) <= MAX_OBSERVATION_BYTES);
    let kept = payload["paths"].as_array().expect("paths").len();
    assert!(kept > 0 && kept < 32, "{kept}");
    assert_eq!(payload["morePaths"], json!(32 - kept));
    assert_eq!(payload["status"], json!("failed"));
}

#[test]
fn a_long_plan_is_bounded_and_counts_what_it_leaves_out() {
    let plan = (0..40)
        .map(|index| json!({"step": format!("Step {index} {}", "s".repeat(300)), "status": "pending"}))
        .collect::<Vec<_>>();
    let payload = plan_payload(&json!({"explanation": "Why", "plan": plan}));
    assert!(serialized_len(&payload) <= MAX_OBSERVATION_BYTES);
    let kept = payload["steps"].as_array().expect("steps").len();
    assert_eq!(payload["moreSteps"], json!(40 - kept));
}
