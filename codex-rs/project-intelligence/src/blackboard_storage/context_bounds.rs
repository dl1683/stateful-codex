//! Legacy context is quarantined by scalar checks inside the selecting SQLite statement.
//! CASE prevents unsupported text (including JSON) from crossing into Rust allocations.

use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::KnowledgeAuthority;
use crate::KnowledgeCategory;
use crate::KnowledgeContext;

use super::BlackboardStoreError;
use super::knowledge::parse;

// MATERIALIZED keeps the newest row selected even when it fails policy; an older context
// must never be substituted. JSON traversal happens only after its byte bound is checked.
const CONTEXT_CHECK: &str = "
WITH current AS MATERIALIZED (
    SELECT context.rowid AS context_rowid FROM knowledge_context AS context
    JOIN blackboard_entries AS entry ON entry.id = context.entry_id
    WHERE context.project_id = ? AND context.entry_id = ?
      AND context.revision <= entry.revision
    ORDER BY context.revision DESC LIMIT 1
), checked AS MATERIALIZED (
    SELECT context.rowid AS context_rowid, CASE
        WHEN octet_length(context.project_id) > 512
          OR octet_length(context.entry_id) > 512
          OR octet_length(category) > 32 OR octet_length(authority) > 32
          OR octet_length(validity) > 32
          OR octet_length(scope_id) > 512 OR octet_length(group_id) > 512
          OR octet_length(end_condition) > 2000 OR octet_length(payload) > 8192
        THEN 0
        WHEN category IN ('rule','background','attributed_context','decision','brainstorm_option',
            'ruled_out','open_check','recipe','code_observation','commit_observation','note','legacy')
          AND authority IN ('human_direct','assistant_reported','reported_third_party','host_observed','legacy_unknown')
          AND validity IN ('current','needs_check','obsolete','historical')
          AND (source_sequence IS NULL OR source_sequence > 0)
          AND (unit_ordinal IS NULL OR unit_ordinal BETWEEN 0 AND 4294967295)
          AND CASE WHEN payload IS NULL THEN 1
            WHEN json_valid(payload)
            THEN NOT EXISTS (SELECT 1 FROM json_tree(payload) AS component
                WHERE component.type = 'text' AND octet_length(component.value) >
                    CASE WHEN component.key = 'speaker' THEN 160
                         WHEN component.key IN ('locator','path','scope_id','group_id','source_id',
                             'project_id','turn_id','thread_id','entry_id') THEN 512
                         ELSE 4096 END)
            ELSE 0 END
        THEN 1 ELSE 0 END AS supported FROM knowledge_context AS context
      JOIN current ON context.rowid = current.context_rowid
)
";

#[derive(Clone, Copy)]
pub(super) enum ContextFields {
    Complete,
    Identity,
    Successor,
}

pub(super) async fn read_context(
    connection: &mut SqliteConnection,
    project_id: &str,
    entry_id: &str,
    fields: ContextFields,
) -> Result<Option<KnowledgeContext>, BlackboardStoreError> {
    read_context_row(connection, project_id, entry_id, fields)
        .await?
        .map(StoredContext::into_context)
        .transpose()
}

/// Bounded meaning used by model policy; never contains historical text or JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextPolicy {
    pub category: KnowledgeCategory,
    pub authority: KnowledgeAuthority,
}

pub(super) async fn policy_of(
    connection: &mut SqliteConnection,
    project_id: &str,
    entry_id: &str,
) -> Result<Option<ContextPolicy>, BlackboardStoreError> {
    let row =
        sqlx::query_as::<_, (bool, Option<String>, Option<String>)>(sqlx::AssertSqlSafe(format!(
            "{CONTEXT_CHECK} SELECT supported, CASE WHEN supported THEN category END,
         CASE WHEN supported THEN authority END
         FROM checked JOIN knowledge_context AS context ON context.rowid = checked.context_rowid"
        )))
        .bind(project_id)
        .bind(entry_id)
        .fetch_optional(connection)
        .await?;
    row.map(|(supported, category, authority)| {
        if !supported {
            return Err(BlackboardStoreError::UnsupportedContext);
        }
        Ok(ContextPolicy {
            category: parse(
                category
                    .as_deref()
                    .ok_or(BlackboardStoreError::UnsupportedContext)?,
            )?,
            authority: parse(
                authority
                    .as_deref()
                    .ok_or(BlackboardStoreError::UnsupportedContext)?,
            )?,
        })
    })
    .transpose()
}

impl super::BlackboardStore {
    /// Checks legacy bounds and reads only category/authority for the current entry snapshot.
    pub async fn knowledge_policy(
        &self,
        project_id: &str,
        id: &crate::BlackboardEntryId,
    ) -> Result<Option<ContextPolicy>, BlackboardStoreError> {
        let mut connection = self.pool.acquire().await?;
        policy_of(&mut connection, project_id, id.as_str()).await
    }
}

pub(super) async fn context_of(
    connection: &mut SqliteConnection,
    project_id: &str,
    entry_id: &str,
) -> Result<Option<KnowledgeContext>, BlackboardStoreError> {
    read_context(connection, project_id, entry_id, ContextFields::Complete).await
}

