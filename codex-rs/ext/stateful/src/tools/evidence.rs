use std::fmt::Write;
use std::path::PathBuf;
use std::sync::Arc;

use codex_extension_api::FunctionCallError;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_project_intelligence::BlackboardEvidenceLink;
use codex_project_intelligence::ContextMapEntryId;
use codex_project_intelligence::ContextMapFreshness;
use codex_project_intelligence::EvidenceLineRange;
use codex_project_intelligence::EvidenceReadError;
use codex_project_intelligence::EvidenceReadLocator;
use codex_project_intelligence::EvidenceReadRequest;
use codex_project_intelligence::EvidenceReadResult;
use codex_project_intelligence::EvidenceReader;
use codex_project_intelligence::EvidenceRoute;
use codex_project_intelligence::ProjectIndexFileRequest;
use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::SourceFingerprint;
use codex_thread_store::ThreadStore;
use serde::Deserialize;
use serde_json::json;

use crate::read_receipts::READ_RECEIPT_ID_BYTES;
use crate::services::ProjectIntelligenceServices;

use super::MAX_RESPONSE_BYTES;
use super::bounded_json_output;
use super::fits_response;
use super::parse_arguments;

const TOOL_NAME: &str = "evidence_read";
const DEFAULT_BYTES: u32 = 8 * 1024;
const MAX_BYTES: u32 = 12 * 1024;

