//! Outcomes of recording one model-written knowledge item.
//!
//! The host already keeps every request and final answer, so a record is worth saving only
//! when it adds knowledge. A replay of the same record, or a record identical to current
//! agent knowledge (same kind, wording, node, structured value, verification, promotion,
//! evidence and premises), is already present: nothing is written and no save is reported.
//! The check and the insert share one writer transaction. Reworded duplicates are still
//! stored; no record is refused for how it is worded.

use codex_project_intelligence::BlackboardEntry;

/// What recording one item did.
#[derive(Debug)]
pub(crate) enum RecordOutcome {
    /// A new entry was committed, with the recipe label when it is a recipe.
    Created {
        entry: BlackboardEntry,
        recipe: Option<String>,
    },
    /// The same record is already current knowledge; nothing was written.
    AlreadyPresent(BlackboardEntry),
}
