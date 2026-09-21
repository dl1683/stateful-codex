use std::sync::Arc;

use codex_app_server_protocol::BlackboardEntry as ApiEntry;
use codex_app_server_protocol::BlackboardEntryState as ApiEntryState;
use codex_app_server_protocol::BlackboardEvidenceFreshness as ApiEvidenceFreshness;
use codex_app_server_protocol::BlackboardEvidenceLink as ApiEvidenceLink;
use codex_app_server_protocol::BlackboardImportance as ApiImportance;
use codex_app_server_protocol::BlackboardKind as ApiKind;
use codex_app_server_protocol::BlackboardProvenance as ApiProvenance;
use codex_app_server_protocol::BlackboardProvenanceKind as ApiProvenanceKind;
use codex_app_server_protocol::BlackboardQueryHit as ApiHit;
use codex_app_server_protocol::BlackboardQueryParams;
use codex_app_server_protocol::BlackboardQueryResponse;
use codex_app_server_protocol::BlackboardRelation as ApiRelation;
use codex_app_server_protocol::BlackboardRelationKind as ApiRelationKind;
use codex_app_server_protocol::BlackboardRootPromotion as ApiRootPromotion;
use codex_app_server_protocol::BlackboardStructuredValue as ApiStructuredValue;
use codex_app_server_protocol::BlackboardVerification as ApiVerification;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEvidenceFreshness;
use codex_project_intelligence::BlackboardHit;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardQuery;
use codex_project_intelligence::BlackboardRelation;
use codex_project_intelligence::BlackboardRelationKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_thread_store::ThreadStore;
use codex_thread_store::ThreadStoreError;
use tokio::sync::OnceCell;

use crate::error_code::internal_error;
use crate::error_code::invalid_params;
use crate::error_code::method_not_found;

const DEFAULT_QUERY_LIMIT: u32 = 20;

#[derive(Clone)]
pub(crate) struct BlackboardRequestProcessor {
    thread_store: Arc<dyn ThreadStore>,
    sqlite: Option<SqliteConfig>,
    store: Arc<OnceCell<BlackboardStore>>,
}

impl BlackboardRequestProcessor {
    pub(crate) fn new(thread_store: Arc<dyn ThreadStore>, sqlite: Option<SqliteConfig>) -> Self {
        Self {
            thread_store,
            sqlite,
            store: Arc::new(OnceCell::new()),
        }
    }

    pub(crate) async fn query(
        &self,
        params: BlackboardQueryParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.require_project(&params.project_id).await?;
        let result = self
            .store()
            .await?
            .query(BlackboardQuery {
                project_id: params.project_id,
                text: params.text,
                within_node: params
                    .within_node_id
                    .map(HierarchyNodeId::parse)
                    .transpose()
                    .map_err(|error| invalid_params(error.to_string()))?,
                max_results: params.limit.unwrap_or(DEFAULT_QUERY_LIMIT),
            })
            .await
            .map_err(blackboard_error)?;
        Ok(Some(
            BlackboardQueryResponse {
                data: result.data.into_iter().map(api_hit).collect(),
                truncated: result.truncated,
            }
            .into(),
        ))
    }

    async fn require_project(&self, project_id: &str) -> Result<(), JSONRPCErrorError> {
        self.thread_store
            .read_project(project_id.to_string())
            .await
            .map_err(project_error)?
            .ok_or_else(|| invalid_params(format!("project not found: {project_id}")))?;
        Ok(())
    }

    async fn store(&self) -> Result<&BlackboardStore, JSONRPCErrorError> {
        let sqlite = self.sqlite.as_ref().ok_or_else(|| {
            method_not_found("blackboard/query is unavailable without sqlite state")
        })?;
        self.store
            .get_or_try_init(|| BlackboardStore::open(sqlite))
            .await
            .map_err(blackboard_error)
    }
}

fn api_hit(hit: BlackboardHit) -> ApiHit {
    ApiHit {
        entry: api_entry(hit.entry),
        relations: hit.relations.into_iter().map(api_relation).collect(),
        evidence_freshness: api_evidence_freshness(hit.evidence_freshness),
        effective_verification: api_verification(hit.effective_verification),
    }
}

fn api_entry(entry: BlackboardEntry) -> ApiEntry {
    ApiEntry {
        id: entry.id.to_string(),
        project_id: entry.value.project_id,
        node_id: entry.value.node_id.to_string(),
        kind: api_kind(entry.value.kind),
        content: entry.value.content,
        structured_value: entry
            .value
            .structured_value
            .map(|value| ApiStructuredValue {
                value: value.value,
                unit: value.unit,
            }),
        confidence_basis_points: entry.value.confidence.basis_points(),
        verification: api_verification(entry.value.verification),
        importance: api_importance(entry.value.importance),
        root_promotion: api_root_promotion(entry.value.root_promotion),
        evidence: entry
            .value
            .evidence
            .into_iter()
            .map(|link| ApiEvidenceLink {
                context_map_entry_id: link.context_map_entry_id.to_string(),
                source_fingerprint: link.source_fingerprint.to_string(),
            })
            .collect(),
        provenance: api_provenance(entry.value.provenance),
        state: api_entry_state(entry.state),
        superseded_by: entry.superseded_by.map(|id| id.to_string()),
        revision: entry.revision,
        created_at: entry.created_at_ms.div_euclid(/*rhs*/ 1000),
        updated_at: entry.updated_at_ms.div_euclid(/*rhs*/ 1000),
    }
}

