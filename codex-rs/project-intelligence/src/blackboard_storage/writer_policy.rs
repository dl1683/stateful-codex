//! Actor checks run under the common PI writer lock, including successor retries.

use crate::BlackboardEntry;
use crate::BlackboardProvenanceKind;
use crate::KnowledgeAuthority;
use crate::KnowledgeCategory;

use super::BlackboardStoreError;
use super::context_bounds::policy_of;

#[derive(Clone, Copy)]
pub(super) enum WriterActor {
    Host,
    Model,
}

pub(super) enum ModelOperation {
    Mutation,
    Retirement,
    SuccessionReplay,
}

pub(super) async fn check_model_target(
    connection: &mut sqlx::SqliteConnection,
    entry: &BlackboardEntry,
    operation: ModelOperation,
) -> Result<(), BlackboardStoreError> {
    // Classification or text edits cannot establish assistant origin for historical memory.
    // Judge the current target and its entire provenance history under the writer lock,
    // including Agent revisions produced by older binaries from non-Agent entries.
    if entry.value.provenance.kind != BlackboardProvenanceKind::Agent {
        return Err(BlackboardStoreError::ModelMutationRefused);
    }
    let non_agent_revisions = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM blackboard_entry_revisions
         WHERE entry_id = ? AND provenance_kind IS NOT 'agent'",
    )
    .bind(entry.id.as_str())
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| BlackboardStoreError::ModelMutationRefused)?;
    if non_agent_revisions != 0 {
        return Err(BlackboardStoreError::ModelMutationRefused);
    }
    if !matches!(operation, ModelOperation::SuccessionReplay)
        && !super::identity::entry_storage_eligible_on(
            connection,
            &entry.value.project_id,
            &entry.id,
        )
        .await?
    {
        return Err(BlackboardStoreError::ModelMutationRefused);
    }
    let context = policy_of(connection, &entry.value.project_id, entry.id.as_str()).await?;
    if context.as_ref().is_some_and(|context| {
        matches!(
            context.authority,
            KnowledgeAuthority::HumanDirect | KnowledgeAuthority::LegacyUnknown
        )
    }) || (matches!(
        operation,
        ModelOperation::Retirement | ModelOperation::SuccessionReplay
    ) && context.is_some_and(|context| context.category == KnowledgeCategory::RuledOut))
    {
        return Err(BlackboardStoreError::ModelMutationRefused);
    }
    Ok(())
}

#[cfg(test)]
#[path = "writer_policy_tests.rs"]
mod tests;
