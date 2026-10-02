//! `stateful_run_read`: exact, paged reads of run state that World State may shorten.
//!
//! The run packet bounds the goal, strategy and current obligation, and none of them is
//! guaranteed to survive in retained thread history. This tool returns the stored text
//! exactly, one serialized-size-bounded page at a time. A cursor binds the section, the
//! run, the paged text's identity and length, and a UTF-8 boundary offset, so a stale or
//! foreign cursor is rejected instead of silently restarting.

use codex_extension_api::FunctionCallError;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExposure;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_stateful_runtime::StatefulObligation;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use crate::root_blackboard::short_digest;
use crate::services::ProjectIntelligenceServices;

use super::MAX_RESPONSE_BYTES;
use super::bounded_json_output;
use super::parse_arguments;
use super::respond;
use super::thread_run;

const TOOL_NAME: &str = "stateful_run_read";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum Section {
    Goal,
    Strategy,
    Obligation,
    SubmittedResult,
}

impl Section {
    fn name(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Strategy => "strategy",
            Self::Obligation => "obligation",
            Self::SubmittedResult => "submittedResult",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "goal" => Some(Self::Goal),
            "strategy" => Some(Self::Strategy),
            "obligation" => Some(Self::Obligation),
            "submittedResult" => Some(Self::SubmittedResult),
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
    length: usize,
    offset: usize,
}

/// Cursor format version; a cursor from any other version is rejected.
const CURSOR_VERSION: &str = "v1";
/// Hex length of `short_digest`.
const DIGEST_HEX_LEN: usize = 32;

impl ReadCursor {
    /// `v1.<section>.<run id>.<digest>.<length>.<offset>`; lengths and offsets are UTF-8
    /// byte counts. The run ID may itself contain dots, so it is the middle remainder.
    fn encode(&self) -> String {
        format!(
            "{CURSOR_VERSION}.{}.{}.{}.{}.{}",
            self.section.name(),
            self.run_id,
            self.digest,
            self.length,
            self.offset
        )
    }

    fn decode(value: &str) -> Option<Self> {
        let rest = value.strip_prefix(CURSOR_VERSION)?.strip_prefix('.')?;
        let (rest, offset) = rest.rsplit_once('.')?;
        let (rest, length) = rest.rsplit_once('.')?;
        let (rest, digest) = rest.rsplit_once('.')?;
        let (section, run_id) = rest.split_once('.')?;
        let digest_is_valid = digest.len() == DIGEST_HEX_LEN
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !digest_is_valid || run_id.is_empty() {
            return None;
        }
        Some(Self {
            section: Section::parse(section)?,
            run_id: run_id.to_string(),
            digest: digest.to_string(),
            length: length.parse().ok()?,
            offset: offset.parse().ok()?,
        })
    }
}

/// The exact text a section pages, and the identity its cursors are bound to.
struct SectionText {
    identity: String,
    text: String,
}

impl SectionText {
    fn digest(&self) -> String {
        short_digest(&format!("{}\0{}", self.identity, self.text))
    }

