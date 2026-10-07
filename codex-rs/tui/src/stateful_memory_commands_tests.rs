use codex_app_server_protocol::StatefulMemoryAddKind;
use pretty_assertions::assert_eq;

use super::Addition;
use super::MemoryCommand;
use super::USAGE;

const ADD_USAGE: &str = "Usage: /memory add rule <text>, /memory add about-me <text>, /memory add decision <choice> because <reason>, /memory add note <text>";
use super::parse;

fn addition(kind: StatefulMemoryAddKind, content: &str, reason: Option<&str>) -> MemoryCommand {
    MemoryCommand::Add(Addition {
        kind,
        content: content.to_string(),
        reason: reason.map(str::to_string),
    })
}

#[test]
fn memory_arguments_parse_into_commands() {
    assert_eq!(
        [
            "",
            "more",
            "next",
            "refresh",
            "help",
            "forget entry-a@2",
            "correct entry-b@3 Run only the affected tests.",
            "add rule Never push to main.",
            "add about I'm a Go developer.",
            "add decision Months use mth because readers confused mo with minutes",
            "add note CI runs on Windows.",
            "forget 0",
            "add",
            "add wish Something.",
            "drop 1",
        ]
        .map(parse),
        [
            Ok(MemoryCommand::List),
            Ok(MemoryCommand::More),
            Ok(MemoryCommand::More),
            Ok(MemoryCommand::List),
            Ok(MemoryCommand::Help),
            Ok(MemoryCommand::Forget(super::EntryTarget {
                entry_id: "entry-a".to_string(),
                revision: 2
            })),
            Ok(MemoryCommand::Correct(
                super::EntryTarget {
                    entry_id: "entry-b".to_string(),
                    revision: 3
                },
                "Run only the affected tests.".to_string()
            )),
            Ok(addition(
                StatefulMemoryAddKind::Rule,
                "Never push to main.",
                None
            )),
            Ok(addition(
                StatefulMemoryAddKind::Background,
                "I'm a Go developer.",
                None
            )),
            Ok(addition(
                StatefulMemoryAddKind::Decision,
                "Months use mth",
                Some("readers confused mo with minutes")
            )),
            Ok(addition(
                StatefulMemoryAddKind::Note,
                "CI runs on Windows.",
                None
            )),
            Err(USAGE.to_string()),
            Err(ADD_USAGE.to_string()),
            Err(ADD_USAGE.to_string()),
            Err(USAGE.to_string()),
        ]
    );
}

#[test]
fn explicit_targets_survive_listing_refresh() {
    let command = parse("correct original-entry@7 Preserve §3.2–§4 — α.").expect("target");
    let listing = super::MemoryListing::default();
    *listing.lock() = Some(super::Listing {
        thread_id: "another-thread".to_string(),
        generation: 99,
        ..Default::default()
    });
    assert_eq!(
        command,
        MemoryCommand::Correct(
            super::EntryTarget {
                entry_id: "original-entry".to_string(),
                revision: 7
            },
            "Preserve §3.2–§4 — α.".to_string()
        )
    );
}

#[test]
fn numeric_entry_identity_keeps_its_explicit_revision() {
    assert_eq!(
        parse("forget 1@3"),
        Ok(MemoryCommand::Forget(super::EntryTarget {
            entry_id: "1".to_string(),
            revision: 3
        }))
    );
}
