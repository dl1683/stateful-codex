//! Project-scoped Stateful Codex integration.

mod autonomy;
mod completion;
mod events;
mod outcome_world_state;
mod root_blackboard;
mod run_world_state;
mod services;
mod socratic;
mod source_freshness;
mod tools;
mod world_state;

use std::sync::Arc;

use codex_extension_api::ContextContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionDataInit;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::PromptCacheAffinity;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolContributor;
use codex_extension_api::ToolExecutor;
use codex_extension_api::WorldStateContributionInput;
use codex_extension_api::WorldStateSectionContribution;
use codex_project_intelligence::RootBlackboardQuery;
use codex_state::SqliteConfig;
use codex_thread_store::ThreadStore;

use crate::outcome_world_state::ProjectOutcomesStatus;
use crate::outcome_world_state::project_outcomes_world_state_section;
use crate::root_blackboard::ResolvedRootBlackboard;
use crate::root_blackboard::RootBlackboardStatus;
use crate::run_world_state::RunWorldStateStatus;
use crate::run_world_state::run_world_state_section;
use crate::services::ProjectIntelligenceServices;
use crate::source_freshness::RootEvidenceAudit;
use crate::source_freshness::audit_root_evidence;
use crate::source_freshness::root_evidence_audit_cache_key;
use crate::world_state::ProjectIntelligenceStatus;
use crate::world_state::project_world_state_section;

