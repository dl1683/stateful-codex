//! Host classification of observed actions for the read-only exemption. Only what the host can
//! prove has no effect counts as read-only: a single plain invocation of a small allowlist of
//! programs that cannot write, run other programs, read effect-capable configuration or
//! interpret callbacks (no shell operators, redirection, expansion or substitution), and tools
//! that only read or keep the run's own bookkeeping. Everything else, unknown included, is a
//! side effect; one classification serves both effect recording and workspace invalidation.

use codex_extension_api::ToolName;
use codex_protocol::items::CommandExecutionItem;

/// Shell flags whose next (final) argument is the script the model asked to run.
const SCRIPT_FLAGS: &[&str] = &["-c", "-lc", "-Command", "-command", "/c", "/C"];

/// Programs that only read and have no option that writes a file or runs another program,
/// reads configuration that could, or interprets a callback (so no `git`, `rg`, `find`,
/// `diff`, `sed` or `awk`).
const READ_ONLY_PROGRAMS: &[&str] = &[
    "basename",
    "cat",
    "cmp",
    "cut",
    "df",
    "dirname",
    "du",
    "echo",
    "grep",
    "head",
    "ls",
    "md5sum",
    "nl",
    "pwd",
    "readlink",
    "realpath",
    "sha256sum",
    "stat",
    "tail",
    "true",
    "uname",
    "wc",
    "whoami",
];

/// Tools that only read, or that keep the run's own bookkeeping (its obligations, steering
/// and acceptance records). Project-memory writers and index refreshes are effects. Commands
/// and patches are classified from their observed items instead.
const READ_ONLY_TOOLS: &[&str] = &[
    "blackboard_query",
    "context_map_query",
    "conversation_read",
    "evidence_read",
    "list_mcp_resource_templates",
    "list_mcp_resources",
    "memory_read",
    "obligation_update",
    "read_mcp_resource",
    "request_user_input",
    "stateful_acceptance_update",
    "stateful_run_read",
    "stateful_run_update",
    "steering_query",
    "steering_reconcile",
    "tool_search",
    "update_plan",
    "view_image",
    "web_search",
];

/// Command tools whose effects are classified from the command items they produce (an
/// execution that never produces an item is closed as a side effect).
const COMMAND_TOOLS: &[&str] = &["exec_command", "shell", "shell_command", "local_shell"];

/// Whether a finished command is proven read-only.
pub(crate) fn proven_read_only(command: &CommandExecutionItem) -> bool {
    let script = match command.command.as_slice() {
        [.., flag, script] if SCRIPT_FLAGS.contains(&flag.as_str()) => script.clone(),
        argv => argv.join(" "),
    };
    read_only_script(&script)
}

/// Whether a script is one plain invocation of a read-only program.
pub(crate) fn read_only_script(script: &str) -> bool {
    // No operators, redirection, expansion, substitution, escapes or line breaks anywhere,
    // quoted or not.
    if script
        .chars()
        .any(|character| ";&|<>$`(){}\\!\n\r#~".contains(character))
    {
        return false;
    }
    let Some(argv) = split_words(script) else {
        return false;
    };
    argv.first()
        .is_some_and(|program| READ_ONLY_PROGRAMS.contains(&program.as_str()))
}

/// Splits on whitespace, honoring single and double quotes; `None` for an unterminated quote.
fn split_words(script: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote = None;
    for character in script.chars() {
        match (quote, character) {
            (Some(open), character) if character == open => quote = None,
            (Some(_), character) => current.push(character),
            (None, '\'' | '"') => {
                quote = Some(character);
                in_word = true;
            }
            (None, character) if character.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            (None, character) => {
                current.push(character);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return None;
    }
    if in_word {
        words.push(current);
    }
    Some(words)
}

/// Whether a finished tool call may have had effects outside the run's own bookkeeping.
pub(crate) fn tool_has_effects(tool_name: &ToolName) -> bool {
    !(tool_name.is_default_namespace()
        && (READ_ONLY_TOOLS.contains(&tool_name.name.as_str())
            || COMMAND_TOOLS.contains(&tool_name.name.as_str())))
}

#[cfg(test)]
#[path = "acceptance_effects_tests.rs"]
mod tests;
