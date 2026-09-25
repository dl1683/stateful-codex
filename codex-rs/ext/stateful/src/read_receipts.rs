use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use codex_project_intelligence::BlackboardEvidenceLink;
use sha2::Digest;
use sha2::Sha256;

const MAX_READ_RECEIPTS: usize = 1_024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct EvidenceReadReceipt {
    pub(super) evidence: BlackboardEvidenceLink,
    content_digest: [u8; 32],
    content_bytes: u32,
}

impl EvidenceReadReceipt {
    pub(super) fn comparison_byte_limit(&self) -> u32 {
        self.content_bytes.saturating_add(1)
    }

    pub(super) fn matches_content(&self, content: &[u8]) -> bool {
        self.content_digest == Sha256::digest(content).as_slice()
    }
}

#[derive(Clone, Default)]
pub(super) struct EvidenceReadReceipts {
    state: Arc<Mutex<ReceiptState>>,
}

#[derive(Default)]
struct ReceiptState {
    next_sequence: u64,
    receipts: HashMap<String, StoredReceipt>,
    insertion_order: VecDeque<String>,
}

struct StoredReceipt {
    project_id: String,
    thread_id: String,
    receipt: EvidenceReadReceipt,
}

impl EvidenceReadReceipts {
    pub(super) fn issue(
        &self,
        project_id: &str,
        thread_id: &str,
        source_call_id: &str,
        evidence: BlackboardEvidenceLink,
        content: &[u8],
    ) -> String {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.next_sequence = state.next_sequence.wrapping_add(1);
        let mut hasher = Sha256::new();
        hasher.update(project_id.as_bytes());
        hasher.update([0]);
        hasher.update(thread_id.as_bytes());
        hasher.update([0]);
        hasher.update(source_call_id.as_bytes());
        hasher.update([0]);
        hasher.update(state.next_sequence.to_le_bytes());
        hasher.update([0]);
        hasher.update(evidence.context_map_entry_id.as_str().as_bytes());
        hasher.update([0]);
        hasher.update(evidence.source_fingerprint.as_str().as_bytes());
        let content_digest: [u8; 32] = Sha256::digest(content).into();
        hasher.update([0]);
        hasher.update(content_digest);
        let receipt_id = format!("stateful-read-{:x}", hasher.finalize());

        while state.receipts.len() >= MAX_READ_RECEIPTS {
            let Some(expired_id) = state.insertion_order.pop_front() else {
                break;
            };
            state.receipts.remove(&expired_id);
        }
        state.insertion_order.push_back(receipt_id.clone());
        state.receipts.insert(
            receipt_id.clone(),
            StoredReceipt {
                project_id: project_id.to_string(),
                thread_id: thread_id.to_string(),
                receipt: EvidenceReadReceipt {
                    evidence,
                    content_digest,
                    content_bytes: u32::try_from(content.len()).unwrap_or(u32::MAX),
                },
            },
        );
        receipt_id
    }

    pub(super) fn resolve(
        &self,
        project_id: &str,
        thread_id: &str,
        receipt_id: &str,
    ) -> Option<EvidenceReadReceipt> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stored = state.receipts.get(receipt_id)?;
        (stored.project_id == project_id && stored.thread_id == thread_id)
            .then(|| stored.receipt.clone())
    }
}
