-- Revision-bound meaning of blackboard entries, investigation scopes, capture groups, and the
-- journal of committed memory changes. Entries written before this migration have no context
-- row and are treated as legacy.

CREATE TABLE knowledge_context (
    entry_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    project_id TEXT NOT NULL,
    category TEXT NOT NULL CHECK (category IN (
        'rule', 'background', 'attributed_context', 'decision', 'brainstorm_option',
        'ruled_out', 'open_check', 'recipe', 'code_observation', 'commit_observation',
        'note', 'legacy'
    )),
    authority TEXT NOT NULL CHECK (authority IN (
        'human_direct', 'assistant_reported', 'reported_third_party', 'host_observed',
        'legacy_unknown'
    )),
    scope_id TEXT NULL,
    end_condition TEXT NULL,
    source_sequence INTEGER NULL CHECK (source_sequence IS NULL OR source_sequence > 0),
    unit_ordinal INTEGER NULL CHECK (unit_ordinal IS NULL OR unit_ordinal >= 0),
    group_id TEXT NULL,
    validity TEXT NOT NULL CHECK (validity IN ('current', 'needs_check', 'obsolete', 'historical')),
    payload TEXT NULL,
    PRIMARY KEY (entry_id, revision),
    FOREIGN KEY (entry_id, revision)
        REFERENCES blackboard_entry_revisions(entry_id, revision) ON DELETE CASCADE
);

CREATE INDEX knowledge_context_scope
ON knowledge_context (project_id, scope_id, category, validity);

CREATE INDEX knowledge_context_group
ON knowledge_context (project_id, group_id, source_sequence, unit_ordinal);

CREATE TABLE knowledge_scopes (
    project_id TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('investigation', 'task')),
    title TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('open', 'ended')),
    end_condition TEXT NULL,
    opened_source TEXT NOT NULL,
    ended_source TEXT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (project_id, scope_id),
    CHECK ((state = 'ended') = (ended_source IS NOT NULL))
);

CREATE TABLE knowledge_scope_bindings (
    project_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    bound_at_ms INTEGER NOT NULL,
    PRIMARY KEY (project_id, thread_id),
    FOREIGN KEY (project_id, scope_id)
        REFERENCES knowledge_scopes(project_id, scope_id) ON DELETE CASCADE
);

CREATE TABLE knowledge_source_sequences (
    project_id TEXT PRIMARY KEY NOT NULL,
    next_sequence INTEGER NOT NULL CHECK (next_sequence > 0)
);

CREATE TABLE capture_groups (
    project_id TEXT NOT NULL,
    group_id TEXT NOT NULL,
    thread_id TEXT NULL,
    turn_id TEXT NULL,
    kind TEXT NOT NULL,
    declared_count INTEGER NULL,
    recognized INTEGER NOT NULL CHECK (recognized >= 0),
    saved INTEGER NOT NULL CHECK (saved >= 0),
    already_present INTEGER NOT NULL CHECK (already_present >= 0),
    pending INTEGER NOT NULL CHECK (pending >= 0),
    omitted INTEGER NOT NULL CHECK (omitted >= 0),
    failed INTEGER NOT NULL CHECK (failed >= 0),
    recorded_at_ms INTEGER NOT NULL,
    PRIMARY KEY (project_id, group_id)
);

CREATE TABLE memory_changes (
    project_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    entry_id TEXT NULL,
    revision INTEGER NULL,
    operation TEXT NOT NULL CHECK (operation IN (
        'saved', 'promoted', 'corrected', 'forgotten', 'invalidated', 'scope_ended',
        'capture_incomplete'
    )),
    origin TEXT NOT NULL CHECK (origin IN (
        'host_capture', 'direct_control', 'model_tool', 'host_observed'
    )),
    category TEXT NOT NULL,
    action_id TEXT NULL,
    thread_id TEXT NULL,
    turn_id TEXT NULL,
    group_id TEXT NULL,
    preview TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (project_id, sequence)
);

CREATE INDEX memory_changes_thread
ON memory_changes (project_id, thread_id, sequence);
