-- Immutable observed sources, separate from semantic entries and their journal clock.
CREATE TABLE capture_sources (
    source_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    original_event_id TEXT NOT NULL,
    authoritative_thread_id TEXT NOT NULL CHECK(octet_length(authoritative_thread_id) <= 512),
    turn_id TEXT NOT NULL CHECK(octet_length(turn_id) <= 512),
    binding_generation INTEGER NOT NULL CHECK(binding_generation > 0),
    source_revision INTEGER NOT NULL CHECK (source_revision > 0),
    part_index INTEGER NOT NULL CHECK (part_index >= 0),
    digest TEXT NOT NULL,
    metadata TEXT NOT NULL CHECK (octet_length(metadata) <= 8192),
    observed_sequence INTEGER NOT NULL CHECK (observed_sequence > 0),
    recorded_at_ms INTEGER NOT NULL,
    UNIQUE(project_id, original_event_id, source_revision, part_index)
);
CREATE INDEX capture_sources_project ON capture_sources(project_id, observed_sequence);
CREATE INDEX capture_sources_turn ON capture_sources(project_id, turn_id);
CREATE INDEX capture_sources_digest ON capture_sources(project_id, digest);
CREATE TABLE capture_projection_coverage (
    project_id TEXT PRIMARY KEY,
    after_source TEXT NOT NULL DEFAULT '',
    after_byte INTEGER NOT NULL DEFAULT -1,
    complete INTEGER NOT NULL DEFAULT 0,
    generation INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE capture_source_chunks (
    source_id TEXT NOT NULL REFERENCES capture_sources(source_id),
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    exact_bytes TEXT NOT NULL CHECK (octet_length(exact_bytes) <= 4096),
    chunk_digest TEXT NOT NULL,
    search_text TEXT NOT NULL,
    PRIMARY KEY(source_id, start_byte)
);
CREATE TABLE capture_source_terms (
    project_id TEXT NOT NULL,
    term TEXT NOT NULL,
    source_id TEXT NOT NULL,
    start_byte INTEGER NOT NULL,
    PRIMARY KEY(project_id, term, source_id, start_byte),
    FOREIGN KEY(source_id, start_byte) REFERENCES capture_source_chunks(source_id, start_byte)
);
CREATE TABLE capture_entry_sources (
    entry_id TEXT NOT NULL REFERENCES blackboard_entries(id),
    source_id TEXT NOT NULL REFERENCES capture_sources(source_id),
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    role TEXT NOT NULL,
    PRIMARY KEY(entry_id, source_id, start_byte, end_byte, role)
);
CREATE INDEX capture_entry_sources_range ON capture_entry_sources(source_id, start_byte, end_byte);
-- Exclusions survive projection rebuilds and cover copies of the same exact source part.
CREATE TABLE capture_source_exclusions (
    project_id TEXT NOT NULL,
    digest TEXT NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    entry_id TEXT NOT NULL,
    PRIMARY KEY(project_id, digest, start_byte, end_byte, entry_id)
);
CREATE TABLE capture_group_actions (
    project_id TEXT NOT NULL,
    action_id TEXT NOT NULL,
    group_id TEXT NOT NULL,
    request_fingerprint TEXT NOT NULL,
    PRIMARY KEY(project_id, action_id),
    UNIQUE(project_id, group_id),
    FOREIGN KEY(project_id, group_id) REFERENCES capture_groups(project_id, group_id)
);

-- Bounded omission state retains the native tuple without inventing exact bytes/digests.
CREATE TABLE capture_source_omissions (
    project_id TEXT NOT NULL,
    original_event_id TEXT NOT NULL,
    source_revision INTEGER NOT NULL,
    metadata TEXT NOT NULL CHECK (octet_length(metadata) <= 8192),
    observed_sequence INTEGER NOT NULL,
    recorded_at_ms INTEGER NOT NULL,
    PRIMARY KEY(project_id, original_event_id, source_revision)
);
