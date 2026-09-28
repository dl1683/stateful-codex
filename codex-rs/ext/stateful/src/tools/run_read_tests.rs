use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use codex_stateful_runtime::NewObligation;
use codex_stateful_runtime::ObligationPacket;
use codex_stateful_runtime::StatefulObligation;
use codex_stateful_runtime::StatefulRunId;

use super::ReadCursor;
use super::Section;
use super::obligation_text;
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
    let digest = "0123456789abcdef0123456789abcdef";
    let cursor = ReadCursor {
        section: Section::Obligation,
        run_id: "run.with.dots".to_string(),
        digest: digest.to_string(),
        length: 8_192,
        offset: 4_096,
    };

    assert_eq!(
        cursor.encode(),
        format!("v1.obligation.run.with.dots.{digest}.8192.4096")
    );
    assert_eq!(
        [
            ReadCursor::decode(&cursor.encode()),
            ReadCursor::decode(&format!("v2.obligation.run-1.{digest}.10.0")),
            ReadCursor::decode(&format!("v1.result.run-1.{digest}.10.0")),
            ReadCursor::decode(&format!("v1.goal.run-1.{digest}.ten.0")),
            ReadCursor::decode(&format!("v1.goal.run-1.{digest}.10.-1")),
            ReadCursor::decode("v1.goal.run-1.0123ABCD0123ABCD0123ABCD0123ABCD.10.0"),
            ReadCursor::decode("v1.goal.run-1.0123abcd.10.0"),
            ReadCursor::decode(&format!("v1.goal..{digest}.10.0")),
            ReadCursor::decode("garbage"),
        ],
        [Some(cursor), None, None, None, None, None, None, None, None]
    );
}

#[test]
fn obligation_json_is_unambiguous_and_pages_back_to_the_exact_packet() {
    let obligation = |packet: ObligationPacket, revision: u64| StatefulObligation {
        id: "obligation-1".to_string(),
        value: NewObligation {
            project_id: "project-1".to_string(),
            run_id: StatefulRunId::parse("run-1").expect("valid run id"),
            packet,
            provenance_source_id: "turn-1".to_string(),
        },
        sequence: 1,
        revision,
        created_at_ms: 1,
    };
    let embedded = ObligationPacket {
        learning: vec!["a\nNext: b".to_string()],
        ..Default::default()
    };
    let split = ObligationPacket {
        learning: vec!["a".to_string()],
        next: vec!["b".to_string()],
        ..Default::default()
    };
    let large = ObligationPacket {
        learning: vec!["Clause “détail” \"quoted\"\n".repeat(200); 8],
        ..Default::default()
    };
    let text = |packet, revision| {
        obligation_text(&obligation(packet, revision)).expect("packet serializes")
    };

    assert_ne!(text(embedded, 1).text, text(split, 1).text);
    assert_ne!(
        text(large.clone(), 1).digest(),
        text(large.clone(), 2).digest()
    );
    let paged = text(large.clone(), 1);
    let rebuilt = pages(&paged.text, /*budget*/ 9_000)
        .iter()
        .map(|page| page["content"].as_str().expect("content").to_string())
        .collect::<String>();
    assert_eq!(
        serde_json::from_str::<ObligationPacket>(&rebuilt).expect("pages rebuild the packet"),
        large
    );
}
