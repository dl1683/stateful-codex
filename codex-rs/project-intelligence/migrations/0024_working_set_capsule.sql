-- Working-set capsule storage contract. Projections, file samples, tickets and invalidations
-- are disposable derived state keyed by project/binding generation/thread; no run is needed.

-- Monotonic per-project clock of capsule-relevant eligibility. The triggers below advance it
-- inside the writer's own transaction, so every host and model writer is covered. A spurious
-- advance only forces a rebuild. Binding generation and source watermark are separate values.
CREATE TABLE capsule_eligibility_generations (
    project_id TEXT PRIMARY KEY NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0)
);

CREATE TABLE working_set_projections (
    project_id TEXT NOT NULL CHECK (octet_length(project_id) BETWEEN 1 AND 512),
    binding_generation INTEGER NOT NULL CHECK (binding_generation > 0),
    thread_id TEXT NOT NULL CHECK (octet_length(thread_id) BETWEEN 1 AND 512),
    projection_version INTEGER NOT NULL CHECK (projection_version > 0),
    run_id TEXT NULL CHECK (run_id IS NULL OR octet_length(run_id) BETWEEN 1 AND 512),
    projection_revision INTEGER NOT NULL CHECK (projection_revision > 0),
    source_watermark INTEGER NOT NULL CHECK (source_watermark >= 0),
    source_digest TEXT NOT NULL CHECK (octet_length(source_digest) <= 128),
    eligibility_generation INTEGER NOT NULL CHECK (eligibility_generation >= 0),
    payload TEXT NOT NULL CHECK (octet_length(payload) <= 131072),
    payload_digest TEXT NOT NULL CHECK (octet_length(payload_digest) <= 128),
    invalidation TEXT NULL CHECK (invalidation IN (
        'retirement', 'source_revision', 'scope', 'binding', 'archive', 'source_gap',
        'version', 'corrupt'
    )),
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (project_id, binding_generation, thread_id, projection_version)
);

CREATE TABLE working_set_file_samples (
    project_id TEXT NOT NULL CHECK (octet_length(project_id) BETWEEN 1 AND 512),
    binding_generation INTEGER NOT NULL CHECK (binding_generation > 0),
    thread_id TEXT NOT NULL CHECK (octet_length(thread_id) BETWEEN 1 AND 512),
    executor_id TEXT NOT NULL CHECK (octet_length(executor_id) BETWEEN 1 AND 512),
    root TEXT NOT NULL CHECK (octet_length(root) BETWEEN 1 AND 4096),
    relative_path TEXT NOT NULL CHECK (octet_length(relative_path) BETWEEN 1 AND 4096),
    sample_event TEXT NOT NULL CHECK (octet_length(sample_event) BETWEEN 1 AND 512),
    coverage TEXT NOT NULL CHECK (coverage IN (
        'complete', 'truncated', 'binary', 'oversized', 'unreadable', 'out_of_root'
    )),
    content TEXT NULL CHECK (content IS NULL OR octet_length(content) <= 65536),
    content_digest TEXT NULL CHECK (content_digest IS NULL OR octet_length(content_digest) <= 128),
    recorded_at_ms INTEGER NOT NULL,
    PRIMARY KEY (project_id, binding_generation, thread_id, executor_id, root, relative_path),
    CHECK ((content IS NULL) = (content_digest IS NULL)),
    CHECK ((coverage IN ('complete', 'truncated')) = (content IS NOT NULL))
);

CREATE TABLE working_set_publication_tickets (
    ticket_id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    binding_generation INTEGER NOT NULL CHECK (binding_generation > 0),
    thread_id TEXT NOT NULL,
    window_id TEXT NOT NULL CHECK (octet_length(window_id) BETWEEN 1 AND 128),
    history_generation INTEGER NOT NULL CHECK (history_generation >= 0),
    projection_version INTEGER NOT NULL CHECK (projection_version > 0),
    projection_revision INTEGER NOT NULL CHECK (projection_revision > 0),
    eligibility_generation INTEGER NOT NULL CHECK (eligibility_generation >= 0),
    capsule_digest TEXT NOT NULL CHECK (octet_length(capsule_digest) BETWEEN 1 AND 128),
    state TEXT NOT NULL CHECK (state IN ('admitted', 'installed', 'cancelled')),
    created_at_ms INTEGER NOT NULL
);

CREATE INDEX working_set_tickets_thread
ON working_set_publication_tickets (project_id, binding_generation, thread_id, ticket_id);

CREATE TABLE working_set_invalidations (
    project_id TEXT NOT NULL,
    binding_generation INTEGER NOT NULL CHECK (binding_generation > 0),
    thread_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    reason TEXT NOT NULL CHECK (reason IN (
        'retirement', 'source_revision', 'scope', 'binding', 'archive', 'source_gap',
        'version', 'corrupt'
    )),
    eligibility_generation INTEGER NOT NULL CHECK (eligibility_generation >= 0),
    recorded_at_ms INTEGER NOT NULL,
    PRIMARY KEY (project_id, binding_generation, thread_id, sequence)
);

