use codex_app_server_protocol::BlackboardKind;
use codex_app_server_protocol::BlackboardProvenanceKind;
use codex_app_server_protocol::StatefulMemoryAddKind;
use codex_app_server_protocol::StatefulMemoryItem;
use codex_app_server_protocol::StatefulMemorySection;
use pretty_assertions::assert_eq;

use super::Addition;
use super::Listing;
use super::MemoryCommand;
use super::USAGE;

const ADD_USAGE: &str = "Usage: /memory add rule <text> (or: rule for <scope>: <text>), /memory add about-me <text>, /memory add decision <choice> because <reason>, /memory add note <text>";
use super::numbered;
use super::parse;

fn item(entry_id: &str, section: StatefulMemorySection, content: &str) -> StatefulMemoryItem {
    StatefulMemoryItem {
        entry_id: entry_id.to_string(),
        revision: 1,
        section,
        kind: BlackboardKind::Fact,
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

fn addition(
    kind: StatefulMemoryAddKind,
    content: &str,
    scope: Option<&str>,
    reason: Option<&str>,
) -> MemoryCommand {
    MemoryCommand::Add(Addition {
        kind,
        content: content.to_string(),
        scope: scope.map(str::to_string),
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
            "investigations",
            "join 2",
            "end 1",
            "leave",
            "help",
            "forget 2",
            "correct 1 Run only the affected tests.",
            "add rule Never push to main.",
            "add rule for this investigation, until we agree: Do not change code.",
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
            Ok(MemoryCommand::Investigations),
            Ok(MemoryCommand::Join(2)),
            Ok(MemoryCommand::End(1)),
            Ok(MemoryCommand::Leave),
            Ok(MemoryCommand::Help),
            Ok(MemoryCommand::Forget(2)),
            Ok(MemoryCommand::Correct(
                1,
                "Run only the affected tests.".to_string()
            )),
            Ok(addition(
                StatefulMemoryAddKind::Rule,
                "Never push to main.",
                None,
                None
            )),
            Ok(addition(
                StatefulMemoryAddKind::Rule,
                "Do not change code.",
                Some("this investigation, until we agree"),
                None
            )),
            Ok(addition(
                StatefulMemoryAddKind::Background,
                "I'm a Go developer.",
                None,
                None
            )),
            Ok(addition(
                StatefulMemoryAddKind::Decision,
                "Months use mth",
                None,
                Some("readers confused mo with minutes")
            )),
            Ok(addition(
                StatefulMemoryAddKind::Note,
                "CI runs on Windows.",
                None,
                None
            )),
            Err(USAGE.to_string()),
            Err(ADD_USAGE.to_string()),
            Err(ADD_USAGE.to_string()),
            Err(USAGE.to_string()),
        ]
    );
}

/// tui8: correcting one item and then forgetting another uses the numbers first shown;
/// pages and additions append numbers; nothing renumbers.
#[test]
fn numbers_stay_put_through_changes_pages_and_additions() {
    let mut listing = Listing {
        thread_id: "thread-1".to_string(),
        generation: 1,
        slots: Vec::new(),
        cursor: Some("next".to_string()),
    };
    listing.append(vec![
        item(
            "knowledge",
            StatefulMemorySection::Knowledge,
            "Orientation.",
        ),
        item("rule", StatefulMemorySection::UserRule, "Never commit."),
        item("decision", StatefulMemorySection::Decision, "Use mth."),
    ]);
    // Correct item 1 (the rule, first in section order), then forget item 3.
    let corrected = item(
        "rule-2",
        StatefulMemorySection::UserRule,
        "Never commit or push.",
    );
    listing.update("rule", |slot| slot.item = corrected.clone());
    listing.update("knowledge", |slot| slot.forgotten = true);
    let page_numbers = listing.append(vec![item(
        "later",
        StatefulMemorySection::Knowledge,
        "From page two.",
    )]);
    let shown = numbered(&listing, 1)
        .into_iter()
        .map(|(number, item)| (number, item.content))
        .collect::<Vec<_>>();
    assert_eq!(
        (
            shown,
            page_numbers,
            listing.current(3).map(|item| item.content.clone()),
            listing.current(1).map(|item| item.content.clone()),
        ),
        (
            vec![
                (1, "Never commit or push.".to_string()),
                (2, "Use mth.".to_string()),
                (4, "From page two.".to_string()),
            ],
            vec![4],
            Err("Item 3 was forgotten.".to_string()),
            Ok("Never commit or push.".to_string()),
        )
    );
}

/// Item 3 review 3: numbers resolve only against the listing on screen; a listing published
/// but not yet displayed (or one for another thread) is refused, never used.
#[test]
fn numbers_resolve_only_against_the_listing_on_screen() {
    let listing = super::MemoryListing::default();
    let generation = listing.begin();
    let mut shown = Listing {
        thread_id: "thread-1".to_string(),
        generation,
        slots: Vec::new(),
        cursor: None,
    };
    shown.append(vec![item(
        "e1",
        StatefulMemorySection::UserRule,
        "Never push.",
    )]);
    *listing.lock() = Some(shown);
    let before_display = listing.on_screen("thread-1").is_ok();
    listing.displayed("thread-1", generation);
    let on_screen = listing.on_screen("thread-1").map(|shown| shown.generation);
    let other_thread = listing.on_screen("thread-2").is_ok();
    // A newer listing published after the display is not yet on screen.
    let newer = listing.begin();
    if let Some(current) = listing.lock().as_mut() {
        current.generation = newer;
    }
    let replaced = listing.on_screen("thread-1").is_ok();
    assert_eq!(
        (before_display, on_screen, other_thread, replaced),
        (false, Ok(generation), false, false)
    );
}

/// Item 3 final review: investigation numbers resolve only against the investigations list on
/// screen, so a list published in the background never shifts what a number ends.
#[test]
fn investigation_numbers_resolve_only_against_the_list_on_screen() {
    let listing = super::MemoryListing::default();
    let first = listing.begin();
    *listing
        .investigations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((
        first,
        "thread-1".to_string(),
        vec!["scope-a".to_string(), "scope-b".to_string()],
    ));
    listing.displayed("thread-1", first);
    let shown = listing.scope_on_screen("thread-1", 2);
    // A newer list ([C, A, B]) is published but not yet displayed.
    let newer = listing.begin();
    *listing
        .investigations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((
        newer,
        "thread-1".to_string(),
        vec![
            "scope-c".to_string(),
            "scope-a".to_string(),
            "scope-b".to_string(),
        ],
    ));
    let before_display = listing.scope_on_screen("thread-1", 2).is_ok();
    listing.displayed("thread-1", newer);
    let after_display = listing.scope_on_screen("thread-1", 2);
    assert_eq!(
        (shown, before_display, after_display),
        (Ok("scope-b".to_string()), false, Ok("scope-a".to_string()))
    );
}
