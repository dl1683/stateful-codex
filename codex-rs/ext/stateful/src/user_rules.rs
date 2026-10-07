//! Deterministic recognition of standing rules the user marks as such in their own words.
//!
//! This is a conservative safety net, not general rule extraction: a clause counts only
//! when it carries an explicit standing marker ("never", "from now on", "end each of your
//! replies", a list introduced as preferences). A clause that also carries a task marker
//! ("yet", "for now", "during this pass") is pending: it is kept for inspection but never
//! applied. Rules phrased without markers are left to the model's verified-quote path.

#[cfg(test)]
pub(crate) use crate::rule_units::marked_rules;

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
pub(crate) const IMPERATIVE_LEADS: &[&str] = &[
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
    "ground rules",
    "rules for this",
    "house rules",
    "style rules",
];

/// Phrases naming how long a rule lasts ("for the whole time we work on it"). They mark a
/// rule only in a directive, never in the user's description of themselves ("I'm a backend
/// developer for this project").
const DURATION_PHRASES: &[&str] = &[
    "for the whole time",
    "the whole time we work",
    "while we work on",
    "throughout this project",
    "for this project",
    "for this essay",
    "for this book",
    "for this document",
];

/// Phrases that scope a rule to a whole investigation, which may span sessions.
pub(crate) const INVESTIGATION_PHRASES: &[&str] = &[
    "for this investigation",
    "for this whole investigation",
    "for the whole investigation",
    "during this investigation",
    "throughout this investigation",
    "for the rest of this investigation",
    "this whole investigation",
    "the whole investigation",
];

/// Markers that make a rule's standing unambiguous even next to a task word.
pub(crate) const STRONG_STANDING_PHRASES: &[&str] = &[
    "for the whole time",
    "the whole time we work",
    "throughout this project",
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
    "that's her preference",
    "that's his preference",
    "that's their preference",
    "that is her preference",
    "that is his preference",
    "that is their preference",
    "preference not mine",
    "rule not mine",
];

