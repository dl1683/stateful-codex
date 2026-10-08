-- Acceptance ledger owned by a run. Version 0010: runtime versions 0005-0008 are reserved for
-- the preserved branch histories and 0009 for the candidate capsule sources (INTEGRATION_PLAN
-- section 4), so this forward migration takes the next free version.
CREATE TABLE stateful_acceptance_ledgers (
    run_id TEXT PRIMARY KEY NOT NULL REFERENCES stateful_runs(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    workspace_generation INTEGER NOT NULL CHECK (workspace_generation >= 0),
    observed_executions INTEGER NOT NULL CHECK (observed_executions >= 0),
    stalled_completions INTEGER NOT NULL CHECK (stalled_completions >= 0),
    stalled_at_revision INTEGER NOT NULL CHECK (stalled_at_revision >= 0),
    verification_attempt INTEGER NOT NULL CHECK (verification_attempt >= 0),
    verification_owner TEXT,
    verification_lease_expires_at_ms INTEGER,
    updated_at_ms INTEGER NOT NULL,
    CHECK ((verification_owner IS NULL) = (verification_lease_expires_at_ms IS NULL))
);

CREATE TABLE stateful_acceptance_criteria (
    run_id TEXT NOT NULL REFERENCES stateful_acceptance_ledgers(run_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    criterion_id TEXT NOT NULL UNIQUE,
    origin TEXT NOT NULL CHECK (origin IN ('user', 'derived', 'omission')),
    kind TEXT NOT NULL CHECK (
        kind IN ('deliverable', 'constraint', 'check', 'manual', 'existence')
    ),
    state TEXT NOT NULL CHECK (state IN ('active', 'proposed', 'dismissed', 'retired')),
    statement TEXT NOT NULL,
    requirement TEXT NOT NULL,
    required INTEGER NOT NULL CHECK (required IN (0, 1)),
    depends_on_json TEXT NOT NULL,
    milestone TEXT,
    expected_observation TEXT,
    span_start INTEGER CHECK (span_start IS NULL OR span_start >= 0),
    span_end INTEGER,
    artifacts_json TEXT NOT NULL,
    check_command TEXT,
    check_cwd TEXT,
    approved_by_steering TEXT,
    dismissal_steering_id TEXT,
    dismissal_quote TEXT,
    dismissal_covered_by INTEGER,
    note TEXT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    ledger_revision INTEGER NOT NULL CHECK (ledger_revision > 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (run_id, ordinal),
    CHECK (check_command IS NULL OR expected_observation IS NOT NULL),
    CHECK (approved_by_steering IS NULL OR check_command IS NOT NULL),
    CHECK ((span_start IS NULL) = (span_end IS NULL)),
    CHECK (span_end IS NULL OR span_end > span_start),
    CHECK (origin = 'derived' OR span_start IS NOT NULL),
    CHECK (state NOT IN ('dismissed', 'retired') OR note IS NOT NULL),
    CHECK ((dismissal_steering_id IS NULL) = (dismissal_quote IS NULL)),
    CHECK (
        state <> 'dismissed'
        OR dismissal_steering_id IS NOT NULL
        OR dismissal_covered_by IS NOT NULL
    )
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
