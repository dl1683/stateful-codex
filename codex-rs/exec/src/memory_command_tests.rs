use codex_app_server_protocol::BlackboardKind;
use codex_app_server_protocol::BlackboardProvenanceKind;
use codex_app_server_protocol::StatefulMemoryItem;
use codex_app_server_protocol::StatefulMemorySection;
use pretty_assertions::assert_eq;

use super::listing;
use super::parse_target;

#[test]
fn input_parser_preserves_unicode_after_valid_revision_targets() {
    use clap::Parser;
    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: super::MemoryArgs,
    }
    let parsed = TestCli::try_parse_from([
        "memory",
        "--thread",
        "T",
        "correct",
        "entry@1",
        "Preserve §3.2–§4 — α.",
    ])
    .expect("valid input");
    match parsed.args.action {
        super::MemoryAction::Correct { target, text } => assert_eq!(
            (target, text),
            (
                ("entry".to_string(), 1),
                vec!["Preserve §3.2–§4 — α.".to_string()]
            )
        ),
        action => panic!("unexpected action: {action:?}"),
    }
}

#[test]
fn retained_scope_review_never_claims_current_application() {
    use codex_app_server_protocol::StatefulMemoryScopeState;
    let mut scoped = item("scoped", StatefulMemorySection::UserRule, "Never push.");
    scoped.scope_state = Some(StatefulMemoryScopeState::Unsupported);
    assert_eq!(
        listing(&[scoped]),
        "Your retained rules\n  - Never push.\n    scoped@2\n    investigation: unsupported; held back\n\n"
    );
    let mut long = item("long", StatefulMemorySection::Decision, "We chose SQLite.");
    long.exceeds_apply_bound = true;
    assert_eq!(
        listing(&[long]),
        "Decisions\n  - We chose SQLite.\n    long@2\n    not applied: longer than 240 bytes; re-add it with `memory add`\n\n"
    );
}

fn item(entry_id: &str, section: StatefulMemorySection, content: &str) -> StatefulMemoryItem {
    StatefulMemoryItem {
        entry_id: entry_id.to_string(),
        revision: 2,
        section,
        kind: BlackboardKind::Instruction,
        content: content.to_string(),
        content_truncated: false,
        source: BlackboardProvenanceKind::User,
        updated_at: 1_790_000_000,
        replaces: Vec::new(),
        authority: None,
        scope_state: None,
        attributed_to: None,
        exceeds_apply_bound: false,
    }
}

#[test]
fn listing_names_each_entry_by_identity_and_revision() {
    let items = vec![
        item(
            "stateful-user-rule-a",
            StatefulMemorySection::UserRule,
            "Never commit.",
        ),
        item(
            "stateful-user-background-b",
            StatefulMemorySection::Background,
            "I'm a Go developer.",
        ),
    ];
    assert_eq!(
        (
            listing(&items),
            parse_target("stateful-user-rule-a@2").ok(),
            parse_target("stateful-user-rule-a").is_err(),
        ),
        (
            "Your retained rules\n  - Never commit.\n    stateful-user-rule-a@2\n\nAbout you (your words)\n  - I'm a Go developer.\n    stateful-user-background-b@2\n\n"
                .to_string(),
            Some(("stateful-user-rule-a".to_string(), 2)),
            true,
        )
    );
}