    fn cursor(&self, section: Section, run: &StatefulRun, offset: usize) -> String {
        ReadCursor {
            section,
            run_id: run.id.to_string(),
            digest: self.digest(),
            length: self.text.len(),
            offset,
        }
        .encode()
    }
}

fn goal_text(run: &StatefulRun) -> SectionText {
    SectionText {
        identity: "goal".to_string(),
        text: run.value.goal.clone(),
    }
}

fn strategy_text(run: &StatefulRun) -> Option<SectionText> {
    Some(SectionText {
        identity: format!("strategy@{}", run.strategy_revision),
        text: run.strategy.clone()?,
    })
}

/// Canonical JSON of the packet: unambiguous, so distinct packets never page as the
/// same text.
fn obligation_text(obligation: &StatefulObligation) -> Result<SectionText, FunctionCallError> {
    Ok(SectionText {
        identity: format!("obligation:{}@{}", obligation.id, obligation.revision),
        text: serde_json::to_string(&obligation.value.packet).map_err(respond)?,
    })
}

/// The first `length` bytes of a completed run's stored result: the result the model
/// submitted, without any suffix completion appended for durability.
fn submitted_result_text(run: &StatefulRun, length: usize) -> Option<SectionText> {
    if run.status != StatefulRunStatus::Completed {
        return None;
    }
    let stored = run.result.as_deref()?;
    Some(SectionText {
        identity: format!("submittedResult@{}", run.revision),
        text: stored.get(..length)?.to_string(),
    })
}

/// First-page cursor for a run's final obligation, readable after the run is terminal.
pub(super) fn obligation_cursor(
    run: &StatefulRun,
    obligation: &StatefulObligation,
) -> Result<String, FunctionCallError> {
    Ok(obligation_text(obligation)?.cursor(Section::Obligation, run, 0))
}

/// First-page cursor for the submitted result of a just-completed run, for a completion
/// response too large to carry the result itself.
pub(super) fn submitted_result_cursor(run: &StatefulRun, submitted: &str) -> Option<String> {
    let section = submitted_result_text(run, submitted.len())?;
    (section.text == submitted).then(|| section.cursor(Section::SubmittedResult, run, 0))
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
        let section = arguments.section;
        let cursor = arguments
            .cursor
            .as_deref()
            .map(|cursor| {
                ReadCursor::decode(cursor)
                    .filter(|cursor| cursor.section == section)
                    .ok_or_else(|| {
                        FunctionCallError::RespondToModel(format!(
                            "cursor is not a {} cursor from stateful_run_read; omit it to start from the beginning",
                            section.name()
                        ))
                    })
            })
            .transpose()?;
        let run = match &cursor {
            Some(cursor) => self.cursor_run(&cursor.run_id).await?,
            None if section == Section::SubmittedResult => {
                return Err(FunctionCallError::RespondToModel(
                    "submittedResult is read only with the cursor returned by the completing stateful_run_update".to_string(),
                ));
            }
            None => thread_run(&self.project_id, &self.thread_id, &self.services).await?,
        };
        let stale = || {
            FunctionCallError::RespondToModel(format!(
                "the run {} changed after this cursor was issued; read it again without a cursor",
                section.name()
            ))
        };
        let paged = match section {
            Section::Goal => goal_text(&run),
            Section::Strategy => strategy_text(&run).ok_or_else(|| {
                FunctionCallError::RespondToModel("this run has no strategy yet".to_string())
            })?,
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
                obligation_text(&obligation)?
            }
            Section::SubmittedResult => cursor
                .as_ref()
                .and_then(|cursor| submitted_result_text(&run, cursor.length))
                .ok_or_else(stale)?,
        };
        let offset = match &cursor {
            Some(cursor)
                if cursor.digest != paged.digest() || cursor.length != paged.text.len() =>
            {
                return Err(stale());
            }
            Some(cursor)
                if cursor.offset > paged.text.len()
                    || !paged.text.is_char_boundary(cursor.offset) =>
            {
                return Err(FunctionCallError::RespondToModel(
                    "cursor offset is not a valid position in this text".to_string(),
                ));
            }
            Some(cursor) => cursor.offset,
            None => 0,
        };
        let page = read_page(
            &paged.text,
            offset,
            call.response_byte_budget(MAX_RESPONSE_BYTES),
            |end| (end < paged.text.len()).then(|| paged.cursor(section, &run, end)),
            |content, next_cursor| {
                json!({
                    "section": section.name(),
                    "runId": run.id.as_str(),
                    "totalBytes": paged.text.len(),
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
        bounded_json_output(&call, page)
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

/// Returns the largest page starting at `offset` whose serialized envelope fits the
/// budget. A nonterminal page is never empty; `None` means not even one character (or
/// the empty terminal page at the end of the text) fits.
///
/// Serialized size grows with the page end for every page that carries a continuation
/// cursor, so those ends are binary-searched over UTF-8 boundaries. The terminal page
/// swaps the cursor for `null` and can be smaller, so it is checked on its own first.
fn read_page(
    text: &str,
    offset: usize,
    budget: usize,
    next_cursor: impl Fn(usize) -> Option<String>,
    envelope: impl Fn(&str, Option<String>) -> Value,
) -> Option<Value> {
    let fitting = |end: usize| {
        let page = envelope(&text[offset..end], next_cursor(end));
        (page.to_string().len() <= budget).then_some(page)
    };
    if let Some(page) = fitting(text.len()) {
        return Some(page);
    }
    if offset == text.len() {
        return None;
    }
    let floor = |mut index: usize| {
        while !text.is_char_boundary(index) {
            index -= 1;
        }
        index
    };
    let ceil = |mut index: usize| {
        while index < text.len() && !text.is_char_boundary(index) {
            index += 1;
        }
        index
    };
    let mut best = None;
    let mut low = ceil(offset + 1);
    let mut high = floor(text.len() - 1);
    while low <= high && low < text.len() {
        let middle = floor(low + (high - low) / 2).max(low);
        match fitting(middle) {
            Some(page) => {
                best = Some(page);
                low = ceil(middle + 1);
            }
            None if middle == low => break,
            None => high = floor(middle - 1),
        }
    }
    best
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
            description: "Read the exact stored goal, strategy, or current obligation of the selected thread's Stateful run, or a completed run's submitted result with the cursor its completion returned, one bounded page at a time; follow nextCursor until null.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "section": {"type": "string", "enum": ["goal", "strategy", "obligation", "submittedResult"]},
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
