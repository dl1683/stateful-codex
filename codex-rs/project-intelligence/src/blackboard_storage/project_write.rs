use codex_project_store::DomainManifest;
use codex_project_store::ProjectWriteError;
use codex_project_store::ProjectWriteTransaction;
use thiserror::Error;

use super::ProjectKnowledgeSnapshot;

/// Refusal of a trusted caller's project knowledge clock precondition.
#[derive(Debug, Error)]
pub enum ProjectKnowledgeWriteError {
    #[error(transparent)]
    Writer(#[from] ProjectWriteError),
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
    #[error(transparent)]
    Intelligence(#[from] super::BlackboardStoreError),
    #[error("expected project/store identity does not match")]
    IdentityMismatch,
    #[error("malformed project knowledge clock")]
    InvalidClock,
    #[error("project knowledge or retirement generation changed")]
    SnapshotChanged,
    #[error("identity coverage is incomplete")]
    CoverageIncomplete,
}

/// Recheck knowledge and retirement clocks under the final writer reservation.
/// Refusal/cancellation drops the guard, rolling back prior writes and making its
/// owner close-only. No separate database lock or corpus scan is performed.
///
/// This is a trusted clock precondition, not a completion certificate. The caller
/// must admit domain ownership and validate material findings, source freshness,
/// steering and runtime acceptance separately. The expected snapshot is not an
/// authority credential. Further domain writes may invalidate this check; place
/// it at the final boundary before terminal commit. Native routing is unchanged.
pub async fn revalidate_project_knowledge_clocks<'a>(
    mut transaction: ProjectWriteTransaction<'a>,
    expected: &ProjectKnowledgeSnapshot,
) -> Result<ProjectWriteTransaction<'a>, ProjectKnowledgeWriteError> {
    transaction.require_domain(&DomainManifest {
        domain: "project-intelligence",
        migrations: &super::MIGRATOR,
    })?;
    if transaction.identity() != &expected.identity {
        return Err(ProjectKnowledgeWriteError::IdentityMismatch);
    }
    let project = expected.identity.project_uuid.to_string();
    let (revision, generation): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT
         (SELECT CASE WHEN typeof(revision)='integer' AND revision>=0
          THEN CAST(revision AS TEXT) ELSE 'invalid' END
          FROM project_intelligence_revisions WHERE project_id=?),
         (SELECT CASE WHEN typeof(generation)='integer' AND generation>0
          THEN CAST(generation AS TEXT) ELSE 'invalid' END
          FROM capsule_eligibility_generations WHERE project_id=?)",
    )
    .bind(&project)
    .bind(&project)
    .fetch_one(transaction.connection())
    .await?;
    let revision = revision
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| ProjectKnowledgeWriteError::InvalidClock)?
        .unwrap_or_default();
    let generation = generation
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| ProjectKnowledgeWriteError::InvalidClock)?
        .unwrap_or_default();
    if revision != expected.project_revision || generation != expected.eligibility_generation {
        return Err(ProjectKnowledgeWriteError::SnapshotChanged);
    }
    if !expected.coverage_complete
        || !super::identity::coverage_available(transaction.connection(), &project).await?
    {
        return Err(ProjectKnowledgeWriteError::CoverageIncomplete);
    }
    Ok(transaction)
}

#[cfg(test)]
#[path = "project_write_tests.rs"]
mod tests;
