//! Host evidence for the acceptance ledger, taken from what the session actually executed.
//!
//! The host never runs a command because the model proposed it. A criterion's check runs
//! through the session's own shell tool, so it executes in the same sandbox, approval policy
//! and timeout as the rest of the work. The host binds an observed check to a criterion only
//! when the command is verbatim the check and ran in the check's pinned directory, and it
//! attributes the observation to the run of the turn that started the command (never to a
//! run that started later on the same thread).
//!
//! Every started command is recorded as pending in the run's ledger until its effects
//! (execution count, mutation, receipt) are durably accounted for; the terminal gate refuses
//! completion while any is pending. Only a command proven never to have launched is
//! forgotten. At a matched check's start the host snapshots the workspace generation, the
//! check's pinned artifacts and checker bytes, and every criterion's pinned files. At its end
//! it records the receipt against that start state: unavailable, never passed, when its own
//! pinned content changed while it ran, when another mutation was observed meanwhile, or when
//! no start snapshot exists. A check that changed any criterion's pinned files is itself a
//! mutation: earlier evidence becomes stale and only its own unchanged post-write state is
//! qualified. The host also takes the workspace's content identity around a check (see
//! `acceptance_workspace`); when a file no active criterion pins changed (or the identity is
//! unavailable), or another change was observed while the check ran, its own receipt cannot
//! qualify and every receipt made before or during it becomes stale. A command that returned with no live process and whose end item
//! never arrives is closed as terminated with unknown effects. Every other observed mutation (unmatched commands the host
//! cannot prove read-only, input written into a running session, applied or partially applied
//! patches) advances the workspace generation.

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

use crate::acceptance_workspace::Manifest;
use crate::acceptance_workspace::changed_unpinned;
use crate::acceptance_workspace::workspace_manifest;
use crate::services::ProjectIntelligenceServices;

/// Whole-ledger bound for reading declared artifacts.
const ARTIFACT_READ_BUDGET: Duration = Duration::from_secs(5);
/// Larger files cannot be content-pinned within the budget; they are unavailable.
const MAX_HASHED_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024;
/// Artifact reader workers that may run at once, abandoned ones included.
pub(crate) const MAX_OUTSTANDING_READERS: usize = 4;
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
    /// Content identity of the workspace at start; `None` when it could not be taken.
    manifest: Option<Manifest>,
    /// Matched criteria: revision, pinned-artifact digest and checker digest at start.
    criteria: BTreeMap<u32, (u64, Option<String>, Option<String>)>,
    /// Every active criterion's pinned artifacts and checker files at start.
    governing: Governing,
}

/// Digests of every active criterion's pinned artifacts and checker files.
type Governing = BTreeMap<u32, (Option<String>, Option<String>)>;
/// Runs this process has bound a turn to; the first binding is a cold re-entry.
static ENTERED_RUNS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
pub(crate) static OUTSTANDING_READERS: AtomicUsize = AtomicUsize::new(0);
/// Commands whose end item arrived and is being accounted for right now.
static ENDS_IN_FLIGHT: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
/// How long an exited command's end item may take to arrive before the command is closed as
/// terminated with unknown effects.
pub(crate) const END_ITEM_GRACE: Duration = Duration::from_secs(10);
/// Upper bound on waiting for an end item whose accounting is already under way.
const END_ACCOUNTING_LIMIT: Duration = Duration::from_secs(60);

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

/// Forgets a command proven never to have launched (blocked by policy, or failed before its
/// handler ran). An aborted command may still be running, so it stays pending until its end.
pub(crate) async fn forget_command(
    services: &ProjectIntelligenceServices,
    turn_store: &ExtensionData,
    call_id: &str,
) {
    if let Some(binding) = turn_store.get::<TurnRunBinding>()
        && let Ok(store) = services.runtime().await
        && let Err(error) = store.finish_command(&binding.run_id, call_id).await
    {
        tracing::warn!(run_id = %binding.run_id, %error, "failed to forget an unlaunched command");
    }
}

