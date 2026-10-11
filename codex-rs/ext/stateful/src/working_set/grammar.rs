//! Structural extraction grammar: clause splitting, markdown-neutral `file:line` locators,
//! strict `label: value unit` output lines, command statements, tool families and the shells'
//! own error formats. Pure text functions with no word list that classifies meaning; covered
//! by `GRAMMAR_VERSION`.

use codex_utils_string::take_bytes_at_char_boundary;

pub(super) const MAX_INSPECTED_BYTES: usize = 64 * 1024;
const MAX_CLAUSES: usize = 32;
pub(super) const NEXT_MARKERS: [&str; 2] = ["next step:", "next:"];
/// The shells' own error formats for a program that cannot run.
pub(super) const UNAVAILABLE: &str =
    "command not found|is not recognized as|permission denied|no such file or directory";
const SHELLS: &str = "bash|sh|zsh|pwsh|pwsh.exe|powershell|powershell.exe|cmd|cmd.exe";
/// Tool families by function: a successful command replaces an unavailable program only
/// within its family. This names tools, not task meaning.
const FAMILIES: [&str; 4] = [
    "rg|grep|egrep|git grep|findstr|select-string|sls|ag|ack",
    "fd|find|ls|dir|get-childitem|gci|git ls-files|tree",
    "cat|type|get-content|gc|head|tail|more|less",
    "sed|awk|perl",
];
/// Wrapping and trailing punctuation around a token, trimmed before locator checks.
const WRAPPING: &str = "`'\"()[]{}*,;:!?.<>";

pub(super) fn any_in(markers: &str, lower: &str) -> bool {
    markers.split('|').any(|marker| lower.contains(marker))
}

fn one_of(words: &str, word: &str) -> bool {
    words.split('|').any(|candidate| candidate == word)
}

/// Byte ranges of trimmed lines, then sentences, in the inspected head and tail of `text`.
pub(super) fn clauses(text: &str, uninspected: &mut u32) -> Vec<(usize, usize)> {
    let half = MAX_INSPECTED_BYTES / 2;
    let segments = if text.len() > MAX_INSPECTED_BYTES {
        *uninspected += 1;
        let head = take_bytes_at_char_boundary(text, half).len();
        let mut tail = text.len() - half;
        while !text.is_char_boundary(tail) {
            tail += 1;
        }
        vec![(0, head), (tail, text.len())]
    } else {
        vec![(0, text.len())]
    };
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    for (seg_start, seg_end) in segments {
        let mut start = seg_start;
        for i in seg_start..=seg_end {
            let boundary = i == seg_end
                || bytes[i] == b'\n'
                || (matches!(bytes[i], b'.' | b'?' | b'!' | b';')
                    && bytes.get(i + 1).is_none_or(u8::is_ascii_whitespace)
                    && !next_marker_line(&text[start..i]));
            if boundary {
                let end = if i < seg_end && bytes[i] != b'\n' {
                    i + 1
                } else {
                    i
                };
                let clause = &text[start..end];
                let lead = clause.len() - clause.trim_start().len();
                let trimmed = clause.trim();
                if !trimmed.is_empty() && out.len() < MAX_CLAUSES {
                    out.push((start + lead, start + lead + trimmed.len()));
                }
                start = end;
            }
        }
    }
    out
}

