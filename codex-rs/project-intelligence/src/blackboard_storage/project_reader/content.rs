//! Wire-ready attributed history; no effective truth or standing-rule projection.

use codex_project_store::StoreIdentity;
use serde::Serialize;
use sqlx::SqliteConnection;

use super::ProjectKnowledgeReadError;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardKind;
use crate::BlackboardProvenance;
use crate::HierarchyNodeId;
use crate::blackboard_storage::effective_hit::attributed;
use crate::blackboard_storage::effective_hit::attributed::AttributedReadError;

/// Historical text with recorded provenance, never a verification certificate.
/// Revisions/timestamps use decimal strings; timestamp units are milliseconds.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAttributedEntry {
    pub id: BlackboardEntryId,
    pub node_id: HierarchyNodeId,
    pub revision: String,
    pub created_at_ms: String,
    pub updated_at_ms: String,
    pub kind: BlackboardKind,
    pub state: BlackboardEntryState,
    pub content: String,
    pub provenance: BlackboardProvenance,
}

/// A complete-coverage snapshot, including uniform policy misses as null entries.
/// The fixed projection, verification and omissions travel with every response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAttributedRead {
    pub project_uuid: String,
    pub store_uuid: String,
    pub snapshot_revision: String,
    pub eligibility_generation: String,
    pub coverage: &'static str,
    pub projection: &'static str,
    pub entry: Option<ProjectAttributedEntry>,
    pub verification: &'static str,
    pub omissions: [&'static str; 6],
}

pub(super) async fn read_on(
    connection: &mut SqliteConnection,
    identity: StoreIdentity,
    entry_id: &BlackboardEntryId,
) -> Result<ProjectAttributedRead, ProjectKnowledgeReadError> {
    let metadata = super::ownership::inspect_on(connection, identity).await?;
    if !metadata.coverage_complete {
        return Err(ProjectKnowledgeReadError::CoverageIncomplete);
    }
    let project_uuid = metadata.identity.project_uuid.to_string();
    let entry = attributed::read_on(connection, &project_uuid, entry_id)
        .await
        .map_err(|error| match error {
            AttributedReadError::Sqlite(error) => ProjectKnowledgeReadError::Sqlite(error),
            AttributedReadError::Intelligence(error) => {
                ProjectKnowledgeReadError::Intelligence(error)
            }
            AttributedReadError::CoverageIncomplete => {
                ProjectKnowledgeReadError::CoverageIncomplete
            }
            AttributedReadError::InvalidClock => ProjectKnowledgeReadError::InvalidClock,
            AttributedReadError::InvalidEntry => ProjectKnowledgeReadError::InvalidEntry,
            AttributedReadError::UnsupportedContent => {
                ProjectKnowledgeReadError::UnsupportedContent
            }
        })?
        .map(|entry| ProjectAttributedEntry {
            id: entry.id,
            node_id: entry.node_id,
            revision: entry.revision.to_string(),
            created_at_ms: entry.created_at_ms.to_string(),
            updated_at_ms: entry.updated_at_ms.to_string(),
            kind: entry.kind,
            state: entry.state,
            content: entry.content,
            provenance: entry.provenance,
        });
    Ok(ProjectAttributedRead {
        project_uuid,
        store_uuid: metadata.identity.store_uuid.to_string(),
        snapshot_revision: metadata.project_revision.to_string(),
        eligibility_generation: metadata.eligibility_generation.to_string(),
        coverage: "complete",
        projection: "attributedContent",
        entry,
        verification: "notEvaluated",
        omissions: [
            "structuredValue",
            "evidence",
            "premises",
            "relations",
            "context",
            "applicability",
        ],
    })
}
