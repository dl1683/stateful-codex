//! `statefulMemory/*`: the user reviews, forgets and corrects project memory with no model
//! turn. Forget and correct act with the user's authority (see `stateful_user_authority`).

use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::StatefulMemoryAddKind;
use codex_app_server_protocol::StatefulMemoryAddOutcome;
use codex_app_server_protocol::StatefulMemoryAddParams;
use codex_app_server_protocol::StatefulMemoryAddResponse;
use codex_app_server_protocol::StatefulMemoryCorrectParams;
use codex_app_server_protocol::StatefulMemoryCorrectResponse;
use codex_app_server_protocol::StatefulMemoryForgetParams;
use codex_app_server_protocol::StatefulMemoryForgetResponse;
use codex_app_server_protocol::StatefulMemoryItem;
use codex_app_server_protocol::StatefulMemoryReadParams;
use codex_app_server_protocol::StatefulMemoryReadResponse;
use codex_app_server_protocol::StatefulMemoryReplaced;
use codex_app_server_protocol::StatefulMemoryScope;
use codex_app_server_protocol::StatefulMemoryScopeAction;
use codex_app_server_protocol::StatefulMemoryScopeParams;
use codex_app_server_protocol::StatefulMemoryScopeResponse;
use codex_app_server_protocol::StatefulMemorySection as ApiSection;
use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::ProjectIndexer;
use codex_protocol::ThreadId;
use codex_stateful_extension::AddOutcome;
use codex_stateful_extension::BlackboardEntityKind;
use codex_stateful_extension::MemoryAddition;
use codex_stateful_extension::MemoryControlError;
use codex_stateful_extension::MemorySection;
use codex_stateful_extension::StatefulEvent;
use codex_thread_store::ReadThreadParams;

use super::BlackboardRequestProcessor;
use super::blackboard_error;
use super::project_error;
use crate::error_code::internal_error;
use crate::error_code::invalid_params;
use crate::request_processors::blackboard_api::api_kind;
use crate::request_processors::blackboard_api::api_provenance_kind;

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;
const MAX_CONTENT_BYTES: usize = 2_000;
const MAX_REPLACED_BYTES: usize = 240;
const MAX_REPLACED: usize = 3;

impl BlackboardRequestProcessor {
    pub(crate) async fn memory_read(
        &self,
        params: StatefulMemoryReadParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let project_id = self.thread_project(&params.thread_id).await?;
        let store = self.store().await?;
        // A cursor names the project and the memory revision its page came from; after any
        // change the order may have shifted, so the reader starts again instead of skipping
        // entries. The check and the page share one snapshot.
        let (expected_revision, offset) = match params.cursor.as_deref() {
            None => (None, 0),
            Some(cursor) => {
                let mut parts = cursor.rsplitn(3, ':');
                let (Some(offset), Some(revision), Some(cursor_project)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    return Err(invalid_params("cursor is not one this method returned"));
                };
                let (Ok(offset), Ok(revision)) = (offset.parse::<u32>(), revision.parse::<u64>())
                else {
                    return Err(invalid_params("cursor is not one this method returned"));
                };
                if cursor_project != project_id {
                    return Err(invalid_params(
                        "this cursor belongs to another project's memory; read again from the start",
                    ));
                }
                (Some(revision), offset)
            }
        };
        let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let page = store
            .active_review_page(&project_id, offset, limit, expected_revision)
            .await
            .map_err(blackboard_error)?
            .ok_or_else(|| {
                invalid_params(
                    "project memory changed since this page was read; read it again from the start",
                )
            })?;
        let mut data = Vec::with_capacity(page.entries.len());
        for entry in page.entries {
            data.push(memory_item(store, entry, Sections::of(params.background_section)).await?);
        }
        let revision = page.revision;
        let next_cursor = page
            .more
            .then(|| format!("{project_id}:{revision}:{}", offset + limit));
        Ok(Some(
            StatefulMemoryReadResponse {
                project_id,
                data,
                next_cursor,
            }
            .into(),
        ))
    }

