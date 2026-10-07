//! Hold the authoritative thread database's writer admission through a memory operation.
//! Project intelligence has its own database; keeping this guard alive prevents another
//! server from committing a rebind/unlink between validation and the PI commit.

use codex_app_server_protocol::JSONRPCErrorError;
use codex_state::ThreadProjectAdmission;

use super::BlackboardRequestProcessor;
use crate::error_code::internal_error;
use crate::error_code::invalid_params;

impl BlackboardRequestProcessor {
    pub(super) async fn admit_memory(
        &self,
        thread_id: &str,
        expected_project_id: &str,
    ) -> Result<ThreadProjectAdmission, JSONRPCErrorError> {
        let thread_id = codex_protocol::ThreadId::from_string(thread_id)
            .map_err(|_| invalid_params("threadId must be a valid thread ID"))?;
        let sqlite = self
            .sqlite
            .as_ref()
            .ok_or_else(|| invalid_params("memory needs persisted thread metadata"))?;
        // Memory never flushes staged bindings. Only an already durable authoritative row
        // can admit a control; otherwise a stale pending patch could overwrite a rebind.
        ThreadProjectAdmission::acquire(sqlite, thread_id, expected_project_id).await
            .map_err(|error| internal_error(error.to_string()))?
            .ok_or_else(|| invalid_params("project binding changed or is not persisted; refresh the thread before using memory"))
    }
}
