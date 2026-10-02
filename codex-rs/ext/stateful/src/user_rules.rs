//! Deterministic recognition of standing rules the user marks as such in their own words.
//!
//! This is a conservative safety net, not general rule extraction: a clause counts only
//! when it carries an explicit standing marker ("never", "from now on", "end each of your
//! replies", a list introduced as preferences). A clause that also carries a task marker
//! ("yet", "for now", "during this pass") is pending: it is kept for inspection but never
//! applied. Rules phrased without markers are left to the model's verified-quote path.

/// Longest clause stored as a rule; longer clauses are never captured automatically.
pub(crate) const MAX_RULE_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuleStanding {
    /// Explicitly marked as applying to future work.
    Standing,
    /// Marked as standing but also limited to the current task; never applied.
    Pending,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuleClause {
    /// The clause exactly as the user wrote it (trimmed).
    pub(crate) text: String,
    pub(crate) standing: RuleStanding,
}

/// Imperative openings that make "never"/"always" a rule rather than narration.
const IMPERATIVE_LEADS: &[&str] = &[
    "never",
    "always",
    "please never",
    "please always",
    "so never",
    "so please never",
    "and never",
    "and always",
    "but never",
    "you should never",
    "you must never",
    "you should always",
    "you must always",
    "please don't ever",
    "don't ever",
    "do not ever",
];

const STANDING_PHRASES: &[&str] = &[
    "from now on",
    "going forward",
    "in future",
    "in the future",
    "every reply",
    "every replies",
    "each reply",
    "each of your replies",
    "every one of your replies",
    "every response",
    "each response",
    "each of your responses",
    "every answer",
    "each answer",
    "every time",
    "unless i ask",
    "unless i say",
    "unless i tell you",
    "standing",
    "preference",
    "preferences",
    "i like to work",
    "the whole week",
    "all week",
    "later sessions",
    "all our work",
    "all of our work",
];

const TASK_PHRASES: &[&str] = &[
    "yet",
    "for now",
    "right now",
    "this time",
    "today",
    "in this pass",
    "this pass",
    "during the",
    "during this",
    "orientation",
    "until",
    "before writing",
    "before starting",
    "before you start",
    "for this task",
    "this turn",
];

/// Clauses of `text` that the user explicitly marked as standing (or pending) rules.
pub(crate) fn marked_rules(text: &str) -> Vec<RuleClause> {
    let mut rules = Vec::new();
    let mut list_inherits = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            list_inherits = false;
            continue;
        }
        let is_list_item = list_item_body(trimmed).is_some();
        if !is_list_item {
            // A header such as "My working preferences for the whole week:" marks the
            // list that follows; any other prose line ends a list.
            list_inherits = trimmed.ends_with(':') && has_standing_marker(&normalize(trimmed));
        }
        let inherited = is_list_item && list_inherits;
        for clause in clauses(trimmed) {
            if clause.ends_with('?') || clause.len() > MAX_RULE_BYTES {
                continue;
            }
            let normalized = normalize(clause);
            let standing = inherited || has_standing_marker(&normalized);
            if !standing || clause.ends_with(':') {
                continue;
            }
            let pending = has_phrase(&normalized, TASK_PHRASES);
            rules.push(RuleClause {
                text: clause.to_string(),
                standing: if pending {
                    RuleStanding::Pending
                } else {
                    RuleStanding::Standing
                },
            });
        }
    }
    rules
}

/// The whole clause of `text` that contains `quote` (whitespace-insensitive, case-sensitive),
/// when exactly one clause contains it. A quote spanning clauses matches none.
pub(crate) fn clause_containing(text: &str, quote: &str) -> Option<String> {
    let quote = collapse_whitespace(quote);
    if quote.is_empty() {
        return None;
    }
    let mut found = text
        .lines()
        .flat_map(|line| clauses(line.trim()))
        .filter(|clause| collapse_whitespace(clause).contains(&quote));
    let clause = found.next()?;
    found.next().is_none().then(|| clause.to_string())
}

/// Splits one line into sentence clauses, keeping their punctuation; a list marker is
/// part of its item's first clause.
fn clauses(line: &str) -> Vec<&str> {
    let mut clauses = Vec::new();
    let mut start = 0;
    let bytes = line.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        let terminal = matches!(byte, b'.' | b'!' | b'?');
        let at_boundary = bytes.get(index + 1).is_none_or(u8::is_ascii_whitespace);
        if terminal && at_boundary && !ends_with_abbreviation(&line[start..=index]) {
            let clause = line[start..=index].trim();
            if !clause.is_empty() && !is_list_marker_only(clause) {
                clauses.push(clause);
                start = index + 1;
            }
        }
    }
    let rest = line[start..].trim();
    if !rest.is_empty() {
        clauses.push(rest);
    }
    clauses
}

fn ends_with_abbreviation(clause: &str) -> bool {
    let lower = clause.to_ascii_lowercase();
    ["e.g.", "i.e.", "etc.", "vs.", "approx."]
        .iter()
        .any(|abbreviation| lower.ends_with(abbreviation))
}

/// "1." or "P1." alone is a list marker, not a clause.
fn is_list_marker_only(clause: &str) -> bool {
    let body = clause.trim_end_matches(['.', ')', ':']);
    !body.is_empty()
        && body.len() <= 3
        && body
            .trim_start_matches(['p', 'P'])
            .chars()
            .all(|character| character.is_ascii_digit())
}

/// The text after a list marker ("1.", "2)", "-", "*", "P1:"), when the line is a list item.
fn list_item_body(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix(['-', '*', '\u{2022}']) {
        return rest.starts_with(' ').then_some(rest);
    }
    let marker_end = line.find([' ', '\t'])?;
    let marker = &line[..marker_end];
    let core = marker.trim_end_matches(['.', ')', ':']);
    let numbered = marker.len() > core.len()
        && !core.is_empty()
        && core.len() <= 3
        && core
            .trim_start_matches(['p', 'P'])
            .chars()
            .all(|character| character.is_ascii_digit())
        && core.chars().any(|character| character.is_ascii_digit());
    numbered.then(|| &line[marker_end..])
}

fn has_standing_marker(normalized: &str) -> bool {
    let body = strip_list_marker(normalized);
    if body.starts_with("never mind") {
        return false;
    }
    IMPERATIVE_LEADS
        .iter()
        .any(|lead| body.starts_with(&format!("{lead} ")))
        || [
            " please never ",
            " so never ",
            " and never ",
            " please always ",
        ]
        .iter()
        .any(|phrase| format!(" {body} ").contains(phrase))
        || has_phrase(normalized, STANDING_PHRASES)
}

fn strip_list_marker(normalized: &str) -> &str {
    match normalized.split_once(' ') {
        Some((first, rest))
            if first.len() <= 3
                && first
                    .trim_start_matches('p')
                    .chars()
                    .all(|character| character.is_ascii_digit()) =>
        {
            rest
        }
        _ => normalized,
    }
}

fn has_phrase(normalized: &str, phrases: &[&str]) -> bool {
    let padded = format!(" {normalized} ");
    phrases
        .iter()
        .any(|phrase| padded.contains(&format!(" {phrase} ")))
}

/// Lowercase words separated by single spaces; punctuation becomes a separator.
fn normalize(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_alphanumeric() || character == '\'' {
                character.to_lowercase().next().unwrap_or(character)
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
#[path = "user_rules_tests.rs"]
mod tests;