    pub(crate) async fn memory_forget(
        &self,
        params: StatefulMemoryForgetParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let project_id = self.thread_project(&params.thread_id).await?;
        let id = entry_id(params.entry_id)?;
        let actor = codex_stateful_extension::MemoryActor {
            thread_id: Some(params.thread_id.clone()),
            action_id: None,
        };
        let entry = codex_stateful_extension::forget_entry(
            self.store().await?,
            &actor,
            &project_id,
            &id,
            params.expected_revision,
        )
        .await
        .map_err(control_error)?;
        self.announce(&entry);
        Ok(Some(
            StatefulMemoryForgetResponse {
                entry_id: entry.id.to_string(),
                revision: entry.revision,
            }
            .into(),
        ))
    }

    pub(crate) async fn memory_correct(
        &self,
        params: StatefulMemoryCorrectParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let project_id = self.thread_project(&params.thread_id).await?;
        let id = entry_id(params.entry_id)?;
        let store = self.store().await?;
        let actor = codex_stateful_extension::MemoryActor {
            thread_id: Some(params.thread_id.clone()),
            action_id: None,
        };
        let succession = codex_stateful_extension::correct_entry(
            store,
            &actor,
            &project_id,
            &id,
            params.expected_revision,
            &params.content,
        )
        .await
        .map_err(control_error)?;
        for replaced in &succession.superseded {
            self.announce(replaced);
        }
        self.announce(&succession.successor);
        Ok(Some(
            StatefulMemoryCorrectResponse {
                item: memory_item(
                    store,
                    succession.successor,
                    Sections::of(params.background_section),
                )
                .await?,
            }
            .into(),
        ))
    }

    pub(crate) async fn memory_add(
        &self,
        params: StatefulMemoryAddParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let project_id = self.thread_project(&params.thread_id).await?;
        if params.client_action_id.trim().is_empty() || params.client_action_id.len() > 128 {
            return Err(invalid_params("clientActionId must be 1-128 bytes"));
        }
        let node_id = self.memory_node(&project_id).await?;
        let store = self.store().await?;
        let addition = match params.kind {
            StatefulMemoryAddKind::Rule => MemoryAddition::Rule {
                scope: params.scope,
            },
            StatefulMemoryAddKind::Background => MemoryAddition::Background,
            StatefulMemoryAddKind::Decision => MemoryAddition::Decision {
                reason: params.reason,
            },
            StatefulMemoryAddKind::Note => MemoryAddition::Note,
        };
        let actor = codex_stateful_extension::MemoryActor {
            thread_id: Some(params.thread_id.clone()),
            action_id: Some(params.client_action_id.clone()),
        };
        let (entry, outcome) = codex_stateful_extension::add_entry(
            store,
            &actor,
            &project_id,
            node_id,
            addition,
            &params.content,
        )
        .await
        .map_err(control_error)?;
        let outcome = match outcome {
            AddOutcome::Added => {
                self.announce(&entry);
                StatefulMemoryAddOutcome::Added
            }
            AddOutcome::AlreadyPresent => StatefulMemoryAddOutcome::AlreadyPresent,
            AddOutcome::AlreadyDone => StatefulMemoryAddOutcome::AlreadyDone,
        };
        Ok(Some(
            StatefulMemoryAddResponse {
                item: memory_item(store, entry, Sections::of(params.background_section)).await?,
                outcome,
            }
            .into(),
        ))
    }

