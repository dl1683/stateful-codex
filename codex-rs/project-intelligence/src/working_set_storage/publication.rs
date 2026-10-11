//! Publication admission. One short `BEGIN IMMEDIATE` transaction orders a capsule ticket
//! against every eligibility writer; no SQLite guard outlives it, and none is held across a
//! model, tool or persistence wait. A retirement committed before admission refuses the
//! ticket; one committed after admission makes the ticket stale.

use super::WorkingSetInvalidation;
use super::WorkingSetKey;
use super::WorkingSetStore;
use super::WorkingSetStoreError;
use super::generation_on;
use super::keyed;
use super::load_projection_on;
use super::name;
use super::to_i64;
use super::validate_key;
use crate::blackboard_storage::source_turn_eligible_on;
use crate::blackboard_storage::text_eligible_on;
use crate::storage::unix_timestamp_millis;

const MAX_SOURCE_CHECKS: usize = 128;
const MAX_SOURCE_TEXT_BYTES: usize = 128 * 1024;
const RETAINED_TICKETS: i64 = 8;
const RETAINED_INVALIDATIONS: i64 = 32;

/// What preparation read and rendered; every value is rechecked at admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationRequest {
    pub key: WorkingSetKey,
    pub window_id: String,
    pub history_generation: u64,
    pub projection_revision: u64,
    pub eligibility_generation: u64,
    pub capsule_digest: String,
    /// Turns and complete selected source enclosures behind the rendered candidates.
    pub source_turn_ids: Vec<String>,
    pub source_texts: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationTicket {
    pub ticket_id: i64,
    pub key: WorkingSetKey,
    pub window_id: String,
    pub history_generation: u64,
    pub projection_revision: u64,
    pub eligibility_generation: u64,
    pub capsule_digest: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationRefusal {
    BindingChanged,
    EligibilityChanged { current: u64 },
    ProjectionMissing,
    ProjectionChanged,
    Invalidated(WorkingSetInvalidation),
    SourceIneligible,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublicationAdmission {
    Admitted(PublicationTicket),
    Refused(PublicationRefusal),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TicketStatus {
    Admitted,
    Installed,
    Cancelled,
    /// Eligibility advanced after admission: the admitted context is an older snapshot.
    Stale {
        current_generation: u64,
    },
}

impl WorkingSetStore {
    /// Admits one publication for the durably bound thread and cancels its earlier pending
    /// tickets. The caller releases `admission` as soon as this returns.
    pub async fn admit_publication(
        &self,
        admission: &codex_state::ThreadProjectAdmission,
        request: &PublicationRequest,
    ) -> Result<PublicationAdmission, WorkingSetStoreError> {
        let key = &request.key;
        validate_key(key)?;
        let text_bytes: usize = request.source_texts.iter().map(String::len).sum();
        if !(1..=128).contains(&request.window_id.len())
            || !(1..=128).contains(&request.capsule_digest.len())
            || request.source_turn_ids.len() > MAX_SOURCE_CHECKS
            || request.source_texts.len() > MAX_SOURCE_CHECKS
            || text_bytes > MAX_SOURCE_TEXT_BYTES
        {
            return Err(WorkingSetStoreError::InvalidRequest("publication bounds"));
        }
        if (
            admission.project_id(),
            admission.thread_id(),
            admission.binding_generation(),
        ) != (
            key.project_id.as_str(),
            key.thread_id.as_str(),
            key.binding_generation,
        ) {
            return Ok(PublicationAdmission::Refused(
                PublicationRefusal::BindingChanged,
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = generation_on(&mut tx, &key.project_id).await?;
        let projection = load_projection_on(&mut tx, key).await?;
        let mut eligible = true;
        for turn in &request.source_turn_ids {
            eligible &= source_turn_eligible_on(&mut tx, &key.project_id, turn).await?;
        }
        for text in &request.source_texts {
            eligible &= text_eligible_on(&mut tx, &key.project_id, text).await?;
        }
        let refusal = match projection {
            _ if current != request.eligibility_generation => {
                PublicationRefusal::EligibilityChanged { current }
            }
            None => PublicationRefusal::ProjectionMissing,
            Some(projection) => match projection.invalidation {
                Some(reason) => PublicationRefusal::Invalidated(reason),
                None if projection.revision != request.projection_revision
                    || projection.eligibility_generation != current =>
                {
                    PublicationRefusal::ProjectionChanged
                }
                None if !eligible => PublicationRefusal::SourceIneligible,
                None => return self.issue(tx, request, current).await,
            },
        };
        Ok(PublicationAdmission::Refused(refusal))
    }

    async fn issue(
        &self,
        mut tx: sqlx::Transaction<'static, sqlx::Sqlite>,
        request: &PublicationRequest,
        current: u64,
    ) -> Result<PublicationAdmission, WorkingSetStoreError> {
        let key = &request.key;
        keyed(
            "UPDATE working_set_publication_tickets SET state = 'cancelled'
             WHERE project_id = ?1 AND binding_generation = ?2 AND thread_id = ?3
               AND state = 'admitted'",
            key,
        )?
        .execute(&mut *tx)
        .await?;
        let ticket_id: i64 = keyed(
            "INSERT INTO working_set_publication_tickets (
                project_id, binding_generation, thread_id, window_id, history_generation,
                projection_version, projection_revision, eligibility_generation,
                capsule_digest, state, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'admitted', ?10) RETURNING ticket_id",
            key,
        )?
        .bind(&request.window_id)
        .bind(to_i64(request.history_generation)?)
        .bind(i64::from(key.projection_version))
        .bind(to_i64(request.projection_revision)?)
        .bind(to_i64(current)?)
        .bind(&request.capsule_digest)
        .bind(unix_timestamp_millis()?)
        .try_map(|row| sqlx::Row::try_get(&row, 0))
        .fetch_one(&mut *tx)
        .await?;
        keyed(
            "DELETE FROM working_set_publication_tickets
             WHERE project_id = ?1 AND binding_generation = ?2 AND thread_id = ?3
               AND ticket_id NOT IN (SELECT ticket_id FROM working_set_publication_tickets
                   WHERE project_id = ?1 AND binding_generation = ?2 AND thread_id = ?3
                   ORDER BY ticket_id DESC LIMIT ?4)",
            key,
        )?
        .bind(RETAINED_TICKETS)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(PublicationAdmission::Admitted(PublicationTicket {
            ticket_id,
            key: key.clone(),
            window_id: request.window_id.clone(),
            history_generation: request.history_generation,
            projection_revision: request.projection_revision,
            eligibility_generation: current,
            capsule_digest: request.capsule_digest.clone(),
        }))
    }

    /// Marks an admitted ticket installed after its checkpoint persisted, unless eligibility
    /// advanced in between; a stale admitted ticket is cancelled instead.
    pub async fn confirm_installed(
        &self,
        ticket: &PublicationTicket,
    ) -> Result<TicketStatus, WorkingSetStoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let status = ticket_status_on(&mut tx, ticket).await?;
        let (state, status) = match status {
            TicketStatus::Admitted => ("installed", TicketStatus::Installed),
            TicketStatus::Stale { .. } => ("cancelled", status),
            TicketStatus::Installed | TicketStatus::Cancelled => return Ok(status),
        };
        sqlx::query(
            "UPDATE working_set_publication_tickets SET state = ?
             WHERE ticket_id = ? AND state = 'admitted'",
        )
        .bind(state)
        .bind(ticket.ticket_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(status)
    }

    /// Read-only recheck for sampling admission.
    pub async fn ticket_status(
        &self,
        ticket: &PublicationTicket,
    ) -> Result<TicketStatus, WorkingSetStoreError> {
        let mut tx = self.pool.begin().await?;
        let status = ticket_status_on(&mut tx, ticket).await?;
        tx.commit().await?;
        Ok(status)
    }

    /// Invalidates every projection version of the thread, cancels its pending tickets and
    /// keeps a bounded record (retirement, rebind/clear, archive, gap, version, corruption).
    pub async fn invalidate(
        &self,
        key: &WorkingSetKey,
        reason: WorkingSetInvalidation,
    ) -> Result<(), WorkingSetStoreError> {
        validate_key(key)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = generation_on(&mut tx, &key.project_id).await?;
        // Statements share one argument list; SQLite binds only the parameters each names.
        let sql = [
            "UPDATE working_set_projections SET invalidation = ?4
             WHERE project_id = ?1 AND binding_generation = ?2 AND thread_id = ?3",
            "UPDATE working_set_publication_tickets SET state = 'cancelled'
             WHERE project_id = ?1 AND binding_generation = ?2 AND thread_id = ?3
               AND state = 'admitted'",
            "INSERT INTO working_set_invalidations
             SELECT ?1, ?2, ?3, COALESCE(MAX(sequence), 0) + 1, ?4, ?5, ?6
             FROM working_set_invalidations
             WHERE project_id = ?1 AND binding_generation = ?2 AND thread_id = ?3",
            "DELETE FROM working_set_invalidations
             WHERE project_id = ?1 AND binding_generation = ?2 AND thread_id = ?3
               AND sequence <= (SELECT MAX(sequence) FROM working_set_invalidations
                   WHERE project_id = ?1 AND binding_generation = ?2 AND thread_id = ?3)
                   - ?7",
        ];
        for statement in sql {
            keyed(statement, key)?
                .bind(name(reason))
                .bind(to_i64(current)?)
                .bind(unix_timestamp_millis()?)
                .bind(RETAINED_INVALIDATIONS)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

async fn ticket_status_on(
    connection: &mut sqlx::SqliteConnection,
    ticket: &PublicationTicket,
) -> Result<TicketStatus, WorkingSetStoreError> {
    let row: Option<(String, i64, String)> = sqlx::query_as(
        "SELECT state, eligibility_generation, capsule_digest
         FROM working_set_publication_tickets
         WHERE ticket_id = ? AND project_id = ? AND binding_generation = ? AND thread_id = ?",
    )
    .bind(ticket.ticket_id)
    .bind(&ticket.key.project_id)
    .bind(to_i64(ticket.key.binding_generation)?)
    .bind(&ticket.key.thread_id)
    .fetch_optional(&mut *connection)
    .await?;
    let current = generation_on(connection, &ticket.key.project_id).await?;
    Ok(match row {
        Some((state, generation, digest))
            if state != "cancelled"
                && digest == ticket.capsule_digest
                && generation == to_i64(ticket.eligibility_generation)? =>
        {
            if current != ticket.eligibility_generation {
                TicketStatus::Stale {
                    current_generation: current,
                }
            } else if state == "installed" {
                TicketStatus::Installed
            } else {
                TicketStatus::Admitted
            }
        }
        _ => TicketStatus::Cancelled,
    })
}

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;
