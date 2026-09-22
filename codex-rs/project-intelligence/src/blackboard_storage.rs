use codex_state::SqliteConfig;
use sqlx::FromRow;
use sqlx::SqliteConnection;
use sqlx::SqlitePool;
use sqlx::migrate::MigrateError;
use thiserror::Error;

use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardError;
use crate::BlackboardEvidenceLink;
use crate::BlackboardImportance;
use crate::BlackboardKind;
use crate::BlackboardProvenance;
use crate::BlackboardProvenanceKind;
use crate::BlackboardStructuredValue;
use crate::BlackboardVerification;
use crate::ConfidenceScore;
use crate::ContextMapEntryId;
use crate::NewBlackboardEntry;
use crate::RootPromotion;
use crate::SourceFingerprint;
use crate::storage::DATABASE_NAME;
use crate::storage::HierarchyStoreError;
use crate::storage::load_node;
use crate::storage::unix_timestamp_millis;

mod query;
mod relation;
mod update;

const INITIAL_REVISION: i64 = 1;
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct BlackboardStore {
    pool: SqlitePool,
}

impl BlackboardStore {
    pub async fn open(sqlite: &SqliteConfig) -> Result<Self, BlackboardStoreError> {
        tokio::fs::create_dir_all(sqlite.home()).await?;
        let pool = sqlite
            .open_read_write_pool(&sqlite.home().join(DATABASE_NAME))
            .await?;
        if let Err(error) = MIGRATOR.run(&pool).await {
            pool.close().await;
            return Err(error.into());
        }
        Ok(Self { pool })
    }

