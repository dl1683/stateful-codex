//! Host-observed grounding for recorded recipes.
//!
//! A recipe is worth reusing only when the host saw it work. Every builtin command a
//! thread runs is observed at start (its script and working directory) and kept once it
//! completes successfully; failed commands stay diagnostic and are never kept. When the
//! model records a `Recipe:` fact, a backticked command in it is matched against those
//! observations, and a match stores "last observed successful under these conditions":
//! the command, working directory, executable, observation identity and time, and the
//! fingerprints of dependency manifests beside it. The recipe is not guaranteed to work
//! forever; `recipe_applicability` rechecks those conditions before it is offered again.
//!
//! The lifecycle reports whether a command completed, not its exit status. The exit
//! status is confirmed from the command's own output in the conversation when it is
//! there (a direct command); a command known to have exited non-zero never grounds a
//! recipe, and one whose status is not visible (inside a code-mode cell) is stored with
//! `exitStatus: unknown` rather than claimed successful.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;

use codex_protocol::models::ResponseItem;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;

/// Successful commands kept per thread.
const MAX_OBSERVED_COMMANDS: usize = 64;
/// Commands started but not yet finished, per process.
const MAX_PENDING_COMMANDS: usize = 256;
/// Dependency manifests whose fingerprints describe a recipe's environment.
const MANIFESTS: &[&str] = &[
    "pyproject.toml",
    "uv.lock",
    "poetry.lock",
    "requirements.txt",
    "package.json",
    "package-lock.json",
    "Cargo.toml",
    "Cargo.lock",
];
/// Larger manifests are not fingerprinted.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Argument spellings that carry credentials and must never be stored in a recipe.
const CREDENTIAL_MARKERS: &[&str] = &[
    "password",
    "passwd",
    "token=",
    "secret",
    "api_key",
    "apikey",
    "api-key",
    "authorization:",
    "bearer ",
];

/// One successful command the host observed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObservedCommand {
    pub(crate) turn_id: String,
    pub(crate) call_id: String,
    pub(crate) script: String,
    pub(crate) cwd: PathBuf,
}

/// The stored conditions under which a recipe was last observed to succeed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecipeObservation {
    pub(crate) command: String,
    pub(crate) cwd: String,
    /// The command's first word as typed (`python`, `.venv/bin/python`, `uv`).
    pub(crate) executable: String,
    pub(crate) observed_turn_id: String,
    pub(crate) observed_call_id: String,
    pub(crate) observed_at_ms: i64,
    pub(crate) exit_status: ExitStatus,
    pub(crate) manifests: Vec<ManifestFingerprint>,
}

/// What the conversation shows about how an observed command exited.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ExitStatus {
    Zero,
    NonZero,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManifestFingerprint {
    pub(crate) path: String,
    pub(crate) sha256: String,
}

/// Commands observed by this process, by thread.
#[derive(Clone, Default)]
pub(crate) struct ObservedCommands {
    inner: std::sync::Arc<std::sync::Mutex<ObservedState>>,
}

#[derive(Default)]
struct ObservedState {
    pending: HashMap<String, (String, ObservedCommand)>,
    pending_order: VecDeque<String>,
    succeeded: HashMap<String, VecDeque<ObservedCommand>>,
}

impl ObservedCommands {
    /// Remembers a command at start; it is kept only if it later succeeds.
    pub(crate) fn started(&self, thread_id: &str, command: ObservedCommand) {
        let mut state = self.lock();
        let key = command.call_id.clone();
        if state
            .pending
            .insert(key.clone(), (thread_id.to_string(), command))
            .is_none()
        {
            state.pending_order.push_back(key);
        }
        while state.pending_order.len() > MAX_PENDING_COMMANDS {
            if let Some(oldest) = state.pending_order.pop_front() {
                state.pending.remove(&oldest);
            }
        }
    }

    /// Settles a started command: kept on success, forgotten otherwise.
    pub(crate) fn finished(&self, call_id: &str, succeeded: bool) {
        let mut state = self.lock();
        let Some((thread_id, command)) = state.pending.remove(call_id) else {
            return;
        };
        state.pending_order.retain(|pending| pending != call_id);
        if !succeeded {
            return;
        }
        let kept = state.succeeded.entry(thread_id).or_default();
        kept.push_back(command);
        while kept.len() > MAX_OBSERVED_COMMANDS {
            kept.pop_front();
        }
    }

