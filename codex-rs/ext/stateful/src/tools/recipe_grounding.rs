//! Resolves whether a `Recipe:` fact being recorded names a command the host saw succeed.
//! Resolution happens before the entry is written, so the entry and its observation commit
//! together.

use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeContext;
use codex_protocol::models::ResponseItem;

use crate::recipe_applicability::RecipeCheck;
use crate::recipe_applicability::is_recipe;
use crate::recipe_capture::ExitStatus;
use crate::recipe_capture::exit_status;
use crate::recipe_capture::observation;
use crate::services::ProjectIntelligenceServices;

/// The recorded recipe's observation context, if grounded, and its result label.
pub(super) struct Grounding {
    pub(super) context: Option<KnowledgeContext>,
    pub(super) label: String,
}

/// `None` for a record that is not a recipe. A recipe with no exactly matching kept
/// command, or whose command the host header shows exiting non-zero, has no context.
pub(super) fn resolve(
    services: &ProjectIntelligenceServices,
    thread_id: &str,
    kind: BlackboardKind,
    content: &str,
    history: &[ResponseItem],
) -> Option<Grounding> {
    if !is_recipe(kind, content) {
        return None;
    }
    let not_observed = || Grounding {
        context: None,
        label: RecipeCheck::NotObserved.label(),
    };
    let Some(command) = services.observed_commands().matching(thread_id, content) else {
        return Some(not_observed());
    };
    let exit = exit_status(history, &command.call_id);
    if exit == ExitStatus::NonZero {
        return Some(not_observed());
    }
    let Some(observation) = observation(&command, exit) else {
        return Some(not_observed());
    };
    let Ok(payload) = serde_json::to_string(&observation) else {
        return Some(not_observed());
    };
    let mut context =
        KnowledgeContext::new(KnowledgeCategory::Recipe, KnowledgeAuthority::HostObserved);
    context.payload = Some(payload);
    Some(Grounding {
        context: Some(context),
        label: match exit {
            ExitStatus::Zero => RecipeCheck::Current,
            ExitStatus::NonZero | ExitStatus::Unknown => RecipeCheck::CurrentExitUnconfirmed,
        }
        .label(),
    })
}
