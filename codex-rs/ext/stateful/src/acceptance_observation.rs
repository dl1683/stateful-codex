//! Host evidence for the acceptance ledger, taken from what the session actually executed.
//!
//! The host never runs a command because the model proposed it. A criterion's check runs
//! through the session's own shell tool, so it executes in the same sandbox, approval policy
//! and timeout as the rest of the work. The host binds an observed check to a criterion only
//! when the command is verbatim the check and ran in the check's pinned directory, and it
//! attributes the observation to the run of the turn that started the command (never to a
//! run that started later on the same thread).
//!
//! At the command's start the host snapshots the workspace generation and the content of the
//! criterion's pinned artifacts; at its end it records the receipt against that start state.
//! A receipt is unavailable, never passed, when the pinned content changed while it ran, when
//! a mutation was observed meanwhile, or when no start snapshot exists. Every other observed
//! mutation (unmatched commands that are not read-only, input written into a running
//! session, applied or partially applied patches) advances the workspace generation. Commands
//! still running are tracked so completion can refuse while relevant work is in flight.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use codex_extension_api::ExtensionData;
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
/// Larger files cannot be content-pinned within the budget; they are unavailable.
const MAX_HASHED_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024;
/// Artifact reader workers that may run at once, abandoned ones included.
const MAX_OUTSTANDING_READERS: usize = 4;
/// Shell flags whose next (final) argument is the script the model asked to run.
const SCRIPT_FLAGS: &[&str] = &["-c", "-lc", "-Command", "-command", "/c", "/C"];

/// The run the turn was bound to when it started; kept in that turn's own store so a command
/// that finishes later is still attributed to it.
pub(crate) struct TurnRunBinding {
    pub(crate) run_id: StatefulRunId,
}

/// Start snapshots of matched checks, by call ID, in the originating turn's store.
#[derive(Default)]
struct CheckStarts(Mutex<HashMap<String, CheckStart>>);

struct CheckStart {
    generation: u64,
    /// Criterion revision and the content digest of its pinned artifacts at start.
    criteria: BTreeMap<u32, (u64, Option<String>)>,
}

/// Commands started for each run (by run ID) whose end has not been observed.
static IN_FLIGHT: LazyLock<Mutex<HashMap<String, HashSet<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Runs this process has bound a turn to; the first binding is a cold re-entry.
static ENTERED_RUNS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static OUTSTANDING_READERS: AtomicUsize = AtomicUsize::new(0);

