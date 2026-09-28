//! `stateful_run_read`: exact, paged reads of run state that World State may shorten.
//!
//! The run packet bounds the goal and the current obligation, and neither is guaranteed
//! to survive in retained thread history. This tool returns the stored text exactly,
//! one serialized-size-bounded page at a time. A cursor binds the run, the section, a
//! digest of the text it pages, and a UTF-8 boundary offset, so a stale or foreign cursor
//! is rejected instead of silently restarting.

use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExposure;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_stateful_runtime::ObligationPacket;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunId;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use crate::root_blackboard::short_digest;
use crate::services::ProjectIntelligenceServices;

use super::parse_arguments;
use super::respond;
use super::thread_run;

const TOOL_NAME: &str = "stateful_run_read";
/// Serialized response bound: one UTF-8 byte is the worst case per token, so this keeps
/// every page under the 10K-token item limit with room for framing.
const MAX_READ_RESPONSE_BYTES: usize = 9_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum Section {
    Goal,
    Obligation,
}

impl Section {
    fn name(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Obligation => "obligation",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "goal" => Some(Self::Goal),
            "obligation" => Some(Self::Obligation),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Arguments {
    section: Section,
    cursor: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct ReadCursor {
    section: Section,
    run_id: String,
    digest: String,
    offset: usize,
}

impl ReadCursor {
    fn encode(&self) -> String {
        format!(
            "{}.{}.{}.{}",
            self.section.name(),
            self.run_id,
            self.digest,
            self.offset
        )
    }

    fn decode(value: &str) -> Option<Self> {
        let (rest, offset) = value.rsplit_once('.')?;
        let (rest, digest) = rest.rsplit_once('.')?;
        let (section, run_id) = rest.split_once('.')?;
        Some(Self {
            section: Section::parse(section)?,
            run_id: run_id.to_string(),
            digest: digest.to_string(),
            offset: offset.parse().ok()?,
        })
    }
}

pub(super) struct StatefulRunReadTool {
    project_id: String,
    thread_id: String,
    services: ProjectIntelligenceServices,
}

impl StatefulRunReadTool {
    pub(super) fn new(
        project_id: String,
        thread_id: String,
        services: ProjectIntelligenceServices,
    ) -> Self {
        Self {
            project_id,
            thread_id,
            services,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let arguments: Arguments = parse_arguments(&call)?;
        let cursor = arguments
            .cursor
            .as_deref()
            .map(|cursor| {
                ReadCursor::decode(cursor)
                    .filter(|cursor| cursor.section == arguments.section)
                    .ok_or_else(|| {
                        FunctionCallError::RespondToModel(format!(
                            "cursor is not a {} cursor from stateful_run_read; omit it to start from the beginning",
                            arguments.section.name()
                        ))
                    })
            })
            .transpose()?;
        let run = match &cursor {
            Some(cursor) => self.cursor_run(&cursor.run_id).await?,
            None => thread_run(&self.project_id, &self.thread_id, &self.services).await?,
        };
        let text = match arguments.section {
            Section::Goal => run.value.goal.clone(),
            Section::Obligation => {
                let obligation = self
                    .services
                    .runtime()
                    .await
                    .map_err(respond)?
                    .latest_obligation(&run.id)
                    .await
                    .map_err(respond)?
                    .ok_or_else(|| {
                        FunctionCallError::RespondToModel(
                            "this run has no recorded obligation yet".to_string(),
                        )
                    })?;
                obligation_text(&obligation.value.packet)
            }
        };
        let digest = short_digest(&text);
        let offset = match cursor {
            Some(cursor) if cursor.digest != digest => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "the run {} changed after this cursor was issued; read it again without a cursor",
                    arguments.section.name()
                )));
            }
            Some(cursor) if cursor.offset > text.len() || !text.is_char_boundary(cursor.offset) => {
                return Err(FunctionCallError::RespondToModel(
                    "cursor offset is not a valid position in this text".to_string(),
                ));
            }
            Some(cursor) => cursor.offset,
            None => 0,
        };
        let page = read_page(
            &text,
            offset,
            call.response_byte_budget(MAX_READ_RESPONSE_BYTES),
            |end| {
                (end < text.len()).then(|| {
                    ReadCursor {
                        section: arguments.section,
                        run_id: run.id.to_string(),
                        digest: digest.clone(),
                        offset: end,
                    }
                    .encode()
                })
            },
            |content, next_cursor| {
                json!({
                    "section": arguments.section.name(),
                    "runId": run.id.as_str(),
                    "totalBytes": text.len(),
                    "offset": offset,
                    "content": content,
                    "nextCursor": next_cursor,
                })
            },
        )
        .ok_or_else(|| {
            FunctionCallError::RespondToModel(
                "the response budget is too small to return any of this text".to_string(),
            )
        })?;
        Ok(Box::new(JsonToolOutput::new(page)))
    }