    /// The most recent successful command of the thread that a backticked command in
    /// `content` names.
    pub(crate) fn matching(&self, thread_id: &str, content: &str) -> Option<ObservedCommand> {
        let candidates = backticked(content);
        if candidates.is_empty() {
            return None;
        }
        let state = self.lock();
        state
            .succeeded
            .get(thread_id)?
            .iter()
            .rev()
            .find_map(|command| {
                let script = normalized(&command.script);
                candidates
                    .iter()
                    .any(|candidate| script.contains(candidate.as_str()))
                    .then(|| command.clone())
            })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ObservedState> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// The script a resolved argv runs: the payload of a shell wrapper, or the argv itself.
pub(crate) fn command_script(argv: &[String]) -> String {
    let lowered = argv
        .iter()
        .map(|argument| argument.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let wrapper_flag = lowered.iter().position(|argument| {
        matches!(
            argument.as_str(),
            "-c" | "-lc" | "-command" | "/c" | "-encodedcommand"
        )
    });
    match wrapper_flag {
        Some(flag) if flag + 1 < argv.len() && lowered[flag] != "-encodedcommand" => {
            argv[flag + 1..].join(" ")
        }
        _ => argv.join(" "),
    }
}

/// Whether `content` contains a credential-like argument that must not be stored.
pub(crate) fn carries_credentials(content: &str) -> bool {
    let lowered = content.to_ascii_lowercase();
    CREDENTIAL_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
}

/// Builds the stored observation for a matched command, fingerprinting the dependency
/// manifests in its working directory. Blocking: call from a blocking context.
pub(crate) fn observe_recipe(
    command: &ObservedCommand,
    observed_at_ms: i64,
    exit_status: ExitStatus,
) -> RecipeObservation {
    RecipeObservation {
        command: command.script.clone(),
        cwd: command.cwd.display().to_string(),
        executable: first_word(&command.script),
        observed_turn_id: command.turn_id.clone(),
        observed_call_id: command.call_id.clone(),
        observed_at_ms,
        exit_status,
        manifests: manifest_fingerprints(&command.cwd),
    }
}

pub(crate) fn manifest_fingerprints(cwd: &Path) -> Vec<ManifestFingerprint> {
    MANIFESTS
        .iter()
        .filter_map(|name| {
            let path = cwd.join(name);
            let metadata = std::fs::metadata(&path).ok()?;
            if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
                return None;
            }
            let bytes = std::fs::read(&path).ok()?;
            Some(ManifestFingerprint {
                path: (*name).to_string(),
                sha256: format!("{:x}", Sha256::digest(&bytes)),
            })
        })
        .collect()
}

/// The first word of a script, without surrounding quotes or a PowerShell call operator.
pub(crate) fn first_word(script: &str) -> String {
    let trimmed = script.trim_start().trim_start_matches("& ").trim_start();
    let word = match trimmed.chars().next() {
        Some(quote @ ('"' | '\'')) => trimmed[1..].split(quote).next().unwrap_or_default(),
        _ => trimmed.split_whitespace().next().unwrap_or_default(),
    };
    word.to_string()
}

/// The exit status the conversation shows for the direct command `call_id`.
pub(crate) fn exit_status(history: &[ResponseItem], call_id: &str) -> ExitStatus {
    history
        .iter()
        .rev()
        .find_map(|item| match item {
            ResponseItem::FunctionCallOutput {
                call_id: Some(output_call_id),
                output,
                ..
            } if output_call_id == call_id => Some(output.body.to_text().unwrap_or_default()),
            _ => None,
        })
        .and_then(|text| {
            text.lines().find_map(|line| {
                line.strip_prefix("Process exited with code ")
                    .map(|code| match code.trim() {
                        "0" => ExitStatus::Zero,
                        _ => ExitStatus::NonZero,
                    })
            })
        })
        .unwrap_or(ExitStatus::Unknown)
}

fn backticked(content: &str) -> Vec<String> {
    content
        .split('`')
        .skip(1)
        .step_by(2)
        .map(normalized)
        .filter(|command| command.len() >= 4)
        .collect()
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
#[path = "recipe_capture_tests.rs"]
mod tests;
