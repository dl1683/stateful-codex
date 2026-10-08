//! Preserved historical scope quarantine predicates.

/// An instruction recorded before meanings were kept (no context row) whose words limit it to
/// a piece of work ("for this investigation", "this bug", as historical capture recognized them): which
/// piece is unknown, so it applies nowhere until the user restates it.
pub(super) const LEGACY_LIMITED_RULE: &str = "revision.kind = 'instruction'
                   AND NOT EXISTS (
                       SELECT 1 FROM knowledge_context AS any_context
                       WHERE any_context.entry_id = entry.id)
                   AND (LOWER(revision.content) LIKE '%investigation%'
                        OR LOWER(revision.content) LIKE '%this bug%'
                        OR LOWER(revision.content) LIKE '%this issue%'
                        OR LOWER(revision.content) LIKE '%this incident%'
                        OR LOWER(revision.content) LIKE '%this debugging%')";
