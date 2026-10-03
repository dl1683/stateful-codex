use pretty_assertions::assert_eq;
use serde_json::json;

use super::CheckoutReport;
use super::CheckoutReportPlan;
use super::END_MARKER;
use super::MAX_REPORT_BYTES;
use super::MAX_WINDOW_REPORT_BYTES;
use super::START_MARKER;
use super::render_report;
use codex_extension_api::PreviousWorldStateSection;

#[test]
fn reports_render_once_per_turn_and_never_exceed_the_window() {
    let report = CheckoutReport("x".repeat(1_400));
    let mut previous: Option<serde_json::Value> = None;
    let mut shown = Vec::new();
    for turn in 1..=4 {
        let plan =
            CheckoutReportPlan::new(previous.as_ref(), &format!("turn-{turn}"), Some(&report));
        assert!(
            plan.window_bytes <= MAX_WINDOW_REPORT_BYTES,
            "{}",
            plan.window_bytes
        );
        let section = plan.section();
        shown.push(section.snapshot()["shown"].as_str().map(str::to_string));
        previous = Some(section.snapshot().clone());
    }
    assert_eq!(
        shown,
        vec![
            Some("full".to_string()),
            Some("full".to_string()),
            Some("fallback".to_string()),
            Some("none".to_string()),
        ]
    );

    // A later step of the same turn re-renders only when a new window lost the report.
    let first = CheckoutReportPlan::new(None, "turn-1", Some(&report));
    let fragment = START_MARKER.len() + 1_400 + END_MARKER.len();
    let first_bytes = first.window_bytes;
    let snapshot = first.section().snapshot().clone();
    let later = CheckoutReportPlan::new(Some(&snapshot), "turn-1", Some(&report));
    let later_bytes = later.window_bytes;
    let later = later.section();
    assert_eq!(
        (
            first_bytes,
            later_bytes,
            later
                .render_diff(PreviousWorldStateSection::Known(&snapshot))
                .is_none(),
            later
                .render_diff(PreviousWorldStateSection::Absent)
                .is_some(),
            CheckoutReportPlan::new(Some(&json!({})), "turn-9", None).window_bytes,
        ),
        (fragment, fragment, true, true, 0)
    );
}

/// Escaping expands markup sixfold; the bound applies to the escaped text, notice included.
#[test]
fn the_report_is_bounded_after_escaping() {
    let lines = (0..20)
        .map(|index| format!("- abc{index} ({}) {}", "<&>".repeat(80), "&".repeat(400)))
        .collect::<Vec<_>>();
    let report = render_report(0, lines);
    assert!(report.len() <= MAX_REPORT_BYTES, "{}", report.len());
    assert!(report.ends_with("run git log and git status."));
    assert!(!report.contains('<') && !report.contains('&'));
}