/// Longest a read may spend building a never-built project index before it gives up.
const ON_DEMAND_INDEX_BUDGET: std::time::Duration = std::time::Duration::from_secs(20);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EvidenceArguments {
    evidence_route: Option<EvidenceRouteArguments>,
    relative_path: Option<String>,
    project_root: Option<String>,
    line_range: Option<LineRangeArguments>,
    max_bytes: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EvidenceRouteArguments {
    context_map_entry_id: String,
    source_fingerprint: String,
    line_range: Option<LineRangeArguments>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LineRangeArguments {
    start: u64,
    end: u64,
}

pub(super) struct EvidenceReadTool {
    project_id: String,
    thread_id: String,
    services: ProjectIntelligenceServices,
    projects: Arc<dyn ThreadStore>,
}

impl EvidenceReadTool {
    pub(super) fn new(
        project_id: String,
        thread_id: String,
        services: ProjectIntelligenceServices,
        projects: Arc<dyn ThreadStore>,
    ) -> Self {
        Self {
            project_id,
            thread_id,
            services,
            projects,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let arguments: EvidenceArguments = parse_arguments(&call).map_err(|error| {
            call.function_arguments()
                .ok()
                .filter(|arguments| is_route_item_wrapper(arguments))
                .map_or(error, |_| {
                    FunctionCallError::RespondToModel(ROUTE_WRAPPER_DIAGNOSTIC.to_string())
                })
        })?;
        let requested_max_bytes = arguments.max_bytes.unwrap_or(DEFAULT_BYTES);
        if requested_max_bytes == 0 {
            return Err(FunctionCallError::RespondToModel(format!(
                "maxBytes must be between 1 and {MAX_BYTES}"
            )));
        }
        let max_bytes = requested_max_bytes.min(MAX_BYTES);
        let project = self
            .projects
            .read_project(self.project_id.clone())
            .await
            .map_err(respond)?
            .ok_or_else(|| {
                FunctionCallError::RespondToModel("selected project no longer exists".to_string())
            })?;
        let project_roots = project
            .roots
            .iter()
            .map(|root| PathBuf::from(&root.path))
            .collect::<Vec<_>>();
        let locator = match (arguments.evidence_route, arguments.relative_path) {
            (Some(route), None)
                if arguments.project_root.is_none() && arguments.line_range.is_none() =>
            {
                EvidenceReadLocator::ContextMapRoute(EvidenceRoute {
                    context_map_entry_id: ContextMapEntryId::parse(route.context_map_entry_id)
                        .map_err(respond)?,
                    source_fingerprint: SourceFingerprint::parse(route.source_fingerprint)
                        .map_err(respond)?,
                    line_range: route.line_range.map(|range| EvidenceLineRange {
                        start: range.start,
                        end: range.end,
                    }),
                })
            }
            (None, Some(relative_path)) => EvidenceReadLocator::Source {
                project_root: arguments.project_root.map(PathBuf::from),
                relative_path: ProjectRelativePath::parse(relative_path).map_err(respond)?,
                line_range: arguments.line_range.map(|range| EvidenceLineRange {
                    start: range.start,
                    end: range.end,
                }),
            },
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "provide either evidenceRoute unchanged or relativePath with optional source bounds"
                        .to_string(),
                ));
            }
        };
        let (result, source_refreshed) = self
            .read_with_refresh(project_roots, locator, max_bytes)
            .await?;

        let byte_budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let hit = &result.hit;
        let render = |evidence: &NumberedEvidence, receipt_id: Option<&str>| {
            json!({
                "projectId": self.project_id,
                "contextMapEntryId": hit.entry.id.to_string(),
                "sourceFingerprint": hit.entry.value.source_fingerprint.to_string(),
                "source": {
                    "projectRoot": hit.source.project_root,
                    "relativePath": hit.source.relative_path.to_string(),
                },
                "contentFormat": "lineNumbered",
                "content": evidence.content,
                "bytesReturned": evidence.source_bytes,
                "totalBytes": result.total_bytes,
                "totalLines": result.total_lines,
                "firstLine": evidence.first_line,
                "lastLine": evidence.last_line,
                "lastLinePartial": evidence.last_line_partial,
                "truncated": evidence.extent == EvidenceExtent::Truncated,
                "maxBytesApplied": max_bytes,
                "maxBytesClamped": requested_max_bytes != max_bytes,
                "sourceRefreshed": source_refreshed,
                "blackboardEvidence": receipt_id.map(|receipt_id| json!({"readReceiptId": receipt_id})),
                "revision": hit.entry.revision,
            })
        };
        let read_extent = if result.truncated {
            EvidenceExtent::Truncated
        } else {
            EvidenceExtent::Complete
        };
        let receipt_placeholder = "x".repeat(READ_RECEIPT_ID_BYTES);
        let lines = source_lines(&result.content, result.first_line, read_extent);
        let evidence = pack_evidence(&lines, read_extent, |evidence| {
            let receipt_id = evidence
                .receipt_range()
                .map(|_| receipt_placeholder.as_str());
            fits_response(&render(evidence, receipt_id), byte_budget)
        })
        .ok_or_else(|| {
            FunctionCallError::RespondToModel(
                "response budget leaves no room for evidence metadata".to_string(),
            )
        })?;
        let receipt_id = evidence.receipt_range().map(|line_range| {
            self.services.read_receipts().issue(
                &self.project_id,
                &self.thread_id,
                &call.call_id,
                BlackboardEvidenceLink {
                    context_map_entry_id: hit.entry.id.clone(),
                    source_fingerprint: hit.entry.value.source_fingerprint.clone(),
                    line_range: Some(line_range),
                },
            )
        });
        let output = render(&evidence, receipt_id.as_deref());
        bounded_json_output(&call, output)
    }

    async fn read_with_refresh(
        &self,
        project_roots: Vec<PathBuf>,
        locator: EvidenceReadLocator,
        max_bytes: u32,
    ) -> Result<(EvidenceReadResult, bool), FunctionCallError> {
        let reader =
            EvidenceReader::new(self.services.context_map().await.map_err(respond)?.clone());
        let refresh_source = match &locator {
            EvidenceReadLocator::Source {
                project_root,
                relative_path,
                ..
            } => Some((project_root.clone(), relative_path.clone())),
            EvidenceReadLocator::ContextMapRoute(_) => None,
        };
        let request = EvidenceReadRequest {
            project_id: self.project_id.clone(),
            project_roots: project_roots.clone(),
            locator,
            max_bytes,
        };
        match reader.read(request.clone()).await {
            Ok(result) => Ok((result, false)),
            Err(
                error @ (EvidenceReadError::SourceChanged
                | EvidenceReadError::SourceNotCurrent(ContextMapFreshness::Stale)
                | EvidenceReadError::SourceNotIndexed(_)),
            ) => {
                let Some((project_root, relative_path)) = refresh_source else {
                    return Err(read_error(error));
                };
                let indexer = ProjectIndexer::new(
                    self.services.hierarchy().await.map_err(respond)?.clone(),
                    self.services.context_map().await.map_err(respond)?.clone(),
                );
                if matches!(error, EvidenceReadError::SourceNotIndexed(_)) {
                    // Only a project whose source map was never built (for example a
                    // documents-only project) is indexed here, within a time bound; a file
                    // missing from a built index is reported, not a reason to reindex.
                    let never_indexed = self
                        .services
                        .hierarchy()
                        .await
                        .map_err(respond)?
                        .project_intelligence_status(&self.project_id)
                        .await
                        .is_ok_and(|status| status.last_refresh.is_none());
                    // One attempt per project and process: a scan cut off by the time bound
                    // keeps running in the background and must not be started again.
                    if !never_indexed || !self.services.claim_index_attempt(&self.project_id) {
                        return Err(read_error(error));
                    }
                    match tokio::time::timeout(
                        ON_DEMAND_INDEX_BUDGET,
                        indexer.refresh(ProjectIndexRequest {
                            project_id: self.project_id.clone(),
                            roots: project_roots.clone(),
                        }),
                    )
                    .await
                    {
                        Ok(Ok(_)) => {}
                        Ok(Err(refresh)) => {
                            return Err(respond(format!(
                                "{error}; indexing the project failed ({refresh}). Read the file directly instead; an evidence receipt is optional"
                            )));
                        }
                        Err(_) => {
                            return Err(respond(format!(
                                "{error}; the project is too large to index during this call. Read the file directly instead; an evidence receipt is optional"
                            )));
                        }
                    }
                } else {
                    let project_root = self
                        .refresh_root(&project_roots, project_root.as_ref(), &relative_path)
                        .await?;
                    indexer
                        .refresh_file(ProjectIndexFileRequest {
                            project_id: self.project_id.clone(),
                            project_root,
                            relative_path,
                        })
                        .await
                        .map_err(respond)?;
                }
                reader
                    .read(request)
                    .await
                    .map(|result| (result, true))
                    .map_err(read_error)
            }
            Err(error) => Err(read_error(error)),
        }
    }

    async fn refresh_root(
        &self,
        project_roots: &[PathBuf],
        requested_root: Option<&PathBuf>,
        relative_path: &ProjectRelativePath,
    ) -> Result<PathBuf, FunctionCallError> {
        if let Some(root) = requested_root {
            return Ok(root.clone());
        }
        if let [root] = project_roots {
            return Ok(root.clone());
        }
        let hits = self
            .services
            .context_map()
            .await
            .map_err(respond)?
            .file_hits_for_path(&self.project_id, relative_path)
            .await
            .map_err(respond)?;
        let mut matching_roots = hits
            .into_iter()
            .filter_map(|hit| {
                project_roots
                    .iter()
                    .find(|root| *root == &PathBuf::from(&hit.source.project_root))
                    .cloned()
            })
            .collect::<Vec<_>>();
        matching_roots.dedup();
        match matching_roots.as_slice() {
            [root] => Ok(root.clone()),
            _ => Err(FunctionCallError::RespondToModel(format!(
                "source path exists in multiple project roots; provide projectRoot: {relative_path}"
            ))),
        }
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for EvidenceReadTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "Exact file bytes: {relativePath,lineRange?} (projectRoot if ambiguous), or unchanged issued {evidenceRoute}; changed routes refuse. L<n> is display only. sourceRefreshed marks old knowledge stale. sourceVerified requires unchanged non-null blackboardEvidence; null means incomplete.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "evidenceRoute": {
                        "type": "object",
                        "description": "The evidenceRoute object of a current context_map_query or context_map_refresh item, passed unchanged; not the whole item. Do not combine it with relativePath, projectRoot, or lineRange.",
                        "properties": {
                            "contextMapEntryId": {"type": "string"},
                            "sourceFingerprint": {"type": "string"},
                            "lineRange": {
                                "type": "object",
                                "description": "Copy it when the issued route has one; omit it when the route's lineRange is null.",
                                "properties": {
                                    "start": {"type": "integer", "minimum": 1},
                                    "end": {"type": "integer", "minimum": 1}
                                },
                                "required": ["start", "end"],
                                "additionalProperties": false
                            }
                        },
                        "required": ["contextMapEntryId", "sourceFingerprint"],
                        "additionalProperties": false
                    },
                    "relativePath": {"type": "string", "description": "Project-relative path shown by the root blackboard or context map."},
                    "projectRoot": {"type": "string", "description": "Required only when the same relative path exists under multiple selected roots."},
                    "lineRange": {
                        "type": "object",
                        "description": "Optional inclusive 1-based line range. Prefer a narrow range when known.",
                        "properties": {
                            "start": {"type": "integer", "minimum": 1},
                            "end": {"type": "integer", "minimum": 1}
                        },
                        "required": ["start", "end"],
                        "additionalProperties": false
                    },
                    "maxBytes": {"type": "integer", "minimum": 1, "maximum": MAX_BYTES}
                },
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static evidence read schema: {error}")),
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

