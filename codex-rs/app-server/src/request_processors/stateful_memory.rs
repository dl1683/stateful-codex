//! `statefulMemory/*`: the user reviews, forgets and corrects project memory with no model
//! turn. Forget and correct act with the user's authority (see `stateful_user_authority`).

use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::StatefulMemoryCorrectParams;
use codex_app_server_protocol::StatefulMemoryCorrectResponse;
use codex_app_server_protocol::StatefulMemoryForgetParams;
use codex_app_server_protocol::StatefulMemoryForgetResponse;
use codex_app_server_protocol::StatefulMemoryItem;
use codex_app_server_protocol::StatefulMemoryReadParams;
use codex_app_server_protocol::StatefulMemoryReadResponse;
use codex_app_server_protocol::StatefulMemoryReplaced;
use codex_app_server_protocol::StatefulMemorySection as ApiSection;
use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardStore;
use codex_protocol::ThreadId;
use codex_stateful_extension::BlackboardEntityKind;
use codex_stateful_extension::MemoryControlError;
use codex_stateful_extension::MemorySection;
use codex_stateful_extension::StatefulEvent;
use codex_thread_store::ReadThreadParams;

use super::BlackboardRequestProcessor;
use super::blackboard_error;
use super::project_error;
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
        let offset = match params.cursor.as_deref() {
            None => 0,
            Some(cursor) => cursor
                .parse::<u32>()
                .map_err(|_| invalid_params("cursor is not one this method returned"))?,
        };
        let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let store = self.store().await?;
        let (entries, more) = store
            .active_review_page(&project_id, offset, limit)
            .await
            .map_err(blackboard_error)?;
        let mut data = Vec::with_capacity(entries.len());
        for entry in entries {
            data.push(memory_item(store, entry).await?);
        }
        let next_cursor = more.then(|| (offset + limit).to_string());
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
        let entry = codex_stateful_extension::forget_entry(
            self.store().await?,
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
        let succession = codex_stateful_extension::correct_entry(
            store,
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
                item: memory_item(store, succession.successor).await?,
            }
            .into(),
        ))
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

async fn memory_item(
    store: &BlackboardStore,
    entry: BlackboardEntry,
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
        MemorySection::Knowledge => ApiSection::Knowledge,
    };
    let (content, content_truncated) = bounded(&entry.value.content, MAX_CONTENT_BYTES);
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