    pub(crate) async fn memory_scope(
        &self,
        params: StatefulMemoryScopeParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let project_id = self.thread_project(&params.thread_id).await?;
        let store = self.store().await?;
        let named = || {
            params
                .scope_id
                .clone()
                .ok_or_else(|| invalid_params("scopeId names the investigation for join and end"))
        };
        match params.action {
            StatefulMemoryScopeAction::List => {}
            StatefulMemoryScopeAction::Join => {
                let scope_id = named()?;
                let scope = store
                    .scope(&project_id, &scope_id)
                    .await
                    .map_err(blackboard_error)?
                    .ok_or_else(|| invalid_params("no such investigation in this project"))?;
                if scope.state != codex_project_intelligence::ScopeState::Open {
                    return Err(invalid_params("that investigation has ended"));
                }
                store
                    .bind_thread_scope(&project_id, &params.thread_id, &scope_id)
                    .await
                    .map_err(blackboard_error)?;
            }
            StatefulMemoryScopeAction::Leave => {
                store
                    .unbind_thread_scope(&project_id, &params.thread_id)
                    .await
                    .map_err(blackboard_error)?;
            }
            StatefulMemoryScopeAction::End => {
                let scope_id = named()?;
                let scope = store
                    .scope(&project_id, &scope_id)
                    .await
                    .map_err(blackboard_error)?
                    .ok_or_else(|| invalid_params("no such investigation in this project"))?;
                let actor = codex_stateful_extension::MemoryActor {
                    thread_id: Some(params.thread_id.clone()),
                    action_id: None,
                };
                store
                    .end_scope(
                        &project_id,
                        &scope_id,
                        &format!("direct-control:{}", params.thread_id),
                        &codex_project_intelligence::ChangeRecord {
                            operation: codex_project_intelligence::ChangeOperation::ScopeEnded,
                            origin: codex_project_intelligence::ChangeOrigin::DirectControl,
                            category: codex_project_intelligence::KnowledgeCategory::Rule,
                            action_id: actor.action_id,
                            thread_id: actor.thread_id,
                            turn_id: None,
                            group_id: None,
                            preview: scope.title,
                        },
                    )
                    .await
                    .map_err(blackboard_error)?;
            }
        }
        let bound = store
            .thread_scope(&project_id, &params.thread_id)
            .await
            .map_err(blackboard_error)?
            .map(|scope| scope.scope_id);
        let scopes = store
            .scopes(&project_id, /*state*/ None)
            .await
            .map_err(blackboard_error)?
            .into_iter()
            .map(|scope| StatefulMemoryScope {
                this_thread: bound.as_deref() == Some(scope.scope_id.as_str()),
                open: scope.state == codex_project_intelligence::ScopeState::Open,
                scope_id: scope.scope_id,
                title: scope.title,
                end_condition: scope.end_condition,
            })
            .collect();
        Ok(Some(StatefulMemoryScopeResponse { scopes }.into()))
    }

    /// The project's root node, created when the project has none yet.
    async fn memory_node(&self, project_id: &str) -> Result<HierarchyNodeId, JSONRPCErrorError> {
        let hierarchy = self.hierarchy().await?;
        if let Some(node) = hierarchy
            .project_node(project_id)
            .await
            .map_err(|error| internal_error(error.to_string()))?
        {
            return Ok(node.id);
        }
        let context_map = self.context_map().await?;
        ProjectIndexer::new(hierarchy.clone(), context_map.clone())
            .ensure_project_node(project_id)
            .await
            .map(|node| node.id)
            .map_err(|error| internal_error(error.to_string()))
    }

    fn announce(&self, entry: &BlackboardEntry) {
        self.event_sink.emit(StatefulEvent::BlackboardUpdated {
            project_id: entry.value.project_id.clone(),
            entity_kind: BlackboardEntityKind::Entry,
            entity_id: entry.id.to_string(),
            revision: entry.revision,
        });
    }

    /// The project of a thread, including one whose project is still pending persistence.
    async fn thread_project(&self, raw_thread_id: &str) -> Result<String, JSONRPCErrorError> {
        let thread_id = ThreadId::from_string(raw_thread_id)
            .map_err(|_| invalid_params("threadId must be a valid thread ID"))?;
        if let Some(project_id) = self
            .thread_store
            .read_pending_thread_metadata(thread_id)
            .await
            .map_err(project_error)?
            .and_then(|metadata| metadata.project_id.flatten())
        {
            return Ok(project_id);
        }
        self.thread_store
            .read_thread(ReadThreadParams {
                thread_id,
                include_archived: true,
                include_history: false,
            })
            .await
            .map_err(project_error)?
            .project_id
            .ok_or_else(|| {
                invalid_params("this thread has no project, so it has no project memory")
            })
    }
}

