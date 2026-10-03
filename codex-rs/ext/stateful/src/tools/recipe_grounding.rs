//! Grounds a newly recorded `Recipe:` fact in a command the host saw succeed.

use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeContext;
use codex_protocol::models::ResponseItem;

use crate::recipe_applicability::RecipeCheck;
use crate::recipe_applicability::is_recipe;
use crate::recipe_capture::ExitStatus;
use crate::recipe_capture::carries_credentials;
use crate::recipe_capture::exit_status;
use crate::recipe_capture::observe_recipe;
use crate::services::ProjectIntelligenceServices;

/// Whether a recipe record carries a credential-like argument and must be refused.
pub(super) fn refuses_credentials(kind: BlackboardKind, content: &str) -> bool {
    is_recipe(kind, content) && carries_credentials(content)
}

/// Stores the conditions a just-recorded recipe was observed under and returns its packet
/// label, or `None` for an entry that is not a recipe. A recipe with no matching
/// command, or whose matching command exited non-zero, stays an unverified fact.
pub(super) async fn ground(
    services: &ProjectIntelligenceServices,
    thread_id: &str,
    entry: &BlackboardEntry,
    history: &[ResponseItem],
) -> Option<String> {
    if !is_recipe(entry.value.kind, &entry.value.content) {
        return None;
    }
    let Some(command) = services
        .observed_commands()
        .matching(thread_id, &entry.value.content)
    else {
        return Some(RecipeCheck::NotObserved.label());
    };
    let exit = exit_status(history, &command.call_id);
    if exit == ExitStatus::NonZero {
        return Some(RecipeCheck::NotObserved.label());
    }
    let observed_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        });
    let observation =
        tokio::task::spawn_blocking(move || observe_recipe(&command, observed_at_ms, exit))
            .await
            .ok()?;
    let mut context =
        KnowledgeContext::new(KnowledgeCategory::Recipe, KnowledgeAuthority::HostObserved);
    context.payload = serde_json::to_string(&observation).ok();
    let store = services.blackboard().await.ok()?;
    if let Err(error) = store.record_context(entry, &context, /*change*/ None).await {
        tracing::warn!(%error, "failed to store a recipe observation");
        return Some(RecipeCheck::NotObserved.label());
    }
    Some(
        match exit {
            ExitStatus::Zero => RecipeCheck::Current,
            ExitStatus::NonZero | ExitStatus::Unknown => RecipeCheck::CurrentExitUnconfirmed,
        }
        .label(),
    )
}