    pub async fn create_entry(
        &self,
        id: BlackboardEntryId,
        value: NewBlackboardEntry,
    ) -> Result<BlackboardEntry, BlackboardStoreError> {
        value.validate()?;
        let now = unix_timestamp_millis()?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = load_entry_by_id(&mut transaction, &id).await? {
            if existing.value != value
                || existing.state != BlackboardEntryState::Active
                || existing.superseded_by.is_some()
            {
                return Err(BlackboardStoreError::EntryIdentityConflict(id.to_string()));
            }
            transaction.commit().await?;
            return Ok(existing);
        }
        load_node(&mut transaction, &value.project_id, &value.node_id)
            .await?
            .ok_or_else(|| BlackboardStoreError::NodeNotFound(value.node_id.to_string()))?;
        validate_evidence(&mut transaction, &value).await?;
        sqlx::query(
            "INSERT INTO blackboard_entries (
                id, project_id, node_id, revision, created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(&value.project_id)
        .bind(value.node_id.as_str())
        .bind(INITIAL_REVISION)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        write_revision(
            &mut transaction,
            &id,
            INITIAL_REVISION,
            &value,
            BlackboardEntryState::Active,
            None,
            now,
        )
        .await?;
        let entry = load_entry(&mut transaction, &value.project_id, &id)
            .await?
            .ok_or_else(|| BlackboardStoreError::EntryNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok(entry)
    }

    pub async fn get_entry(
        &self,
        project_id: &str,
        id: &BlackboardEntryId,
    ) -> Result<Option<BlackboardEntry>, BlackboardStoreError> {
        let mut connection = self.pool.acquire().await?;
        load_entry(&mut connection, project_id, id).await
    }
}

#[derive(FromRow)]
struct StoredBlackboardEntry {
    id: String,
    project_id: String,
    node_id: String,
    revision: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    kind: String,
    content: String,
    structured_value: Option<String>,
    structured_unit: Option<String>,
    confidence_basis_points: i64,
    verification: String,
    importance: String,
    root_promotion: String,
    state: String,
    superseded_by: Option<String>,
    provenance_kind: String,
    provenance_source_id: String,
}

#[derive(FromRow)]
struct StoredEvidenceLink {
    context_map_entry_id: String,
    source_fingerprint: String,
    first_line: Option<i64>,
    last_line: Option<i64>,
}

#[derive(FromRow)]
struct StoredEvidenceSource {
    project_id: String,
    source_fingerprint: String,
    lifecycle: String,
    current_fingerprint: Option<String>,
}

async fn load_entry(
    connection: &mut SqliteConnection,
    project_id: &str,
    id: &BlackboardEntryId,
) -> Result<Option<BlackboardEntry>, BlackboardStoreError> {
    let Some(stored) = sqlx::query_as::<_, StoredBlackboardEntry>(
        "SELECT entry.*, revision.kind, revision.content, revision.structured_value,
                revision.structured_unit, revision.confidence_basis_points,
                revision.verification, revision.importance, revision.root_promotion,
                revision.state, revision.superseded_by, revision.provenance_kind,
                revision.provenance_source_id
         FROM blackboard_entries AS entry
         JOIN blackboard_entry_revisions AS revision
           ON revision.entry_id = entry.id AND revision.revision = entry.revision
         WHERE entry.project_id = ? AND entry.id = ?",
    )
    .bind(project_id)
    .bind(id.as_str())
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };
    let evidence = sqlx::query_as::<_, StoredEvidenceLink>(
        "SELECT context_map_entry_id, source_fingerprint, first_line, last_line
         FROM blackboard_evidence_links
         WHERE entry_id = ? AND revision = ?
         ORDER BY position",
    )
    .bind(&stored.id)
    .bind(stored.revision)
    .fetch_all(connection)
    .await?
    .into_iter()
    .map(|link| {
        Ok(BlackboardEvidenceLink {
            context_map_entry_id: parse_stored(
                ContextMapEntryId::parse(link.context_map_entry_id),
                &stored.id,
            )?,
            source_fingerprint: parse_stored(
                SourceFingerprint::parse(link.source_fingerprint),
                &stored.id,
            )?,
            line_range: match (link.first_line, link.last_line) {
                (None, None) => None,
                (Some(first_line), Some(last_line)) => Some(crate::EvidenceLineRange {
                    start: u64::try_from(first_line)
                        .map_err(|_| BlackboardStoreError::CorruptEntry(stored.id.clone()))?,
                    end: u64::try_from(last_line)
                        .map_err(|_| BlackboardStoreError::CorruptEntry(stored.id.clone()))?,
                }),
                _ => return Err(BlackboardStoreError::CorruptEntry(stored.id.clone())),
            },
        })
    })
    .collect::<Result<Vec<_>, BlackboardStoreError>>()?;
    let revision = u64::try_from(stored.revision)
        .map_err(|_| BlackboardStoreError::CorruptEntry(stored.id.clone()))?;
    let confidence = u16::try_from(stored.confidence_basis_points)
        .ok()
        .and_then(|value| ConfidenceScore::from_basis_points(value).ok())
        .ok_or_else(|| BlackboardStoreError::CorruptEntry(stored.id.clone()))?;
    let value = NewBlackboardEntry {
        project_id: stored.project_id,
        node_id: parse_stored(crate::HierarchyNodeId::parse(stored.node_id), &stored.id)?,
        kind: parse_kind(&stored.kind)?,
        content: stored.content,
        structured_value: stored
            .structured_value
            .map(|value| BlackboardStructuredValue {
                value,
                unit: stored.structured_unit,
            }),
        confidence,
        verification: parse_verification(&stored.verification)?,
        importance: parse_importance(&stored.importance)?,
        root_promotion: parse_promotion(&stored.root_promotion)?,
        evidence,
        provenance: BlackboardProvenance {
            kind: parse_provenance(&stored.provenance_kind)?,
            source_id: stored.provenance_source_id,
        },
    };
    value
        .validate()
        .map_err(|_| BlackboardStoreError::CorruptEntry(stored.id.clone()))?;
    Ok(Some(BlackboardEntry {
        id: parse_stored(BlackboardEntryId::parse(&stored.id), &stored.id)?,
        value,
        state: parse_state(&stored.state)?,
        superseded_by: stored
            .superseded_by
            .map(|id| parse_stored(BlackboardEntryId::parse(id), &stored.id))
            .transpose()?,
        revision,
        created_at_ms: stored.created_at_ms,
        updated_at_ms: stored.updated_at_ms,
    }))
}

fn parse_stored<T, E>(result: Result<T, E>, entry_id: &str) -> Result<T, BlackboardStoreError> {
    result.map_err(|_| BlackboardStoreError::CorruptEntry(entry_id.to_string()))
}

async fn load_entry_by_id(
    connection: &mut SqliteConnection,
    id: &BlackboardEntryId,
) -> Result<Option<BlackboardEntry>, BlackboardStoreError> {
    let project_id: Option<String> =
        sqlx::query_scalar("SELECT project_id FROM blackboard_entries WHERE id = ?")
            .bind(id.as_str())
            .fetch_optional(&mut *connection)
            .await?;
    match project_id {
        Some(project_id) => load_entry(connection, &project_id, id).await,
        None => Ok(None),
    }
}

async fn validate_evidence(
    connection: &mut SqliteConnection,
    value: &NewBlackboardEntry,
) -> Result<(), BlackboardStoreError> {
    for link in &value.evidence {
        let source = sqlx::query_as::<_, StoredEvidenceSource>(
            "SELECT evidence.project_id, evidence.source_fingerprint,
                    source.lifecycle, source.source_fingerprint AS current_fingerprint
             FROM context_map_entries AS evidence
             JOIN hierarchy_nodes AS source ON source.id = evidence.node_id
             WHERE evidence.id = ?",
        )
        .bind(link.context_map_entry_id.as_str())
        .fetch_optional(&mut *connection)
        .await?
        .ok_or_else(|| {
            BlackboardStoreError::EvidenceNotFound(link.context_map_entry_id.to_string())
        })?;
        if source.project_id != value.project_id {
            return Err(BlackboardStoreError::EvidenceProjectMismatch);
        }
        if source.source_fingerprint != link.source_fingerprint.as_str() {
            return Err(BlackboardStoreError::EvidenceFingerprintMismatch);
        }
        if source.lifecycle != "active"
            || source.current_fingerprint.as_deref() != Some(&source.source_fingerprint)
        {
            return Err(BlackboardStoreError::EvidenceNotCurrent);
        }
    }
    Ok(())
}

async fn write_revision(
    connection: &mut SqliteConnection,
    id: &BlackboardEntryId,
    revision: i64,
    value: &NewBlackboardEntry,
    state: BlackboardEntryState,
    superseded_by: Option<&BlackboardEntryId>,
    now: i64,
) -> Result<(), BlackboardStoreError> {
    sqlx::query(
        "INSERT INTO blackboard_entry_revisions (
            entry_id, revision, kind, content, structured_value, structured_unit,
            confidence_basis_points, verification, importance, root_promotion,
            state, superseded_by, provenance_kind, provenance_source_id, recorded_at_ms
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.as_str())
    .bind(revision)
    .bind(kind_name(value.kind))
    .bind(&value.content)
    .bind(value.structured_value.as_ref().map(|value| &value.value))
    .bind(
        value
            .structured_value
            .as_ref()
            .and_then(|value| value.unit.as_ref()),
    )
    .bind(i64::from(value.confidence.basis_points()))
    .bind(verification_name(value.verification))
    .bind(importance_name(value.importance))
    .bind(promotion_name(value.root_promotion))
    .bind(state_name(state))
    .bind(superseded_by.map(BlackboardEntryId::as_str))
    .bind(provenance_name(value.provenance.kind))
    .bind(&value.provenance.source_id)
    .bind(now)
    .execute(&mut *connection)
    .await?;
    for (position, link) in value.evidence.iter().enumerate() {
        sqlx::query(
            "INSERT INTO blackboard_evidence_links (
                entry_id, revision, position, context_map_entry_id, source_fingerprint,
                first_line, last_line
             ) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(revision)
        .bind(i64::try_from(position).map_err(|_| BlackboardStoreError::PositionOverflow)?)
        .bind(link.context_map_entry_id.as_str())
        .bind(link.source_fingerprint.as_str())
        .bind(
            link.line_range
                .map(|range| i64::try_from(range.start))
                .transpose()
                .map_err(|_| BlackboardStoreError::PositionOverflow)?,
        )
        .bind(
            link.line_range
                .map(|range| i64::try_from(range.end))
                .transpose()
                .map_err(|_| BlackboardStoreError::PositionOverflow)?,
        )
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

macro_rules! enum_codec {
    ($name:ident, $parse:ident, $type:ty, {$($variant:path => $value:literal),+ $(,)?}) => {
        fn $name(value: $type) -> &'static str {
            match value {
                $($variant => $value,)+
            }
        }

        fn $parse(value: &str) -> Result<$type, BlackboardStoreError> {
            match value {
                $($value => Ok($variant),)+
                _ => Err(BlackboardStoreError::CorruptEnum(value.to_string())),
            }
        }
    };
}

enum_codec!(kind_name, parse_kind, BlackboardKind, {
    BlackboardKind::Instruction => "instruction", BlackboardKind::Fact => "fact",
    BlackboardKind::Claim => "claim", BlackboardKind::Number => "number",
    BlackboardKind::Decision => "decision", BlackboardKind::Strategy => "strategy",
    BlackboardKind::Question => "question", BlackboardKind::Contradiction => "contradiction",
    BlackboardKind::Failure => "failure", BlackboardKind::RejectedApproach => "rejected_approach",
    BlackboardKind::Signal => "signal", BlackboardKind::Note => "note",
});
enum_codec!(verification_name, parse_verification, BlackboardVerification, {
    BlackboardVerification::Unverified => "unverified",
    BlackboardVerification::SourceVerified => "source_verified",
    BlackboardVerification::UserConfirmed => "user_confirmed",
    BlackboardVerification::Disputed => "disputed", BlackboardVerification::Stale => "stale",
});
enum_codec!(importance_name, parse_importance, BlackboardImportance, {
    BlackboardImportance::Critical => "critical", BlackboardImportance::High => "high",
    BlackboardImportance::Normal => "normal", BlackboardImportance::Low => "low",
});
enum_codec!(promotion_name, parse_promotion, RootPromotion, {
    RootPromotion::NotPromoted => "not_promoted", RootPromotion::Candidate => "candidate",
    RootPromotion::Promoted => "promoted",
});
enum_codec!(state_name, parse_state, BlackboardEntryState, {
    BlackboardEntryState::Active => "active", BlackboardEntryState::Superseded => "superseded",
    BlackboardEntryState::Tombstoned => "tombstoned",
});
enum_codec!(provenance_name, parse_provenance, BlackboardProvenanceKind, {
    BlackboardProvenanceKind::User => "user", BlackboardProvenanceKind::Agent => "agent",
    BlackboardProvenanceKind::Maintenance => "maintenance",
    BlackboardProvenanceKind::Import => "import",
});

#[derive(Debug, Error)]
pub enum BlackboardStoreError {
    #[error(transparent)]
    InvalidEntry(#[from] BlackboardError),
    #[error(transparent)]
    Hierarchy(#[from] HierarchyStoreError),
    #[error(transparent)]
    Storage(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] MigrateError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("hierarchy node not found for blackboard entry: {0}")]
    NodeNotFound(String),
    #[error("blackboard entry not found: {0}")]
    EntryNotFound(String),
    #[error("blackboard entry ID was already used for different content: {0}")]
    EntryIdentityConflict(String),
    #[error("blackboard revision conflict: expected {expected}, found {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
    #[error("blackboard entry is no longer active: {0}")]
    EntryNotActive(String),
    #[error("blackboard successor entry was not found in this project: {0}")]
    SuccessorNotFound(String),
    #[error("blackboard successor entry is no longer active: {0}")]
    SuccessorNotActive(String),
    #[error("blackboard evidence context-map entry not found: {0}")]
    EvidenceNotFound(String),
    #[error("blackboard evidence belongs to a different project")]
    EvidenceProjectMismatch,
    #[error("blackboard evidence fingerprint does not match the context map")]
    EvidenceFingerprintMismatch,
    #[error("blackboard evidence source is not current")]
    EvidenceNotCurrent,
    #[error("blackboard evidence position overflow")]
    PositionOverflow,
    #[error("blackboard revision overflow")]
    RevisionOverflow,
    #[error("blackboard entry changed during a guarded update")]
    ConcurrentMutation,
    #[error("blackboard result count overflow")]
    CountOverflow,
    #[error("blackboard relation endpoint was not found in this project: {0}")]
    RelationEndpointNotFound(String),
    #[error("blackboard relation ID was already used for different content: {0}")]
    RelationIdentityConflict(String),
    #[error("stored blackboard entry is corrupt: {0}")]
    CorruptEntry(String),
    #[error("stored blackboard enum value is unknown: {0}")]
    CorruptEnum(String),
}

#[cfg(test)]
#[path = "blackboard_storage_tests.rs"]
mod tests;
