//! Host evidence for the acceptance ledger, taken from what the session actually executed.
//!
//! The host never runs a command because the model proposed it. A criterion's check command
//! runs through the session's own shell tool, so it executes in the same sandbox, approval
//! policy and timeout as the rest of the work; the host observes the completed command item
//! and binds its exit status, a bounded output tail and digest, and the criterion's artifact
//! digest to the criterion. A declined command is recorded as unavailable. Any other observed
//! workspace mutation advances the workspace generation, which makes earlier passing checks
//! stale.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use codex_protocol::items::CommandExecutionItem;
use codex_protocol::items::CommandExecutionStatus;
use codex_protocol::items::TurnItem;
use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::protocol::ExecCommandSource;
use codex_protocol::protocol::PatchApplyStatus;
use codex_stateful_runtime::AcceptanceCriterion;
use codex_stateful_runtime::AcceptanceLedger;
use codex_stateful_runtime::AcceptanceState;
use codex_stateful_runtime::ArtifactState;
use codex_stateful_runtime::CommandEvidence;
use codex_stateful_runtime::EvidenceOutcome;
use codex_stateful_runtime::MAX_OUTPUT_TAIL_BYTES;
use codex_stateful_runtime::StatefulRunId;
use sha2::Digest;
use sha2::Sha256;

use crate::services::ProjectIntelligenceServices;

/// Whole-ledger bound for reading declared artifacts.
const ARTIFACT_READ_BUDGET: Duration = Duration::from_secs(5);
/// Files larger than this are identified by size and modification time, not content.
const MAX_HASHED_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024;
/// Shell flags whose next (final) argument is the script the model asked to run.
const SCRIPT_FLAGS: &[&str] = &["-c", "-lc", "-Command", "-command", "/c", "/C"];

/// Records host evidence or a workspace mutation for one completed item of a run's thread.
/// Failures only lose the observation; they never affect the item.
pub(crate) async fn observe_item(
    services: &ProjectIntelligenceServices,
    run_id: &StatefulRunId,
    roots: &[String],
    item: &TurnItem,
) {
    let result = match item {
        TurnItem::CommandExecution(command) => {
            observe_command(services, run_id, roots, command).await
        }
        TurnItem::FileChange(change)
            if change
                .status
                .as_ref()
                .is_none_or(|status| *status == PatchApplyStatus::Completed) =>
        {
            bump(services, run_id).await
        }
        _ => Ok(()),
    };
    if let Err(error) = result {
        tracing::warn!(%run_id, %error, "failed to record acceptance evidence");
    }
}