/// Route-item fields from context_map_query/refresh output, plus the `name` label
/// scripts attach, that identify a whole item passed where only its route belongs.
const ROUTE_ITEM_FIELDS: &[&str] = &[
    "name",
    "headline",
    "coverage",
    "freshness",
    "storedFreshness",
    "source",
    "refreshInput",
    "knownKnowledge",
];
const ROUTE_WRAPPER_DIAGNOSTIC: &str = "Pass {evidenceRoute: item.evidenceRoute}, rather than the whole route item or {name, evidenceRoute} wrapper. Alternatively, use relativePath with optional lineRange. Do not combine both selectors.";

/// True when the arguments wrap a route item instead of passing its evidenceRoute:
/// `{name, evidenceRoute}`, a whole item, or a whole item nested as evidenceRoute.
/// Any other unknown field keeps the ordinary rejection.
fn is_route_item_wrapper(arguments: &str) -> bool {
    let Ok(serde_json::Value::Object(arguments)) = serde_json::from_str(arguments) else {
        return false;
    };
    let only_item_fields = |object: &serde_json::Map<String, serde_json::Value>| {
        object
            .keys()
            .all(|key| key == "evidenceRoute" || ROUTE_ITEM_FIELDS.contains(&key.as_str()))
    };
    // The unwrapped route must itself be valid, so unknown route or range fields keep
    // the ordinary rejection.
    let valid_route = |route: &serde_json::Value| {
        serde_json::from_value::<EvidenceRouteArguments>(route.clone()).is_ok()
    };
    match arguments.get("evidenceRoute") {
        Some(serde_json::Value::Object(nested)) if nested.contains_key("evidenceRoute") => {
            only_item_fields(&arguments)
                && only_item_fields(nested)
                && valid_route(&nested["evidenceRoute"])
        }
        Some(route) => arguments.len() > 1 && only_item_fields(&arguments) && valid_route(route),
        None => false,
    }
}

