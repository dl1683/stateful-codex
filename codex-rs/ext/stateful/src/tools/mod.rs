mod blackboard;
mod blackboard_write;
mod context_map;

use std::sync::Arc;

use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_thread_store::ThreadStore;
use sha2::Digest;
use sha2::Sha256;

use crate::services::ProjectIntelligenceServices;

const MAX_RESPONSE_BYTES: usize = 16 * 1024;

pub(super) fn project_intelligence_tools(
    project_id: String,
    services: ProjectIntelligenceServices,
    projects: Arc<dyn ThreadStore>,
) -> Vec<Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>> {
    vec![
        Arc::new(blackboard::BlackboardQueryTool::new(
            project_id.clone(),
            services.clone(),
        )),
        Arc::new(blackboard_write::BlackboardRecordTool::new(
            project_id.clone(),
            services.clone(),
        )),
        Arc::new(blackboard_write::BlackboardRelateTool::new(
            project_id.clone(),
            services.clone(),
        )),
        Arc::new(context_map::ContextMapQueryTool::new(
            project_id.clone(),
            services.clone(),
            projects.clone(),
        )),
        Arc::new(context_map::ContextMapRefreshTool::new(
            project_id, services, projects,
        )),
    ]
}

fn parse_arguments<T: for<'de> serde::Deserialize<'de>>(
    call: &ToolCall<'_>,
) -> Result<T, codex_extension_api::FunctionCallError> {
    let arguments = call.function_arguments()?;
    let arguments = if arguments.trim().is_empty() {
        "{}"
    } else {
        arguments
    };
    serde_json::from_str(arguments)
        .map_err(|error| codex_extension_api::FunctionCallError::RespondToModel(error.to_string()))
}

fn fits_response(value: &serde_json::Value, byte_budget: usize) -> bool {
    serde_json::to_vec(value).is_ok_and(|serialized| serialized.len() <= byte_budget)
}

fn stable_id(prefix: &str, project_id: &str, idempotency_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(project_id.as_bytes());
    hasher.update([0]);
    hasher.update(idempotency_key.as_bytes());
    let digest = hasher.finalize();
    format!("stateful-{prefix}-{digest:x}")
}
