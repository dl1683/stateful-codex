//! Project-scoped Stateful Codex integration.

mod autonomy;
mod events;
mod root_blackboard;
mod run_world_state;
mod services;
mod socratic;
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
use crate::run_world_state::RunWorldStateStatus;
use crate::run_world_state::run_world_state_section;
use crate::services::ProjectIntelligenceServices;
use crate::world_state::ProjectIntelligenceStatus;
use crate::world_state::project_world_state_section;

pub use autonomy::AutonomousContinuation;
pub use autonomy::AutonomousContinuationFuture;
pub use autonomy::AutonomousContinuationRequest;
pub use autonomy::AutonomousContinuationSink;
pub use events::BlackboardEntityKind;
pub use events::StatefulEvent;
pub use events::StatefulEventSink;

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

#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectedThread {
    thread_id: String,
}

impl SelectedThread {
    fn new(thread_id: impl Into<String>) -> Self {
        Self {
            thread_id: thread_id.into(),
        }
    }
}

struct StatefulExtension {
    projects: Arc<dyn ThreadStore>,
    services: Option<ProjectIntelligenceServices>,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
    autonomous: Option<AutonomousContinuation>,
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
            let mut sections = vec![project_world_state_section(status)];
            if let Some(run_status) = self
                .run_world_state(selected.project_id(), &input.thread_id.to_string())
                .await
            {
                sections.push(run_world_state_section(run_status));
            }
            sections
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

    async fn run_world_state(
        &self,
        project_id: &str,
        thread_id: &str,
    ) -> Option<RunWorldStateStatus> {
        let services = self.services.as_ref()?;
        let store = match services.runtime().await {
            Ok(store) => store,
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to open Stateful run store");
                return Some(RunWorldStateStatus::Unavailable {
                    project_id: project_id.to_string(),
                });
            }
        };
        let run = match store.run_for_thread(thread_id).await {
            Ok(Some(run)) => run,
            Ok(None) => return None,
            Err(error) => {
                tracing::warn!(%project_id, %thread_id, %error, "failed to load Stateful run");
                return Some(RunWorldStateStatus::Unavailable {
                    project_id: project_id.to_string(),
                });
            }
        };
        if run.value.project_id != project_id {
            tracing::warn!(
                selected_project_id = project_id,
                run_project_id = run.value.project_id,
                run_id = %run.id,
                "active Stateful run does not belong to selected project"
            );
            return Some(RunWorldStateStatus::Unavailable {
                project_id: project_id.to_string(),
            });
        }
        let obligation = match store.latest_obligation(&run.id).await {
            Ok(obligation) => obligation,
            Err(error) => {
                tracing::warn!(run_id = %run.id, %error, "failed to load current obligation");
                return Some(RunWorldStateStatus::Unavailable {
                    project_id: project_id.to_string(),
                });
            }
        };
        let steering = match store
            .list_steering(&run.id, /*after*/ None, /*max_results*/ 100)
            .await
        {
            Ok(steering) => steering
                .into_iter()
                .filter(|instruction| {
                    matches!(
                        instruction.status,
                        codex_stateful_runtime::SteeringStatus::Submitted
                            | codex_stateful_runtime::SteeringStatus::Acknowledged
                    )
                })
                .collect(),
            Err(error) => {
                tracing::warn!(run_id = %run.id, %error, "failed to load pending steering");
                return Some(RunWorldStateStatus::Unavailable {
                    project_id: project_id.to_string(),
                });
            }
        };
        Some(RunWorldStateStatus::Available {
            run,
            obligation: obligation.map(Box::new),
            steering,
        })
    }
}

impl ToolContributor for StatefulExtension {
    fn tools(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>> {
        let (Some(selected), Some(thread), Some(services)) = (
            thread_store.get::<SelectedProject>(),
            thread_store.get::<SelectedThread>(),
            self.services.as_ref(),
        ) else {
            return Vec::new();
        };
        tools::project_intelligence_tools(
            selected.project_id().to_string(),
            thread.thread_id.clone(),
            services.clone(),
            self.projects.clone(),
            self.event_sink.clone(),
        )
    }
}

/// Installs project-scoped Stateful context into the Codex extension registry.
pub fn install<C: Sync>(
    registry: &mut ExtensionRegistryBuilder<C>,
    projects: Arc<dyn ThreadStore>,
    sqlite: Option<SqliteConfig>,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
    autonomous: Option<AutonomousContinuation>,
) {
    let extension = Arc::new(StatefulExtension {
        projects,
        services: sqlite.map(ProjectIntelligenceServices::new),
        event_sink,
        autonomous,
    });
    registry.prompt_contributor(extension.clone());
    registry.tool_contributor(extension.clone());
    registry.tool_policy_contributor(extension.clone());
    registry.thread_lifecycle_contributor(extension);
}
