//! Host-side, deterministic request scope for the cheap ordinary path.
//!
//! A turn whose submitted text does not refer to earlier work is self-contained: the
//! project packet then carries the user's standing rules only, the conversation record is
//! deferred, and a scope note asks the model to skip memory reads and writes. Any doubt
//! resolves to continuity, because a false self-contained answer can lose earlier context
//! while a false continuity answer only forgoes the saving.

use codex_extension_api::ExtensionData;
use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use codex_protocol::user_input::UserInput;
use serde_json::Value;
use serde_json::json;

const WORLD_STATE_ID: &str = "stateful_request_scope";
pub(crate) const START_MARKER: &str = "<stateful_request_scope>";
pub(crate) const END_MARKER: &str = "</stateful_request_scope>";
pub(crate) const SELF_CONTAINED_NOTE: &str = "Current request scope (applies until a later scope note): self-contained. Work from the files and the project rules and knowledge shown. Make no memory reads or writes in this turn: no conversation_read, blackboard_query, context_map_query or refresh, evidence_read, blackboard_record_batch, update or relate calls, and no run updates. Exceptions: the request itself states a new standing rule or decision, or the work turns out to depend on earlier work.";
pub(crate) const CONTINUITY_NOTE: &str = "Current request scope (applies until a later scope note): this request may depend on earlier work; the project memory and the conversation record apply, and the earlier self-contained restriction no longer does.";

/// Whether the current request depends on earlier work. Stored in the turn store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestScope {
    SelfContained,
    Continuity,
}

impl RequestScope {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::SelfContained => "selfContained",
            Self::Continuity => "continuity",
        }
    }

    /// The scope recorded for this turn; a turn without a classification (for example a
    /// host-submitted continuation) is treated as continuity.
    pub(crate) fn of_turn(turn_store: &ExtensionData) -> Self {
        turn_store
            .get::<RequestScope>()
            .map_or(Self::Continuity, |scope| *scope)
    }

    /// Records the scope of a turn from the user input that started it.
    pub(crate) fn record_turn_start(turn_store: &ExtensionData, user_input: &[UserInput]) {
        turn_store.insert(classify_input(user_input));
    }

    /// A user message that arrives during a self-contained turn (steering) can widen the
    /// turn to continuity; nothing narrows a turn once it is continuity.
    pub(crate) fn observe_user_message(turn_store: &ExtensionData, content: &[UserInput]) {
        if Self::of_turn(turn_store) == Self::SelfContained
            && classify_input(content) == Self::Continuity
        {
            turn_store.insert(Self::Continuity);
        }
    }
}

/// Per-turn scope note. It renders when the scope changes, and when a self-contained turn
/// finds no note in the retained history (thread start, after compaction); a continuity
/// turn with nothing to retract renders nothing.
pub(crate) fn request_scope_section(scope: RequestScope) -> WorldStateSectionContribution {
    WorldStateSectionContribution::new(
        WORLD_STATE_ID,
        json!({ "scope": scope.name() }),
        move |previous| {
            let previous_scope = match previous {
                PreviousWorldStateSection::Known(previous) => {
                    previous.get("scope").and_then(Value::as_str)
                }
                PreviousWorldStateSection::Absent | PreviousWorldStateSection::Unknown => None,
            };
            let note = match (previous_scope, scope) {
                (Some(previous), current) if previous == current.name() => return None,
                (None, RequestScope::Continuity) => return None,
                (_, RequestScope::SelfContained) => SELF_CONTAINED_NOTE,
                (Some(_), RequestScope::Continuity) => CONTINUITY_NOTE,
            };
            Some(RenderedWorldStateFragment::new(
                "developer",
                (START_MARKER, END_MARKER),
                note,
            ))
        },
    )
    .with_retained_fragment_matcher(|role, text| {
        role == "developer"
            && text.trim_start().starts_with(START_MARKER)
            && text.trim_end().ends_with(END_MARKER)
    })
}

/// Classifies structured user input: any non-text item (image, mention, skill) is treated
/// as continuity because its referent cannot be judged from text.
pub(crate) fn classify_input(user_input: &[UserInput]) -> RequestScope {
    let mut texts = Vec::with_capacity(user_input.len());
    for item in user_input {
        match item {
            UserInput::Text { text, .. } => texts.push(text.as_str()),
            // `UserInput` is non-exhaustive: every other kind keeps continuity.
            _ => return RequestScope::Continuity,
        }
    }
    classify(&texts.join("\n"))
}

