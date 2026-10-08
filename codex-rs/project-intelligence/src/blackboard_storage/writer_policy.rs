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
}

pub(super) async fn check_model_target(
    connection: &mut sqlx::SqliteConnection,
    entry: &BlackboardEntry,
    operation: ModelOperation,
) -> Result<(), BlackboardStoreError> {
    if entry.value.provenance.kind == BlackboardProvenanceKind::User {
        return Err(BlackboardStoreError::ModelMutationRefused);
    }
    let context = policy_of(connection, &entry.value.project_id, entry.id.as_str()).await?;
    if context
        .as_ref()
        .is_some_and(|context| context.authority == KnowledgeAuthority::HumanDirect)
        || (matches!(operation, ModelOperation::Retirement)
            && (entry.value.provenance.kind != BlackboardProvenanceKind::Agent
                || context.is_some_and(|context| {
                    context.authority == KnowledgeAuthority::LegacyUnknown
                        || context.category == KnowledgeCategory::RuledOut
                })))
    {
        return Err(BlackboardStoreError::ModelMutationRefused);
    }
    Ok(())
}

#[cfg(test)]
#[path = "writer_policy_tests.rs"]
mod tests;