/// The sections a client understands.
#[derive(Clone, Copy)]
enum Sections {
    /// Clients from before the background section: background is reported as knowledge.
    Legacy,
    WithBackground,
}

impl Sections {
    fn of(background_section: bool) -> Self {
        if background_section {
            Self::WithBackground
        } else {
            Self::Legacy
        }
    }
}

async fn memory_item(
    store: &BlackboardStore,
    entry: BlackboardEntry,
    sections: Sections,
) -> Result<StatefulMemoryItem, JSONRPCErrorError> {
    let replaced = store
        .superseded_by(&entry.value.project_id, &entry.id)
        .await
        .map_err(blackboard_error)?;
    let replaces = replaced
        .into_iter()
        .take(MAX_REPLACED)
        .map(|older| StatefulMemoryReplaced {
            entry_id: older.id.to_string(),
            content: bounded(&older.value.content, MAX_REPLACED_BYTES).0,
            replaced_at: older.updated_at_ms.div_euclid(1_000),
        })
        .collect();
    let section = match codex_stateful_extension::memory_section(&entry) {
        MemorySection::UserRule => ApiSection::UserRule,
        MemorySection::PendingRule => ApiSection::PendingRule,
        MemorySection::UnverifiedRule => ApiSection::UnverifiedRule,
        MemorySection::Decision => ApiSection::Decision,
        MemorySection::Background => match sections {
            Sections::WithBackground => ApiSection::Background,
            Sections::Legacy => ApiSection::Knowledge,
        },
        MemorySection::Knowledge => ApiSection::Knowledge,
    };
    let (content, content_truncated) = bounded(&entry.value.content, MAX_CONTENT_BYTES);
    let context = store
        .knowledge_context(&entry.value.project_id, &entry.id)
        .await
        .map_err(blackboard_error)?;
    let scope_title = match context
        .as_ref()
        .and_then(|context| context.scope_id.as_deref())
    {
        Some(scope_id) => store
            .scope(&entry.value.project_id, scope_id)
            .await
            .map_err(blackboard_error)?
            .map(|scope| scope.title),
        None => None,
    };
    let attributed_to = context
        .as_ref()
        .and_then(|context| context.payload.as_deref())
        .and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok())
        .and_then(|payload| payload.get("speaker")?.as_str().map(str::to_string));
    let authority = context.map(|context| {
        use codex_app_server_protocol::StatefulMemoryAuthority as Api;
        use codex_project_intelligence::KnowledgeAuthority;
        match context.authority {
            KnowledgeAuthority::HumanDirect => Api::HumanDirect,
            KnowledgeAuthority::AssistantReported => Api::AssistantReported,
            KnowledgeAuthority::ReportedThirdParty => Api::ReportedThirdParty,
            KnowledgeAuthority::HostObserved => Api::HostObserved,
            KnowledgeAuthority::LegacyUnknown => Api::LegacyUnknown,
        }
    });
    Ok(StatefulMemoryItem {
        entry_id: entry.id.to_string(),
        revision: entry.revision,
        section,
        kind: api_kind(entry.value.kind),
        content,
        content_truncated,
        source: api_provenance_kind(entry.value.provenance.kind),
        updated_at: entry.updated_at_ms.div_euclid(1_000),
        replaces,
        authority,
        scope_title,
        attributed_to,
    })
}

/// `text` cut to at most `limit` bytes on a character boundary, and whether it was cut.
fn bounded(text: &str, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text.to_string(), false);
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

fn entry_id(raw: String) -> Result<BlackboardEntryId, JSONRPCErrorError> {
    BlackboardEntryId::parse(raw).map_err(|error| invalid_params(error.to_string()))
}

fn control_error(error: MemoryControlError) -> JSONRPCErrorError {
    match error {
        MemoryControlError::Refused(message) => {
            invalid_params(format!("{message}; nothing was changed"))
        }
        MemoryControlError::Store(error) => blackboard_error(error),
    }
}
