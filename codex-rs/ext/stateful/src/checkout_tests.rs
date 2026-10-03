use pretty_assertions::assert_eq;
use serde_json::json;

use super::CheckoutReport;
use super::CheckoutReportPlan;
use super::END_MARKER;
use super::MAX_WINDOW_REPORT_BYTES;
use super::START_MARKER;
use codex_extension_api::PreviousWorldStateSection;

#[test]
fn a_report_renders_once_per_turn_and_shares_a_window_bound() {
    let report = CheckoutReport("x".repeat(1_400));
    let first = CheckoutReportPlan::new(None, "turn-1", Some(&report));
    let first_bytes = first.window_bytes;
    let first_snapshot = first.section().snapshot().clone();
    let later_step = CheckoutReportPlan::new(Some(&first_snapshot), "turn-1", Some(&report));
    let later_bytes = later_step.window_bytes;
    let later_section = later_step.section();
    let second = CheckoutReportPlan::new(Some(&first_snapshot), "turn-2", Some(&report));
    let second_snapshot = second.section().snapshot().clone();
    let third = CheckoutReportPlan::new(Some(&second_snapshot), "turn-3", Some(&report)).section();
    let fragment = START_MARKER.len() + 1_400 + END_MARKER.len();
    assert_eq!(
        (
            first_bytes,
            later_bytes,
            later_section
                .render_diff(PreviousWorldStateSection::Known(&first_snapshot))
                .is_none(),
            later_section
                .render_diff(PreviousWorldStateSection::Absent)
                .is_some(),
            second_snapshot["windowBytes"].as_u64(),
            third
                .render_diff(PreviousWorldStateSection::Known(&second_snapshot))
                .map(|fragment| fragment.body().starts_with("Further checkout changes")),
            CheckoutReportPlan::new(Some(&json!({})), "turn-4", None).window_bytes,
        ),
        (
            fragment,
            fragment,
            true,
            true,
            Some(u64::try_from(fragment * 2).expect("bytes")),
            Some(true),
            0,
        )
    );
    assert!(fragment * 2 <= MAX_WINDOW_REPORT_BYTES);
}
