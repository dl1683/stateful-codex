//! `stateful_acceptance_update`: declare, refine and settle the run's acceptance criteria.
//!
//! User criteria quote the exact goal text they come from, and the host stores the byte span.
//! Their statements are fixed and they cannot be retired; derived criteria are refinable.
//! Evidence the model can write is labelled: a manual observation, or `noCheck` with the
//! reason no safe check exists. Host evidence comes only from observed check executions.

use codex_extension_api::FunctionCallError;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExposure;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_stateful_runtime::AcceptanceChange;
use codex_stateful_runtime::AcceptanceKind;
use codex_stateful_runtime::AcceptanceOrigin;
use codex_stateful_runtime::ArtifactState;
use codex_stateful_runtime::CriterionTerms;
use codex_stateful_runtime::DismissalReceipt;
use codex_stateful_runtime::MAX_ACCEPTANCE_CHANGES;
use codex_stateful_runtime::MAX_CRITERION_ARTIFACTS;
use codex_stateful_runtime::MAX_CRITERION_DEPENDENCIES;
use codex_stateful_runtime::RequestSpan;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::TermsUpdate;
use serde::Deserialize;
use serde_json::json;

use std::sync::Arc;

use codex_thread_store::ThreadStore;

use crate::acceptance_observation::artifact_states;
use crate::acceptance_render::AcceptanceView;
use crate::acceptance_render::bounded;
use crate::services::ProjectIntelligenceServices;

use super::MAX_RESPONSE_BYTES;
use super::bounded_json_output;
use super::bounded_respond;
use super::parse_arguments;
use super::provenance_source_id;
use super::respond;
use super::thread_run;