/// Explicit limits to the current task; no standing marker overrides them.
pub(crate) const EXPLICIT_TASK_PHRASES: &[&str] = &[
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

/// Tracks Markdown fences: a fence opened by three or more backticks or tildes closes only
/// at a line of the same character at least as long.
#[derive(Default)]
pub(crate) struct Fence {
    open: Option<(char, usize)>,
}

impl Fence {
    /// Feeds one trimmed line; true when the line is a delimiter or inside a fence.
    pub(crate) fn skips(&mut self, trimmed: &str) -> bool {
        let delimiter = ['`', '~'].into_iter().find_map(|mark| {
            let run = trimmed
                .chars()
                .take_while(|character| *character == mark)
                .count();
            (run >= 3).then_some((mark, run))
        });
        match (self.open, delimiter) {
            (Some((mark, length)), Some((found, run)))
                if found == mark && run >= length && trimmed.len() == run =>
            {
                self.open = None;
                true
            }
            (Some(_), _) => true,
            (None, Some(opened)) => {
                self.open = Some(opened);
                true
            }
            (None, None) => false,
        }
    }
}

/// The scope a list header gives the items below it: a header such as "My working
/// preferences for the whole week:" makes them standing, "For this task:" makes them
/// pending; any other line ends a list.
pub(crate) fn header_scope(line: &str) -> Option<HeaderScope> {
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

/// Whether `line` belongs to a list item: an item itself, or an indented continuation of
/// one (a Markdown lazy continuation keeps the item, and so the header, it continues).
/// Tracks across lines whether a list is open.
pub(crate) fn in_list_item(line: &str, trimmed: &str, in_list: &mut bool) -> bool {
    let indented = *in_list && line.starts_with([' ', '\t']);
    // An indented header ("  For this task:") opens its own nested scope instead, and the
    // list stays open around it so a further nested header still narrows the same list.
    let continuation = indented && !trimmed.ends_with(':');
    let is_item = list_item_body(trimmed).is_some() || continuation;
    *in_list = is_item || indented;
    is_item
}

/// The scope a non-item line gives the list items after it. A header nested inside a list
/// narrows the enclosing scope and never widens it, so a neutral "Details:" under "For this
/// task:" keeps its items task-limited.
pub(crate) fn next_header(
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
    let normalized = normalize(clause);
    if !has_phrase(&normalized, RULE_NOUNS) {
        return false;
    }
    let opens_request = |phrase: String| {
        let body = strip_list_marker(&phrase).to_string();
        RULE_REQUEST_OPENINGS
            .iter()
            .any(|opening| body == *opening || body.starts_with(&format!("{opening} ")))
    };
    let body = list_item_body(clause.trim()).unwrap_or(clause);
    let mut phrases = body.split([':', ',', ';', '-']).map(normalize);
    // "List the preferences I gave you for all our work." opens with the request; "From now
    // on, repeat my instructions word for word." makes the request a standing rule.
    phrases.next().is_some_and(opens_request)
        || (!has_phrase(&normalized, STRONG_STANDING_PHRASES) && phrases.any(opens_request))
}

/// Whether `clause` reports what someone else said or advised rather than stating the
/// user's own rule; an assistant's advice must never become the user's rule.
pub(crate) fn reports_speech(clause: &str) -> bool {
    is_reported_speech(&normalize(clause))
        || crate::quotation::Quotations::new(clause).relays_clause(clause)
}

/// Whether `normalized` limits itself explicitly to the current task. "During this
/// investigation" is a rule's scope, not a task limit.
pub(crate) fn has_explicit_task_limit(normalized: &str) -> bool {
    let investigation = has_phrase(normalized, INVESTIGATION_PHRASES);
    EXPLICIT_TASK_PHRASES
        .iter()
        .filter(|phrase| !(investigation && **phrase == "during this"))
        .any(|phrase| has_phrase(normalized, &[phrase]))
}

/// Whether a quoted passage reads as an instruction (it opens with an imperative or a
/// standing phrase), rather than naming a term such as "standing rule".
pub(crate) fn reads_as_instruction(normalized: &str) -> bool {
    let body = strip_list_marker(normalized);
    IMPERATIVE_LEADS
        .iter()
        .chain(STRONG_STANDING_PHRASES)
        .chain(["only", "don't", "do not", "please", "make sure"].iter())
        .any(|lead| body.starts_with(&format!("{lead} ")))
}

pub(crate) fn standing_of(normalized: &str) -> RuleStanding {
    // A rule for a whole investigation outlives the current turn, and its other words are
    // end conditions.
    let investigation = has_phrase(normalized, INVESTIGATION_PHRASES);
    if has_explicit_task_limit(normalized)
        || (has_phrase(normalized, WEAK_TASK_PHRASES)
            && !has_phrase(normalized, STRONG_STANDING_PHRASES)
            && !investigation)
    {
        RuleStanding::Pending
    } else {
        RuleStanding::Standing
    }
}

pub(crate) fn is_reported_speech(normalized: &str) -> bool {
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
pub(crate) fn clauses(line: &str) -> Vec<&str> {
    let mut clauses = Vec::new();
    let mut start = 0;
    let bytes = line.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        let terminal = matches!(byte, b'.' | b'!' | b'?');
        // A sentence may end inside a closing quote ("... first." Next sentence).
        let quote_len = if terminal {
            line[index + 1..]
                .chars()
                .next()
                .filter(|next| matches!(next, '"' | '\'' | '\u{201d}' | '\u{2019}'))
                .map_or(0, char::len_utf8)
        } else {
            0
        };
        let quote_closes = quote_len > 0
            && bytes
                .get(index + 1 + quote_len)
                .is_none_or(u8::is_ascii_whitespace);
        let at_boundary = bytes.get(index + 1).is_none_or(u8::is_ascii_whitespace) || quote_closes;
        if terminal && at_boundary && !ends_with_abbreviation(&line[start..=index]) {
            let end = if quote_closes {
                index + quote_len
            } else {
                index
            };
            let clause = line[start..=end].trim();
            if !clause.is_empty() && !is_list_marker_only(clause) {
                clauses.push(clause);
                start = end + 1;
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
pub(crate) fn list_item_body(line: &str) -> Option<&str> {
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

pub(crate) fn has_standing_marker(normalized: &str) -> bool {
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
        || has_phrase(normalized, INVESTIGATION_PHRASES)
        || (has_phrase(normalized, DURATION_PHRASES) && !describes_the_user(body))
}

/// Whether a sentence describes the user or the work rather than directing it: it opens with
/// the speaker ("I maintain this fork for this project") and asks nothing of the assistant.
fn describes_the_user(body: &str) -> bool {
    let first_person = [
        "i ", "i'm ", "i've ", "i'd ", "my ", "we ", "we're ", "we've ", "our ",
    ]
    .iter()
    .any(|opening| body.starts_with(opening));
    let directs = [
        " never ",
        " always ",
        " don't ",
        " do not ",
        " must ",
        " should ",
        " want you ",
        " need you ",
        " please ",
    ]
    .iter()
    .any(|word| format!(" {body} ").contains(word));
    first_person && !directs
}

pub(crate) fn strip_list_marker(normalized: &str) -> &str {
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

pub(crate) fn has_phrase(normalized: &str, phrases: &[&str]) -> bool {
    let padded = format!(" {normalized} ");
    phrases
        .iter()
        .any(|phrase| padded.contains(&format!(" {phrase} ")))
}

/// Lowercase words separated by single spaces; punctuation becomes a separator.
pub(crate) fn normalize(text: &str) -> String {
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
