use super::prepend_path;
use pretty_assertions::assert_eq;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::PathBuf;

#[test]
fn joining_preserves_order_spaces_duplicates_and_empty_entries() {
    let parent = std::env::temp_dir().join("codex binary directory");
    let inherited_entries = vec![
        parent.join("tools with spaces;more"),
        PathBuf::new(),
        parent.clone(),
    ];
    let inherited = std::env::join_paths(&inherited_entries).unwrap();
    let joined = prepend_path(&parent, Some(&inherited)).unwrap();
    let expected: Vec<_> = std::iter::once(parent).chain(inherited_entries).collect();
    assert_eq!(std::env::split_paths(&joined).collect::<Vec<_>>(), expected);
}

#[test]
fn joining_distinguishes_unset_and_empty_path() {
    let parent = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    for (inherited, expected) in [
        (None, vec![parent.clone()]),
        (Some(OsStr::new("")), vec![parent.clone(), PathBuf::new()]),
    ] {
        let joined = prepend_path(&parent, inherited).unwrap();
        assert_eq!(std::env::split_paths(&joined).collect::<Vec<_>>(), expected);
    }
}

#[test]
#[cfg(any(unix, windows))]
fn joining_preserves_non_unicode_paths() {
    #[cfg(unix)]
    let name = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![b'x', 0xff])
    };
    #[cfg(windows)]
    let name = {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0x78, 0xd800])
    };
    let parent = std::env::temp_dir().join(name);
    let inherited = std::env::join_paths([parent.join("inherited")]).unwrap();
    let joined = prepend_path(&parent, Some(&inherited)).unwrap();
    assert_eq!(
        std::env::split_paths(&joined).collect::<Vec<_>>(),
        vec![parent.clone(), parent.join("inherited")]
    );
}
