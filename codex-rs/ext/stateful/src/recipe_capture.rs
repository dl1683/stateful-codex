//! Host-observed grounding for recorded recipes.
//!
//! A recipe is worth reusing only when the host saw it work. Every builtin command a
//! thread runs is observed at start (its script and working directory). When it completes,
//! a simple command (one executable with its arguments: no command chaining, pipes,
//! substitution or credential-like arguments) has its conditions captured at that moment:
//! the resolved executable's identity and the dependency manifests beside it. Compound and
//! failed commands are never kept, so they can never ground a recipe.
//!
//! When the model records a `Recipe:` fact, it must name exactly one backticked command,
//! and that command must equal a kept command character for character (surrounding
//! whitespace aside); a recipe naming several commands is not grounded. A match stores "last observed successful under
//! these conditions". The lifecycle reports whether a command completed, not its exit
//! status, so the exit status is read from the host's own header in the command's output
//! (the lines before `Output:`) when the conversation holds it; a command shown to exit
//! non-zero never grounds a recipe, and one whose status is not visible is stored with
//! `exitStatus: unknown`, never claimed successful. Paths are checked on this host: a
//! command whose working directory is not a local directory (a remote executor) is not
//! grounded.

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
/// Larger manifests are recorded as present but not fingerprinted.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Spellings that carry credentials and must never be stored in a recipe.
const CREDENTIAL_MARKERS: &[&str] = &[
    "password",
    "passwd",
    "--pass",
    "credential",
    "token",
    "secret",
    "api_key",
    "apikey",
    "api-key",
    "private_key",
    "authorization",
    "bearer",
    "--key",
];
/// Shell syntax that makes a script more than one simple command.
const COMPOUND_SYNTAX: &[&str] = &["&&", "||", ";", "|", "&", "\n", "`", "$(", ">", "<"];

/// One command the host observed, with the conditions captured when it completed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObservedCommand {
    pub(crate) turn_id: String,
    pub(crate) call_id: String,
    pub(crate) script: String,
    pub(crate) cwd: PathBuf,
    /// Set when the command completes: the moment and conditions it ran under.
    pub(crate) completed: Option<CompletedConditions>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompletedConditions {
    pub(crate) at_ms: i64,
    pub(crate) executable: ExecutableIdentity,
    pub(crate) manifests: Vec<ManifestFingerprint>,
}

/// The stored conditions under which a recipe was last observed to succeed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecipeObservation {
    pub(crate) command: String,
    pub(crate) cwd: String,
    pub(crate) executable: ExecutableIdentity,
    pub(crate) observed_turn_id: String,
    pub(crate) observed_call_id: String,
    /// When the command completed, not when the recipe was recorded.
    pub(crate) observed_at_ms: i64,
    pub(crate) exit_status: ExitStatus,
    /// Every known manifest present in the working directory when the command completed.
    pub(crate) manifests: Vec<ManifestFingerprint>,
}

/// The executable a command resolved to, by path, size and modification time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecutableIdentity {
    /// The command's first word as typed (`python`, `.venv/bin/python`, `uv`).
    pub(crate) typed: String,
    pub(crate) resolved: String,
    pub(crate) bytes: u64,
    pub(crate) modified_ms: Option<i64>,
}