pub use autonomy::AutonomousContinuation;
pub use autonomy::AutonomousContinuationFuture;
pub use autonomy::AutonomousContinuationOutcome;
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

    /// Installs project selection before a thread runtime is created.
    pub fn insert_initial(data: &mut ExtensionDataInit, project_id: impl Into<String>) {
        let selected = Self::new(project_id);
        data.insert(PromptCacheAffinity::new(selected.prompt_cache_key()));
        data.insert(selected);
    }

    /// Updates project selection for an existing thread runtime.
    pub fn insert(data: &ExtensionData, project_id: impl Into<String>) {
        let selected = Self::new(project_id);
        data.get_or_init(PromptCacheAffinity::default)
            .set(selected.prompt_cache_key());
        data.insert(selected);
    }

    /// Removes project selection and returns cache routing to thread scope.
    pub fn remove(data: &ExtensionData) {
        data.remove::<Self>();
        data.get_or_init(PromptCacheAffinity::default).clear();
    }

    fn prompt_cache_key(&self) -> String {
        format!("stateful-project:{}", self.project_id)
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
                    let root_blackboard = self.root_blackboard(&project, input.turn_store).await;
                    ProjectIntelligenceStatus::Available {
                        project: Box::new(project),
                        root_blackboard: Box::new(root_blackboard),
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
            if let Some(outcomes) = self.project_outcomes(selected.project_id()).await {
                sections.push(project_outcomes_world_state_section(outcomes));
            }
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
    async fn project_outcomes(&self, project_id: &str) -> Option<ProjectOutcomesStatus> {
        const MAX_OUTCOMES: usize = 5;

        let services = self.services.as_ref()?;
        let store = match services.runtime().await {
            Ok(store) => store,
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to open Stateful outcome store");
                return Some(ProjectOutcomesStatus::Unavailable {
                    project_id: project_id.to_string(),
                });
            }
        };
        let mut outcomes = match store
            .recent_completed_outcomes(project_id, (MAX_OUTCOMES + 1) as u32)
            .await
        {
            Ok(outcomes) => outcomes,
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to load recent Stateful outcomes");
                return Some(ProjectOutcomesStatus::Unavailable {
                    project_id: project_id.to_string(),
                });
            }
        };
        if outcomes.is_empty() {
            return None;
        }
        let has_more = outcomes.len() > MAX_OUTCOMES;
        outcomes.truncate(MAX_OUTCOMES);
        Some(ProjectOutcomesStatus::Available {
            project_id: project_id.to_string(),
            outcomes,
            has_more,
        })
    }

    async fn root_blackboard(
        &self,
        project: &codex_thread_store::StoredProject,
        turn_store: &ExtensionData,
    ) -> RootBlackboardStatus {
        let project_id = &project.id;
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
            Ok(projection) => {
                let context_map = match services.context_map().await {
                    Ok(context_map) => context_map,
                    Err(error) => {
                        tracing::warn!(%project_id, %error, "failed to resolve root evidence routes");
                        return RootBlackboardStatus::Available(ResolvedRootBlackboard {
                            projection,
                            evidence_routes: Default::default(),
                            evidence_audit: Some(RootEvidenceAudit {
                                project_id: project_id.to_string(),
                                statuses: Default::default(),
                                cache_key: None,
                            }),
                        });
                    }
                };
                let mut evidence_routes = std::collections::HashMap::new();
                let mut complete_evidence_routes = true;
                for evidence in projection
                    .data
                    .iter()
                    .flat_map(|hit| &hit.entry.value.evidence)
                {
                    if evidence_routes.contains_key(&evidence.context_map_entry_id) {
                        continue;
                    }
                    match context_map
                        .get_hit(project_id, &evidence.context_map_entry_id)
                        .await
                    {
                        Ok(Some(hit)) => {
                            evidence_routes.insert(evidence.context_map_entry_id.clone(), hit);
                        }
                        Ok(None) => complete_evidence_routes = false,
                        Err(error) => {
                            complete_evidence_routes = false;
                            tracing::warn!(
                                %project_id,
                                context_map_entry_id = %evidence.context_map_entry_id,
                                %error,
                                "failed to resolve a root evidence route"
                            );
                        }
                    }
                }
                let evidence_ids = projection
                    .data
                    .iter()
                    .flat_map(|hit| &hit.entry.value.evidence)
                    .map(|evidence| evidence.context_map_entry_id.clone())
                    .collect::<Vec<_>>();
                let roots = project
                    .roots
                    .iter()
                    .map(|root| std::path::PathBuf::from(&root.path))
                    .collect::<Vec<_>>();
                let audit_cache_key = if complete_evidence_routes {
                    root_evidence_audit_cache_key(&roots, &evidence_routes).await
                } else {
                    None
                };
                let (evidence_audit, audit_recomputed) = match turn_store.get::<RootEvidenceAudit>()
                {
                    Some(audit)
                        if audit.project_id == *project_id
                            && audit_cache_key.is_some()
                            && audit.cache_key.as_ref() == audit_cache_key.as_ref() =>
                    {
                        (audit, false)
                    }
                    _ => {
                        let audit = Arc::new(
                            audit_root_evidence(
                                services,
                                project_id,
                                &roots,
                                evidence_ids,
                                audit_cache_key,
                            )
                            .await,
                        );
                        turn_store.insert((*audit).clone());
                        (audit, true)
                    }
                };
                let projection = if audit_recomputed {
                    match store
                        .root_projection(RootBlackboardQuery {
                            project_id: project_id.to_string(),
                            max_entries: 256,
                        })
                        .await
                    {
                        Ok(projection) => projection,
                        Err(error) => {
                            tracing::warn!(
                                %project_id,
                                %error,
                                "failed to reload Stateful root blackboard after source audit"
                            );
                            return RootBlackboardStatus::Unavailable;
                        }
                    }
                } else {
                    projection
                };
                RootBlackboardStatus::Available(ResolvedRootBlackboard {
                    projection,
                    evidence_routes,
                    evidence_audit: Some((*evidence_audit).clone()),
                })
            }
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
        let steering_records = match store
            .list_steering(&run.id, /*after*/ None, /*max_results*/ 100)
            .await
        {
            Ok(steering) => steering,
            Err(error) => {
                tracing::warn!(run_id = %run.id, %error, "failed to load pending steering");
                return Some(RunWorldStateStatus::Unavailable {
                    project_id: project_id.to_string(),
                });
            }
        };
        let steering_complete = steering_records.len() < 100;
        let steering = steering_records
            .into_iter()
            .filter(|instruction| {
                matches!(
                    instruction.status,
                    codex_stateful_runtime::SteeringStatus::Submitted
                        | codex_stateful_runtime::SteeringStatus::Acknowledged
                )
            })
            .collect();
        Some(RunWorldStateStatus::Available {
            run: Box::new(run),
            obligation: obligation.map(Box::new),
            steering,
            steering_complete,
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
    registry.turn_lifecycle_contributor(extension.clone());
    registry.thread_lifecycle_contributor(extension);
}

#[cfg(test)]
#[path = "freshness_tests.rs"]
mod freshness_tests;
