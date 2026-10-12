//! Receipted obligation writes that keep host-retained open issues durable.
//!
//! A retained issue is a blocker of the run's obligation packet that carries
//! [`RETAINED_ISSUE_PREFIX`]. Only the host creates one ([`ObligationOperation::Retain`]); every
//! later model update ([`ObligationOperation::Update`]) carries it forward unless the update
//! names it in `resolved_issues`. Narrative text never resolves an issue.
//!
//! Each operation is one `BEGIN IMMEDIATE` transaction on one connection: the receipt is
//! checked first (an identical retry returns the stored original row, so a replay never
//! reconstructs from newer state and never resurrects a resolved issue), then the run and the
//! latest packet are loaded, the operation is validated, resolutions are applied, retained
//! issues are carried forward, the final packet is bounded, and the row is appended.

use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use sqlx::SqliteConnection;

use crate::NewObligation;
use crate::ObligationPacket;
use crate::StatefulObligation;
use crate::StatefulRunId;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::run::MAX_PACKET_LIST_ITEMS;
use crate::storage::append_obligation_in_transaction;
use crate::storage::load_obligation_by_id;
use crate::storage::load_run;
use crate::storage::validate_record_id;

/// The frozen prefix of every host-retained open issue in an obligation packet's `blockers`.
pub const RETAINED_ISSUE_PREFIX: &str = "Open issue (retained): ";
/// The history line a resolution appends to `changed`.
const RESOLVED_PREFIX: &str = "Resolved: ";
/// Per-call caps on retained issues and resolutions (the completion `openIssues` caps).
const MAX_OPERATION_ISSUES: usize = 16;
const MAX_ISSUE_BYTES: usize = 1_024;
const OVERFLOW: &str = "too many retained issues or resolution entries; resolve some first";

/// One obligation write that respects host-retained issues.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObligationOperation {
    /// The host retains these open issues (trimmed, raw) on the latest packet.
    Retain { issues: Vec<String> },
    /// The model records `packet`, resolving the named retained issues.
    Update {
        packet: Box<ObligationPacket>,
        resolved_issues: Vec<String>,
    },
}

/// The caller's retry key for an operation: the provider call id for `Retain`, the model's
/// idempotency key for `Update`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationReceiptKey(String);

impl OperationReceiptKey {
    pub fn parse(value: impl Into<String>) -> Result<Self, StatefulRunStoreError> {
        let value = value.into();
        validate_record_id(&value)?;
        Ok(Self(value))
    }
}

/// The canonical submitted operation; its field order is fixed and it is the receipt digest.
#[derive(Serialize)]
struct CanonicalOperation<'a> {
    kind: &'static str,
    run_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    issues: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    packet: Option<&'a ObligationPacket>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_issues: Option<&'a [String]>,
}

impl ObligationOperation {
    fn kind(&self) -> &'static str {
        match self {
            Self::Retain { .. } => "retain",
            Self::Update { .. } => "update",
        }
    }

    fn digest(&self, run_id: &StatefulRunId) -> Result<String, StatefulRunStoreError> {
        let canonical = match self {
            Self::Retain { issues } => CanonicalOperation {
                kind: self.kind(),
                run_id: run_id.as_str(),
                issues: Some(issues),
                packet: None,
                resolved_issues: None,
            },
            Self::Update {
                packet,
                resolved_issues,
            } => CanonicalOperation {
                kind: self.kind(),
                run_id: run_id.as_str(),
                issues: None,
                packet: Some(packet.as_ref()),
                resolved_issues: Some(resolved_issues),
            },
        };
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&canonical)?)
        ))
    }
}