/// What the host's output header shows about how an observed command exited.
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
    /// `None` when the manifest is present but too large to fingerprint.
    pub(crate) sha256: Option<String>,
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
    /// Remembers a simple command at start; compound commands can never ground a recipe
    /// and are not tracked.
    pub(crate) fn started(&self, thread_id: &str, command: ObservedCommand) {
        if !is_simple_command(&command.script) {
            return;
        }
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

    /// Removes a started command once its call finishes.
    pub(crate) fn finished(&self, call_id: &str) -> Option<(String, ObservedCommand)> {
        let mut state = self.lock();
        let finished = state.pending.remove(call_id)?;
        state.pending_order.retain(|pending| pending != call_id);
        Some(finished)
    }

    /// Keeps a completed command whose conditions were captured.
    pub(crate) fn keep(&self, thread_id: String, command: ObservedCommand) {
        if command.completed.is_none() {
            return;
        }
        let mut state = self.lock();
        let kept = state.succeeded.entry(thread_id).or_default();
        kept.push_back(command);
        while kept.len() > MAX_OBSERVED_COMMANDS {
            kept.pop_front();
        }
    }

    /// The most recent kept command of the thread that the recipe's single backticked
    /// command names exactly.
    pub(crate) fn matching(&self, thread_id: &str, content: &str) -> Option<ObservedCommand> {
        let named = single_recipe_command(content)?;
        let state = self.lock();
        state
            .succeeded
            .get(thread_id)?
            .iter()
            .rev()
            .find(|command| command.script.trim() == named)
            .cloned()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ObservedState> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Captures the conditions of a command that just completed: the resolved executable and
/// the manifests in its working directory. `None` when the working directory is not a
/// local directory or the executable does not resolve. Blocking.
pub(crate) fn capture_conditions(
    command: &ObservedCommand,
    at_ms: i64,
) -> Option<CompletedConditions> {
    if !command.cwd.is_dir() {
        return None;
    }
    Some(CompletedConditions {
        at_ms,
        executable: resolve_executable(&command.cwd, &first_word(&command.script))?,
        manifests: manifest_fingerprints(&command.cwd),
    })
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

/// Whether a command contains a credential-like argument that must not be stored:
/// credential words, URL userinfo, a `-u user:secret` pair, or an attached short password
/// flag such as `-pSecret`.
pub(crate) fn carries_credentials(command: &str) -> bool {
    let lowered = command.to_ascii_lowercase();
    let words = lowered
        .split_whitespace()
        .map(|word| word.trim_matches(['"', '\'']))
        .collect::<Vec<_>>();
    CREDENTIAL_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
        || lowered.split("://").skip(1).any(|rest| {
            rest.split(['/', ' '])
                .next()
                .is_some_and(|authority| authority.contains('@'))
        })
        || words.iter().any(|word| {
            word.starts_with("-p") && word.len() > 2 && word.chars().nth(2) != Some('=')
        })
        || words
            .windows(2)
            .any(|pair| matches!(pair, ["-u" | "--user", value] if value.contains(':')))
}

/// The backticked commands a recipe names, each trimmed.
pub(crate) fn recipe_commands(content: &str) -> impl Iterator<Item = &str> {
    content
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::trim)
        .filter(|command| !command.is_empty())
}

/// The one command a recipe names, or `None` when it names none or several.
pub(crate) fn single_recipe_command(content: &str) -> Option<&str> {
    let mut commands = recipe_commands(content);
    let command = commands.next()?;
    commands.next().is_none().then_some(command)
}

/// A single executable with arguments: no chaining, pipes, redirection, substitution,
/// environment assignment, variable first word, or credential-like argument.
pub(crate) fn is_simple_command(script: &str) -> bool {
    let first = first_word(script);
    // A leading PowerShell call operator runs one executable; it is not chaining.
    let trimmed = script.trim_start();
    let body = trimmed.strip_prefix("& ").unwrap_or(trimmed);
    !first.is_empty()
        && !COMPOUND_SYNTAX.iter().any(|syntax| body.contains(syntax))
        && !first.starts_with('$')
        && !first.contains('=')
        && !carries_credentials(script)
}

/// Builds the stored observation for a matched, completed command.
pub(crate) fn observation(
    command: &ObservedCommand,
    exit_status: ExitStatus,
) -> Option<RecipeObservation> {
    let completed = command.completed.as_ref()?;
    Some(RecipeObservation {
        command: command.script.clone(),
        cwd: command.cwd.display().to_string(),
        executable: completed.executable.clone(),
        observed_turn_id: command.turn_id.clone(),
        observed_call_id: command.call_id.clone(),
        observed_at_ms: completed.at_ms,
        exit_status,
        manifests: completed.manifests.clone(),
    })
}

/// Every known manifest present in `cwd`, fingerprinted when small enough. Blocking.
pub(crate) fn manifest_fingerprints(cwd: &Path) -> Vec<ManifestFingerprint> {
    MANIFESTS
        .iter()
        .filter_map(|name| {
            let path = cwd.join(name);
            let metadata = std::fs::metadata(&path).ok()?;
            if !metadata.is_file() {
                return None;
            }
            let sha256 = (metadata.len() <= MAX_MANIFEST_BYTES)
                .then(|| std::fs::read(&path).ok())
                .flatten()
                .map(|bytes| format!("{:x}", Sha256::digest(&bytes)));
            Some(ManifestFingerprint {
                path: (*name).to_string(),
                sha256,
            })
        })
        .collect()
}

/// Resolves the executable a command starts with: a path relative to `cwd`, or a bare
/// name on this host's `PATH`. Blocking.
pub(crate) fn resolve_executable(cwd: &Path, typed: &str) -> Option<ExecutableIdentity> {
    if typed.is_empty() {
        return None;
    }
    let candidates = |base: PathBuf| {
        let mut names = vec![base.clone()];
        if cfg!(windows) && base.extension().is_none() {
            names.extend(["exe", "cmd", "bat"].map(|extension| base.with_extension(extension)));
        }
        names
    };
    let resolved = if typed.contains(['/', '\\']) {
        candidates(cwd.join(typed))
            .into_iter()
            .find(|path| path.is_file())?
    } else {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .flat_map(|directory| candidates(directory.join(typed)))
            .find(|path| path.is_file())?
    };
    let metadata = std::fs::metadata(&resolved).ok()?;
    Some(ExecutableIdentity {
        typed: typed.to_string(),
        resolved: resolved.display().to_string(),
        bytes: metadata.len(),
        modified_ms: metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok()),
    })
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

/// The exit status the host's header shows for the direct command `call_id`. Only the
/// header lines before `Output:` are read, so a command cannot report its own status.
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
            text.lines()
                .take_while(|line| *line != "Output:")
                .find_map(|line| {
                    line.strip_prefix("Process exited with code ")
                        .map(|code| match code.trim() {
                            "0" => ExitStatus::Zero,
                            _ => ExitStatus::NonZero,
                        })
                })
        })
        .unwrap_or(ExitStatus::Unknown)
}

#[cfg(test)]
#[path = "recipe_capture_tests.rs"]
mod tests;