/// Requests shorter than this many words are replies to the previous answer.
const MIN_SELF_CONTAINED_WORDS: usize = 4;

/// Reply openings that answer the previous message.
const REPLY_OPENINGS: &[&str] = &[
    "yes",
    "yeah",
    "yep",
    "ok",
    "okay",
    "sure",
    "no",
    "nope",
    "please do",
    "go ahead",
    "sounds good",
    "approved",
    "proceed",
    "lgtm",
    "approve",
    "do it",
    "do that",
    "that",
    "this",
    "it",
    "same",
    "great",
    "thanks",
    "perfect",
];

/// Phrases, matched on word boundaries, that refer to earlier work.
const REFERENTS: &[&str] = &[
    // Temporal.
    "yesterday",
    "earlier",
    "previous",
    "previously",
    "prior",
    "before",
    "last time",
    "last session",
    "last week",
    "last night",
    "this morning",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "the other day",
    "ago",
    "again",
    "still",
    "anymore",
    "back",
    "already",
    // Continuation.
    "continue",
    "continuing",
    "carry on",
    "pick up",
    "resume",
    "keep going",
    "left off",
    "where were we",
    "finish",
    "remaining",
    "the rest",
    "what's left",
    "whats left",
    "next step",
    "next steps",
    "what's next",
    "whats next",
    "status",
    "progress",
    "recap",
    "summary",
    "summarize",
    "summarise",
    // Memory.
    "remember",
    "recall",
    "memory",
    "forget",
    "forgot",
    "remind",
    "history",
    // Reference to an earlier statement, proposal or decision.
    "proposed",
    "proposal",
    "suggested",
    "suggestion",
    "you said",
    "you mentioned",
    "we said",
    "discussed",
    "decided",
    "decision",
    "agreed",
    "as planned",
    "the plan",
    "our plan",
    "idea",
    "ideas",
    "option",
    "options",
    "as before",
    "as usual",
    "same as",
    "like before",
    "also",
    "instead",
    "another",
    "standing",
    "preference",
    "preferences",
    "rule",
    "rules",
    "convention",
    "conventions",
    "why did",
    "how did we",
    "rationale",
    "changed",
    "revert",
    "reverted",
    "undo",
    "we were",
    "you were",
    "you did",
    "you made",
    "you wrote",
    "you added",
    "i added",
    "i changed",
    "i made",
    "i wrote",
    "i noted",
    "my notes",
    "my changes",
    // Ordinal and deictic references to something said before.
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "last",
    "latter",
    "former",
    "it",
    "this",
    "that",
    "these",
    "those",
    "them",
    "they",
    "too",
    "approach",
    "version",
    "variant",
    // Approval and authorization.
    "permission",
    "authorize",
    "authorized",
    "allowed",
    "go ahead",
];

/// Classifies one submitted request from its text alone.
pub(crate) fn classify(text: &str) -> RequestScope {
    let normalized = normalize(text);
    let words = normalized.split_whitespace().collect::<Vec<_>>();
    if words.len() < MIN_SELF_CONTAINED_WORDS {
        return RequestScope::Continuity;
    }
    let padded = format!(" {} ", words.join(" "));
    let opens_with_reply = REPLY_OPENINGS
        .iter()
        .any(|opening| padded.starts_with(&format!(" {opening} ")));
    if opens_with_reply
        || REFERENTS
            .iter()
            .any(|referent| padded.contains(&format!(" {referent} ")))
    {
        return RequestScope::Continuity;
    }
    RequestScope::SelfContained
}

/// Lowercases and turns every character that cannot be part of a word into a space, so
/// phrases match on word boundaries ("yesterday's" matches "yesterday").
fn normalize(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_lowercase().next().unwrap_or(character)
            } else if character == '\'' || character == '\u{2019}' {
                '\''
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(|word| {
            // "yesterday's" keeps "what's" style contractions but drops possessives.
            word.strip_suffix("'s")
                .filter(|stem| !matches!(*stem, "what" | "that" | "it" | "let" | "here"))
                .unwrap_or(word)
                .trim_matches('\'')
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
#[path = "request_scope_tests.rs"]
mod tests;