/// Records the run binding of a turn that is starting, and, the first time this process sees
/// the run, invalidates evidence recorded before (changes made while no observer ran are
/// unknown).
pub(crate) async fn bind_turn(
    services: &ProjectIntelligenceServices,
    turn_store: &ExtensionData,
    run_id: &StatefulRunId,
) {
    turn_store.insert(TurnRunBinding {
        run_id: run_id.clone(),
    });
    let first_entry = ENTERED_RUNS
        .lock()
        .is_ok_and(|mut entered| entered.insert(run_id.to_string()));
    if first_entry {
        let result = match services.runtime().await {
            Ok(store) => store
                .invalidate_after_reentry(run_id)
                .await
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        if let Err(error) = result {
            tracing::warn!(%run_id, %error, "failed to invalidate acceptance evidence on re-entry");
        }
    }
}

/// How many commands of the run are still running, as far as this process observed.
pub(crate) fn in_flight(run_id: &StatefulRunId) -> usize {
    IN_FLIGHT
        .lock()
        .map(|in_flight| in_flight.get(run_id.as_str()).map_or(0, HashSet::len))
        .unwrap_or(usize::MAX)
}

/// Forgets a command the host never ran (blocked, failed before the handler, or aborted).
pub(crate) fn forget_command(turn_store: &ExtensionData, call_id: &str) {
    if let Some(binding) = turn_store.get::<TurnRunBinding>() {
        finish_in_flight(&binding.run_id, call_id);
    }
}

fn finish_in_flight(run_id: &StatefulRunId, call_id: &str) {
    if let Ok(mut in_flight) = IN_FLIGHT.lock()
        && let Some(calls) = in_flight.get_mut(run_id.as_str())
    {
        calls.remove(call_id);
        if calls.is_empty() {
            in_flight.remove(run_id.as_str());
        }
    }
}

/// A builtin command is about to execute: track it as in flight and snapshot the checks it
/// runs.
pub(crate) async fn command_started(
    services: &ProjectIntelligenceServices,
    roots: &[String],
    turn_store: &ExtensionData,
    call_id: &str,
    argv: &[String],
    cwd: &Path,
) {
    let Some(binding) = turn_store.get::<TurnRunBinding>() else {
        return;
    };
    if let Ok(mut in_flight) = IN_FLIGHT.lock() {
        in_flight
            .entry(binding.run_id.to_string())
            .or_default()
            .insert(call_id.to_string());
    }
    let Ok(store) = services.runtime().await else {
        return;
    };
    let Ok(ledger) = store.acceptance_ledger(&binding.run_id).await else {
        return;
    };
    let matched = matched_checks(&ledger, roots, argv, cwd);
    if matched.is_empty() {
        return;
    }
    let states = artifact_states(roots, matched.iter().copied()).await;
    let start = CheckStart {
        generation: ledger.workspace_generation,
        criteria: matched
            .iter()
            .map(|criterion| {
                (
                    criterion.ordinal,
                    (
                        criterion.revision,
                        pinned_digest(states.get(&criterion.ordinal)),
                    ),
                )
            })
            .collect(),
    };
    if let Ok(mut starts) = turn_store.get_or_init(CheckStarts::default).0.lock() {
        starts.insert(call_id.to_string(), start);
    }
}

/// Records host evidence or a workspace mutation for one completed item, attributed to the run
/// of the turn that started it. Failures only lose the observation.
pub(crate) async fn observe_item(
    services: &ProjectIntelligenceServices,
    roots: &[String],
    turn_store: &ExtensionData,
    item: &TurnItem,
) {
    let Some(binding) = turn_store.get::<TurnRunBinding>() else {
        return;
    };
    let run_id = &binding.run_id;
    let result = match item {
        TurnItem::CommandExecution(command) => {
            if command.status != CommandExecutionStatus::InProgress {
                finish_in_flight(run_id, &command.id);
            }
            observe_command(services, run_id, roots, turn_store, command).await
        }
        // A failed patch may have applied a prefix; only a declined one changed nothing.
        TurnItem::FileChange(change)
            if change
                .status
                .as_ref()
                .is_none_or(|status| *status != PatchApplyStatus::Declined) =>
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
    turn_store: &ExtensionData,
    command: &CommandExecutionItem,
) -> Result<(), String> {
    if command.status == CommandExecutionStatus::InProgress {
        return Ok(());
    }
    let store = services
        .runtime()
        .await
        .map_err(|error| error.to_string())?;
    match command.source {
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
    if command.status != CommandExecutionStatus::Declined {
        store
            .record_execution(run_id)
            .await
            .map_err(|error| error.to_string())?;
    }
    let ledger = store
        .acceptance_ledger(run_id)
        .await
        .map_err(|error| error.to_string())?;
    let matched = matched_checks(&ledger, roots, &command.command, &command.cwd.to_path_buf());
    let start = turn_store
        .get::<CheckStarts>()
        .and_then(|starts| starts.0.lock().ok()?.remove(&command.id));
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
    let end = artifact_states(roots, matched.iter().copied()).await;
    let evidence = matched
        .iter()
        .map(|criterion| {
            let started = start
                .as_ref()
                .and_then(|start| Some((start.generation, start.criteria.get(&criterion.ordinal)?)));
            let (outcome, detail, generation, digest) = match started {
                None => (
                    EvidenceOutcome::Unavailable,
                    Some("no start snapshot of this check exists (it started before this process observed it)".to_string()),
                    ledger.workspace_generation,
                    None,
                ),
                Some((_, (revision, _))) if *revision != criterion.revision => (
                    EvidenceOutcome::Unavailable,
                    Some("the criterion changed while the check ran".to_string()),
                    ledger.workspace_generation,
                    None,
                ),
                Some((generation, (_, start_digest))) => {
                    let end_digest = pinned_digest(end.get(&criterion.ordinal));
                    let (outcome, detail) = match (command.status, command.exit_code) {
                        (CommandExecutionStatus::Declined, _) => (
                            EvidenceOutcome::Unavailable,
                            Some("the session's approval policy declined the check, so it did not run".to_string()),
                        ),
                        (CommandExecutionStatus::Completed, Some(0))
                            if start_digest.is_none() || *start_digest != end_digest =>
                        {
                            (
                                EvidenceOutcome::Unavailable,
                                Some("the pinned artifacts were unreadable or changed while the check ran".to_string()),
                            )
                        }
                        (CommandExecutionStatus::Completed, Some(0)) => (EvidenceOutcome::Passed, None),
                        _ => (EvidenceOutcome::Failed, None),
                    };
                    (outcome, detail, generation, start_digest.clone())
                }
            };
            CommandEvidence {
                ordinal: criterion.ordinal,
                criterion_revision: criterion.revision,
                outcome,
                command: criterion.check_command.clone().unwrap_or_default(),
                exit_code: command.exit_code,
                output_tail: tail(&output),
                output_digest: output_digest.clone(),
                artifact_digest: digest,
                detail,
                start_generation: generation,
                source_id: command.id.clone(),
            }
        })
        .collect();
    store
        .record_command_evidence(run_id, evidence)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Active checks this execution runs verbatim, in their pinned directory.
fn matched_checks<'a>(
    ledger: &'a AcceptanceLedger,
    roots: &[String],
    argv: &[String],
    cwd: &Path,
) -> Vec<&'a AcceptanceCriterion> {
    let ran_in = std::fs::canonicalize(cwd).ok();
    ledger
        .criteria
        .iter()
        .filter(|criterion| {
            criterion.state == AcceptanceState::Active
                && criterion
                    .check_command
                    .as_deref()
                    .is_some_and(|check| runs_check(argv, check))
                && ran_in.is_some()
                && check_directory(roots, criterion.check_cwd.as_deref()) == ran_in
        })
        .collect()
}

/// The canonical directory a criterion's check must run in.
pub(crate) fn check_directory(roots: &[String], check_cwd: Option<&str>) -> Option<PathBuf> {
    let root = roots.first().map(PathBuf::from)?;
    let directory = match check_cwd {
        None => root,
        Some(path) if Path::new(path).is_absolute() => PathBuf::from(path),
        Some(path) => root.join(path),
    };
    let directory = std::fs::canonicalize(directory).ok()?;
    roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .any(|root| directory.starts_with(root))
        .then_some(directory)
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

fn pinned_digest(state: Option<&ArtifactState>) -> Option<String> {
    match state {
        Some(ArtifactState::Observed { digest, missing }) if missing.is_empty() => {
            Some(digest.clone())
        }
        Some(ArtifactState::Observed { .. } | ArtifactState::Unavailable(_)) | None => None,
    }
}

/// Reads the declared artifacts of the given criteria under one deadline. The worker stops
/// at the deadline (or when its caller gives up), never reads past the size it measured, and
/// at most `MAX_OUTSTANDING_READERS` workers exist at once; otherwise the artifacts are
/// unavailable.
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
    let unavailable = |reason: &str| {
        requests
            .iter()
            .map(|(ordinal, _)| (*ordinal, ArtifactState::Unavailable(reason.to_string())))
            .collect::<BTreeMap<_, _>>()
    };
    if OUTSTANDING_READERS.fetch_add(1, Ordering::SeqCst) >= MAX_OUTSTANDING_READERS {
        OUTSTANDING_READERS.fetch_sub(1, Ordering::SeqCst);
        return unavailable("too many artifact reads are still outstanding");
    }
    let roots = roots.iter().map(PathBuf::from).collect::<Vec<_>>();
    let deadline = Instant::now() + ARTIFACT_READ_BUDGET;
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let pending = requests.clone();
    let read = tokio::task::spawn_blocking(move || {
        let stop = || worker_cancelled.load(Ordering::SeqCst) || Instant::now() >= deadline;
        let states = pending
            .into_iter()
            .map(|(ordinal, artifacts)| (ordinal, read_artifacts(&roots, &artifacts, &stop)))
            .collect::<BTreeMap<_, _>>();
        OUTSTANDING_READERS.fetch_sub(1, Ordering::SeqCst);
        states
    });
    match tokio::time::timeout(ARTIFACT_READ_BUDGET, read).await {
        Ok(Ok(states)) => states,
        Ok(Err(_)) | Err(_) => {
            cancelled.store(true, Ordering::SeqCst);
            unavailable(&format!(
                "reading them did not finish within {} seconds",
                ARTIFACT_READ_BUDGET.as_secs()
            ))
        }
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

fn read_artifacts(
    roots: &[PathBuf],
    artifacts: &[String],
    stop: &dyn Fn() -> bool,
) -> ArtifactState {
    let mut sorted = artifacts.to_vec();
    sorted.sort();
    sorted.dedup();
    let mut hasher = Sha256::new();
    let mut missing = Vec::new();
    for artifact in &sorted {
        let digest = match file_digest(roots, artifact, stop) {
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
fn file_digest(
    roots: &[PathBuf],
    artifact: &str,
    stop: &dyn Fn() -> bool,
) -> Result<Option<String>, String> {
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
    let mut file = std::fs::File::open(&path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("it is not a regular file".to_string());
    }
    if metadata.len() > MAX_HASHED_ARTIFACT_BYTES {
        return Err(format!(
            "it is larger than {MAX_HASHED_ARTIFACT_BYTES} bytes, so its content cannot be pinned"
        ));
    }
    let limit = metadata.len();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        if stop() {
            return Err("the read budget ran out".to_string());
        }
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > limit {
            return Err("it grew while being read".to_string());
        }
        hasher.update(&buffer[..read]);
    }
    if total != limit {
        return Err("it changed size while being read".to_string());
    }
    Ok(Some(format!("sha256:{:x}", hasher.finalize())))
}

#[cfg(test)]
#[path = "acceptance_observation_tests.rs"]
mod tests;
