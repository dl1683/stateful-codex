use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::ExtractedDocument;
use crate::ExtractorIdentity;
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
    ) -> Option<ExtractedDocument> {
        let path = self.path(original_fingerprint, identity);
        let metadata = fs::metadata(&path).ok()?;
        if metadata.len() > 24 * 1024 * 1024 {
            let _ = fs::remove_file(path);
            return None;
        }
        let bytes = fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub(crate) fn store(
        &self,
        document: &ExtractedDocument,
        max_entry_bytes: u64,
        max_cache_bytes: u64,
    ) {
        let path = self.path(&document.original_fingerprint, &document.extractor);
        let Ok(bytes) = serde_json::to_vec(document) else {
            return;
        };
        if bytes.len() as u64 > max_entry_bytes {
            return;
        }
        let Some(parent) = path.parent() else {
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let temp = path.with_extension(format!("tmp-{}", std::process::id()));
        if fs::write(&temp, bytes).is_ok() {
            if fs::rename(&temp, &path).is_err() {
                let _ = fs::remove_file(&path);
                let _ = fs::rename(&temp, &path);
            }
            self.evict_old_entries(max_cache_bytes);
        }
    }

    fn path(&self, original_fingerprint: &str, identity: &ExtractorIdentity) -> PathBuf {
        let digest = original_fingerprint
            .strip_prefix("sha256:")
            .unwrap_or_default();
        let (prefix, remainder) = digest.split_at(digest.len().min(2));
        self.root
            .join(&identity.name)
            .join(&identity.version)
            .join(prefix)
            .join(format!("{remainder}.json"))
    }

    fn evict_old_entries(&self, max_bytes: u64) {
        let mut entries = Vec::new();
        collect_json_files(&self.root, &mut entries);
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
        if path.is_dir() {
            collect_json_files(&path, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
            && let Ok(metadata) = entry.metadata()
        {
            files.push((
                path,
                metadata.len(),
                metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            ));
        }
    }
}
