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

pub(crate) const WORLD_STATE_ID: &str = "stateful_request_scope";
pub(crate) const START_MARKER: &str = "<stateful_request_scope>";
pub(crate) const END_MARKER: &str = "</stateful_request_scope>";
/// Shown after the quoted opening of the request the note governs.
pub(crate) const SELF_CONTAINED_NOTE: &str = "is self-contained. For that request only: work from the files and the project rules and knowledge shown, and make no memory reads or writes (no conversation_read, blackboard_query, context_map_query or refresh, evidence_read, blackboard_record_batch, update or relate calls, no run updates). Exceptions: the request states a new standing rule or decision, or the work turns out to depend on earlier work. Later requests are not covered by this note.";
/// Shown when steering widens a self-contained request in the same turn.
pub(crate) const WIDENED_NOTE: &str = "now refers to earlier work: the self-contained restriction for it no longer applies, and the project memory and conversation record do.";
/// Retires notes written before notes named their request.
pub(crate) const LEGACY_RETIREMENT: &str = "Earlier scope notes in this conversation that said they applied until a later scope note no longer apply.";
/// Scope-note bytes one context window may hold (about two notes). Charges carried across
/// an injected compaction can only shrink this reserve, never the record below its floor.
pub(crate) const MAX_WINDOW_NOTE_BYTES: usize = 1_536;
/// Bytes of the quoted request opening in a note.
const MAX_HEAD_BYTES: usize = 80;

/// Opening words of the request that started the turn, stored in the turn store so the
/// scope note can name the request it governs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RequestHead(pub(crate) String);

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
        let text = user_input
            .iter()
            .filter_map(|item| match item {
                UserInput::Text { text, .. } => Some(text.as_str()),
                // `UserInput` is non-exhaustive; only text has an opening to quote.
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut end = collapsed.len().min(MAX_HEAD_BYTES);
        while !collapsed.is_char_boundary(end) {
            end -= 1;
        }
        turn_store.insert(RequestHead(collapsed[..end].to_string()));
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

/// The scope note planned for one sampling step and the scope-note bytes the current
/// window holds once it is rendered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScopeNotePlan {
    pub(crate) scope: RequestScope,
    turn_id: String,
    /// Body rendered when the harness reports this turn's note missing or the scope widened.
    note: Option<String>,
    /// Whether this turn's self-contained request carries a note (the reserve allowed it).
    noted: bool,
    /// Bytes of scope notes this window holds, including the one planned here.
    pub(crate) window_bytes: usize,
}

impl ScopeNotePlan {
    /// Plans the note for `turn_id`. Each self-contained turn gets its own note naming the
    /// request by its opening words, so a note left in history (for example after an
    /// interrupted write) can never govern a later request; continuity turns add nothing
    /// unless steering widened this turn's own self-contained request.
    pub(crate) fn new(
        previous: Option<&Value>,
        turn_id: &str,
        scope: RequestScope,
        head: Option<&RequestHead>,
    ) -> Self {
        let field = |name: &str| previous.and_then(|previous| previous.get(name));
        let previous_turn = field("turnId").and_then(Value::as_str);
        let previous_scope = field("scope").and_then(Value::as_str);
        let previous_noted = field("noted").and_then(Value::as_bool).unwrap_or(false);
        let previous_bytes = field("windowBytes")
            .and_then(Value::as_u64)
            .and_then(|bytes| usize::try_from(bytes).ok())
            .unwrap_or(0);
        // A note written before notes named their request said it applied "until a later
        // scope note"; retire it explicitly once.
        let legacy_restriction =
            previous_turn.is_none() && previous_scope == Some(RequestScope::SelfContained.name());
        let quoted = crate::continuity::escape(
            &serde_json::to_string(head.map_or("", |head| head.0.as_str()))
                .unwrap_or_else(|_| "\"\"".to_string()),
        );
        let same_turn = previous_turn == Some(turn_id);
        let fragment_bytes = |note: &str| START_MARKER.len() + note.len() + END_MARKER.len();
        let mut parts = Vec::new();
        if legacy_restriction {
            parts.push(LEGACY_RETIREMENT.to_string());
        }
        let mut noted = false;
        match scope {
            RequestScope::SelfContained if same_turn && previous_scope == Some(scope.name()) => {
                // Already planned at an earlier step of this turn.
                noted = previous_noted;
            }
            RequestScope::SelfContained => {
                let note = format!(
                    "Scope note for the request that begins {quoted}: it {SELF_CONTAINED_NOTE}"
                );
                // Notes share a bounded reserve per window; past it, a narrow turn still
                // defers the record but carries no note.
                if previous_bytes.saturating_add(fragment_bytes(&note)) <= MAX_WINDOW_NOTE_BYTES {
                    parts.push(note);
                    noted = true;
                }
            }
            RequestScope::Continuity
                if same_turn
                    && previous_noted
                    && previous_scope == Some(RequestScope::SelfContained.name()) =>
            {
                parts.push(format!(
                    "Scope note for the request that begins {quoted}: it {WIDENED_NOTE}"
                ));
            }
            RequestScope::Continuity => {}
        }
        let note = (!parts.is_empty()).then(|| parts.join("\n"));
        let added = note.as_deref().map_or(0, fragment_bytes);
        Self {
            scope,
            turn_id: turn_id.to_string(),
            note,
            noted,
            window_bytes: previous_bytes.saturating_add(added),
        }
    }

    /// The section rendering this plan. A note renders once per turn and scope, and again
    /// when the harness reports it missing (a new window).
    pub(crate) fn section(self) -> WorldStateSectionContribution {
        let Self {
            scope,
            turn_id,
            note,
            noted,
            window_bytes,
        } = self;
        let snapshot = json!({
            "scope": scope.name(),
            "turnId": turn_id,
            "noted": noted,
            "windowBytes": window_bytes,
        });
        WorldStateSectionContribution::new(WORLD_STATE_ID, snapshot, move |previous| {
            if let PreviousWorldStateSection::Known(previous) = previous
                && previous.get("turnId").and_then(Value::as_str) == Some(turn_id.as_str())
                && previous.get("scope").and_then(Value::as_str) == Some(scope.name())
            {
                return None;
            }
            note.as_ref().map(|note| {
                RenderedWorldStateFragment::new("developer", (START_MARKER, END_MARKER), note)
            })
        })
        .with_retained_fragment_matcher(|role, text| {
            role == "developer"
                && text.trim_start().starts_with(START_MARKER)
                && text.trim_end().ends_with(END_MARKER)
        })
    }
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
    // First-person-plural and second-person possessive references to shared work.
    "we",
    "our",
    "ours",
    "us",
    "your",
    "yours",
    "recommended",
    "recommendation",
    "recommend",
    "identified",
    "found",
    "noticed",
    "flagged",
    "spotted",
    "mentioned",
    "pointed out",
    "outlined",
    "listed",
    "described",
    "explained",
    "drafted",
    "sketched",
    "showed",
    "shown",
    "provided",
    "gave",
    "given",
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