/// Model-facing failure text: the reader's diagnosis plus this tool's next action.
fn read_error(error: EvidenceReadError) -> FunctionCallError {
    let next = match &error {
        EvidenceReadError::RouteNotFound(_) => {
            "Obtain a route from context_map_query or context_map_refresh and pass item.evidenceRoute unchanged, or read a known relativePath."
        }
        EvidenceReadError::RouteFingerprintMismatch => {
            "Obtain the route again and pass item.evidenceRoute unchanged; do not repair fingerprints by hand."
        }
        EvidenceReadError::RouteChanged => {
            "Query the region again; do not repair its coordinates manually."
        }
        EvidenceReadError::SourceNotCurrent(ContextMapFreshness::Stale) => {
            "Refresh the affected source, then query again for the current region before using a guarded route."
        }
        EvidenceReadError::SourceChanged => {
            "Refresh the affected file; query again if you need the corresponding region."
        }
        EvidenceReadError::SourceNotIndexed(_) => {
            "The project index does not contain this file (even after indexing on demand). Read it directly instead; an evidence receipt is optional."
        }
        EvidenceReadError::SourceNotCurrent(ContextMapFreshness::SourceUnavailable) => {
            "Check its path or removal and refresh the index."
        }
        EvidenceReadError::SourceNotCurrent(ContextMapFreshness::Current)
        | EvidenceReadError::InvalidRequest
        | EvidenceReadError::InvalidLineRange
        | EvidenceReadError::RootOutsideProject
        | EvidenceReadError::InvalidRegionAnchor
        | EvidenceReadError::UnsupportedRegionAnchor(_)
        | EvidenceReadError::AmbiguousSource(_)
        | EvidenceReadError::SourceOutsideRoot
        | EvidenceReadError::NonUtf8Source
        | EvidenceReadError::CountOverflow
        | EvidenceReadError::ContextMap(_)
        | EvidenceReadError::Io(_)
        | EvidenceReadError::ReadTask(_) => return respond(error),
    };
    FunctionCallError::RespondToModel(format!("{error}. {next}"))
}

fn respond(error: impl std::fmt::Display) -> FunctionCallError {
    FunctionCallError::RespondToModel(error.to_string())
}

/// One source line from a read, with its absolute 1-based number. A line is
/// complete when it ends in a newline or is the last line of a complete read.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SourceLine<'a> {
    number: u64,
    text: &'a str,
    complete: bool,
}

