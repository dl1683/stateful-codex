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

/// What a list header says about the items below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HeaderScope {
    /// "My working preferences for the whole week:"
    Standing,
    /// "For this task:"
    Pending,
    /// "The assistant suggested these preferences:" - the items are not the user's rules.
    Reported,
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

/// Markers that make a rule's standing unambiguous even next to a task word.
const STRONG_STANDING_PHRASES: &[&str] = &[
    "from now on",
    "going forward",
    "in future",
    "in the future",
    "the whole week",
    "all week",
    "later sessions",
    "all our work",
    "all of our work",
];

/// Reported speech: the sentence relays someone else's words or advice.
const REPORTED_SPEECH_PHRASES: &[&str] = &[
    "told me",
    "told us",
    "you told",
    "said to",
    "says to",
    "you said",
    "you suggested",
    "you recommended",
    "it suggested",
    "it recommended",
    "suggested that",
    "recommended that",
    "advised me",
    "advised us",
    "according to",
    "codex said",
    "it said",
    "they said",
    "he said",
    "she said",
    "the assistant said",
    "the assistant told",
    "the assistant suggested",
    "the assistant recommended",
];

/// Explicit limits to the current task; no standing marker overrides them.
const EXPLICIT_TASK_PHRASES: &[&str] = &[
    "for now",
    "just for now",
    "right now",
    "this time",
    "this time only",
    "in this pass",
    "this pass",
    "during this",
    "for this task",
    "this task only",
    "for this task only",
    "only for this task",
    "this turn",
    "today only",
    "only today",
    "for today",
];

/// Words that usually limit a sentence to the task, unless a strong standing marker says
/// otherwise ("today and in later sessions").
const WEAK_TASK_PHRASES: &[&str] = &[
    "yet",
    "today",
    "during the",
    "orientation",
    "until",
    "before writing",
    "before starting",
    "before you start",
];

/// Clauses of `text` that the user explicitly marked as standing (or pending) rules.
pub(crate) fn marked_rules(text: &str) -> Vec<RuleClause> {
    let mut rules = Vec::new();
    // The scope of the list the current line belongs to, from its header. Blank lines keep
    // it (Markdown lists often follow a blank line); any other prose line replaces it.
    let mut list_header: Option<HeaderScope> = None;
    let mut in_list = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let was_in_list = in_list;
        let is_list_item = in_list_item(line, trimmed, &mut in_list);
        if !is_list_item {
            list_header = next_header(line, trimmed, was_in_list, list_header);
        }
        let inherited = if is_list_item { list_header } else { None };
        for clause in clauses(trimmed) {
            if clause.ends_with('?') || clause.ends_with(':') || clause.len() > MAX_RULE_BYTES {
                continue;
            }
            let normalized = normalize(clause);
            if is_reported_speech(&normalized) || asks_about_rules(clause) {
                continue;
            }
            let standing = match inherited {
                // Items relayed from someone else are never the user's rules.
                Some(HeaderScope::Reported) => continue,
                // A task-limited list header limits every item under it.
                Some(HeaderScope::Pending) => RuleStanding::Pending,
                Some(HeaderScope::Standing) => standing_of(&normalized),
                None if has_standing_marker(&normalized) => standing_of(&normalized),
                None => continue,
            };
            rules.push(RuleClause {
                text: clause.to_string(),
                standing,
            });
        }
    }
    rules
}

/// Whether the user limited `clause` to the current task, phase or pass. An explicit limit
/// ("for this task only") always counts; a weak task word ("today") yields to a strong
/// standing marker ("in later sessions").
pub(crate) fn is_task_limited(clause: &str) -> bool {
    standing_of(&normalize(clause)) == RuleStanding::Pending
}

/// The scope a list header gives the items below it: a header such as "My working
/// preferences for the whole week:" makes them standing, "For this task:" makes them
/// pending; any other line ends a list.
fn header_scope(line: &str) -> Option<HeaderScope> {
    if !line.ends_with(':') {
        return None;
    }
    let normalized = normalize(line);
    if is_reported_speech(&normalized) {
        return Some(HeaderScope::Reported);
    }
    match standing_of(&normalized) {
        RuleStanding::Pending => Some(HeaderScope::Pending),
        RuleStanding::Standing => has_standing_marker(&normalized).then_some(HeaderScope::Standing),
    }
}

