use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use crate::ExtractedDocument;
use crate::ExtractionLimits;
use crate::ExtractorIdentity;

const EVICTION_INTERVAL: u64 = 16;
static STORE_COUNT: AtomicU64 = AtomicU64::new(0);
static TEMPORARY_COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub(crate) struct ExtractionCache {
    root: PathBuf,
}

impl ExtractionCache {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn load(
        &self,
        original_fingerprint: &str,
        identity: &ExtractorIdentity,
        limits: &ExtractionLimits,
    ) -> Option<ExtractedDocument> {
        let path = self.path(original_fingerprint, identity, limits);
        let metadata = fs::symlink_metadata(&path).ok()?;
        if !metadata.file_type().is_file() || metadata.len() > limits.max_cache_entry_bytes {
            return None;
        }
        let bytes = fs::read(path).ok()?;
        if bytes.len() as u64 > limits.max_cache_entry_bytes {
            return None;
        }
        serde_json::from_slice(&bytes).ok()
    }

    pub(crate) fn store(&self, document: &ExtractedDocument, limits: &ExtractionLimits) {
        let path = self.path(&document.original_fingerprint, &document.extractor, limits);
        let Ok(bytes) = serde_json::to_vec(document) else {
            return;
        };
        if bytes.len() as u64 > limits.max_cache_entry_bytes {
            return;
        }
        let Some(parent) = path.parent() else {
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let temp = unique_temp_path(&path);
        let Ok(mut file) = OpenOptions::new().write(true).create_new(true).open(&temp) else {
            return;
        };
        if file.write_all(&bytes).is_err() || file.sync_all().is_err() {
            let _ = fs::remove_file(&temp);
            return;
        }
        drop(file);
        if replace_file(&temp, &path).is_ok()
            && STORE_COUNT.fetch_add(1, Ordering::Relaxed) % EVICTION_INTERVAL
                == EVICTION_INTERVAL - 1
        {
            self.evict_old_entries(&path, limits.max_cache_bytes);
        }
    }

    fn path(
        &self,
        original_fingerprint: &str,
        identity: &ExtractorIdentity,
        limits: &ExtractionLimits,
    ) -> PathBuf {
        let digest = original_fingerprint
            .strip_prefix("sha256:")
            .unwrap_or_default();
        let (prefix, remainder) = digest.split_at(digest.len().min(2));
        self.namespace(identity, limits)
            .join(prefix)
            .join(format!("{remainder}.json"))
    }

    fn namespace(&self, identity: &ExtractorIdentity, limits: &ExtractionLimits) -> PathBuf {
        self.root
            .join(&identity.name)
            .join(&identity.version)
            .join(crate::canonical::limits_digest(limits).replace("sha256:", ""))
    }

    fn evict_old_entries(&self, path: &Path, max_bytes: u64) {
        let mut entries = Vec::new();
        let Some(namespace) = path.parent().and_then(Path::parent) else {
            return;
        };
        collect_json_files(namespace, &mut entries);
        let mut total = entries.iter().map(|(_, size, _)| *size).sum::<u64>();
        entries.sort_by_key(|(_, _, modified)| *modified);
        for (path, size, _) in entries {
            if total <= max_bytes {
                break;
            }
            if fs::remove_file(path).is_ok() {
                total -= size;
            }
        }
    }
}

fn collect_json_files(directory: &Path, files: &mut Vec<(PathBuf, u64, SystemTime)>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_dir()
            || metadata.file_type().is_symlink()
            || !is_hash_prefix(entry.file_name().to_str().unwrap_or_default())
        {
            continue;
        }
        let Ok(files_in_prefix) = fs::read_dir(path) else {
            continue;
        };
        for entry in files_in_prefix.flatten() {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            let name = entry.file_name();
            let name = name.to_str().unwrap_or_default();
            let Some(stem) = name.strip_suffix(".json") else {
                continue;
            };
            if !metadata.file_type().is_file()
                || metadata.file_type().is_symlink()
                || stem.len() != 62
                || !stem.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                continue;
            }
            files.push((
                path,
                metadata.len(),
                metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            ));
        }
    }
}

fn is_hash_prefix(name: &str) -> bool {
    name.len() == 2 && name.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let count = TEMPORARY_COUNT.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("cache");
    path.with_file_name(format!(".{file_name}.tmp-{}-{count}", std::process::id()))
}

#[cfg(not(windows))]
fn replace_file(temp: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(temp, destination)
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;

#[cfg(windows)]
fn replace_file(temp: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING;
    use windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    use windows_sys::Win32::Storage::FileSystem::REPLACEFILE_WRITE_THROUGH;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    let temp = temp
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let replaced = unsafe {
        ReplaceFileW(
            destination.as_ptr(),
            temp.as_ptr(),
            null_mut(),
            REPLACEFILE_WRITE_THROUGH,
            null_mut(),
            null_mut(),
        )
    };
    if replaced != 0 {
        return Ok(());
    }
    let move_result = unsafe {
        MoveFileExW(
            temp.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if move_result != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