/// A command returned with no live process. Its end item normally follows within moments
/// and accounts for it; a sandbox denial or a failure before launch emits none. After the
/// grace period an unaccounted command is closed as terminated with unknown effects: no
/// evidence, and every earlier receipt becomes stale. A command still running in the
/// background never reaches this path, and neither does an aborted one.
pub(crate) async fn command_exited(
    services: &ProjectIntelligenceServices,
    run_id: &StatefulRunId,
    call_id: &str,
    grace: Duration,
) {
    let Ok(store) = services.runtime().await else {
        return;
    };
    let started = Instant::now();
    loop {
        match store.command_pending(run_id, call_id).await {
            Ok(false) => return,
            Ok(true) => {}
            Err(error) => {
                tracing::warn!(%run_id, %error, "failed to read a pending command");
                return;
            }
        }
        let accounting = ENDS_IN_FLIGHT
            .lock()
            .is_ok_and(|in_flight| in_flight.contains(call_id));
        let waited = started.elapsed();
        if waited >= END_ACCOUNTING_LIMIT || (!accounting && waited >= grace) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    if let Err(error) = store.close_unaccounted_command(run_id, call_id).await {
        tracing::warn!(%run_id, %error, "failed to close an unaccounted command");
    }
}

/// A builtin command is about to execute: record it as pending and snapshot the checks it
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
    let Ok(store) = services.runtime().await else {
        return;
    };
    if let Err(error) = store.begin_command(&binding.run_id, call_id).await {
        tracing::warn!(run_id = %binding.run_id, %error, "failed to record a pending command");
    }
    let Ok(ledger) = store.acceptance_ledger(&binding.run_id).await else {
        return;
    };
    let matched = matched_checks(&ledger, roots, argv, cwd);
    if matched.is_empty() {
        return;
    }
    let governing = governing_digests(roots, &ledger).await;
    let manifest = workspace_manifest(roots).await;
    let start = CheckStart {
        generation: ledger.workspace_generation,
        manifest,
        criteria: matched
            .iter()
            .map(|criterion| {
                let (artifacts, checker) = governing
                    .get(&criterion.ordinal)
                    .cloned()
                    .unwrap_or_default();
                (criterion.ordinal, (criterion.revision, artifacts, checker))
            })
            .collect(),
        governing,
    };
    if let Ok(mut starts) = turn_store.get_or_init(CheckStarts::default).0.lock() {
        starts.insert(call_id.to_string(), start);
    }
}

