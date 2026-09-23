use std::sync::Arc;

use codex_app_server_protocol::BlackboardEntryState as ApiEntryState;
use codex_app_server_protocol::BlackboardQueryParams;
use codex_app_server_protocol::BlackboardQueryResponse;
use codex_app_server_protocol::BlackboardRelateParams;
use codex_app_server_protocol::BlackboardRelateResponse;
use codex_app_server_protocol::BlackboardUpsertParams;
use codex_app_server_protocol::BlackboardUpsertResponse;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEntryUpdate;
use codex_project_intelligence::BlackboardQuery;
use codex_project_intelligence::BlackboardRelationId;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::BlackboardStructuredValue;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::HierarchyStore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::NewBlackboardRelation;
use codex_state::SqliteConfig;
use codex_thread_store::ThreadStore;
use codex_thread_store::ThreadStoreError;
use tokio::sync::OnceCell;

use codex_stateful_extension::BlackboardEntityKind;
use codex_stateful_extension::StatefulEvent;
use codex_stateful_extension::StatefulEventSink;

use super::blackboard_api::api_entry;
use super::blackboard_api::api_hit;
use super::blackboard_api::api_relation;
use super::blackboard_api::internal_evidence;
use super::blackboard_api::internal_importance;
use super::blackboard_api::internal_kind;
use super::blackboard_api::internal_provenance;
use super::blackboard_api::internal_relation_kind;
use super::blackboard_api::internal_root_promotion;
use super::blackboard_api::internal_state;
use super::blackboard_api::internal_verification;

use crate::error_code::internal_error;
use crate::error_code::invalid_params;
use crate::error_code::method_not_found;

const DEFAULT_QUERY_LIMIT: u32 = 20;

#[derive(Clone)]
pub(crate) struct BlackboardRequestProcessor {
    thread_store: Arc<dyn ThreadStore>,
    sqlite: Option<SqliteConfig>,
    store: Arc<OnceCell<BlackboardStore>>,
    hierarchy: Arc<OnceCell<HierarchyStore>>,
    event_sink: Arc<dyn StatefulEventSink>,
}

impl BlackboardRequestProcessor {
    pub(crate) fn new(
        thread_store: Arc<dyn ThreadStore>,
        sqlite: Option<SqliteConfig>,
        event_sink: Arc<dyn StatefulEventSink>,
    ) -> Self {
        Self {
            thread_store,
            sqlite,
            store: Arc::new(OnceCell::new()),
            hierarchy: Arc::new(OnceCell::new()),
            event_sink,
        }
    }

