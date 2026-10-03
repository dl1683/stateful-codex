use pretty_assertions::assert_eq;

use super::excerpt;
use super::mentions_any;
use super::parse_utc_day;
use super::question_terms;

#[test]
fn question_terms_keep_content_words_only() {
    assert_eq!(
        question_terms(
            "Are typo hints on or off by default, how does an app turn them on, and why?"
        ),
        vec![
            "typo".to_string(),
            "hints".to_string(),
            "default".to_string(),
            "app".to_string(),
            "turn".to_string(),
        ]
    );
    assert_eq!(question_terms("why is it on?"), Vec::<String>::new());
}

#[test]
fn matching_and_excerpts_follow_the_question() {
    let text = format!(
        "{}Typo hints are opt-in because downstream snapshot tests were breaking.{}",
        "Background. ".repeat(40),
        " More text.".repeat(40)
    );
    let terms = question_terms("why are typo hints opt-in");
    let cut = excerpt(&text, &terms, 160);
    assert!(cut.len() <= 160, "{}", cut.len());
    assert!(cut.starts_with("...") && cut.ends_with("..."));
    assert!(cut.contains("Typo hints are opt-in"));
    assert_eq!(
        (
            mentions_any("Made typo_hints opt-in", &terms),
            mentions_any("Unrelated change", &terms),
            excerpt("short", &terms, 160),
        ),
        (true, false, "short".to_string())
    );
}

#[test]
fn since_is_a_utc_day() {
    assert_eq!(
        (
            parse_utc_day("1970-01-02"),
            parse_utc_day("2026-10-02"),
            parse_utc_day("2026-13-01").is_err(),
            parse_utc_day("yesterday").is_err(),
        ),
        (Ok(86_400_000), Ok(1_790_899_200_000), true, true)
    );
}
