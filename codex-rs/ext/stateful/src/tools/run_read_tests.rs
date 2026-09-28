use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::ReadCursor;
use super::Section;
use super::read_page;

/// Pages `text` from the start the way the tool does, returning every page.
fn pages(text: &str, budget: usize) -> Vec<Value> {
    let mut pages = Vec::new();
    let mut offset = 0;
    loop {
        let page = read_page(
            text,
            offset,
            budget,
            |end| (end < text.len()).then(|| end.to_string()),
            |content, next_cursor| json!({"content": content, "nextCursor": next_cursor}),
        )
        .expect("a page fits");
        let next = page["nextCursor"].as_str().map(str::to_string);
        pages.push(page);
        match next {
            Some(next) => offset = next.parse().expect("offset cursor"),
            None => return pages,
        }
    }
}

#[test]
fn escape_heavy_unicode_text_pages_back_exactly_within_budget() {
    let text = "Deal \"gate\" \\ ÷ 契約\u{1}\n\t".repeat(700);
    assert!(text.len() > 16 * 1024);

    let pages = pages(&text, /*budget*/ 9_000);

    assert!(pages.len() > 2);
    assert!(pages.iter().all(|page| page.to_string().len() <= 9_000));
    let rebuilt = pages
        .iter()
        .map(|page| page["content"].as_str().expect("content").to_string())
        .collect::<String>();
    assert_eq!(rebuilt, text);
}

#[test]
fn empty_text_is_a_single_complete_page() {
    assert_eq!(
        pages("", /*budget*/ 9_000),
        vec![json!({"content": "", "nextCursor": null})]
    );
}

#[test]
fn budget_too_small_for_any_content_is_refused() {
    assert_eq!(
        read_page(
            "some text",
            /*offset*/ 0,
            /*budget*/ 10,
            |end| Some(end.to_string()),
            |content, next_cursor| json!({"content": content, "nextCursor": next_cursor}),
        ),
        None
    );
}

#[test]
fn cursors_round_trip_and_reject_malformed_input() {
    let cursor = ReadCursor {
        section: Section::Obligation,
        run_id: "run-abc".to_string(),
        digest: "0123abcd".to_string(),
        offset: 4_096,
    };

    assert_eq!(
        (
            ReadCursor::decode(&cursor.encode()),
            ReadCursor::decode("goal.run-abc.0123abcd.not-a-number"),
            ReadCursor::decode("result.run-abc.0123abcd.1"),
            ReadCursor::decode("garbage"),
        ),
        (Some(cursor), None, None, None)
    );
}
