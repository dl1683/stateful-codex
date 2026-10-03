//! Topic-first narrowing of the continuity record for a continuation request.
//!
//! The record quotes recent turns of the project, newest first. A continuation usually
//! needs the latest turns plus the earlier turns about the same subject, not every recent
//! turn on unrelated work. When the request that opens the window names salient terms,
//! the record keeps the newest turns, every turn sharing those terms, and at most one
//! other recent turn; the rest are counted in the record's footer with their retrieval
//! route. A request without salient terms keeps the whole record: an ambiguous reference
//! falls back to full continuity instead of a guessed topic. Current code state, decisions
//! with reasons and recipes stay in their own sections (checkout report and root).

use std::collections::HashSet;

use crate::continuity::CapturedTurn;
use crate::continuity::ContinuityRecord;

/// Newest turns always kept: the immediate context of a continuation.
const NEWEST_KEPT: usize = 2;
/// Older turns kept although they share no term with the request.
const UNRELATED_KEPT: usize = 1;
/// Terms shorter than this are not salient.
const MIN_TERM_CHARS: usize = 4;
/// Words too common in requests to identify a subject.
const COMMON_WORDS: &[&str] = &[
    "about",
    "after",
    "again",
    "also",
    "back",
    "been",
    "before",
    "both",
    "could",
    "does",
    "done",
    "each",
    "earlier",
    "else",
    "even",
    "every",
    "fine",
    "from",
    "have",
    "here",
    "into",
    "just",
    "last",
    "like",
    "look",
    "make",
    "more",
    "most",
    "much",
    "need",
    "next",
    "once",
    "only",
    "other",
    "over",
    "please",
    "same",
    "should",
    "show",
    "some",
    "still",
    "such",
    "sure",
    "take",
    "than",
    "that",
    "their",
    "them",
    "then",
    "there",
    "these",
    "they",
    "thing",
    "things",
    "this",
    "those",
    "today",
    "very",
    "want",
    "were",
    "what",
    "when",
    "where",
    "which",
    "while",
    "with",
    "work",
    "would",
    "yesterday",
    "your",
    "continue",
    "finish",
    "start",
    "keep",
    "going",
    "thanks",
    "okay",
    "left",
    "pick",
    "remaining",
    "rest",
];

/// Narrows `record` to the turns that matter for `request`; see the module comment.
pub(crate) fn focus_on_request(record: &mut ContinuityRecord, request: Option<&str>) {
    let Some(request) = request else {
        return;
    };
    let terms = salient_terms(request);
    if terms.is_empty() || record.turns.len() <= NEWEST_KEPT + UNRELATED_KEPT {
        return;
    }
    let mut unrelated_kept = 0;
    let mut omitted = 0;
    let turns = std::mem::take(&mut record.turns);
    for (index, turn) in turns.into_iter().enumerate() {
        let keep = index < NEWEST_KEPT || shares_terms(&turn, &terms) || {
            unrelated_kept += 1;
            unrelated_kept <= UNRELATED_KEPT
        };
        if keep {
            record.turns.push(turn);
        } else {
            omitted += 1;
        }
    }
    record.unrelated_omitted += omitted;
}

fn shares_terms(turn: &CapturedTurn, terms: &HashSet<String>) -> bool {
    let text = [turn.user.as_deref(), turn.answer.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    let found = salient_terms(&text);
    let shared = terms.intersection(&found).count();
    // One shared term identifies the subject of a short request; a longer one needs two.
    shared >= terms.len().clamp(1, 2)
}

fn salient_terms(text: &str) -> HashSet<String> {
    text.split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .filter(|word| word.chars().count() >= MIN_TERM_CHARS)
        .map(str::to_lowercase)
        .filter(|word| !COMMON_WORDS.contains(&word.as_str()))
        .collect()
}

#[cfg(test)]
#[path = "continuation_card_tests.rs"]
mod tests;
