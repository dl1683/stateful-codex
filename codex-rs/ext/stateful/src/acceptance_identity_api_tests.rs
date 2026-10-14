use std::path::Path;
use std::time::Duration;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::acceptance_identity_content::check_handle;
use crate::acceptance_identity_reason::EntryKind;
use crate::acceptance_identity_reason::IdentityChange;
use crate::acceptance_identity_reason::IdentityGap;
use crate::acceptance_identity_reason::Side;
use crate::acceptance_identity_worker::read_identity;

#[test]
fn w7_directory_handle_is_special_without_a_path_race() {
    let directory = TempDir::new().unwrap();
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0200_0000);
    }
    let handle = options.open(directory.path()).expect("directory handle");
    let metadata = handle.metadata().unwrap();
    assert_eq!(check_handle(directory.path(), &metadata, metadata.len()), Err(IdentityGap::Entry {
        path: directory.path().to_path_buf(),
        kind: EntryKind::Special,
    }));
}

#[test]
fn w9_typed_time_reason_is_specific() {
    assert_eq!(IdentityChange::Gap(Side::Start, IdentityGap::Time).render(), "workspace identity unavailable at start: identity deadline expired");
    assert_eq!(IdentityChange::Path(Path::new("input.csv").to_path_buf()).render(), "the check changed a file no criterion pins: input.csv");
}

#[tokio::test]
async fn w13_zero_budget_precedes_filesystem_work() {
    assert_eq!(read_identity(Duration::ZERO, |_| -> Result<(), IdentityGap> { panic!("must not read roots") }).await, Err(IdentityGap::Time));
}
