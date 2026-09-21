//! Project-scoped Stateful Codex integration.

mod root_blackboard;
mod services;
mod tools;
mod world_state;

use std::sync::Arc;

use codex_extension_api::ContextContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolContributor;
use codex_extension_api::ToolExecutor;
use codex_extension_api::WorldStateContributionInput;
use codex_extension_api::WorldStateSectionContribution;
use codex_project_intelligence::RootBlackboardQuery;
use codex_state::SqliteConfig;
use codex_thread_store::ThreadStore;

use crate::root_blackboard::RootBlackboardStatus;
use crate::services::ProjectIntelligenceServices;
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
    services: Option<ProjectIntelligenceServices>,
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
                Ok(Some(project)) => {
                    let root_blackboard = self.root_blackboard(&project.id).await;
                    ProjectIntelligenceStatus::Available {
                        project,
                        root_blackboard,
                    }
                }
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

impl StatefulExtension {
    async fn root_blackboard(&self, project_id: &str) -> RootBlackboardStatus {
        let Some(services) = self.services.as_ref() else {
            return RootBlackboardStatus::NotConfigured;
        };
        let store = match services.blackboard().await {
            Ok(store) => store,
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to open Stateful blackboard");
                return RootBlackboardStatus::Unavailable;
            }
        };
        match store
            .root_projection(RootBlackboardQuery {
                project_id: project_id.to_string(),
                max_entries: 256,
            })
            .await
        {
            Ok(projection) => RootBlackboardStatus::Available(projection),
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to load Stateful root blackboard");
                RootBlackboardStatus::Unavailable
            }
        }
    }
}

impl ToolContributor for StatefulExtension {
    fn tools(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>> {
        let (Some(selected), Some(services)) = (
            thread_store.get::<SelectedProject>(),
            self.services.as_ref(),
        ) else {
            return Vec::new();
        };
        tools::project_intelligence_tools(
            selected.project_id().to_string(),
            services.clone(),
            self.projects.clone(),
        )
    }
}

/// Installs project-scoped Stateful context into the Codex extension registry.
pub fn install<C: Sync>(
    registry: &mut ExtensionRegistryBuilder<C>,
    projects: Arc<dyn ThreadStore>,
    sqlite: Option<SqliteConfig>,
) {
    let extension = Arc::new(StatefulExtension {
        projects,
        services: sqlite.map(ProjectIntelligenceServices::new),
    });
    registry.prompt_contributor(extension.clone());
    registry.tool_contributor(extension);
}
