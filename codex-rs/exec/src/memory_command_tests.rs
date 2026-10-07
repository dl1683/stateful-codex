use codex_app_server_protocol::BlackboardKind;
use codex_app_server_protocol::BlackboardProvenanceKind;
use codex_app_server_protocol::StatefulMemoryItem;
use codex_app_server_protocol::StatefulMemorySection;
use pretty_assertions::assert_eq;

use super::listing;
use super::parse_target;

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
        scope_title: None,
        attributed_to: None,
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
            "Your rules (applied)\n  - Never commit.\n    stateful-user-rule-a@2\n\nAbout you (your words)\n  - I'm a Go developer.\n    stateful-user-background-b@2\n\n"
                .to_string(),
            Some(("stateful-user-rule-a".to_string(), 2)),
            true,
        )
    );
}
