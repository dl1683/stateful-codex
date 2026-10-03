//! Whether a stored recipe still applies before it is offered again.
//!
//! A host-observed recipe records the conditions it last succeeded under. Before the
//! packet offers it, cheap local checks confirm those conditions: the working directory
//! and executable still exist and the dependency manifests are unchanged. A failed check
//! marks the recipe `needsCheck` with the reason, so the model discovers the environment
//! once instead of trusting a dead interpreter or creating a second environment. A
//! `Recipe:` entry the host never observed is shown as `notObserved`. Nothing here runs
//! the recipe.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::RootBlackboardProjection;

use crate::recipe_capture::ExitStatus;
use crate::recipe_capture::RecipeObservation;
use crate::recipe_capture::manifest_fingerprints;

/// Recipes checked per packet; others are shown without a check.
const MAX_CHECKED_RECIPES: usize = 16;
/// Shell words that are not executables on `PATH`.
const SHELL_WORDS: &[&str] = &[
    "cd",
    "set",
    "export",
    "env",
    "source",
    ".",
    "call",
    "pushd",
    "set-location",
    "sl",
];

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
        }
    }
}

/// Whether a recipe entry's content marks it as a recipe.
pub(crate) fn is_recipe(kind: BlackboardKind, content: &str) -> bool {
    kind == BlackboardKind::Fact && content.starts_with("Recipe:")
}

/// Checks the recipe entries shown in the root, by entry ID.
pub(crate) async fn check_root_recipes(
    store: &BlackboardStore,
    project_id: &str,
    projection: &RootBlackboardProjection,
) -> HashMap<String, RecipeCheck> {
    let ids = projection
        .data
        .iter()
        .filter(|hit| is_recipe(hit.entry.value.kind, &hit.entry.value.content))
        .take(MAX_CHECKED_RECIPES)
        .map(|hit| hit.entry.id.clone())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return HashMap::new();
    }
    let contexts = match store.knowledge_contexts(project_id, &ids).await {
        Ok(contexts) => contexts,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to load recipe observations");
            return HashMap::new();
        }
    };
    let observations = ids
        .iter()
        .map(|id| {
            let observation = contexts
                .get(id.as_str())
                .filter(|context| {
                    context.category == KnowledgeCategory::Recipe
                        && context.authority == KnowledgeAuthority::HostObserved
                })
                .and_then(|context| context.payload.as_deref())
                .and_then(|payload| serde_json::from_str::<RecipeObservation>(payload).ok());
            (id.to_string(), observation)
        })
        .collect::<Vec<_>>();
    tokio::task::spawn_blocking(move || {
        observations
            .into_iter()
            .map(|(id, observation)| {
                let check = observation.map_or(RecipeCheck::NotObserved, |observation| {
                    check_observation(&observation)
                });
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
    if !executable_exists(&cwd, &observation.executable) {
        return RecipeCheck::NeedsCheck(format!(
            "executable {} no longer resolves",
            observation.executable
        ));
    }
    let current = manifest_fingerprints(&cwd);
    for recorded in &observation.manifests {
        match current
            .iter()
            .find(|manifest| manifest.path == recorded.path)
        {
            Some(manifest) if manifest.sha256 == recorded.sha256 => {}
            Some(_) => {
                return RecipeCheck::NeedsCheck(format!("{} changed since", recorded.path));
            }
            None => {
                return RecipeCheck::NeedsCheck(format!("{} was removed", recorded.path));
            }
        }
    }
    match observation.exit_status {
        ExitStatus::Zero => RecipeCheck::Current,
        ExitStatus::NonZero | ExitStatus::Unknown => RecipeCheck::CurrentExitUnconfirmed,
    }
}

/// Whether the executable a recipe starts with still resolves: a path relative to the
/// working directory, or a bare name on `PATH`. An empty or shell-builtin word is not
/// checked.
fn executable_exists(cwd: &Path, executable: &str) -> bool {
    if executable.is_empty()
        || executable.starts_with('$')
        || executable.contains('=')
        || SHELL_WORDS.contains(&executable.to_ascii_lowercase().as_str())
    {
        return true;
    }
    let candidates = |base: PathBuf| {
        let mut names = vec![base.clone()];
        if cfg!(windows) && base.extension().is_none() {
            names.extend(["exe", "cmd", "bat"].map(|extension| base.with_extension(extension)));
        }
        names
    };
    if executable.contains(['/', '\\']) {
        return candidates(cwd.join(executable))
            .iter()
            .any(|path| path.is_file());
    }
    let Some(path) = std::env::var_os("PATH") else {
        return true;
    };
    std::env::split_paths(&path).any(|directory| {
        candidates(directory.join(executable))
            .iter()
            .any(|path| path.is_file())
    })
}

#[cfg(test)]
#[path = "recipe_applicability_tests.rs"]
mod tests;
