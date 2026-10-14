use std::path::PathBuf;

use pretty_assertions::assert_eq;

use super::Bound;
use super::EntryKind;
use super::IdentityChange;
use super::IdentityGap;
use super::Side;

#[test]
fn w9_gap_reasons_name_the_cause_and_bound_unicode_paths() {
    let long = PathBuf::from("é".repeat(5_000));
    for bound in [Bound::Visits, Bound::Entries, Bound::Directories, Bound::Pending, Bound::RetainedBytes, Bound::PathLength, Bound::Bytes, Bound::GitOutput] {
        let change = IdentityChange::Gap(Side::Start, IdentityGap::Bound(bound.clone()));
        assert_eq!(change.render(), format!("workspace identity unavailable at start: identity bound exceeded: {bound:?}"));
    }
    for kind in [EntryKind::EscapingLink, EntryKind::DirectoryLink, EntryKind::Special, EntryKind::Unreadable, EntryKind::Reparse, EntryKind::NotContained, EntryKind::Submodule, EntryKind::EmbeddedRepository, EntryKind::InexactName] {
        let change = IdentityChange::Gap(Side::End, IdentityGap::Entry { path: long.clone(), kind: kind.clone() });
        let expected = format!("workspace identity unavailable at end: identity entry {}: {kind:?}", "é".repeat(100));
        assert_eq!(change.render(), expected);
        assert!(change.render().len() <= 512);
    }
    for gap in [IdentityGap::Roots, IdentityGap::Git, IdentityGap::Slots, IdentityGap::Worker, IdentityGap::Time] {
        let reason = IdentityChange::Gap(Side::End, gap).render();
        assert!(reason.starts_with("workspace identity unavailable at end: "));
        assert!(reason.len() <= 512);
    }
    assert_eq!(IdentityChange::Path(long).render(), format!("the check changed a file no criterion pins: {}", "é".repeat(100)));
}
