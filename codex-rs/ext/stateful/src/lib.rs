//! Project-scoped Stateful Codex integration.

mod world_state;

use std::sync::Arc;

use codex_extension_api::ContextContributor;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::WorldStateContributionInput;
use codex_extension_api::WorldStateSectionContribution;
use codex_thread_store::ThreadStore;

use crate::world_state::ProjectIntelligenceStatus;
use crate::world_state::project_world_state_section;

/// Canonical project selected by the user for a thread view.
///
/// The attachment stores only the durable project ID. Current project metadata
/// and roots are resolved from the host's project store at each sampling step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedProject {
    project_id: String,
}

impl SelectedProject {
    pub fn new(project_id: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
        }
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

struct StatefulExtension {
    projects: Arc<dyn ThreadStore>,
}

impl ContextContributor for StatefulExtension {
    fn contribute_world_state<'a>(
        &'a self,
        input: WorldStateContributionInput<'a>,
    ) -> ExtensionFuture<'a, Vec<WorldStateSectionContribution>> {
        Box::pin(async move {
            let Some(selected) = input.thread_store.get::<SelectedProject>() else {
                return Vec::new();
            };
            let status = match self
                .projects
                .read_project(selected.project_id().to_string())
                .await
            {
                Ok(Some(project)) => ProjectIntelligenceStatus::Available(project),
                Ok(None) => ProjectIntelligenceStatus::Missing {
                    project_id: selected.project_id().to_string(),
                },
                Err(error) => {
                    tracing::warn!(
                        project_id = selected.project_id(),
                        %error,
                        "failed to resolve selected Stateful project"
                    );
                    ProjectIntelligenceStatus::Unavailable {
                        project_id: selected.project_id().to_string(),
                    }
                }
            };
            vec![project_world_state_section(status)]
        })
    }
}

/// Installs project-scoped Stateful context into the Codex extension registry.
pub fn install<C: Sync>(
    registry: &mut ExtensionRegistryBuilder<C>,
    projects: Arc<dyn ThreadStore>,
) {
    registry.prompt_contributor(Arc::new(StatefulExtension { projects }));
}
