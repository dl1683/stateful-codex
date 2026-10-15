use std::io;

use codex_utils_absolute_path::AbsolutePathBuf;
use thiserror::Error;

/// Qualify the exact existing file or directory, including its resolved filesystem.
/// A project-root check does not qualify descendant mounts.
/// Conservative admission is not discovery of arbitrary sync software.
/// Callers must additionally establish that the selected project is unsynchronized.
pub fn qualify_local_filesystem(
    path: &AbsolutePathBuf,
) -> Result<(), FilesystemQualificationError> {
    let canonical = path.canonicalize()?;
    let path = canonical.as_path();
    for component in path.components() {
        let name = component.as_os_str().to_string_lossy().to_ascii_lowercase();
        if name == "onedrive"
            || name.starts_with("onedrive - ")
            || matches!(name.as_str(), "dropbox" | "google drive" | "googledrive")
        {
            return Err(FilesystemQualificationError::UnsupportedFilesystem);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::path::Component;
        use std::path::Prefix;
        use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
        use windows_sys::Win32::Storage::FileSystem::GetVolumePathNameW;

        if !matches!(
            path.components().next(),
            Some(Component::Prefix(prefix))
                if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
        ) {
            return Err(FilesystemQualificationError::UnsupportedFilesystem);
        }
        let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut volume = vec![0_u16; 32_768];
        // Both buffers are terminated/allocated for the lengths passed to Windows.
        if unsafe {
            GetVolumePathNameW(
                path.as_ptr(),
                volume.as_mut_ptr(),
                /*cchbufferlength*/ 32_768,
            )
        } == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        // DRIVE_FIXED. Network, removable and unknown volumes are unqualified.
        if unsafe { GetDriveTypeW(volume.as_ptr()) } != 3 {
            return Err(FilesystemQualificationError::UnsupportedFilesystem);
        }
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::ffi::CString;
        use std::mem::MaybeUninit;
        use std::os::unix::ffi::OsStrExt;

        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in project path"))?;
        let mut stat = MaybeUninit::<libc::statfs>::uninit();
        // statfs initializes the entire result on success; path is NUL terminated.
        if unsafe { libc::statfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        let stat = unsafe { stat.assume_init() };
        #[cfg(target_os = "linux")]
        if !matches!(
            stat.f_type,
            libc::EXT4_SUPER_MAGIC
                | libc::XFS_SUPER_MAGIC
                | libc::BTRFS_SUPER_MAGIC
                | libc::TMPFS_MAGIC
                | libc::OVERLAYFS_SUPER_MAGIC
        ) {
            return Err(FilesystemQualificationError::UnsupportedFilesystem);
        }
        #[cfg(target_os = "macos")]
        {
            // The kernel's filesystem name is a NUL-terminated fixed-size field.
            let filesystem = unsafe { std::ffi::CStr::from_ptr(stat.f_fstypename.as_ptr()) };
            if !matches!(filesystem.to_bytes(), b"apfs" | b"hfs") {
                return Err(FilesystemQualificationError::UnsupportedFilesystem);
            }
        }
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    return Err(FilesystemQualificationError::UnsupportedFilesystem);

    Ok(())
}

/// Why a concrete file or directory cannot pass conservative local-storage checks.
#[derive(Debug, Error)]
pub enum FilesystemQualificationError {
    #[error("the location requires a qualified local, unsynchronized filesystem")]
    UnsupportedFilesystem,
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[cfg(test)]
#[path = "filesystem_tests.rs"]
mod tests;
