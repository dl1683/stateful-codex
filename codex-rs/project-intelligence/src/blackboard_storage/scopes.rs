//! Preserved historical scope quarantine predicates.

/// Leaves out an entry (`entry` in the enclosing query) whose current meaning limits it to
/// an investigation other than the open one the thread (the bound parameter) continues.
pub(super) const OUTSIDE_THREAD_SCOPE: &str = "
               AND NOT EXISTS (
                   SELECT 1 FROM knowledge_context AS scoped
                   WHERE scoped.entry_id = entry.id
                     AND scoped.revision = (
                         SELECT MAX(latest.revision) FROM knowledge_context AS latest
                         WHERE latest.entry_id = entry.id AND latest.revision <= entry.revision)
                     AND scoped.scope_id IS NOT NULL
                     AND scoped.scope_id IS NOT (
                         SELECT binding.scope_id FROM knowledge_scope_bindings AS binding
                         JOIN knowledge_scopes AS bound
                           ON bound.project_id = binding.project_id
                          AND bound.scope_id = binding.scope_id
                         WHERE binding.project_id = entry.project_id AND binding.thread_id = ?
                           AND bound.state = 'open'))";

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
