//! Rust classifies junctions as symlinks. Read the native tag so only genuine symbolic
//! links reach D-7; junctions and other reparse points remain Reparse gaps.
use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use crate::acceptance_identity_content::entry;
use crate::acceptance_identity_reason::EntryKind;
use crate::acceptance_identity_reason::IdentityGap;

pub(super) fn is_symlink(path: &Path) -> Result<bool, IdentityGap> {
    use std::ffi::c_void;
    const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const ATTRIBUTE_TAG_INFO: i32 = 9;
    const SYMLINK_TAG: u32 = 0xa000_000c;
    #[repr(C)]
    struct AttributeTag {
        attributes: u32,
        tag: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandleEx(
            file: *mut c_void,
            class: i32,
            info: *mut c_void,
            size: u32,
        ) -> i32;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_REPARSE_POINT | BACKUP_SEMANTICS)
        .open(path)
        .map_err(|_| entry(path, EntryKind::Unreadable))?;
    let mut info = AttributeTag {
        attributes: 0,
        tag: 0,
    };
    // SAFETY: the live handle is opened without following the reparse point, and the
    // output buffer has the layout and size required by FileAttributeTagInfo.
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            ATTRIBUTE_TAG_INFO,
            (&raw mut info).cast(),
            std::mem::size_of::<AttributeTag>() as u32,
        )
    };
    if success == 0 {
        return Err(entry(path, EntryKind::Unreadable));
    }
    Ok(info.tag == SYMLINK_TAG)
}