async fn read_context_row(
    connection: &mut SqliteConnection,
    project_id: &str,
    entry_id: &str,
    fields: ContextFields,
) -> Result<Option<StoredContext>, BlackboardStoreError> {
    let (end_condition, source_sequence, unit_ordinal, group_id, payload) = match fields {
        ContextFields::Complete => (
            "end_condition",
            "source_sequence",
            "unit_ordinal",
            "group_id",
            "payload",
        ),
        ContextFields::Identity => ("NULL", "NULL", "NULL", "NULL", "NULL"),
        ContextFields::Successor => (
            "end_condition",
            "source_sequence",
            "unit_ordinal",
            "group_id",
            "NULL",
        ),
    };
    let columns = format!(
        "SELECT supported,
        CASE WHEN supported THEN category END AS category,
        CASE WHEN supported THEN authority END AS authority,
        CASE WHEN supported THEN validity END AS validity,
        CASE WHEN supported THEN scope_id END AS scope_id,
        CASE WHEN supported THEN {end_condition} END AS end_condition,
        CASE WHEN supported THEN {source_sequence} END AS source_sequence,
        CASE WHEN supported THEN {unit_ordinal} END AS unit_ordinal,
        CASE WHEN supported THEN {group_id} END AS group_id,
        CASE WHEN supported THEN {payload} END AS payload
        FROM checked JOIN knowledge_context AS context ON context.rowid = checked.context_rowid"
    );
    Ok(
        sqlx::query_as::<_, StoredContext>(sqlx::AssertSqlSafe(format!(
            "{CONTEXT_CHECK}{columns}"
        )))
        .bind(project_id)
        .bind(entry_id)
        .fetch_optional(connection)
        .await?,
    )
}

#[derive(Debug, FromRow, PartialEq, Eq)]
struct StoredContext {
    supported: bool,
    category: Option<String>,
    authority: Option<String>,
    validity: Option<String>,
    scope_id: Option<String>,
    end_condition: Option<String>,
    source_sequence: Option<i64>,
    unit_ordinal: Option<i64>,
    group_id: Option<String>,
    payload: Option<String>,
}

impl StoredContext {
    fn into_context(self) -> Result<KnowledgeContext, BlackboardStoreError> {
        if !self.supported {
            return Err(BlackboardStoreError::UnsupportedContext);
        }
        Ok(KnowledgeContext {
            category: parse(
                self.category
                    .as_deref()
                    .ok_or(BlackboardStoreError::UnsupportedContext)?,
            )?,
            authority: parse(
                self.authority
                    .as_deref()
                    .ok_or(BlackboardStoreError::UnsupportedContext)?,
            )?,
            validity: parse(
                self.validity
                    .as_deref()
                    .ok_or(BlackboardStoreError::UnsupportedContext)?,
            )?,
            scope_id: self.scope_id,
            end_condition: self.end_condition,
            source_sequence: self
                .source_sequence
                .map(u64::try_from)
                .transpose()
                .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
            unit_ordinal: self
                .unit_ordinal
                .map(u32::try_from)
                .transpose()
                .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
            group_id: self.group_id,
            payload: self.payload,
        })
    }
}

pub(super) async fn validate_context(
    connection: &mut SqliteConnection,
    context: &KnowledgeContext,
) -> Result<(), BlackboardStoreError> {
    for (value, limit) in [
        (&context.scope_id, 512),
        (&context.group_id, 512),
        (&context.end_condition, 2000),
        (&context.payload, 8192),
    ] {
        if value.as_ref().is_some_and(|value| value.len() > limit) {
            return Err(BlackboardStoreError::UnsupportedContext);
        }
    }
    if let Some(payload) = &context.payload {
        let json: serde_json::Value =
            serde_json::from_str(payload).map_err(|_| BlackboardStoreError::UnsupportedContext)?;
        if let Some(temporal) = json.get("temporal") {
            let temporal: crate::TemporalContext = serde_json::from_value(temporal.clone())
                .map_err(|_| BlackboardStoreError::UnsupportedContext)?;
            temporal
                .validate()
                .map_err(|_| BlackboardStoreError::UnsupportedContext)?;
        }
    }
    let supported: bool = sqlx::query_scalar(
        "WITH input(payload) AS (SELECT ?) SELECT CASE WHEN payload IS NULL THEN 1
         WHEN json_valid(payload) THEN NOT EXISTS (
            SELECT 1 FROM json_tree(payload) AS component
            WHERE component.type = 'text' AND octet_length(component.value) >
                CASE WHEN component.key = 'speaker' THEN 160
                     WHEN component.key IN ('locator','path','scope_id','group_id','source_id',
                         'project_id','turn_id','thread_id','entry_id') THEN 512
                     ELSE 4096 END)
         ELSE 0 END FROM input",
    )
    .bind(&context.payload)
    .fetch_one(connection)
    .await?;
    if !supported {
        return Err(BlackboardStoreError::UnsupportedContext);
    }
    Ok(())
}

#[cfg(test)]
#[path = "context_bounds_tests.rs"]
pub(super) mod tests;