const TOOL_NAME: &str = "stateful_acceptance_update";
/// Largest argument text decoded; a larger call is refused before decoding.
const MAX_ARGUMENT_BYTES: usize = 64 * 1024;
/// Minimal valid call shown with every argument decoding rejection.
const EXAMPLE: &str = r#"{"expectedLedgerRevision":0,"changes":[{"action":"add","origin":"user","kind":"check","statement":"<requirement>","requestQuote":"<exact goal text>","checkCommand":"<command>","expectedObservation":"<what passing output shows>"}]}"#;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Arguments {
    expected_ledger_revision: u64,
    changes: Vec<ChangeArguments>,
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum Action {
    Add,
    Refine,
    Accept,
    Dismiss,
    Retire,
    Approve,
    Observe,
    NoCheck,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChangeArguments {
    action: Action,
    criterion: Option<String>,
    origin: Option<AcceptanceOrigin>,
    kind: Option<AcceptanceKind>,
    statement: Option<String>,
    request_quote: Option<String>,
    required: Option<bool>,
    depends_on: Option<Vec<String>>,
    milestone: Option<String>,
    artifacts: Option<Vec<String>>,
    check_command: Option<String>,
    check_cwd: Option<String>,
    expected_observation: Option<String>,
    steering_id: Option<String>,
    steering_quote: Option<String>,
    covered_by: Option<String>,
    text: Option<String>,
}

pub(super) struct AcceptanceUpdateTool {
    project_id: String,
    thread_id: String,
    services: ProjectIntelligenceServices,
    projects: Arc<dyn ThreadStore>,
}

impl AcceptanceUpdateTool {
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

    /// Every refusal, including semantic ones, fits the call's serialized allowance.
    async fn handle_bounded(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let reply = call.clone();
        match self.handle_call(call).await {
            Err(FunctionCallError::RespondToModel(message)) => {
                Err(bounded_respond(&reply, &message))
            }
            outcome => outcome,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        if call
            .function_arguments()
            .is_ok_and(|arguments| arguments.len() > MAX_ARGUMENT_BYTES)
        {
            return Err(FunctionCallError::RespondToModel(format!(
                "nothing changed: the arguments exceed {MAX_ARGUMENT_BYTES} bytes; send fewer or shorter changes"
            )));
        }
        let arguments: Arguments = parse_arguments(&call, EXAMPLE)?;
        let source_id = provenance_source_id(&call)?;
        let run = thread_run(&self.project_id, &self.thread_id, &self.services).await?;
        let mut changes = arguments
            .changes
            .into_iter()
            .enumerate()
            .map(|(index, change)| convert(&run, index, change))
            .collect::<Result<Vec<_>, _>>()?;
        let runtime = self.services.runtime().await.map_err(respond)?;
        // A manual observation is pinned to the content of the criterion's artifacts, read by
        // the host now; outside the local executor nothing can be pinned.
        if changes
            .iter()
            .any(|change| matches!(change, AcceptanceChange::Observe { .. }))
        {
            let ledger = runtime.acceptance_ledger(&run.id).await.map_err(respond)?;
            let roots = if super::run::local_executor(&call) {
                match self.projects.read_project(self.project_id.clone()).await {
                    Ok(Some(project)) => project.roots.into_iter().map(|root| root.path).collect(),
                    Ok(None) | Err(_) => Vec::new(),
                }
            } else {
                Vec::new()
            };
            for change in &mut changes {
                if let AcceptanceChange::Observe {
                    ordinal,
                    artifact_digest,
                    ..
                } = change
                    && let Some(criterion) = ledger.criterion(*ordinal)
                    && !roots.is_empty()
                {
                    let states = artifact_states(&roots, std::iter::once(criterion)).await;
                    *artifact_digest = match states.get(ordinal) {
                        Some(ArtifactState::Observed { digest, missing }) if missing.is_empty() => {
                            Some(digest.clone())
                        }
                        Some(ArtifactState::Observed { .. } | ArtifactState::Unavailable(_))
                        | None => None,
                    };
                }
            }
        }
        let ledger = runtime
            .revise_acceptance(
                &run.id,
                arguments.expected_ledger_revision,
                changes,
                &source_id,
            )
            .await
            .map_err(|error| {
                FunctionCallError::RespondToModel(format!(
                    "nothing changed: {error}. Read the ledger in <stateful_run> or with stateful_run_read section \"acceptance\" before retrying"
                ))
            })?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| {
                i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
            });
        let mut lines = AcceptanceView::new(&run, ledger.clone(), now_ms).ledger_lines();
        let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let mut omitted = 0;
        loop {
            let output = json!({
                "ledgerRevision": ledger.revision,
                "acceptance": lines,
                "omittedLines": omitted,
                "readExactly": (omitted > 0).then_some("stateful_run_read with section \"acceptance\""),
            });
            if output.to_string().len() <= budget || lines.is_empty() {
                return bounded_json_output(&call, output);
            }
            lines.pop();
            omitted += 1;
        }
    }
}

fn convert(
    run: &StatefulRun,
    index: usize,
    change: ChangeArguments,
) -> Result<AcceptanceChange, FunctionCallError> {
    let field = |name: &str| {
        FunctionCallError::RespondToModel(format!(
            "nothing changed: changes[{index}] needs {name} for this action"
        ))
    };
    let unexpected = |names: &[(&str, bool)]| -> Result<(), FunctionCallError> {
        let present = names
            .iter()
            .filter_map(|(name, present)| present.then_some(*name))
            .collect::<Vec<_>>();
        if present.is_empty() {
            Ok(())
        } else {
            Err(FunctionCallError::RespondToModel(format!(
                "nothing changed: changes[{index}] does not take {} for this action",
                present.join(", ")
            )))
        }
    };
    let parse_alias = |alias: &str, name: &str| -> Result<u32, FunctionCallError> {
        alias
            .strip_prefix('C')
            .filter(|digits| !digits.starts_with('0'))
            .and_then(|digits| digits.parse::<u32>().ok())
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(format!(
                    "nothing changed: changes[{index}].{name} must hold C aliases such as C1, not {}",
                    bounded(alias, 40)
                ))
            })
    };
    let ordinal = || -> Result<u32, FunctionCallError> {
        parse_alias(
            change
                .criterion
                .as_deref()
                .ok_or_else(|| field("criterion"))?,
            "criterion",
        )
    };
    let depends_on = || -> Result<Option<Vec<u32>>, FunctionCallError> {
        if change
            .depends_on
            .as_ref()
            .is_some_and(|aliases| aliases.len() > MAX_CRITERION_DEPENDENCIES)
        {
            return Err(FunctionCallError::RespondToModel(format!(
                "nothing changed: changes[{index}].dependsOn names at most {MAX_CRITERION_DEPENDENCIES} criteria"
            )));
        }
        change
            .depends_on
            .as_ref()
            .map(|aliases| {
                aliases
                    .iter()
                    .map(|alias| parse_alias(alias, "dependsOn"))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()
    };
    let terms_update = || -> Result<TermsUpdate, FunctionCallError> {
        Ok(TermsUpdate {
            depends_on: depends_on()?,
            milestone: change.milestone.clone(),
            artifacts: change.artifacts.clone(),
            check_command: change.check_command.clone(),
            check_cwd: change.check_cwd.clone(),
            expected_observation: change.expected_observation.clone(),
        })
    };
    // Dismiss, retire, observe and noCheck take only a criterion and its text.
    let labelled = || -> Result<(u32, String), FunctionCallError> {
        unexpected(&[
            ("origin", change.origin.is_some()),
            ("kind", change.kind.is_some()),
            ("statement", change.statement.is_some()),
            ("requestQuote", change.request_quote.is_some()),
            ("artifacts", change.artifacts.is_some()),
            ("checkCommand", change.check_command.is_some()),
            ("checkCwd", change.check_cwd.is_some()),
            ("expectedObservation", change.expected_observation.is_some()),
            ("required", change.required.is_some()),
            ("dependsOn", change.depends_on.is_some()),
            ("milestone", change.milestone.is_some()),
        ])?;
        Ok((
            ordinal()?,
            change.text.clone().ok_or_else(|| field("text"))?,
        ))
    };
    let ChangeArguments {
        action,
        origin,
        kind,
        statement,
        request_quote,
        required,
        text,
        ..
    } = &change;
    match action {
        Action::Add => {
            unexpected(&[
                ("criterion", change.criterion.is_some()),
                ("text", text.is_some()),
            ])?;
            let request_span = request_quote
                .as_deref()
                .map(|quote| span_of(run, index, quote))
                .transpose()?;
            Ok(AcceptanceChange::Add {
                origin: origin.ok_or_else(|| field("origin"))?,
                kind: kind.ok_or_else(|| field("kind"))?,
                statement: statement.clone().ok_or_else(|| field("statement"))?,
                request_span,
                terms: CriterionTerms {
                    required: required.unwrap_or(true),
                    depends_on: depends_on()?.unwrap_or_default(),
                    milestone: change.milestone.clone(),
                    artifacts: change.artifacts.clone().unwrap_or_default(),
                    check_command: change.check_command.clone(),
                    check_cwd: change.check_cwd.clone(),
                    expected_observation: change.expected_observation.clone(),
                },
            })
        }
        Action::Refine | Action::Accept => {
            unexpected(&[
                ("origin", origin.is_some()),
                ("requestQuote", request_quote.is_some()),
                ("text", text.is_some()),
            ])?;
            if *action == Action::Refine {
                unexpected(&[("kind", kind.is_some())])?;
                Ok(AcceptanceChange::Refine {
                    ordinal: ordinal()?,
                    statement: statement.clone(),
                    required: *required,
                    terms: terms_update()?,
                })
            } else {
                unexpected(&[
                    ("statement", statement.is_some()),
                    ("required", required.is_some()),
                ])?;
                Ok(AcceptanceChange::Accept {
                    ordinal: ordinal()?,
                    kind: *kind,
                    terms: terms_update()?,
                })
            }
        }
        Action::Dismiss => {
            let receipt_fields = (
                change.steering_id.as_deref(),
                change.steering_quote.as_deref(),
                change.covered_by.as_deref(),
            );
            let receipt = match receipt_fields {
                (Some(steering_id), Some(quote), None) => DismissalReceipt::UserSteering {
                    steering_id: steering_id.to_string(),
                    quote: quote.to_string(),
                },
                (None, None, Some(alias)) => {
                    DismissalReceipt::CoveredBy(parse_alias(alias, "coveredBy")?)
                }
                _ => {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "nothing changed: changes[{index}] dismisses a user requirement only with a receipt: steeringId plus steeringQuote (exact text of the user's own steering), or coveredBy (a user criterion whose quote covers it)"
                    )));
                }
            };
            let (ordinal, reason) = labelled()?;
            Ok(AcceptanceChange::Dismiss {
                ordinal,
                reason,
                receipt,
            })
        }
        Action::Approve => {
            unexpected(&[("text", change.text.is_some())])?;
            let steering_id = change
                .steering_id
                .clone()
                .ok_or_else(|| field("steeringId"))?;
            let ordinal = ordinal()?;
            Ok(AcceptanceChange::Approve {
                ordinal,
                steering_id,
            })
        }
        Action::Retire => {
            let (ordinal, reason) = labelled()?;
            Ok(AcceptanceChange::Retire { ordinal, reason })
        }
        Action::Observe => {
            let (ordinal, observation) = labelled()?;
            Ok(AcceptanceChange::Observe {
                ordinal,
                observation,
                artifact_digest: None,
            })
        }
        Action::NoCheck => {
            let (ordinal, reason) = labelled()?;
            Ok(AcceptanceChange::NoCheck { ordinal, reason })
        }
    }
}

