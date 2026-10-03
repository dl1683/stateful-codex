//! A host-owned journal of each thread's work and its publication outbox.
//!
//! The host appends one committed row per completed observation (an edit, a command, a plan,
//! a message), so a crash never loses a completed observation and no model request is needed
//! to remember it. A publication freezes one disjoint suffix of a thread's journal, with the
//! exact content and entry identity it will write to project memory, before writing it; a
//! retry after a crash therefore writes the same entry.

use serde_json::Value;
use sqlx::FromRow;

use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::storage::unix_timestamp_millis;
use crate::storage::validate_list_limit;

const MAX_IDENTITY_BYTES: usize = 256;
/// Bound of one serialized observation; callers keep payloads well below it.
pub const MAX_WINDOW_EVENT_PAYLOAD_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowEventKind {
    User,
    Command,
    Edit,
    Plan,
    Message,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewWindowEvent {
    pub thread_id: String,
    /// Host identity of the observation (an item or call ID plus its kind); a retry with the
    /// same key and payload is the same event.
    pub event_key: String,
    pub project_id: String,
    pub turn_id: String,
    pub kind: WindowEventKind,
    pub payload: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowEvent {
    pub seq: u64,
    pub event: NewWindowEvent,
    pub created_at_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowPublicationState {
    Pending,
    Published,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowPublication {
    pub thread_id: String,
    pub from_seq: u64,
    pub through_seq: u64,
    pub project_id: String,
    pub entry_id: String,
    pub content: String,
    pub state: WindowPublicationState,
}

#[derive(FromRow)]
struct StoredEvent {
    thread_id: String,
    seq: i64,
    event_key: String,
    project_id: String,
    turn_id: String,
    kind: String,
    payload_json: String,
    created_at_ms: i64,
}

#[derive(FromRow)]
struct StoredPublication {
    thread_id: String,
    from_seq: i64,
    through_seq: i64,
    project_id: String,
    entry_id: String,
    content: String,
    state: String,
}

impl StatefulRunStore {
    /// Appends an observation and returns its sequence number. Replaying the same key with the
    /// same payload returns the original sequence; the same key with another payload is refused
    /// rather than silently kept or overwritten.
    pub async fn append_window_event(
        &self,
        event: &NewWindowEvent,
    ) -> Result<u64, StatefulRunStoreError> {
        for identity in [
            &event.thread_id,
            &event.event_key,
            &event.project_id,
            &event.turn_id,
        ] {
            validate_identity(identity)?;
        }
        let payload = serde_json::to_string(&event.payload)?;
        if payload.len() > MAX_WINDOW_EVENT_PAYLOAD_BYTES {
            return Err(StatefulRunStoreError::InvalidRecordId);
        }
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some((seq, existing)) = sqlx::query_as::<_, (i64, String)>(
            "SELECT seq, payload_json FROM stateful_window_events
             WHERE thread_id = ? AND event_key = ?",
        )
        .bind(&event.thread_id)
        .bind(&event.event_key)
        .fetch_optional(&mut *transaction)
        .await?
        {
            transaction.commit().await?;
            if existing != payload {
                return Err(StatefulRunStoreError::WindowEventConflict(
                    event.event_key.clone(),
                ));
            }
            return parse_seq(seq);
        }
        let seq = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM stateful_window_events WHERE thread_id = ?",
        )
        .bind(&event.thread_id)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO stateful_window_events (
                thread_id, seq, event_key, project_id, turn_id, kind, payload_json, created_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&event.thread_id)
        .bind(seq)
        .bind(&event.event_key)
        .bind(&event.project_id)
        .bind(&event.turn_id)
        .bind(kind_name(event.kind))
        .bind(&payload)
        .bind(unix_timestamp_millis()?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        parse_seq(seq)
    }

    /// The newest sequence number of a thread's journal (0 when empty).
    pub async fn window_event_watermark(
        &self,
        thread_id: &str,
    ) -> Result<u64, StatefulRunStoreError> {
        let seq = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(seq), 0) FROM stateful_window_events WHERE thread_id = ?",
        )
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await?;
        parse_seq(seq)
    }

    /// One page of a thread's observations in `(after_seq, through_seq]`, newest first.
    pub async fn window_events_newest_first(
        &self,
        thread_id: &str,
        after_seq: u64,
        through_seq: u64,
        max_results: u32,
    ) -> Result<Vec<WindowEvent>, StatefulRunStoreError> {
        validate_list_limit(max_results)?;
        let rows = sqlx::query_as::<_, StoredEvent>(
            "SELECT thread_id, seq, event_key, project_id, turn_id, kind, payload_json,
                    created_at_ms
             FROM stateful_window_events
             WHERE thread_id = ? AND seq > ? AND seq <= ?
             ORDER BY seq DESC LIMIT ?",
        )
        .bind(thread_id)
        .bind(to_i64(after_seq)?)
        .bind(to_i64(through_seq)?)
        .bind(i64::from(max_results))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(parse_event).collect()
    }

    /// Threads of `project_id` whose journal has observations no publication covers yet.
    pub async fn threads_with_unpublished_events(
        &self,
        project_id: &str,
        max_results: u32,
    ) -> Result<Vec<String>, StatefulRunStoreError> {
        validate_list_limit(max_results)?;
        Ok(sqlx::query_scalar::<_, String>(
            "SELECT event.thread_id FROM stateful_window_events AS event
             WHERE event.project_id = ?
             GROUP BY event.thread_id
             HAVING MAX(event.seq) > COALESCE((
                 SELECT MAX(publication.through_seq) FROM stateful_window_publications AS publication
                 WHERE publication.thread_id = event.thread_id
             ), 0)
             ORDER BY MAX(event.created_at_ms) DESC LIMIT ?",
        )
        .bind(project_id)
        .bind(i64::from(max_results))
        .fetch_all(&self.pool)
        .await?)
    }

    /// The first sequence number a new publication of `thread_id` would cover, minus one.
    pub async fn window_publication_watermark(
        &self,
        thread_id: &str,
    ) -> Result<u64, StatefulRunStoreError> {
        let seq = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(through_seq), 0) FROM stateful_window_publications
             WHERE thread_id = ?",
        )
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await?;
        parse_seq(seq)
    }

    /// Freezes a publication of `(from_seq, through_seq]`. It must start exactly where the
    /// thread's previous publication ended, so suffixes never overlap; a repeat of a staged
    /// publication returns the stored one.
    pub async fn stage_window_publication(
        &self,
        publication: &WindowPublication,
    ) -> Result<WindowPublication, StatefulRunStoreError> {
        for identity in [
            &publication.thread_id,
            &publication.project_id,
            &publication.entry_id,
        ] {
            validate_identity(identity)?;
        }
        if publication.through_seq <= publication.from_seq {
            return Err(StatefulRunStoreError::InvalidRecordId);
        }
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = sqlx::query_as::<_, StoredPublication>(
            "SELECT thread_id, from_seq, through_seq, project_id, entry_id, content, state
             FROM stateful_window_publications WHERE thread_id = ? AND from_seq = ?",
        )
        .bind(&publication.thread_id)
        .bind(to_i64(publication.from_seq)?)
        .fetch_optional(&mut *transaction)
        .await?
        {
            transaction.commit().await?;
            return parse_publication(existing);
        }
        let watermark = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(through_seq), 0) FROM stateful_window_publications
             WHERE thread_id = ?",
        )
        .bind(&publication.thread_id)
        .fetch_one(&mut *transaction)
        .await?;
        if parse_seq(watermark)? != publication.from_seq {
            return Err(StatefulRunStoreError::ConcurrentMutation);
        }
        sqlx::query(
            "INSERT INTO stateful_window_publications (
                thread_id, from_seq, through_seq, project_id, entry_id, content, state,
                created_at_ms, published_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, NULL)",
        )
        .bind(&publication.thread_id)
        .bind(to_i64(publication.from_seq)?)
        .bind(to_i64(publication.through_seq)?)
        .bind(&publication.project_id)
        .bind(&publication.entry_id)
        .bind(&publication.content)
        .bind(unix_timestamp_millis()?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(WindowPublication {
            state: WindowPublicationState::Pending,
            ..publication.clone()
        })
    }

    /// Staged publications of `project_id` not yet acknowledged, oldest first.
    pub async fn pending_window_publications(
        &self,
        project_id: &str,
        max_results: u32,
    ) -> Result<Vec<WindowPublication>, StatefulRunStoreError> {
        validate_list_limit(max_results)?;
        let rows = sqlx::query_as::<_, StoredPublication>(
            "SELECT thread_id, from_seq, through_seq, project_id, entry_id, content, state
             FROM stateful_window_publications
             WHERE project_id = ? AND state = 'pending'
             ORDER BY created_at_ms, thread_id, from_seq LIMIT ?",
        )
        .bind(project_id)
        .bind(i64::from(max_results))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(parse_publication).collect()
    }

    /// Records that a staged publication's entry exists in project memory.
    pub async fn mark_window_publication_published(
        &self,
        thread_id: &str,
        from_seq: u64,
    ) -> Result<(), StatefulRunStoreError> {
        sqlx::query(
            "UPDATE stateful_window_publications
             SET state = 'published', published_at_ms = ?
             WHERE thread_id = ? AND from_seq = ? AND state = 'pending'",
        )
        .bind(unix_timestamp_millis()?)
        .bind(thread_id)
        .bind(to_i64(from_seq)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn validate_identity(value: &str) -> Result<(), StatefulRunStoreError> {
    if value.is_empty() || value.len() > MAX_IDENTITY_BYTES || value.chars().any(char::is_control) {
        return Err(StatefulRunStoreError::InvalidRecordId);
    }
    Ok(())
}

fn parse_seq(value: i64) -> Result<u64, StatefulRunStoreError> {
    u64::try_from(value).map_err(|_| StatefulRunStoreError::CorruptCount)
}

fn to_i64(value: u64) -> Result<i64, StatefulRunStoreError> {
    i64::try_from(value).map_err(|_| StatefulRunStoreError::CountOverflow)
}

fn kind_name(kind: WindowEventKind) -> &'static str {
    match kind {
        WindowEventKind::User => "user",
        WindowEventKind::Command => "command",
        WindowEventKind::Edit => "edit",
        WindowEventKind::Plan => "plan",
        WindowEventKind::Message => "message",
    }
}

fn parse_kind(value: &str) -> Result<WindowEventKind, StatefulRunStoreError> {
    match value {
        "user" => Ok(WindowEventKind::User),
        "command" => Ok(WindowEventKind::Command),
        "edit" => Ok(WindowEventKind::Edit),
        "plan" => Ok(WindowEventKind::Plan),
        "message" => Ok(WindowEventKind::Message),
        _ => Err(StatefulRunStoreError::CorruptEnum(value.to_string())),
    }
}

fn parse_event(row: StoredEvent) -> Result<WindowEvent, StatefulRunStoreError> {
    Ok(WindowEvent {
        seq: parse_seq(row.seq)?,
        event: NewWindowEvent {
            thread_id: row.thread_id,
            event_key: row.event_key,
            project_id: row.project_id,
            turn_id: row.turn_id,
            kind: parse_kind(&row.kind)?,
            payload: serde_json::from_str(&row.payload_json)?,
        },
        created_at_ms: row.created_at_ms,
    })
}

fn parse_publication(row: StoredPublication) -> Result<WindowPublication, StatefulRunStoreError> {
    Ok(WindowPublication {
        thread_id: row.thread_id,
        from_seq: parse_seq(row.from_seq)?,
        through_seq: parse_seq(row.through_seq)?,
        project_id: row.project_id,
        entry_id: row.entry_id,
        content: row.content,
        state: match row.state.as_str() {
            "pending" => WindowPublicationState::Pending,
            "published" => WindowPublicationState::Published,
            other => return Err(StatefulRunStoreError::CorruptEnum(other.to_string())),
        },
    })
}

#[cfg(test)]
#[path = "window_journal_tests.rs"]
mod tests;