-- Eligibility writers. Entry revisions, lifecycle and supersession; contexts (category,
-- authority, scope, validity); scopes and thread bindings; source exclusions and links;
-- identity aliases and current words (retirement); identity coverage; relations; source
-- removal. Premise/evidence links are written only with a new revision, which advances it.
CREATE TRIGGER capsule_generation_entry_insert AFTER INSERT ON blackboard_entries
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_entry_update AFTER UPDATE ON blackboard_entries
WHEN OLD.revision IS NOT NEW.revision OR OLD.project_id IS NOT NEW.project_id
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_entry_delete AFTER DELETE ON blackboard_entries
BEGIN INSERT INTO capsule_eligibility_generations VALUES (OLD.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_revision_insert AFTER INSERT ON blackboard_entry_revisions
BEGIN INSERT INTO capsule_eligibility_generations SELECT project_id, 1 FROM blackboard_entries WHERE id = NEW.entry_id ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_revision_update AFTER UPDATE ON blackboard_entry_revisions
BEGIN INSERT INTO capsule_eligibility_generations SELECT project_id, 1 FROM blackboard_entries WHERE id = NEW.entry_id ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_revision_delete AFTER DELETE ON blackboard_entry_revisions
BEGIN INSERT INTO capsule_eligibility_generations SELECT project_id, 1 FROM blackboard_entries WHERE id = OLD.entry_id ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_context_insert AFTER INSERT ON knowledge_context
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_context_update AFTER UPDATE ON knowledge_context
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_context_delete AFTER DELETE ON knowledge_context
BEGIN INSERT INTO capsule_eligibility_generations VALUES (OLD.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_scope_insert AFTER INSERT ON knowledge_scopes
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_scope_update AFTER UPDATE ON knowledge_scopes
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_scope_delete AFTER DELETE ON knowledge_scopes
BEGIN INSERT INTO capsule_eligibility_generations VALUES (OLD.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_scope_binding_insert AFTER INSERT ON knowledge_scope_bindings
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_scope_binding_update AFTER UPDATE ON knowledge_scope_bindings
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_scope_binding_delete AFTER DELETE ON knowledge_scope_bindings
BEGIN INSERT INTO capsule_eligibility_generations VALUES (OLD.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_exclusion_insert AFTER INSERT ON capture_source_exclusions
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_exclusion_delete AFTER DELETE ON capture_source_exclusions
BEGIN INSERT INTO capsule_eligibility_generations VALUES (OLD.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_entry_source_insert AFTER INSERT ON capture_entry_sources
BEGIN INSERT INTO capsule_eligibility_generations SELECT project_id, 1 FROM blackboard_entries WHERE id = NEW.entry_id ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_entry_source_delete AFTER DELETE ON capture_entry_sources
BEGIN INSERT INTO capsule_eligibility_generations SELECT project_id, 1 FROM blackboard_entries WHERE id = OLD.entry_id ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_alias_insert AFTER INSERT ON capture_identity_aliases
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_alias_update AFTER UPDATE ON capture_identity_aliases
WHEN OLD.retired IS NOT NEW.retired OR OLD.project_id IS NOT NEW.project_id
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_alias_delete AFTER DELETE ON capture_identity_aliases
BEGIN INSERT INTO capsule_eligibility_generations VALUES (OLD.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_words_insert AFTER INSERT ON capture_current_words
BEGIN INSERT INTO capsule_eligibility_generations SELECT project_id, 1 FROM blackboard_entries WHERE id = NEW.entry_id ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_words_update AFTER UPDATE ON capture_current_words
WHEN OLD.revision IS NOT NEW.revision OR OLD.scope_id IS NOT NEW.scope_id OR OLD.primary_words IS NOT NEW.primary_words OR OLD.retirement_words IS NOT NEW.retirement_words
BEGIN INSERT INTO capsule_eligibility_generations SELECT project_id, 1 FROM blackboard_entries WHERE id = NEW.entry_id ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_words_delete AFTER DELETE ON capture_current_words
BEGIN INSERT INTO capsule_eligibility_generations SELECT project_id, 1 FROM blackboard_entries WHERE id = OLD.entry_id ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_coverage_insert AFTER INSERT ON capture_identity_coverage
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_coverage_update AFTER UPDATE ON capture_identity_coverage
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_coverage_delete AFTER DELETE ON capture_identity_coverage
BEGIN INSERT INTO capsule_eligibility_generations VALUES (OLD.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_relation_insert AFTER INSERT ON blackboard_relations
BEGIN INSERT INTO capsule_eligibility_generations VALUES (NEW.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_relation_delete AFTER DELETE ON blackboard_relations
BEGIN INSERT INTO capsule_eligibility_generations VALUES (OLD.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
CREATE TRIGGER capsule_generation_source_delete AFTER DELETE ON capture_sources
BEGIN INSERT INTO capsule_eligibility_generations VALUES (OLD.project_id, 1) ON CONFLICT(project_id) DO UPDATE SET generation = generation + 1; END;