impl StatefulRunStore {
    /// Applies `operation` to the obligations of `run_id` under the receipt `receipt`.
    pub async fn apply_obligation_operation(
        &self,
        run_id: &StatefulRunId,
        operation: ObligationOperation,
        receipt: &OperationReceiptKey,
        source_id: &str,
    ) -> Result<StatefulObligation, StatefulRunStoreError> {
        crate::run::validate_source_id(source_id)?;
        let digest = operation.digest(run_id)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let run = load_run(&mut transaction, run_id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(run_id.to_string()))?;
        let project_id = run.value.project_id.clone();
        let base = stable_id(
            operation.kind(),
            &project_id,
            &format!("{run_id}\0{}", receipt.0),
        );
        // 1. Receipt first: nothing is reconstructed or revalidated for a known call.
        if let Some(stored) = stored_receipt(
            &mut transaction,
            run_id,
            &operation,
            &project_id,
            &base,
            &receipt.0,
            &digest,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(stored);
        }
        // 2. A terminal run takes no new obligation.
        if run.status.is_terminal() {
            return Err(StatefulRunStoreError::AcceptanceRunState(run.status));
        }
        // 3. The latest packet, read on this connection, and its retained issues.
        let latest = latest_obligation_packet(&mut transaction, run_id)
            .await?
            .map(|(_, packet)| packet);
        let retained: Vec<String> = latest
            .iter()
            .flat_map(|packet| &packet.blockers)
            .filter(|blocker| blocker.starts_with(RETAINED_ISSUE_PREFIX))
            .cloned()
            .collect();
        let packet = match operation {
            ObligationOperation::Retain { issues } => {
                // 4. Validate.
                validate_issues(&issues)?;
                // 6. Carry forward: the latest packet verbatim plus the new retained issues.
                let mut packet = latest.unwrap_or_default();
                let added = issues
                    .iter()
                    .map(|issue| format!("{RETAINED_ISSUE_PREFIX}{issue}"));
                packet.blockers = union(std::mem::take(&mut packet.blockers), added);
                packet
            }
            ObligationOperation::Update {
                packet,
                resolved_issues,
            } => {
                let mut packet = *packet;
                // 4. Validate: resolutions name retained issues; only the host retains.
                if resolved_issues.len() > MAX_OPERATION_ISSUES {
                    return Err(refused(OVERFLOW));
                }
                if packet
                    .blockers
                    .iter()
                    .chain(&packet.changed)
                    .any(|item| item.starts_with(RETAINED_ISSUE_PREFIX))
                {
                    return Err(refused(&format!(
                        "blockers and changed may not start with \"{RETAINED_ISSUE_PREFIX}\"; only the host retains issues"
                    )));
                }
                let mut resolved = Vec::new();
                for entry in &resolved_issues {
                    let member = if entry.starts_with(RETAINED_ISSUE_PREFIX) {
                        entry.clone()
                    } else {
                        format!("{RETAINED_ISSUE_PREFIX}{entry}")
                    };
                    if !retained.contains(&member) {
                        return Err(refused(&format!(
                            "resolvedIssues names an issue that is not retained: {entry}"
                        )));
                    }
                    if !resolved.contains(&member) {
                        resolved.push(member);
                    }
                }
                // 5. Resolutions: removed from the retained set, kept as history only.
                packet.changed.extend(resolved.iter().map(|member| {
                    format!(
                        "{RESOLVED_PREFIX}{}",
                        &member[RETAINED_ISSUE_PREFIX.len()..]
                    )
                }));
                // 6. Carry forward every retained issue that was not resolved.
                let carried = retained
                    .into_iter()
                    .filter(|member| !resolved.contains(member));
                packet.blockers = union(std::mem::take(&mut packet.blockers), carried);
                packet
            }
        };
        // 7. Bounds on the final, augmented packet; an overflow refuses the whole call.
        let lists = [
            &packet.examined,
            &packet.rationale,
            &packet.learning,
            &packet.implication,
            &packet.strategy,
            &packet.changed,
            &packet.next,
            &packet.uncertainty,
            &packet.blockers,
            &packet.requested_judgment,
        ];
        if lists
            .iter()
            .any(|items| items.len() > MAX_PACKET_LIST_ITEMS)
        {
            return Err(refused(OVERFLOW));
        }
        let value = NewObligation {
            project_id,
            run_id: run_id.clone(),
            packet,
            provenance_source_id: source_id.to_string(),
        };
        value.validate()?;
        // 8. Append and commit.
        let obligation =
            append_obligation_in_transaction(&mut transaction, format!("{base}-{digest}"), value)
                .await?;
        transaction.commit().await?;
        Ok(obligation)
    }
}

