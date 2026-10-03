//! Whether a stored recipe still applies before it is offered again.
//!
//! A host-observed recipe records the conditions it last succeeded under. Before the
//! packet offers it, cheap local checks confirm those conditions: the working directory
//! exists, the executable still resolves to the same file (path, size and modification
//! time), and the set of dependency manifests is unchanged (none added, removed or
//! edited). A failed check marks the recipe `needsCheck` with the reason, so the model
//! checks the environment once instead of trusting a dead interpreter or creating a second
//! environment. Every recipe shown carries a label: `notObserved` when the host never saw
//! it run, `unchecked` when its check could not be performed. Nothing here runs the recipe,
//! and nothing here verifies which copy of a package tests import.

use std::collections::HashMap;
use std::path::PathBuf;

use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::RootBlackboardProjection;

use crate::recipe_capture::ExitStatus;
use crate::recipe_capture::RecipeObservation;
use crate::recipe_capture::manifest_fingerprints;
use crate::recipe_capture::resolve_executable;
use crate::recipe_capture::single_recipe_command;

/// Recipes checked per packet; others are labelled unchecked.
const MAX_CHECKED_RECIPES: usize = 16;

/// The packet label for one recipe entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RecipeCheck {
    /// Observed to exit 0, and its executable and manifests are unchanged.
    Current,
    /// Observed to complete, exit status not visible; conditions are unchanged.
    CurrentExitUnconfirmed,
    /// Observed successful once, but a condition changed since.
    NeedsCheck(String),
    /// Recorded by the model without a matching successful command.
    NotObserved,
    /// Its conditions could not be checked for this packet.
    Unchecked,
}

impl RecipeCheck {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Current => {
                "current (seen succeeding with this executable and manifests; reuse it, do not set up another environment)".to_string()
            }
            Self::CurrentExitUnconfirmed => {
                "ran (exit status unseen; check once, then reuse it)".to_string()
            }
            Self::NeedsCheck(reason) => {
                format!("needsCheck ({reason}; check once, then reuse what works)")
            }
            Self::NotObserved => "notObserved (never seen running; check once)".to_string(),
            Self::Unchecked => "unchecked (check once)".to_string(),
        }
    }
}

/// Whether a recipe entry's content marks it as a recipe.
pub(crate) fn is_recipe(kind: BlackboardKind, content: &str) -> bool {
    kind == BlackboardKind::Fact && content.starts_with("Recipe:")
}

/// Checks the recipe entries shown in the root, by entry ID. Recipes beyond the check
/// budget, or whose observations cannot be loaded, are absent and render as unchecked.
pub(crate) async fn check_root_recipes(
    store: &BlackboardStore,
    project_id: &str,
    projection: &RootBlackboardProjection,
) -> HashMap<String, RecipeCheck> {
    let recipes = projection
        .data
        .iter()
        .filter(|hit| is_recipe(hit.entry.value.kind, &hit.entry.value.content))
        .take(MAX_CHECKED_RECIPES)
        .map(|hit| (hit.entry.id.clone(), hit.entry.value.content.clone()))
        .collect::<Vec<_>>();
    if recipes.is_empty() {
        return HashMap::new();
    }
    let ids = recipes.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
    let contexts = match store.knowledge_contexts(project_id, &ids).await {
        Ok(contexts) => contexts,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to load recipe observations");
            return HashMap::new();
        }
    };
    let observations = recipes
        .into_iter()
        .map(|(id, content)| {
            let observed = contexts
                .get(id.as_str())
                .filter(|context| {
                    context.category == KnowledgeCategory::Recipe
                        && context.authority == KnowledgeAuthority::HostObserved
                })
                .map(|context| {
                    context
                        .payload
                        .as_deref()
                        .and_then(|payload| serde_json::from_str::<RecipeObservation>(payload).ok())
                });
            (id.to_string(), content, observed)
        })
        .collect::<Vec<_>>();
    tokio::task::spawn_blocking(move || {
        observations
            .into_iter()
            .map(|(id, content, observed)| {
                let check = match observed {
                    None => RecipeCheck::NotObserved,
                    // Kept by an earlier build without executable identity: it ran, but its
                    // conditions cannot be compared.
                    Some(None) => RecipeCheck::NeedsCheck(
                        "observed before its conditions were recorded".to_string(),
                    ),
                    // An observation of another command (one carried over from a replaced
                    // recipe) does not ground this one.
                    Some(Some(observation))
                        if single_recipe_command(&content) != Some(observation.command.trim()) =>
                    {
                        RecipeCheck::NotObserved
                    }
                    Some(Some(observation)) => check_observation(&observation),
                };
                (id, check)
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// Rechecks the conditions a recipe was observed under. Blocking.
pub(crate) fn check_observation(observation: &RecipeObservation) -> RecipeCheck {
    let cwd = PathBuf::from(&observation.cwd);
    if !cwd.is_dir() {
        return RecipeCheck::NeedsCheck(format!(
            "working directory {} no longer exists",
            observation.cwd
        ));
    }
    let typed = &observation.executable.typed;
    match resolve_executable(&cwd, typed) {
        None => {
            return RecipeCheck::NeedsCheck(format!("executable {typed} no longer resolves"));
        }
        Some(current) if current != observation.executable => {
            return RecipeCheck::NeedsCheck(format!(
                "executable {typed} now resolves to a different or changed file"
            ));
        }
        Some(_) => {}
    }
    let current = manifest_fingerprints(&cwd);
    for recorded in &observation.manifests {
        match current
            .iter()
            .find(|manifest| manifest.path == recorded.path)
        {
            Some(manifest) if manifest.sha256.is_none() || recorded.sha256.is_none() => {
                return RecipeCheck::NeedsCheck(format!(
                    "{} is too large to compare",
                    recorded.path
                ));
            }
            Some(manifest) if manifest == recorded => {}
            Some(_) => {
                return RecipeCheck::NeedsCheck(format!("{} changed since", recorded.path));
            }
            None => {
                return RecipeCheck::NeedsCheck(format!("{} was removed", recorded.path));
            }
        }
    }
    if let Some(added) = current.iter().find(|manifest| {
        !observation
            .manifests
            .iter()
            .any(|recorded| recorded.path == manifest.path)
    }) {
        return RecipeCheck::NeedsCheck(format!("{} was added since", added.path));
    }
    match observation.exit_status {
        ExitStatus::Zero => RecipeCheck::Current,
        ExitStatus::NonZero | ExitStatus::Unknown => RecipeCheck::CurrentExitUnconfirmed,
    }
}

#[cfg(test)]
#[path = "recipe_applicability_tests.rs"]
mod tests;