    /// Resolves a cursor's run, which must belong to this project and thread.
    async fn cursor_run(&self, run_id: &str) -> Result<StatefulRun, FunctionCallError> {
        let foreign = || {
            FunctionCallError::RespondToModel(
                "cursor does not belong to a run of the selected thread".to_string(),
            )
        };
        let run_id = StatefulRunId::parse(run_id).map_err(|_| foreign())?;
        let run = self
            .services
            .runtime()
            .await
            .map_err(respond)?
            .get_run(&run_id)
            .await
            .map_err(respond)?
            .ok_or_else(foreign)?;
        if run.value.project_id != self.project_id
            || !run.value.thread_ids.contains(&self.thread_id)
        {
            return Err(foreign());
        }
        Ok(run)
    }
}

/// Canonical text of an obligation packet: one `Label: value` line per item, values
/// exact (including any interior newlines).
fn obligation_text(packet: &ObligationPacket) -> String {
    let mut text = String::new();
    for (label, values) in [
        ("Examined", &packet.examined),
        ("Why it matters", &packet.rationale),
        ("Learned", &packet.learning),
        ("Implication", &packet.implication),
        ("Strategy", &packet.strategy),
        ("Changed", &packet.changed),
        ("Next", &packet.next),
        ("Uncertainty", &packet.uncertainty),
        ("Blockers", &packet.blockers),
        ("Useful user judgment", &packet.requested_judgment),
    ] {
        for value in values {
            text.push_str(&format!("{label}: {value}\n"));
        }
    }
    text
}

/// Returns the largest page starting at `offset` whose serialized envelope fits the
/// budget, or `None` when not even an empty page with a continuation fits.
fn read_page(
    text: &str,
    offset: usize,
    budget: usize,
    next_cursor: impl Fn(usize) -> Option<String>,
    envelope: impl Fn(&str, Option<String>) -> Value,
) -> Option<Value> {
    let mut end = text.len();
    loop {
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let page = envelope(&text[offset..end], next_cursor(end));
        let size = page.to_string().len();
        if size <= budget {
            return (end > offset || offset == text.len()).then_some(page);
        }
        if end == offset {
            return None;
        }
        // Escaping never makes a byte serialize smaller, so dropping the excess in raw
        // bytes always makes progress toward the budget.
        end = end.saturating_sub(size - budget).max(offset);
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for StatefulRunReadTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn exposure(&self) -> ToolExposure {
        ToolExposure::DirectModelOnly
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "Read the exact stored goal or current semantic obligation of the selected thread's Stateful run, one bounded page at a time. Use it when <stateful_run> says the goal or obligation was shortened, or when earlier detail may no longer be in context. Follow nextCursor until it is null to read the whole text.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "section": {"type": "string", "enum": ["goal", "obligation"]},
                    "cursor": {"type": "string"}
                },
                "required": ["section"],
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static run read schema: {error}")),
            output_schema: None,
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn handle<'a>(&'a self, call: ToolCall<'call>) -> codex_extension_api::ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(self.handle_call(call))
    }
}

#[cfg(test)]
#[path = "run_read_tests.rs"]
mod tests;
