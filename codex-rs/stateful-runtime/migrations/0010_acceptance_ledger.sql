-- Acceptance ledger owned by a run. Version 0010: runtime versions 0005-0008 are reserved for
-- the preserved branch histories and 0009 for the candidate capsule sources (INTEGRATION_PLAN
-- section 4), so this forward migration takes the next free version.
CREATE TABLE stateful_acceptance_ledgers (
    run_id TEXT PRIMARY KEY NOT NULL REFERENCES stateful_runs(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    workspace_generation INTEGER NOT NULL CHECK (workspace_generation >= 0),
    omission_checked INTEGER NOT NULL CHECK (omission_checked IN (0, 1)),
    stalled_completions INTEGER NOT NULL CHECK (stalled_completions >= 0),
    stalled_at_revision INTEGER NOT NULL CHECK (stalled_at_revision >= 0),
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE stateful_acceptance_criteria (
    run_id TEXT NOT NULL REFERENCES stateful_acceptance_ledgers(run_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    origin TEXT NOT NULL CHECK (origin IN ('user', 'derived', 'omission')),
    kind TEXT NOT NULL CHECK (kind IN ('deliverable', 'constraint', 'check', 'manual')),
    state TEXT NOT NULL CHECK (state IN ('active', 'proposed', 'dismissed', 'retired')),
    statement TEXT NOT NULL,
    span_start INTEGER CHECK (span_start IS NULL OR span_start >= 0),
    span_end INTEGER,
    artifacts_json TEXT NOT NULL,
    check_command TEXT,
    note TEXT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (run_id, ordinal),
    CHECK ((span_start IS NULL) = (span_end IS NULL)),
    CHECK (span_end IS NULL OR span_end > span_start),
    CHECK (origin = 'derived' OR span_start IS NOT NULL),
    CHECK (state NOT IN ('dismissed', 'retired') OR note IS NOT NULL)
);

CREATE TABLE stateful_acceptance_evidence (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('hostCommand', 'manual', 'noCheck')),
    outcome TEXT NOT NULL CHECK (outcome IN ('passed', 'failed', 'observed', 'unavailable')),
    command TEXT,
    exit_code INTEGER,
    output_tail TEXT,
    output_digest TEXT,
    artifact_digest TEXT,
    detail TEXT,
    workspace_generation INTEGER NOT NULL CHECK (workspace_generation >= 0),
    criterion_revision INTEGER NOT NULL CHECK (criterion_revision > 0),
    source_id TEXT NOT NULL,
    observed_at_ms INTEGER NOT NULL,
    FOREIGN KEY (run_id, ordinal)
        REFERENCES stateful_acceptance_criteria(run_id, ordinal) ON DELETE CASCADE
);

CREATE INDEX stateful_acceptance_evidence_criterion
ON stateful_acceptance_evidence (run_id, ordinal, sequence DESC);