/// The stored row of a known call: the original for an identical retry; a conflict for a
/// retry with other inputs.
async fn stored_receipt(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    operation: &ObligationOperation,
    project_id: &str,
    base: &str,
    key: &str,
    digest: &str,
) -> Result<Option<StatefulObligation>, StatefulRunStoreError> {
    let stored = sqlx::query_scalar::<_, String>(
        "SELECT id FROM stateful_obligations WHERE id >= ? AND id < ? ORDER BY id LIMIT 1",
    )
    .bind(format!("{base}-"))
    .bind(format!("{base}."))
    .fetch_optional(&mut *connection)
    .await?;
    if let Some(id) = stored {
        let obligation = load_obligation_by_id(connection, &id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::ObligationNotFound(id.clone()))?;
        if obligation.value.run_id != *run_id || id != format!("{base}-{digest}") {
            return Err(StatefulRunStoreError::ObligationIdentityConflict(id));
        }
        return Ok(Some(obligation));
    }
    // An update written before receipts existed: same call only if it recorded this packet.
    if !matches!(operation, ObligationOperation::Update { .. }) {
        return Ok(None);
    }
    let legacy_id = stable_id("obligation", project_id, key);
    let Some(legacy) = load_obligation_by_id(connection, &legacy_id).await? else {
        return Ok(None);
    };
    let legacy_digest = ObligationOperation::Update {
        packet: Box::new(legacy.value.packet.clone()),
        resolved_issues: Vec::new(),
    }
    .digest(&legacy.value.run_id)?;
    if legacy.value.run_id != *run_id || legacy_digest != digest {
        return Err(StatefulRunStoreError::ObligationIdentityConflict(legacy_id));
    }
    Ok(Some(legacy))
}

/// The newest obligation packet of `run_id` and its sequence, read on `connection`.
pub(crate) async fn latest_obligation_packet(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
) -> Result<Option<(u64, ObligationPacket)>, StatefulRunStoreError> {
    let Some((sequence, packet)) = sqlx::query_as::<_, (i64, String)>(
        "SELECT sequence, packet_json FROM stateful_obligations
         WHERE run_id = ? ORDER BY sequence DESC LIMIT 1",
    )
    .bind(run_id.as_str())
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };
    let sequence = u64::try_from(sequence).map_err(|_| StatefulRunStoreError::CorruptCount)?;
    Ok(Some((
        sequence,
        serde_json::from_str::<ObligationPacket>(&packet)?,
    )))
}

fn validate_issues(issues: &[String]) -> Result<(), StatefulRunStoreError> {
    if issues.is_empty() || issues.len() > MAX_OPERATION_ISSUES {
        return Err(refused(&format!(
            "retain 1-{MAX_OPERATION_ISSUES} open issues per call"
        )));
    }
    if issues.iter().any(|issue| {
        issue.is_empty()
            || issue.trim() != issue
            || issue.len() > MAX_ISSUE_BYTES
            || issue.contains('\0')
    }) {
        return Err(refused(&format!(
            "each open issue must be non-empty, trimmed, at most {MAX_ISSUE_BYTES} bytes, and contain no NUL"
        )));
    }
    // A prefix-shaped raw issue would be stored double-prefixed and make its resolution
    // ambiguous with the issue it quotes, so it is refused.
    if issues
        .iter()
        .any(|issue| issue.starts_with(RETAINED_ISSUE_PREFIX))
    {
        return Err(refused(&format!(
            "an open issue may not start with \"{RETAINED_ISSUE_PREFIX}\""
        )));
    }
    Ok(())
}

/// `existing` unchanged, followed by each `added` entry it does not hold yet (exact bytes), in
/// order.
fn union(mut existing: Vec<String>, added: impl IntoIterator<Item = String>) -> Vec<String> {
    for item in added {
        if !existing.contains(&item) {
            existing.push(item);
        }
    }
    existing
}

fn refused(reason: &str) -> StatefulRunStoreError {
    StatefulRunStoreError::ObligationOperationRefused(reason.to_string())
}

/// The id the obligation tool derives from a key: `stateful-{kind}-{sha256(project ‖ 0 ‖ key)}`.
fn stable_id(kind: &str, project_id: &str, key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(project_id.as_bytes());
    hasher.update([0]);
    hasher.update(key.as_bytes());
    format!("stateful-{kind}-{:x}", hasher.finalize())
}

#[cfg(test)]
#[path = "obligation_operations_tests.rs"]
mod tests;