fn source_lines(
    content: &str,
    first_line: Option<u64>,
    read_extent: EvidenceExtent,
) -> Vec<SourceLine<'_>> {
    let Some(first_line) = first_line else {
        return Vec::new();
    };
    (first_line..)
        .zip(content.split_inclusive('\n'))
        .map(|(number, text)| SourceLine {
            number,
            text,
            complete: text.ends_with('\n') || read_extent == EvidenceExtent::Complete,
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum EvidenceExtent {
    /// Every selected source byte is present.
    Complete,
    /// The reader or the response budget cut the selected source.
    Truncated,
}

/// Model-facing evidence text: source lines with display-only `L<n>: ` labels.
/// `source_bytes`, `first_line`, and `last_line` describe the emitted source, never
/// the labels.
#[derive(Debug, PartialEq)]
struct NumberedEvidence {
    content: String,
    source_bytes: usize,
    first_line: Option<u64>,
    last_line: Option<u64>,
    last_line_partial: bool,
    extent: EvidenceExtent,
}

impl NumberedEvidence {
    fn new(lines: &[SourceLine<'_>], extent: EvidenceExtent) -> Self {
        let mut content = String::new();
        for line in lines {
            let _ = write!(content, "L{}: {}", line.number, line.text);
        }
        Self {
            content,
            source_bytes: lines.iter().map(|line| line.text.len()).sum(),
            first_line: lines.first().map(|line| line.number),
            last_line: lines.last().map(|line| line.number),
            last_line_partial: lines.last().is_some_and(|line| !line.complete),
            extent,
        }
    }

    /// The line range a read receipt may certify: only a complete, non-empty read.
    fn receipt_range(&self) -> Option<EvidenceLineRange> {
        match (self.extent, self.first_line, self.last_line) {
            (EvidenceExtent::Complete, Some(start), Some(end)) => {
                Some(EvidenceLineRange { start, end })
            }
            (EvidenceExtent::Complete | EvidenceExtent::Truncated, _, _) => None,
        }
    }
}

/// Packs the most source that `fits`: every line of a complete read, else the longest
/// run of complete lines, else a labelled prefix of an oversized first line, else no
/// source. Returns `None` when nothing fits.
fn pack_evidence(
    lines: &[SourceLine<'_>],
    read_extent: EvidenceExtent,
    fits: impl Fn(&NumberedEvidence) -> bool,
) -> Option<NumberedEvidence> {
    let truncated =
        |lines: &[SourceLine<'_>]| NumberedEvidence::new(lines, EvidenceExtent::Truncated);
    let max_complete_lines = match read_extent {
        EvidenceExtent::Complete => {
            let complete = NumberedEvidence::new(lines, EvidenceExtent::Complete);
            if fits(&complete) {
                return Some(complete);
            }
            lines.len().saturating_sub(1)
        }
        EvidenceExtent::Truncated => lines.iter().take_while(|line| line.complete).count(),
    };
    if let Some(line_count) = largest_fitting(max_complete_lines, |count| {
        fits(&truncated(&lines[..count]))
    }) {
        return Some(truncated(&lines[..line_count]));
    }
    if let Some(first) = lines.first() {
        // A prefix of a complete line must stop before its last character, so a cut
        // never claims to be partial while returning every byte.
        let available = if first.complete {
            first
                .text
                .char_indices()
                .next_back()
                .map_or(0, |(last_char, _)| last_char)
        } else {
            first.text.len()
        };
        let prefix = |end: usize| SourceLine {
            number: first.number,
            text: &first.text[..first.text.ceil_char_boundary(end)],
            complete: false,
        };
        if let Some(end) = largest_fitting(available, |end| fits(&truncated(&[prefix(end)]))) {
            return Some(truncated(&[prefix(end)]));
        }
    }
    // Empty evidence is tried last: its null line bounds are not always smaller than
    // a short labelled prefix with numeric bounds.
    let empty = truncated(&[]);
    fits(&empty).then_some(empty)
}

/// Largest `n` in `1..=upper` for which the monotone `fits(n)` holds, or `None`
/// when no such `n` exists.
fn largest_fitting(upper: usize, fits: impl Fn(usize) -> bool) -> Option<usize> {
    if upper == 0 || !fits(1) {
        return None;
    }
    let (mut lower, mut upper) = (1, upper);
    while lower < upper {
        let candidate = lower + (upper - lower).div_ceil(2);
        if fits(candidate) {
            lower = candidate;
        } else {
            upper = candidate - 1;
        }
    }
    Some(lower)
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;