async fn observe_command(
    services: &ProjectIntelligenceServices,
    run_id: &StatefulRunId,
    roots: &[String],
    command: &CommandExecutionItem,
) -> Result<(), String> {
    match command.source {
        _ if command.status == CommandExecutionStatus::InProgress => return Ok(()),
        // Input written into a running session is no verifiable check, but it can change
        // the workspace.
        ExecCommandSource::UnifiedExecInteraction => {
            if command.status == CommandExecutionStatus::Declined {
                return Ok(());
            }
            return bump(services, run_id).await;
        }
        ExecCommandSource::Agent
        | ExecCommandSource::UnifiedExecStartup
        | ExecCommandSource::UserShell => {}
    }
    let store = services
        .runtime()
        .await
        .map_err(|error| error.to_string())?;
    let ledger = store
        .acceptance_ledger(run_id)
        .await
        .map_err(|error| error.to_string())?;
    let matched = ledger
        .criteria
        .iter()
        .filter(|criterion| {
            criterion.state == AcceptanceState::Active
                && criterion
                    .check_command
                    .as_deref()
                    .is_some_and(|check| runs_check(&command.command, check))
        })
        .collect::<Vec<_>>();
    if matched.is_empty() {
        if command.status != CommandExecutionStatus::Declined && mutates(command) {
            return bump(services, run_id).await;
        }
        return Ok(());
    }
    let output = command.aggregated_output.clone().unwrap_or_else(|| {
        format!(
            "{}{}",
            command.stdout.as_deref().unwrap_or_default(),
            command.stderr.as_deref().unwrap_or_default()
        )
    });
    let output_digest = format!("sha256:{:x}", Sha256::digest(output.as_bytes()));
    let (outcome, detail) = match (command.status, command.exit_code) {
        (CommandExecutionStatus::Declined, _) => (
            EvidenceOutcome::Unavailable,
            Some("the session's approval policy declined the check, so it did not run".to_string()),
        ),
        (CommandExecutionStatus::Completed, Some(0)) => (EvidenceOutcome::Passed, None),
        _ => (EvidenceOutcome::Failed, None),
    };
    let artifacts = artifact_states(roots, matched.iter().copied()).await;
    let evidence = matched
        .iter()
        .map(|criterion| CommandEvidence {
            ordinal: criterion.ordinal,
            criterion_revision: criterion.revision,
            outcome,
            command: criterion.check_command.clone().unwrap_or_default(),
            exit_code: command.exit_code,
            output_tail: tail(&output),
            output_digest: output_digest.clone(),
            artifact_digest: match artifacts.get(&criterion.ordinal) {
                Some(ArtifactState::Observed { digest, .. }) => Some(digest.clone()),
                Some(ArtifactState::Unavailable(_)) | None => None,
            },
            detail: detail.clone(),
            source_id: command.id.clone(),
        })
        .collect();
    store
        .record_command_evidence(run_id, evidence)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn bump(
    services: &ProjectIntelligenceServices,
    run_id: &StatefulRunId,
) -> Result<(), String> {
    services
        .runtime()
        .await
        .map_err(|error| error.to_string())?
        .bump_workspace_generation(run_id)
        .await
        .map_err(|error| error.to_string())
}

/// Whether an executed argv ran exactly the proposed check: the script of a shell wrapper,
/// or the argv itself joined by spaces.
pub(crate) fn runs_check(argv: &[String], check: &str) -> bool {
    let script = match argv {
        [.., flag, script] if SCRIPT_FLAGS.contains(&flag.as_str()) => Some(script.trim()),
        _ => None,
    };
    script == Some(check) || argv.join(" ") == check
}

/// A command whose parsed form is entirely reads, listings or searches does not change the
/// workspace; anything else (including an unparsed command) is treated as a mutation.
fn mutates(command: &CommandExecutionItem) -> bool {
    command.parsed_cmd.is_empty()
        || command
            .parsed_cmd
            .iter()
            .any(|parsed| matches!(parsed, ParsedCommand::Unknown { .. }))
}

fn tail(output: &str) -> String {
    if output.len() <= MAX_OUTPUT_TAIL_BYTES {
        return output.to_string();
    }
    let mut start = output.len() - MAX_OUTPUT_TAIL_BYTES;
    while !output.is_char_boundary(start) {
        start += 1;
    }
    output[start..].to_string()
}

/// Reads the declared artifacts of the given criteria, bounded in time as a whole.
pub(crate) async fn artifact_states<'a>(
    roots: &[String],
    criteria: impl Iterator<Item = &'a AcceptanceCriterion>,
) -> BTreeMap<u32, ArtifactState> {
    let requests = criteria
        .filter(|criterion| !criterion.artifacts.is_empty())
        .map(|criterion| (criterion.ordinal, criterion.artifacts.clone()))
        .collect::<Vec<_>>();
    if requests.is_empty() {
        return BTreeMap::new();
    }
    let roots = roots.iter().map(PathBuf::from).collect::<Vec<_>>();
    let pending = requests.clone();
    let read = tokio::task::spawn_blocking(move || {
        pending
            .into_iter()
            .map(|(ordinal, artifacts)| (ordinal, read_artifacts(&roots, &artifacts)))
            .collect::<BTreeMap<_, _>>()
    });
    match tokio::time::timeout(ARTIFACT_READ_BUDGET, read).await {
        Ok(Ok(states)) => states,
        Ok(Err(_)) | Err(_) => requests
            .into_iter()
            .map(|(ordinal, _)| {
                (
                    ordinal,
                    ArtifactState::Unavailable(format!(
                        "reading them did not finish within {} seconds",
                        ARTIFACT_READ_BUDGET.as_secs()
                    )),
                )
            })
            .collect(),
    }
}

/// Artifact states for every active criterion of a ledger, for a completion attempt.
pub(crate) async fn ledger_artifact_states(
    roots: &[String],
    ledger: &AcceptanceLedger,
) -> BTreeMap<u32, ArtifactState> {
    artifact_states(
        roots,
        ledger
            .criteria
            .iter()
            .filter(|criterion| criterion.state == AcceptanceState::Active),
    )
    .await
}

fn read_artifacts(roots: &[PathBuf], artifacts: &[String]) -> ArtifactState {
    let mut sorted = artifacts.to_vec();
    sorted.sort();
    sorted.dedup();
    let mut hasher = Sha256::new();
    let mut missing = Vec::new();
    for artifact in &sorted {
        let digest = match file_digest(roots, artifact) {
            Ok(Some(digest)) => digest,
            Ok(None) => {
                missing.push(artifact.clone());
                "absent".to_string()
            }
            Err(reason) => return ArtifactState::Unavailable(format!("{artifact}: {reason}")),
        };
        for part in [artifact.as_str(), digest.as_str()] {
            hasher.update((part.len() as u64).to_be_bytes());
            hasher.update(part.as_bytes());
        }
    }
    ArtifactState::Observed {
        digest: format!("sha256:{:x}", hasher.finalize()),
        missing,
    }
}

/// The content digest of one artifact inside a project root; `None` when it does not exist.
fn file_digest(roots: &[PathBuf], artifact: &str) -> Result<Option<String>, String> {
    let canonical_roots = roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .collect::<Vec<_>>();
    let candidate = Path::new(artifact);
    let path = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        canonical_roots
            .first()
            .ok_or_else(|| "the project has no readable root".to_string())?
            .join(candidate)
    };
    let path = match std::fs::canonicalize(&path) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if !canonical_roots.iter().any(|root| path.starts_with(root)) {
        return Err("it is outside the project roots".to_string());
    }
    let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("it is not a regular file".to_string());
    }
    if metadata.len() > MAX_HASHED_ARTIFACT_BYTES {
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos());
        return Ok(Some(format!("size:{}:modified:{modified}", metadata.len())));
    }
    let mut file = std::fs::File::open(&path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(Some(format!("sha256:{:x}", hasher.finalize())))
}

#[cfg(test)]
#[path = "acceptance_observation_tests.rs"]
mod tests;
