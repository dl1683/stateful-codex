//! The visible receipt for commits a turn's start found in the workspace history: one counted
//! group per turn, so a rebase of fifty commits is one receipt, not fifty. Who made a commit
//! is not known, so the receipt says where it was found, never who made it.

use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::CreateOutcome;
use sha2::Digest;
use sha2::Sha256;

use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::events::CaptureOutcome;
use crate::events::GroupReceipt;
use crate::events::GroupReceiptItem;
use crate::events::KnowledgeCategory;
use crate::events::receipt_text;

/// Most commits listed by name in one receipt; the counts cover the rest.
const MAX_LISTED_COMMITS: usize = 10;

/// The thread and turn an observation belongs to.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TurnRef<'a> {
    pub(crate) thread_id: &'a str,
    pub(crate) turn_id: &'a str,
}

/// What one turn's start remembered from the workspace history.
pub(crate) struct CommitReceipt {
    project_id: String,
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
    pub(crate) group_id: String,
    recognized: u32,
    saved: u32,
    already_present: u32,
    failed: u32,
    items: Vec<GroupReceiptItem>,
}

impl CommitReceipt {
    pub(crate) fn new(project_id: &str, turn: TurnRef<'_>) -> Self {
        let mut hasher = Sha256::new();
        for part in ["commits", project_id, turn.thread_id, turn.turn_id] {
            hasher.update((part.len() as u64).to_be_bytes());
            hasher.update(part.as_bytes());
        }
        Self {
            project_id: project_id.to_string(),
            thread_id: turn.thread_id.to_string(),
            turn_id: turn.turn_id.to_string(),
            group_id: format!("commits-{:x}", hasher.finalize()),
            recognized: 0,
            saved: 0,
            already_present: 0,
            failed: 0,
            items: Vec::new(),
        }
    }

    /// Counts one commit's storage outcome; returns whether it is stored.
    pub(crate) fn record(
        &mut self,
        outcome: Option<(BlackboardEntry, CreateOutcome)>,
        short_sha: &str,
        subject: &str,
    ) -> bool {
        self.recognized += 1;
        let Some((entry, outcome)) = outcome else {
            self.failed += 1;
            return false;
        };
        let outcome = match outcome {
            CreateOutcome::Created => {
                self.saved += 1;
                CaptureOutcome::Stored
            }
            CreateOutcome::AlreadyPresent => {
                self.already_present += 1;
                CaptureOutcome::AlreadyStored
            }
        };
        if self.items.len() < MAX_LISTED_COMMITS {
            self.items.push(GroupReceiptItem {
                entry_id: entry.id.to_string(),
                revision: entry.revision,
                category: KnowledgeCategory::Commit,
                outcome,
                text: receipt_text(&format!("{short_sha} {subject}")),
            });
        }
        true
    }

    /// Sends the receipt when the turn's start found any commit.
    pub(crate) fn emit(self, event_sink: Option<&dyn StatefulEventSink>) {
        let (Some(event_sink), true) = (event_sink, self.recognized > 0) else {
            return;
        };
        event_sink.emit(StatefulEvent::KnowledgeGroupCaptured(GroupReceipt {
            project_id: self.project_id,
            thread_id: self.thread_id,
            turn_id: self.turn_id,
            group_id: self.group_id,
            category: KnowledgeCategory::Commit,
            declared_count: None,
            recognized: self.recognized,
            saved: self.saved,
            already_present: self.already_present,
            pending: 0,
            omitted: 0,
            failed: self.failed,
            items: self.items,
            omitted_items: Vec::new(),
            scope_title: None,
        }));
    }
}
