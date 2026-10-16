use super::bounded;
use pretty_assertions::assert_eq;
use std::ffi::OsString;

#[test]
fn preflight_preserves_valid_unicode_and_refuses_count_and_byte_excess() {
    let arguments = vec![
        OsString::from("program"),
        OsString::from("\u{092e}\u{1f642}"),
    ];
    assert_eq!(
        bounded(arguments.into_iter()).unwrap(),
        vec!["program", "\u{092e}\u{1f642}"]
    );
    assert_eq!(
        bounded(std::iter::repeat_n(OsString::from("x"), /*n*/ 33)),
        Err(())
    );
    assert_eq!(
        bounded(std::iter::once(OsString::from("x".repeat(/*n*/ 16321)))),
        Err(())
    );
}

#[test]
fn preflight_rejects_non_utf8_without_lossy_conversion() {
    #[cfg(windows)]
    let argument = {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0xd800])
    };
    #[cfg(unix)]
    let argument = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![0xff])
    };
    assert_eq!(bounded(std::iter::once(argument)), Err(()));
}
