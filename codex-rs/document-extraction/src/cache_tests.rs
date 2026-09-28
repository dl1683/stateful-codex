use std::fs;
use std::io::Write;

use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::ExtractionCache;
use super::replace_file;
use crate::ExtractedBlock;
use crate::ExtractedDocument;
use crate::ExtractionAnchor;
use crate::ExtractionLimits;
use crate::ExtractionStatus;
use crate::ExtractorIdentity;

fn limits() -> ExtractionLimits {
    ExtractionLimits {
        max_cache_entry_bytes: 16 * 1024,
        max_cache_bytes: 16 * 1024,
        ..ExtractionLimits::default()
    }
}

fn document(index: usize) -> ExtractedDocument {
    ExtractedDocument {
        original_fingerprint: format!("sha256:{index:064x}"),
        original_bytes: index as u64,
        extractor: ExtractorIdentity {
            name: "codex-docx".to_owned(),
            version: "1".to_owned(),
        },
        canonical_representation_digest: "sha256:cache-test".to_owned(),
        status: ExtractionStatus::Complete,
        notices: Vec::new(),
        blocks: vec![ExtractedBlock {
            anchor: ExtractionAnchor {
                scheme: "docx-paragraph".to_owned(),
                locator: format!("body/p[{index}]"),
            },
            text: "cache".to_owned(),
            notices: Vec::new(),
        }],
    }
}

#[test]
fn cache_entry_cap_is_honored_on_store_and_load() {
    let directory = tempdir().unwrap();
    let cache = ExtractionCache::new(directory.path().to_owned());
    let limits = ExtractionLimits {
        max_cache_entry_bytes: 1,
        ..limits()
    };
    let document = document(1);

    cache.store(&document, &limits);

    assert!(
        cache
            .load(&document.original_fingerprint, &document.extractor, &limits)
            .is_none()
    );
}

#[test]
fn replacement_never_removes_destination_before_success() {
    let directory = tempdir().unwrap();
    let destination = directory.path().join("entry.json");
    let temporary = directory.path().join("entry.tmp");
    fs::write(&destination, b"old").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .unwrap()
        .write_all(b"new")
        .unwrap();

    replace_file(&temporary, &destination).unwrap();

    assert_eq!(fs::read(&destination).unwrap(), b"new");
    assert!(!temporary.exists());
}

#[test]
fn eviction_is_limited_to_hash_derived_entries() {
    let directory = tempdir().unwrap();
    let cache = ExtractionCache::new(directory.path().to_owned());
    let limits = limits();
    let first = document(1);
    let path = cache.path(&first.original_fingerprint, &first.extractor, &limits);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path.with_file_name("not-a-hash.json"), b"outside").unwrap();

    for index in 1..=16 {
        cache.store(&document(index), &limits);
    }

    assert!(path.with_file_name("not-a-hash.json").exists());
}

#[cfg(unix)]
#[test]
fn eviction_does_not_follow_directory_symlinks() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let external = outside.path().join("external.json");
    fs::write(&external, b"must survive").unwrap();
    let cache = ExtractionCache::new(directory.path().to_owned());
    let limits = limits();
    let first = document(1);
    let path = cache.path(&first.original_fingerprint, &first.extractor, &limits);
    let prefix = path.parent().unwrap();
    let namespace = prefix.parent().unwrap();
    fs::create_dir_all(namespace).unwrap();
    symlink(outside.path(), namespace.join("aa")).unwrap();

    cache.evict_old_entries(&path, 0);

    assert!(external.exists());
}