/// The byte span of the first exact occurrence of `quote` in the run goal.
fn span_of(run: &StatefulRun, index: usize, quote: &str) -> Result<RequestSpan, FunctionCallError> {
    let quote = quote.trim();
    match (!quote.is_empty())
        .then(|| run.value.goal.find(quote))
        .flatten()
    {
        Some(start) => Ok(RequestSpan {
            start,
            end: start + quote.len(),
        }),
        None => Err(FunctionCallError::RespondToModel(format!(
            "nothing changed: changes[{index}].requestQuote is not exact text of the run goal; copy it verbatim from the goal (stateful_run_read section \"goal\")"
        ))),
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for AcceptanceUpdateTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn exposure(&self) -> ToolExposure {
        // Prose-bearing mutation, like the run update: direct only, never inside exec.
        ToolExposure::DirectModelOnly
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "Acceptance ledger. add (user: requestQuote, exact goal text), refine, accept, dismiss (steeringId+steeringQuote or coveredBy), retire derived, approve (steeringId quoting the check), observe, noCheck. Run checkCommand verbatim in checkCwd.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "expectedLedgerRevision": {"type": "integer", "minimum": 0},
                    "changes": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": MAX_ACCEPTANCE_CHANGES,
                        "items": {
                            "type": "object",
                            "properties": {
                                "action": {"type": "string", "enum": ["add", "refine", "accept", "dismiss", "retire", "approve", "observe", "noCheck"]},
                                "criterion": {"type": "string"},
                                "origin": {"type": "string", "enum": ["user", "derived"]},
                                "kind": {"type": "string", "enum": ["deliverable", "constraint", "check", "manual", "existence"]},
                                "statement": {"type": "string"},
                                "requestQuote": {"type": "string"},
                                "required": {"type": "boolean"},
                                "dependsOn": {"type": "array", "items": {"type": "string"}, "maxItems": MAX_CRITERION_DEPENDENCIES},
                                "milestone": {"type": "string"},
                                "artifacts": {"type": "array", "items": {"type": "string"}, "maxItems": MAX_CRITERION_ARTIFACTS},
                                "checkCommand": {"type": "string"},
                                "checkCwd": {"type": "string"},
                                "expectedObservation": {"type": "string"},
                                "steeringId": {"type": "string"},
                                "steeringQuote": {"type": "string"},
                                "coveredBy": {"type": "string"},
                                "text": {"type": "string"}
                            },
                            "required": ["action"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["expectedLedgerRevision", "changes"],
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static acceptance schema: {error}")),
            output_schema: None,
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        false
    }

    fn handle<'a>(&'a self, call: ToolCall<'call>) -> codex_extension_api::ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(self.handle_bounded(call))
    }
}

#[cfg(test)]
#[path = "acceptance_tests.rs"]
mod tests;
