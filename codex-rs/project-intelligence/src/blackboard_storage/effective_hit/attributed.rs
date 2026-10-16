//! Bounded scalar projection. Caller owns admission and the read transaction.

use sqlx::Row;
use sqlx::SqliteConnection;
use thiserror::Error;

use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardKind;
use crate::BlackboardProvenance;
use crate::HierarchyNodeId;
use crate::blackboard_storage::identity::coverage_available;
use crate::blackboard_storage::identity::entry_source_eligible_on;
use crate::blackboard_storage::parse_kind;
use crate::blackboard_storage::parse_provenance;
use crate::blackboard_storage::parse_state;

/// Historical attributed text only: no verification, applicability or support.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AttributedContent {
    pub id: BlackboardEntryId,
    pub node_id: HierarchyNodeId,
    pub revision: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub kind: BlackboardKind,
    pub state: BlackboardEntryState,
    pub content: String,
    pub provenance: BlackboardProvenance,
}

#[derive(Debug, Error)]
pub(crate) enum AttributedReadError {
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
    #[error(transparent)]
    Intelligence(#[from] super::super::BlackboardStoreError),
    #[error("identity coverage is incomplete")]
    CoverageIncomplete,
    #[error("malformed entry clock")]
    InvalidClock,
    #[error("entry current revision is missing")]
    InvalidEntry,
    #[error("unsupported scalar content")]
    UnsupportedContent,
}

/// Requires prior whole-store ownership/clock validation in this same snapshot.
/// Checks coverage and ModelTool eligibility before allocating only bounded text.
/// CASE guards run inside SQLite; omitted payloads never cross the SQL boundary.
pub(crate) async fn read_on(
    connection: &mut SqliteConnection,
    project_id: &str,
    entry_id: &BlackboardEntryId,
) -> Result<Option<AttributedContent>, AttributedReadError> {
    if !coverage_available(connection, project_id).await? {
        return Err(AttributedReadError::CoverageIncomplete);
    }
    let structure: Option<(bool, bool)> = sqlx::query_as(
        "SELECT typeof(entry.revision)='integer' AND entry.revision>0
            AND typeof(entry.created_at_ms)='integer' AND entry.created_at_ms>=0
            AND typeof(entry.updated_at_ms)='integer' AND entry.updated_at_ms>=0,
            EXISTS(SELECT 1 FROM blackboard_entry_revisions AS revision
                WHERE revision.entry_id=entry.id AND revision.revision=entry.revision)
         FROM blackboard_entries AS entry WHERE entry.project_id=? AND entry.id=?",
    )
    .bind(project_id)
    .bind(entry_id.as_str())
    .fetch_optional(&mut *connection)
    .await?;
    match structure {
        None => return Ok(None),
        Some((false, _)) => return Err(AttributedReadError::InvalidClock),
        Some((true, false)) => return Err(AttributedReadError::InvalidEntry),
        Some((true, true)) => {}
    }
    if !entry_source_eligible_on(connection, project_id, entry_id).await? {
        return Ok(None);
    }
    let row = sqlx::query(
        "SELECT
            CASE WHEN typeof(entry.id)='text' AND octet_length(entry.id) BETWEEN 1 AND 512 THEN entry.id END AS id,
            CASE WHEN typeof(entry.node_id)='text' AND octet_length(entry.node_id) BETWEEN 1 AND 512 THEN entry.node_id END AS node_id,
            entry.revision, entry.created_at_ms, entry.updated_at_ms,
            CASE WHEN typeof(revision.kind)='text' AND octet_length(revision.kind) BETWEEN 1 AND 32 THEN revision.kind END AS kind,
            CASE WHEN typeof(revision.state)='text' AND octet_length(revision.state) BETWEEN 1 AND 32 THEN revision.state END AS state,
            CASE WHEN typeof(revision.content)='text' AND octet_length(revision.content) BETWEEN 1 AND 2048 THEN revision.content END AS content,
            CASE WHEN typeof(revision.provenance_kind)='text' AND octet_length(revision.provenance_kind) BETWEEN 1 AND 32 THEN revision.provenance_kind END AS provenance_kind,
            CASE WHEN typeof(revision.provenance_source_id)='text' AND octet_length(revision.provenance_source_id) BETWEEN 1 AND 512 THEN revision.provenance_source_id END AS source_id
         FROM blackboard_entries AS entry JOIN blackboard_entry_revisions AS revision
           ON revision.entry_id=entry.id AND revision.revision=entry.revision
         WHERE entry.project_id=? AND entry.id=?",
    ).bind(project_id).bind(entry_id.as_str()).fetch_one(connection).await?;
    let text = |name| -> Result<String, AttributedReadError> {
        row.try_get::<Option<String>, _>(name)
            .map_err(|_| AttributedReadError::UnsupportedContent)?
            .ok_or(AttributedReadError::UnsupportedContent)
    };
    Ok(Some(AttributedContent {
        id: BlackboardEntryId::parse(text("id")?)
            .map_err(|_| AttributedReadError::UnsupportedContent)?,
        node_id: HierarchyNodeId::parse(text("node_id")?)
            .map_err(|_| AttributedReadError::UnsupportedContent)?,
        revision: row.try_get::<i64, _>("revision")? as u64,
        created_at_ms: row.try_get("created_at_ms")?,
        updated_at_ms: row.try_get("updated_at_ms")?,
        kind: parse_kind(&text("kind")?).map_err(|_| AttributedReadError::UnsupportedContent)?,
        state: parse_state(&text("state")?).map_err(|_| AttributedReadError::UnsupportedContent)?,
        content: text("content")?,
        provenance: BlackboardProvenance {
            kind: parse_provenance(&text("provenance_kind")?)
                .map_err(|_| AttributedReadError::UnsupportedContent)?,
            source_id: text("source_id")?,
        },
    }))
}
