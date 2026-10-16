//! Project-owned, read-only intelligence admission; no native/home routing.

mod ownership;

use codex_project_store::DomainManifest;
use codex_project_store::ProjectDomainReader;
use codex_project_store::ProjectReadError;
use codex_project_store::ProjectStoreReader;
use codex_project_store::StoreIdentity;
use codex_project_store::StoreRegistryError;
use thiserror::Error;

/// A compiled-domain reader. Every inspection revalidates ownership and clocks
/// under the same admitted transaction; no engine home, pool or fallback exists.
pub struct ProjectKnowledgeReader {
    reader: ProjectDomainReader,
    admission_identity: StoreIdentity,
}

/// Snapshot metadata only, not effective knowledge or an authority certificate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectKnowledgeSnapshot {
    pub identity: StoreIdentity,
    pub project_revision: u64,
    pub eligibility_generation: u64,
    pub coverage_complete: bool,
}

/// Project intelligence admission failures, distinct from an effective read miss.
#[derive(Debug, Error)]
pub enum ProjectKnowledgeReadError {
    #[error(transparent)]
    Registry(#[from] StoreRegistryError),
    #[error(transparent)]
    Snapshot(#[from] ProjectReadError),
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
    #[error(transparent)]
    Intelligence(#[from] super::BlackboardStoreError),
    #[error("expected project/store identity does not match")]
    IdentityMismatch,
    #[error("malformed project clock, entry revision or timestamp")]
    InvalidClock,
    #[error("an entry has no matching current revision")]
    InvalidEntry,
    #[error("foreign project ownership or invalid child dependency")]
    InvalidOwnership,
}

impl ProjectKnowledgeReader {
    /// Consume an admitted store only after checking the crate's actual compiled
    /// migration list. Mismatch awaits connection release; no unchecked constructor.
    pub async fn from_project_store(
        reader: ProjectStoreReader,
    ) -> Result<Self, ProjectKnowledgeReadError> {
        let admission_identity = reader.identity().clone();
        Ok(Self {
            admission_identity,
            reader: reader
                .into_domain_reader(&DomainManifest {
                    domain: "project-intelligence",
                    migrations: &super::MIGRATOR,
                })
                .await?,
        })
    }

    /// Validate one fresh intelligence-domain snapshot. Both expected UUIDs must
    /// match before domain access. Ordinary refusals finish the read transaction;
    /// cancellation/admission/commit failure keeps the kernel owner close-only.
    /// This returns no entry text, model context or effective root.
    pub async fn inspect(
        &mut self,
        expected: &StoreIdentity,
    ) -> Result<ProjectKnowledgeSnapshot, ProjectKnowledgeReadError> {
        if expected != &self.admission_identity {
            return Err(ProjectKnowledgeReadError::IdentityMismatch);
        }
        let mut snapshot = self.reader.begin_snapshot().await?;
        let identity = snapshot.identity().clone();
        let result = ownership::inspect_on(snapshot.connection(), identity).await;
        snapshot.finish().await?;
        result
    }

    /// Await file-handle release, including an unfinished kernel transaction.
    pub async fn close(self) -> Result<(), sqlx::Error> {
        self.reader.close().await
    }
}

#[cfg(test)]
#[path = "project_reader_tests.rs"]
mod tests;