fn api_relation(relation: BlackboardRelation) -> ApiRelation {
    ApiRelation {
        id: relation.id.to_string(),
        project_id: relation.value.project_id,
        from_entry_id: relation.value.from_entry_id.to_string(),
        to_entry_id: relation.value.to_entry_id.to_string(),
        kind: match relation.value.kind {
            BlackboardRelationKind::Supports => ApiRelationKind::Supports,
            BlackboardRelationKind::Contradicts => ApiRelationKind::Contradicts,
            BlackboardRelationKind::DependsOn => ApiRelationKind::DependsOn,
            BlackboardRelationKind::RelatedTo => ApiRelationKind::RelatedTo,
        },
        note: relation.value.note,
        confidence_basis_points: relation.value.confidence.basis_points(),
        provenance: api_provenance(relation.value.provenance),
        revision: relation.revision,
        created_at: relation.created_at_ms.div_euclid(/*rhs*/ 1000),
        updated_at: relation.updated_at_ms.div_euclid(/*rhs*/ 1000),
    }
}

fn api_kind(value: BlackboardKind) -> ApiKind {
    match value {
        BlackboardKind::Instruction => ApiKind::Instruction,
        BlackboardKind::Fact => ApiKind::Fact,
        BlackboardKind::Claim => ApiKind::Claim,
        BlackboardKind::Number => ApiKind::Number,
        BlackboardKind::Decision => ApiKind::Decision,
        BlackboardKind::Strategy => ApiKind::Strategy,
        BlackboardKind::Question => ApiKind::Question,
        BlackboardKind::Contradiction => ApiKind::Contradiction,
        BlackboardKind::Failure => ApiKind::Failure,
        BlackboardKind::RejectedApproach => ApiKind::RejectedApproach,
        BlackboardKind::Signal => ApiKind::Signal,
        BlackboardKind::Note => ApiKind::Note,
    }
}

fn api_verification(value: BlackboardVerification) -> ApiVerification {
    match value {
        BlackboardVerification::Unverified => ApiVerification::Unverified,
        BlackboardVerification::SourceVerified => ApiVerification::SourceVerified,
        BlackboardVerification::UserConfirmed => ApiVerification::UserConfirmed,
        BlackboardVerification::Disputed => ApiVerification::Disputed,
        BlackboardVerification::Stale => ApiVerification::Stale,
    }
}

fn api_importance(value: BlackboardImportance) -> ApiImportance {
    match value {
        BlackboardImportance::Critical => ApiImportance::Critical,
        BlackboardImportance::High => ApiImportance::High,
        BlackboardImportance::Normal => ApiImportance::Normal,
        BlackboardImportance::Low => ApiImportance::Low,
    }
}

fn api_root_promotion(value: RootPromotion) -> ApiRootPromotion {
    match value {
        RootPromotion::NotPromoted => ApiRootPromotion::NotPromoted,
        RootPromotion::Candidate => ApiRootPromotion::Candidate,
        RootPromotion::Promoted => ApiRootPromotion::Promoted,
    }
}

fn api_entry_state(value: BlackboardEntryState) -> ApiEntryState {
    match value {
        BlackboardEntryState::Active => ApiEntryState::Active,
        BlackboardEntryState::Superseded => ApiEntryState::Superseded,
        BlackboardEntryState::Tombstoned => ApiEntryState::Tombstoned,
    }
}

fn api_provenance(value: BlackboardProvenance) -> ApiProvenance {
    ApiProvenance {
        kind: match value.kind {
            BlackboardProvenanceKind::User => ApiProvenanceKind::User,
            BlackboardProvenanceKind::Agent => ApiProvenanceKind::Agent,
            BlackboardProvenanceKind::Maintenance => ApiProvenanceKind::Maintenance,
            BlackboardProvenanceKind::Import => ApiProvenanceKind::Import,
        },
        source_id: value.source_id,
    }
}

fn api_evidence_freshness(value: BlackboardEvidenceFreshness) -> ApiEvidenceFreshness {
    match value {
        BlackboardEvidenceFreshness::NotApplicable => ApiEvidenceFreshness::NotApplicable,
        BlackboardEvidenceFreshness::Current => ApiEvidenceFreshness::Current,
        BlackboardEvidenceFreshness::Stale => ApiEvidenceFreshness::Stale,
        BlackboardEvidenceFreshness::SourceUnavailable => ApiEvidenceFreshness::SourceUnavailable,
    }
}

fn project_error(error: ThreadStoreError) -> JSONRPCErrorError {
    match error {
        ThreadStoreError::Unsupported { .. } => {
            method_not_found("blackboard/query is unavailable without sqlite state")
        }
        ThreadStoreError::InvalidRequest { message } => invalid_params(message),
        error => internal_error(format!("failed to read blackboard project: {error}")),
    }
}

fn blackboard_error(error: BlackboardStoreError) -> JSONRPCErrorError {
    match error {
        BlackboardStoreError::InvalidEntry(error) => invalid_params(error.to_string()),
        BlackboardStoreError::NodeNotFound(_) => invalid_params(error.to_string()),
        error => internal_error(format!("failed to query blackboard: {error}")),
    }
}
