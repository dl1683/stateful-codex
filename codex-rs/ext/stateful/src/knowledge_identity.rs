//! The canonical identity of assistant-reported knowledge: version, category, authority,
//! applicable scope and the normalized words (the same normalizer capture uses), and the
//! bounded backfill that indexes entries written without one (model records, older
//! entries), so capture can tell words already saved or forgotten however they were stored.

use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use sha2::Digest;
use sha2::Sha256;

use crate::user_rules::normalize;

/// Entries indexed per backfill batch.
const BACKFILL_BATCH: u32 = 500;
/// Batches one capture may index before it gives up for now (and creates nothing).
const MAX_BACKFILL_BATCHES: usize = 20;

/// The identity key of `words` (already normalized) in `category`, assistant-reported, in
/// `scope_id` (`None` for project-wide).
pub(crate) fn identity_key(
    category: KnowledgeCategory,
    scope_id: Option<&str>,
    words: &str,
) -> String {
    let mut hasher = Sha256::new();
    for part in [
        "identity-v1",
        category.as_str(),
        KnowledgeAuthority::AssistantReported.as_str(),
        scope_id.unwrap_or_default(),
        words,
    ] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

/// The category an agent-written entry of `kind` would have if an answer stated it.
fn category_of(kind: BlackboardKind) -> Option<KnowledgeCategory> {
    match kind {
        BlackboardKind::RejectedApproach => Some(KnowledgeCategory::RuledOut),
        BlackboardKind::Question => Some(KnowledgeCategory::OpenCheck),
        BlackboardKind::Decision => Some(KnowledgeCategory::Decision),
        BlackboardKind::Instruction
        | BlackboardKind::Fact
        | BlackboardKind::Claim
        | BlackboardKind::Number
        | BlackboardKind::Strategy
        | BlackboardKind::Contradiction
        | BlackboardKind::Failure
        | BlackboardKind::Signal
        | BlackboardKind::Note => None,
    }
}

/// Indexes entries written since the last backfill. Returns whether the index now covers
/// every entry (capture creates nothing while it does not).
pub(crate) async fn index_identities(
    store: &BlackboardStore,
    project_id: &str,
) -> Result<bool, String> {
    for _ in 0..MAX_BACKFILL_BATCHES {
        let (batch, covered, newest) = store
            .identity_backfill_batch(project_id, BACKFILL_BATCH)
            .await
            .map_err(|error| error.to_string())?;
        if covered >= newest {
            return Ok(true);
        }
        let through = batch.last().map_or(newest, |entry| entry.rowid);
        let identities = batch
            .iter()
            .filter(|entry| entry.provenance_kind == BlackboardProvenanceKind::Agent)
            .filter_map(|entry| {
                category_of(entry.kind).map(|category| {
                    (
                        identity_key(
                            category,
                            entry.scope_id.as_deref(),
                            &normalize(&entry.content),
                        ),
                        entry.id.clone(),
                    )
                })
            })
            .collect::<Vec<_>>();
        store
            .record_identity_backfill(project_id, covered, through, &identities)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(false)
}
