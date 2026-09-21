use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::BlackboardEntryId;
use crate::BlackboardProvenance;
use crate::BlackboardRelation;
use crate::BlackboardRelationId;
use crate::BlackboardRelationKind;
use crate::ConfidenceScore;
use crate::NewBlackboardRelation;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::load_entry;
use super::parse_provenance;
use super::provenance_name;
use super::unix_timestamp_millis;

const INITIAL_REVISION: i64 = 1;

impl BlackboardStore {
    pub async fn create_relation(
        &self,
        id: BlackboardRelationId,
        value: NewBlackboardRelation,
    ) -> Result<BlackboardRelation, BlackboardStoreError> {
        value.validate()?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = load_relation(&mut transaction, &value.project_id, &id).await? {
            if existing.value != value {
                return Err(BlackboardStoreError::RelationIdentityConflict(
                    id.to_string(),
                ));
            }
            transaction.commit().await?;
            return Ok(existing);
        }
        for endpoint in [&value.from_entry_id, &value.to_entry_id] {
            if load_entry(&mut transaction, &value.project_id, endpoint)
                .await?
                .is_none()
            {
                return Err(BlackboardStoreError::RelationEndpointNotFound(
                    endpoint.to_string(),
                ));
            }
        }
        let now = unix_timestamp_millis()?;
        sqlx::query(
            "INSERT INTO blackboard_relations (
                id, project_id, from_entry_id, to_entry_id, kind, note,
                confidence_basis_points, provenance_kind, provenance_source_id,
                revision, created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(&value.project_id)
        .bind(value.from_entry_id.as_str())
        .bind(value.to_entry_id.as_str())
        .bind(relation_kind_name(value.kind))
        .bind(&value.note)
        .bind(i64::from(value.confidence.basis_points()))
        .bind(provenance_name(value.provenance.kind))
        .bind(&value.provenance.source_id)
        .bind(INITIAL_REVISION)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        let relation = load_relation(&mut transaction, &value.project_id, &id)
            .await?
            .ok_or_else(|| BlackboardStoreError::CorruptEntry(id.to_string()))?;
        transaction.commit().await?;
        Ok(relation)
    }

    pub async fn list_relations(
        &self,
        project_id: &str,
        entry_id: &BlackboardEntryId,
        max_results: u32,
    ) -> Result<Vec<BlackboardRelation>, BlackboardStoreError> {
        if max_results == 0 || max_results > 256 {
            return Err(crate::BlackboardError::InvalidRelationQuery.into());
        }
        let mut connection = self.pool.acquire().await?;
        if load_entry(&mut connection, project_id, entry_id)
            .await?
            .is_none()
        {
            return Err(BlackboardStoreError::EntryNotFound(entry_id.to_string()));
        }
        load_relations_for_entry(&mut connection, project_id, entry_id, max_results).await
    }
}

#[derive(FromRow)]
struct StoredRelation {
    id: String,
    project_id: String,
    from_entry_id: String,
    to_entry_id: String,
    kind: String,
    note: Option<String>,
    confidence_basis_points: i64,
    provenance_kind: String,
    provenance_source_id: String,
    revision: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
}

pub(super) async fn load_relations_for_entry(
    connection: &mut SqliteConnection,
    project_id: &str,
    entry_id: &BlackboardEntryId,
    max_results: u32,
) -> Result<Vec<BlackboardRelation>, BlackboardStoreError> {
    let stored = sqlx::query_as::<_, StoredRelation>(
        "SELECT * FROM blackboard_relations
         WHERE project_id = ? AND (from_entry_id = ? OR to_entry_id = ?)
         ORDER BY id LIMIT ?",
    )
    .bind(project_id)
    .bind(entry_id.as_str())
    .bind(entry_id.as_str())
    .bind(i64::from(max_results))
    .fetch_all(connection)
    .await?;
    stored.into_iter().map(parse_relation).collect()
}

async fn load_relation(
    connection: &mut SqliteConnection,
    project_id: &str,
    id: &BlackboardRelationId,
) -> Result<Option<BlackboardRelation>, BlackboardStoreError> {
    sqlx::query_as::<_, StoredRelation>(
        "SELECT * FROM blackboard_relations WHERE project_id = ? AND id = ?",
    )
    .bind(project_id)
    .bind(id.as_str())
    .fetch_optional(connection)
    .await?
    .map(parse_relation)
    .transpose()
}

fn parse_relation(stored: StoredRelation) -> Result<BlackboardRelation, BlackboardStoreError> {
    let id = BlackboardRelationId::parse(&stored.id)
        .map_err(|_| BlackboardStoreError::CorruptEntry(stored.id.clone()))?;
    let confidence = u16::try_from(stored.confidence_basis_points)
        .ok()
        .and_then(|value| ConfidenceScore::from_basis_points(value).ok())
        .ok_or_else(|| BlackboardStoreError::CorruptEntry(stored.id.clone()))?;
    let value = NewBlackboardRelation {
        project_id: stored.project_id,
        from_entry_id: BlackboardEntryId::parse(stored.from_entry_id)?,
        to_entry_id: BlackboardEntryId::parse(stored.to_entry_id)?,
        kind: parse_relation_kind(&stored.kind)?,
        note: stored.note,
        confidence,
        provenance: BlackboardProvenance {
            kind: parse_provenance(&stored.provenance_kind)?,
            source_id: stored.provenance_source_id,
        },
    };
    value
        .validate()
        .map_err(|_| BlackboardStoreError::CorruptEntry(stored.id.clone()))?;
    Ok(BlackboardRelation {
        id,
        value,
        revision: u64::try_from(stored.revision)
            .map_err(|_| BlackboardStoreError::CorruptEntry(stored.id.clone()))?,
        created_at_ms: stored.created_at_ms,
        updated_at_ms: stored.updated_at_ms,
    })
}

fn relation_kind_name(kind: BlackboardRelationKind) -> &'static str {
    match kind {
        BlackboardRelationKind::Supports => "supports",
        BlackboardRelationKind::Contradicts => "contradicts",
        BlackboardRelationKind::DependsOn => "depends_on",
        BlackboardRelationKind::RelatedTo => "related_to",
    }
}

fn parse_relation_kind(value: &str) -> Result<BlackboardRelationKind, BlackboardStoreError> {
    match value {
        "supports" => Ok(BlackboardRelationKind::Supports),
        "contradicts" => Ok(BlackboardRelationKind::Contradicts),
        "depends_on" => Ok(BlackboardRelationKind::DependsOn),
        "related_to" => Ok(BlackboardRelationKind::RelatedTo),
        _ => Err(BlackboardStoreError::CorruptEnum(value.to_string())),
    }
}