/// The scope inherited by the list item of `text` that contains `clause`, if any.
pub(crate) fn inherited_scope(text: &str, clause: &str) -> Option<HeaderScope> {
    let mut list_header = None;
    let mut in_list = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let was_in_list = in_list;
        let is_list_item = in_list_item(line, trimmed, &mut in_list);
        if !is_list_item {
            list_header = next_header(line, trimmed, was_in_list, list_header);
        }
        if clauses(trimmed).contains(&clause) {
            return if is_list_item { list_header } else { None };
        }
    }
    None
}

/// Whether `line` belongs to a list item: an item itself, or an indented continuation of
/// one (a Markdown lazy continuation keeps the item, and so the header, it continues).
/// Tracks across lines whether a list is open.
fn in_list_item(line: &str, trimmed: &str, in_list: &mut bool) -> bool {
    // An indented header ("  For this task:") opens its own nested scope instead.
    let continuation = *in_list && line.starts_with([' ', '\t']) && !trimmed.ends_with(':');
    *in_list = list_item_body(trimmed).is_some() || continuation;
    *in_list
}

/// The scope a non-item line gives the list items after it. A header nested inside a list
/// narrows the enclosing scope and never widens it, so a neutral "Details:" under "For this
/// task:" keeps its items task-limited.
fn next_header(
    line: &str,
    trimmed: &str,
    was_in_list: bool,
    enclosing: Option<HeaderScope>,
) -> Option<HeaderScope> {
    let scope = header_scope(trimmed);
    if !(was_in_list && line.starts_with([' ', '\t'])) {
        return scope;
    }
    let strictness = |scope: Option<HeaderScope>| match scope {
        None => 0,
        Some(HeaderScope::Standing) => 1,
        Some(HeaderScope::Pending) => 2,
        Some(HeaderScope::Reported) => 3,
    };
    if strictness(scope) >= strictness(enclosing) {
        scope
    } else {
        enclosing
    }
}

/// Openings of a request to retrieve rules ("list the standing rules I gave you"). They
/// count only at the head of a phrase, so "From now on, quote my instructions word for
/// word." is still a rule.
const RULE_REQUEST_OPENINGS: &[&str] = &[
    "list",
    "show me",
    "show the",
    "show my",
    "repeat",
    "recite",
    "remind me",
    "tell me",
    "what are",
    "what were",
    "which",
    "give me",
    "print",
    "can you list",
    "could you list",
    "can you show",
    "could you show",
    "can you tell",
    "could you tell",
    "please list",
    "please show",
    "please repeat",
];
const RULE_NOUNS: &[&str] = &[
    "rule",
    "rules",
    "preference",
    "preferences",
    "instruction",
    "instructions",
];

/// Whether `clause` asks to retrieve the user's rules rather than stating one: one of its
/// phrases (split at punctuation) opens with a retrieval request, and it names rules.
pub(crate) fn asks_about_rules(clause: &str) -> bool {
    has_phrase(&normalize(clause), RULE_NOUNS)
        && clause
            .split([':', ',', ';', '-'])
            .map(normalize)
            .any(|phrase| {
                let body = strip_list_marker(&phrase);
                RULE_REQUEST_OPENINGS
                    .iter()
                    .any(|opening| body == *opening || body.starts_with(&format!("{opening} ")))
            })
}

/// Whether `clause` reports what someone else said or advised rather than stating the
/// user's own rule; an assistant's advice must never become the user's rule.
pub(crate) fn reports_speech(clause: &str) -> bool {
    is_reported_speech(&normalize(clause))
}

fn standing_of(normalized: &str) -> RuleStanding {
    if has_phrase(normalized, EXPLICIT_TASK_PHRASES)
        || (has_phrase(normalized, WEAK_TASK_PHRASES)
            && !has_phrase(normalized, STRONG_STANDING_PHRASES))
    {
        RuleStanding::Pending
    } else {
        RuleStanding::Standing
    }
}

fn is_reported_speech(normalized: &str) -> bool {
    has_phrase(normalized, REPORTED_SPEECH_PHRASES)
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