/// Removes markdown emphasis, code ticks and link targets (`[text](target)` keeps `text`).
/// Literal characters inside values are untouched.
pub(super) fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        match after
            .find("](")
            .and_then(|mid| after[mid + 2..].find(')').map(|end| (mid, end)))
        {
            Some((mid, end)) if !after[..mid].contains('[') => {
                out.push_str(&rest[..open]);
                out.push_str(&after[..mid]);
                rest = &after[mid + 2 + end + 1..];
            }
            _ => {
                out.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.replace("**", "").replace('`', "")
}

/// A next-step line is kept whole rather than split into sentences.
fn next_marker_line(prefix: &str) -> bool {
    let line = plain(prefix).trim_start().to_lowercase();
    NEXT_MARKERS.iter().any(|m| marker_end(&line, m).is_some())
}

pub(super) fn marker_end(lower: &str, marker: &str) -> Option<usize> {
    let stripped = lower.trim_start_matches(|c: char| c == '-' || c == '*' || c.is_whitespace());
    stripped
        .starts_with(marker)
        .then(|| lower.len() - stripped.len() + marker.len())
}

pub(super) fn mentions(text: Option<&str>, path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    text.is_some_and(|text| !name.is_empty() && (text.contains(path) || text.contains(name)))
}

fn trim_token(word: &str) -> &str {
    word.trim_matches(|c: char| WRAPPING.contains(c))
}

/// Length of a leading `1,234.5` number, if the token starts with one.
fn number_len(token: &str) -> usize {
    let len = token
        .find(|c: char| !(c.is_ascii_digit() || c == ',' || c == '.'))
        .unwrap_or(token.len());
    let len = token[..len].trim_end_matches([',', '.']).len();
    if token.starts_with(|c: char| c.is_ascii_digit()) {
        len
    } else {
        0
    }
}

pub(super) fn is_locator(token: &str) -> bool {
    trim_token(token)
        .rsplit_once(':')
        .is_some_and(|(path, line)| {
            !line.is_empty()
                && line.len() <= 7
                && line.bytes().all(|b| b.is_ascii_digit())
                && path
                    .rsplit(['/', '\\'])
                    .next()
                    .is_some_and(|name| name.contains('.'))
                && !path.contains("://")
        })
}

/// A strict `label: <number> <unit>` (or `<number><unit>`) output line; returns the label and
/// the value. The unit is any short alphabetic, `%` or `<word>/<word>` token, and nothing may
/// follow it.
pub(super) fn labeled_value(line: &str) -> Option<(&str, String)> {
    let (label, value) = line.trim().split_once(": ")?;
    let value = value.trim();
    let label_ok = (1..=40).contains(&label.len())
        && label.starts_with(char::is_alphabetic)
        && label
            .chars()
            .all(|c| c.is_alphanumeric() || " _-".contains(c));
    let words: Vec<&str> = value.split_whitespace().collect();
    let digits = number_len(words.first()?);
    let unit = match words.as_slice() {
        [number] if digits < number.len() => &number[digits..],
        [number, unit] if digits == number.len() => unit,
        _ => return None,
    };
    let unit_ok = (1..=16).contains(&unit.len())
        && unit
            .chars()
            .all(|c| c.is_alphabetic() || c == '%' || c == '/');
    (label_ok && digits > 0 && unit_ok).then(|| (label, value.to_string()))
}

/// A bounded head of a command statement: its first line, at most `max` bytes, marked
/// `[...]` when anything was left out.
pub(super) fn command_head(statement: &str, max: usize) -> String {
    let statement = statement.trim();
    let line = statement.lines().next().unwrap_or_default().trim_end();
    let head = take_bytes_at_char_boundary(line, max);
    if head.len() < statement.len() {
        format!("{head} [...]")
    } else {
        head.to_string()
    }
}

pub(super) fn family(program: &str) -> Option<usize> {
    FAMILIES.iter().position(|names| one_of(names, program))
}

/// `file:line` locators of a markdown-neutral clause.
pub(super) fn locators(clause: &str) -> Vec<String> {
    clause
        .split_whitespace()
        .map(trim_token)
        .filter(|token| is_locator(token))
        .map(str::to_string)
        .collect()
}

/// The command a shell wrapper runs, or the joined argv.
pub(super) fn shell_body(argv: &[String]) -> String {
    let program = argv
        .first()
        .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_lowercase());
    if program.is_some_and(|p| one_of(SHELLS, &p))
        && let Some(flag) = argv
            .iter()
            .position(|a| matches!(a.as_str(), "-c" | "-lc" | "-Command" | "/c" | "/C"))
        && let Some(body) = argv.get(flag + 1)
    {
        return body.clone();
    }
    argv.join(" ")
}

/// Statements of a compound command that run a program: variable and environment
/// assignments (`X=1`, `$env:X = 1`, `set X=1`, `$p = ...`) and here-string bodies are not.
pub(super) fn statements(command: &str) -> Vec<&str> {
    // Separators inside quotes (a search pattern `'a|b'`) do not split statements.
    let mut parts = Vec::new();
    let (mut quote, mut start) = (None, 0);
    for (at, c) in command.char_indices() {
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(open), _) if c == open => quote = None,
            (None, ';' | '|' | '&' | '\n') => {
                parts.push(&command[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&command[start..]);
    parts
        .into_iter()
        .filter_map(|statement| {
            // `X=1 program ...` runs `program` with an environment prefix.
            let statement = statement.trim_start();
            let mut rest = statement;
            while let Some(word) = rest.split_whitespace().next()
                && word.contains('=')
                && !word.starts_with(['-', '$'])
            {
                rest = rest[word.len()..].trim_start();
            }
            let mut words = rest.split_whitespace();
            let first = words.next()?;
            let second = words.next().unwrap_or_default();
            let assignment = first.starts_with(['$', '@', '\'', '"', '#'])
                || second.starts_with('=')
                || (first.eq_ignore_ascii_case("set") && second.contains('='));
            (!assignment).then(|| rest.trim_end())
        })
        .collect()
}

/// A command's identity for "the same command again": its program statements only.
pub(super) fn command_key(command: &str) -> String {
    statements(command).join("; ")
}

/// Program of each statement of a compound command (`git` keeps its subcommand).
pub(super) fn programs(command: &str) -> Vec<String> {
    statements(command)
        .into_iter()
        .filter_map(|statement| {
            let mut words = statement.split_whitespace();
            let first = words.next()?;
            let name = first
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(first)
                .to_lowercase();
            let name = name.trim_end_matches(".exe").to_string();
            Some(match words.next() {
                Some(sub) if name == "git" => format!("git {}", sub.to_lowercase()),
                _ => name,
            })
        })
        .collect()
}

/// The program an error output says is unavailable, from the error text itself.
pub(super) fn unavailable_program(output: &str) -> Option<String> {
    let lower = output.to_lowercase();
    if !any_in(UNAVAILABLE, &lower) {
        return None;
    }
    for line in output.lines() {
        let quoted = line
            .split_once("The term '")
            .or_else(|| line.split_once('\''))
            .and_then(|(_, rest)| rest.split_once('\''))
            .filter(|(_, tail)| tail.trim_start().starts_with("is not recognized"));
        // A shell syntax error naming an assignment is not a missing program.
        if let Some((name, _)) = quoted.filter(|(name, _)| !name.contains(['=', '$', ' '])) {
            return Some(name.to_lowercase());
        }
        if let Some((head, _)) = line.split_once(": command not found") {
            return head.rsplit([':', ' ']).next().map(str::to_lowercase);
        }
    }
    None
}
