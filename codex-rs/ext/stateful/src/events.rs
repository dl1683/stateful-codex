use crate::StatefulAttributionSummary;

/// Stateful mutation hint or bounded turn-attribution result for product clients.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StatefulEvent {
    BlackboardUpdated {
        project_id: String,
        entity_kind: BlackboardEntityKind,
        entity_id: String,
        revision: u64,
    },
    RunUpdated {
        project_id: String,
        run_id: String,
        revision: u64,
    },
    ObligationUpdated {
        project_id: String,
        run_id: String,
        obligation_id: String,
        revision: u64,
    },
    SteeringUpdated {
        project_id: String,
        run_id: String,
        steering_id: String,
        revision: u64,
    },
    AttributionCompleted {
        summary: StatefulAttributionSummary,
    },
    /// A knowledge entry stored during a turn, or found already stored, so clients can show
    /// the user a receipt of what was saved.
    KnowledgeCaptured {
        project_id: String,
        thread_id: String,
        turn_id: String,
        entry_id: String,
        revision: u64,
        category: KnowledgeCategory,
        outcome: CaptureOutcome,
        /// The entry's content, at most `MAX_RECEIPT_TEXT_BYTES` bytes.
        text: String,
    },
}

/// Longest content excerpt carried by a receipt.
pub const MAX_RECEIPT_TEXT_BYTES: usize = 240;

/// What a receipt says was saved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnowledgeCategory {
    /// A standing rule in the user's own words.
    Rule,
    /// A rule the user marked as standing but also limited to a task; kept, never applied.
    PendingRule,
    Decision,
    /// How to build, test or run the project here.
    Recipe,
    Finding,
    /// What the user said about themselves or the whole work, in their words.
    Background,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureOutcome {
    Stored,
    AlreadyStored,
}

/// The receipt excerpt of `content`, cut on a character boundary.
pub(crate) fn receipt_text(content: &str) -> String {
    if content.len() <= MAX_RECEIPT_TEXT_BYTES {
        return content.to_string();
    }
    let mut end = MAX_RECEIPT_TEXT_BYTES - 3;
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    // End at a word boundary when one is near, so the receipt never stops mid-word.
    let cut = &content[..end];
    let cut = match cut.rfind(char::is_whitespace) {
        Some(space) if end - space <= 40 => cut[..space].trim_end(),
        _ => cut,
    };
    format!("{cut}...")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlackboardEntityKind {
    Entry,
    Relation,
}

/// Receives committed Stateful mutations and forwards revision hints to product clients.
///
/// Implementations must treat the durable stores as authoritative. Delivery may be best effort,
/// and clients recover missed events through the read APIs.
pub trait StatefulEventSink: Send + Sync {
    fn emit(&self, event: StatefulEvent);
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
