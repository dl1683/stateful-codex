//! Canonical runtime ownership is independent of future native-home migrations.
use std::borrow::Cow;
use std::sync::LazyLock;

use codex_project_store::DomainManifest;
use codex_project_store::StoreRegistryError;
use sha2::Digest;
use sha2::Sha256;
use sha2::Sha384;
use sqlx::migrate::MigrationType;
use sqlx::migrate::Migrator;

// Frozen from the pre-project-store native history. Changes require a deliberate
// canonical suffix, never edits to these identities or adoption of native flags.
const BASE: [(i64, &str, &str); 8] = [
    (
        1,
        "runs",
        "d03014177cd4b9d26ec6d2f38c4ffa88f4b17820f345fb6e327900c6b389520e",
    ),
    (
        2,
        "steering",
        "0361bc2652910807997af07d5725319152ae31be7b1374338d2bd0ee7186c95f",
    ),
    (
        3,
        "autonomous",
        "951627514faf03dca0df10845ca108a92299e5ea8124a8048587d1341f8bfaa6",
    ),
    (
        4,
        "turn measurements",
        "b65377288714c59b3a0264833f01f2c064eb680e30b8fddcfcbe1a79bc412927",
    ),
    (
        10,
        "acceptance ledger",
        "101453e776bf09d3038ef4d93792254f9e7f1f4ecf10b24aeb1e50045f32019a",
    ),
    (
        11,
        "acceptance exemption",
        "58763c62c1ec819756bacf4c782fc7df8785072087dff0dc7c5c39a04c6b2112",
    ),
    (
        12,
        "completion attempts",
        "00d2ac541dd119259dea9ebe046599862b7a9f10d5227bf36aa89e9b2a931418",
    ),
    (
        13,
        "host answers",
        "ecbc31e62502c6d2d3ac63b2351cfc424d3d4d620f9a74369558f6b76dfbffd6",
    ),
];

static CANONICAL: LazyLock<Result<Migrator, StoreRegistryError>> =
    LazyLock::new(|| select_base(&crate::storage::MIGRATOR));

/// Compiled canonical runtime metadata only. Does not open a database or engine
/// home. Refuses changed base ownership; unrelated native versions are excluded.
pub fn project_store_domain() -> Result<DomainManifest<'static>, StoreRegistryError> {
    match &*CANONICAL {
        Ok(migrations) => Ok(DomainManifest {
            domain: "stateful-runtime",
            migrations,
        }),
        Err(_) => Err(StoreRegistryError::IncompatibleHistory),
    }
}

fn select_base(native: &Migrator) -> Result<Migrator, StoreRegistryError> {
    let mut selected = Vec::with_capacity(BASE.len());
    for (version, description, digest) in BASE {
        let mut matches = native
            .iter()
            .filter(|migration| migration.version == version);
        let migration = matches
            .next()
            .ok_or(StoreRegistryError::IncompatibleHistory)?;
        if matches.next().is_some()
            || migration.description != description
            || migration.migration_type != MigrationType::Simple
            || migration.no_tx
            || format!("{:x}", Sha256::digest(migration.sql.as_str())) != digest
            || migration.checksum.as_ref() != Sha384::digest(migration.sql.as_str()).as_slice()
        {
            return Err(StoreRegistryError::IncompatibleHistory);
        }
        selected.push(migration.clone());
    }
    Ok(Migrator {
        migrations: Cow::Owned(selected),
        ..Migrator::DEFAULT
    })
}

#[cfg(test)]
#[path = "project_domain_tests.rs"]
mod tests;