    pub(crate) async fn upsert(
        &self,
        params: BlackboardUpsertParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.require_project(&params.project_id).await?;
        let entry_id = BlackboardEntryId::parse(params.entry_id)
            .map_err(|error| invalid_params(error.to_string()))?;
        let confidence = ConfidenceScore::from_basis_points(params.confidence_basis_points)
            .map_err(|error| invalid_params(error.to_string()))?;
        let structured_value = params
            .structured_value
            .map(|value| BlackboardStructuredValue {
                value: value.value,
                unit: value.unit,
            });
        let evidence = internal_evidence(params.evidence)?;
        let provenance = internal_provenance(params.provenance);
        let store = self.store().await?;
        let entry = match params.expected_revision {
            None => {
                if !matches!(params.state, None | Some(ApiEntryState::Active))
                    || params.superseded_by.is_some()
                {
                    return Err(invalid_params(
                        "new blackboard entries must begin active and cannot name a successor",
                    ));
                }
                let node_id = match params.node_id {
                    Some(node_id) => HierarchyNodeId::parse(node_id)
                        .map_err(|error| invalid_params(error.to_string()))?,
                    None => {
                        self.hierarchy()
                            .await?
                            .project_node(&params.project_id)
                            .await
                            .map_err(|error| internal_error(error.to_string()))?
                            .ok_or_else(|| {
                                invalid_params(
                                    "project hierarchy is empty; refresh the context map",
                                )
                            })?
                            .id
                    }
                };
                store
                    .create_entry(
                        entry_id,
                        NewBlackboardEntry {
                            project_id: params.project_id,
                            node_id,
                            kind: internal_kind(params.kind),
                            content: params.content,
                            structured_value,
                            confidence,
                            verification: internal_verification(params.verification),
                            importance: internal_importance(params.importance),
                            root_promotion: internal_root_promotion(params.root_promotion),
                            evidence,
                            provenance,
                        },
                    )
                    .await
            }
            Some(expected_revision) => {
                if params.node_id.is_some() {
                    return Err(invalid_params(
                        "an existing entry cannot move to another node",
                    ));
                }
                store
                    .update_entry(
                        &params.project_id,
                        &entry_id,
                        BlackboardEntryUpdate {
                            expected_revision,
                            kind: internal_kind(params.kind),
                            content: params.content,
                            structured_value,
                            confidence,
                            verification: internal_verification(params.verification),
                            importance: internal_importance(params.importance),
                            root_promotion: internal_root_promotion(params.root_promotion),
                            evidence,
                            state: params
                                .state
                                .map_or(BlackboardEntryState::Active, internal_state),
                            superseded_by: params
                                .superseded_by
                                .map(BlackboardEntryId::parse)
                                .transpose()
                                .map_err(|error| invalid_params(error.to_string()))?,
                            provenance,
                        },
                    )
                    .await
            }
        }
        .map_err(blackboard_error)?;
        self.event_sink.emit(StatefulEvent::BlackboardUpdated {
            project_id: entry.value.project_id.clone(),
            entity_kind: BlackboardEntityKind::Entry,
            entity_id: entry.id.to_string(),
            revision: entry.revision,
        });
        Ok(Some(
            BlackboardUpsertResponse {
                entry: api_entry(entry),
            }
            .into(),
        ))
    }

    pub(crate) async fn relate(
        &self,
        params: BlackboardRelateParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.require_project(&params.project_id).await?;
        let relation = self
            .store()
            .await?
            .create_relation(
                BlackboardRelationId::parse(params.relation_id)
                    .map_err(|error| invalid_params(error.to_string()))?,
                NewBlackboardRelation {
                    project_id: params.project_id,
                    from_entry_id: BlackboardEntryId::parse(params.from_entry_id)
                        .map_err(|error| invalid_params(error.to_string()))?,
                    to_entry_id: BlackboardEntryId::parse(params.to_entry_id)
                        .map_err(|error| invalid_params(error.to_string()))?,
                    kind: internal_relation_kind(params.kind),
                    note: params.note,
                    confidence: ConfidenceScore::from_basis_points(params.confidence_basis_points)
                        .map_err(|error| invalid_params(error.to_string()))?,
                    provenance: internal_provenance(params.provenance),
                },
            )
            .await
            .map_err(blackboard_error)?;
        self.event_sink.emit(StatefulEvent::BlackboardUpdated {
            project_id: relation.value.project_id.clone(),
            entity_kind: BlackboardEntityKind::Relation,
            entity_id: relation.id.to_string(),
            revision: relation.revision,
        });
        Ok(Some(
            BlackboardRelateResponse {
                relation: api_relation(relation),
            }
            .into(),
        ))
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
                root_promotion: None,
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

    async fn hierarchy(&self) -> Result<&HierarchyStore, JSONRPCErrorError> {
        let sqlite = self.sqlite.as_ref().ok_or_else(|| {
            method_not_found("blackboard/upsert is unavailable without sqlite state")
        })?;
        self.hierarchy
            .get_or_try_init(|| HierarchyStore::open(sqlite))
            .await
            .map_err(|error| internal_error(format!("failed to open project hierarchy: {error}")))
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
        BlackboardStoreError::Storage(_)
        | BlackboardStoreError::Migration(_)
        | BlackboardStoreError::Io(_)
        | BlackboardStoreError::CorruptEntry(_)
        | BlackboardStoreError::CorruptEnum(_) => {
            internal_error(format!("failed to access blackboard: {error}"))
        }
        error => invalid_params(error.to_string()),
    }
}