/// Digests of every active criterion's pinned artifacts and checker files.
async fn governing_digests(roots: &[String], ledger: &AcceptanceLedger) -> Governing {
    let active = || {
        ledger
            .criteria
            .iter()
            .filter(|criterion| criterion.state == AcceptanceState::Active)
    };
    let artifacts = artifact_states(roots, active()).await;
    let checkers = checker_states(roots, active()).await;
    active()
        .map(|criterion| {
            (
                criterion.ordinal,
                (
                    pinned_digest(artifacts.get(&criterion.ordinal)),
                    pinned_digest(checkers.get(&criterion.ordinal)),
                ),
            )
        })
        .collect()
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
            // Pending is cleared only after the effects are durably accounted for; a
            // failure leaves it pending, which fails completion closed.
            let ended = command.status != CommandExecutionStatus::InProgress;
            if ended && let Ok(mut in_flight) = ENDS_IN_FLIGHT.lock() {
                in_flight.insert(command.id.clone());
            }
            let outcome = match observe_command(services, run_id, roots, turn_store, command).await
            {
                Ok(()) if ended => match services.runtime().await {
                    Ok(store) => store
                        .finish_command(run_id, &command.id)
                        .await
                        .map_err(|error| error.to_string()),
                    Err(error) => Err(error.to_string()),
                },
                outcome => outcome,
            };
            if ended && let Ok(mut in_flight) = ENDS_IN_FLIGHT.lock() {
                in_flight.remove(&command.id);
            }
            outcome
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
    // One classification decides both the run's side effects and workspace invalidation:
    // only a command the host proves read-only has no effect.
    let executed = command.status != CommandExecutionStatus::Declined;
    let effectful = executed && !crate::acceptance_effects::proven_read_only(command);
    if executed {
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
        if effectful {
            return bump(services, run_id).await;
        }
        return Ok(());
    }
    // A matched check's mutation accounting follows below; it is still a side effect.
    if effectful {
        store
            .record_side_effect(run_id)
            .await
            .map_err(|error| error.to_string())?;
    }
    let output = command.aggregated_output.clone().unwrap_or_else(|| {
        format!(
            "{}{}",
            command.stdout.as_deref().unwrap_or_default(),
            command.stderr.as_deref().unwrap_or_default()
        )
    });
    let output_digest = format!("sha256:{:x}", Sha256::digest(output.as_bytes()));
    let end = governing_digests(roots, &ledger).await;
    let end_manifest = workspace_manifest(roots).await;
    // A check's effects are accounted for only when its whole run is known: it started at
    // the current generation, and the workspace's content identity outside the active
    // criteria's pins is unchanged (file metadata is no identity). Otherwise its own receipt
    // cannot qualify and every receipt made before or during it becomes stale. A check that changed
    // pinned files is a mutation too: earlier evidence becomes stale, and only a receipt whose
    // own pinned state is unchanged is qualified against the post-write state.
    let mut stamp_generation = None;
    let mut refused = None;
    if let Some(start) = &start {
        if start.generation != ledger.workspace_generation {
            bump(services, run_id).await?;
            refused = Some(
                "another workspace change was observed while the check ran, so its effects overlap other evidence",
            );
        } else if changed_unpinned(
            roots,
            &ledger,
            start.manifest.as_ref(),
            end_manifest.as_ref(),
        ) {
            bump(services, run_id).await?;
            refused = Some(
                "the check changed files no criterion pins, or the workspace could not be identified, so its effects are unknown",
            );
        } else if start.governing != end {
            bump(services, run_id).await?;
            stamp_generation = Some(ledger.workspace_generation + 1);
        }
    }
    let evidence = matched
        .iter()
        .map(|criterion| {
            let unavailable = |detail: &str| {
                (
                    EvidenceOutcome::Unavailable,
                    Some(detail.to_string()),
                    ledger.workspace_generation,
                    None,
                    None,
                )
            };
            let started = start
                .as_ref()
                .and_then(|start| Some((start.generation, start.criteria.get(&criterion.ordinal)?)));
            let (outcome, detail, generation, digest, checker) = match started {
                None => unavailable(
                    "no start snapshot of this check exists (it started before this process observed it)",
                ),
                Some((_, (revision, _, _))) if *revision != criterion.revision => {
                    unavailable("the criterion changed while the check ran")
                }
                Some((generation, (_, start_digest, start_checker))) => {
                    let (end_digest, end_checker) =
                        end.get(&criterion.ordinal).cloned().unwrap_or_default();
                    let unchanged = start_digest.is_some()
                        && *start_digest == end_digest
                        && *start_checker == end_checker;
                    let (outcome, detail) = match (command.status, command.exit_code) {
                        (CommandExecutionStatus::Declined, _) => (
                            EvidenceOutcome::Unavailable,
                            Some("the session's approval policy declined the check, so it did not run".to_string()),
                        ),
                        (CommandExecutionStatus::Completed, Some(0)) if refused.is_some() => (
                            EvidenceOutcome::Unavailable,
                            refused.map(str::to_string),
                        ),
                        (CommandExecutionStatus::Completed, Some(0)) if !unchanged => (
                            EvidenceOutcome::Unavailable,
                            Some("the pinned artifacts or checker were unreadable or changed while the check ran".to_string()),
                        ),
                        (CommandExecutionStatus::Completed, Some(0)) => (EvidenceOutcome::Passed, None),
                        _ => (EvidenceOutcome::Failed, None),
                    };
                    (
                        outcome,
                        detail,
                        stamp_generation.unwrap_or(generation),
                        start_digest.clone(),
                        start_checker.clone(),
                    )
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
                checker_digest: checker,
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

/// An observed workspace mutation: earlier evidence becomes stale, and the run has a side
/// effect (it is no longer exempt as read-only).
async fn bump(
    services: &ProjectIntelligenceServices,
    run_id: &StatefulRunId,
) -> Result<(), String> {
    let store = services
        .runtime()
        .await
        .map_err(|error| error.to_string())?;
    store
        .record_side_effect(run_id)
        .await
        .map_err(|error| error.to_string())?;
    store
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
    file_states(
        roots,
        criteria
            .filter(|criterion| !criterion.artifacts.is_empty())
            .map(|criterion| (criterion.ordinal, criterion.artifacts.clone()))
            .collect(),
    )
    .await
}

/// The host's reading of the given criteria's checker files.
pub(crate) async fn checker_states<'a>(
    roots: &[String],
    criteria: impl Iterator<Item = &'a AcceptanceCriterion>,
) -> BTreeMap<u32, ArtifactState> {
    file_states(
        roots,
        criteria
            .filter(|criterion| !criterion.checker.is_empty())
            .map(|criterion| (criterion.ordinal, criterion.checker.clone()))
            .collect(),
    )
    .await
}

async fn file_states(
    roots: &[String],
    requests: Vec<(u32, Vec<String>)>,
) -> BTreeMap<u32, ArtifactState> {
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

/// Artifact and checker states for every active criterion of a ledger, for a completion
/// attempt.
pub(crate) async fn ledger_file_states(
    roots: &[String],
    ledger: &AcceptanceLedger,
) -> (BTreeMap<u32, ArtifactState>, BTreeMap<u32, ArtifactState>) {
    let active = || {
        ledger
            .criteria
            .iter()
            .filter(|criterion| criterion.state == AcceptanceState::Active)
    };
    (
        artifact_states(roots, active()).await,
        checker_states(roots, active()).await,
    )
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
    // Refuse non-regular files before opening: opening a FIFO with no writer would block.
    let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("it is not a regular file".to_string());
    }
    let file = open_nonblocking(&path).map_err(|error| error.to_string())?;
    // Re-check the opened handle: the path may have been replaced after the first check.
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("it is not a regular file".to_string());
    }
    if metadata.len() > MAX_HASHED_ARTIFACT_BYTES {
        return Err(format!(
            "it is larger than {MAX_HASHED_ARTIFACT_BYTES} bytes, so its content cannot be pinned"
        ));
    }
    hash_bounded(file, metadata.len(), stop).map(Some)
}

/// Opens a file for reading without blocking on special files where the platform allows it.
pub(crate) fn open_nonblocking(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0o4000);
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x0004);
    }
    options.open(path)
}

/// SHA-256 of exactly `limit` bytes of `reader`, checking `stop` before every chunk; a reader
/// that yields more or fewer bytes than `limit` changed while being read.
pub(crate) fn hash_bounded(
    mut reader: impl Read,
    limit: u64,
    stop: &dyn Fn() -> bool,
) -> Result<String, String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        if stop() {
            return Err("the read budget ran out".to_string());
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
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
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

#[cfg(test)]
#[path = "acceptance_observation_tests.rs"]
mod tests;
