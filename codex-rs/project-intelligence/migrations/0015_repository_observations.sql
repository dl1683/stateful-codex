CREATE TABLE repository_observations (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    format_version INTEGER NOT NULL CHECK (format_version = 1),
    started_at_ms INTEGER NOT NULL,
    completed_at_ms INTEGER NOT NULL,
    roots_digest TEXT NOT NULL,
    roots_coverage TEXT NOT NULL CHECK (roots_coverage IN ('complete', 'unknown')),
    omitted_root_count INTEGER NOT NULL,
    CHECK (completed_at_ms >= started_at_ms),
    CHECK (omitted_root_count >= 0),
    CHECK (roots_coverage = 'unknown' OR omitted_root_count = 0)
);

CREATE INDEX repository_observations_project_completed
ON repository_observations (project_id, completed_at_ms, id);

CREATE TABLE repository_root_observations (
    observation_id TEXT NOT NULL REFERENCES repository_observations(id),
    project_root TEXT NOT NULL,
    git_worktree_root TEXT NULL,
    head_state TEXT NOT NULL CHECK (head_state IN ('commit', 'unborn', 'unknown')),
    head_oid TEXT NULL,
    head_ref TEXT NULL,
    worktree_state TEXT NOT NULL CHECK (worktree_state IN ('clean', 'dirty', 'unknown')),
    dirty_digest TEXT NULL,
    dirty_coverage TEXT NOT NULL CHECK (dirty_coverage IN ('complete', 'unknown')),
    unknown_reason TEXT NULL CHECK (unknown_reason IN (
        'notGit', 'missingRoot', 'inaccessibleRoot', 'gitUnavailable', 'timeout',
        'outputLimit', 'unsupportedPath', 'unstableSample', 'dirtyFingerprintNotCollected'
    )),
    PRIMARY KEY (observation_id, project_root),
    CHECK ((head_state = 'commit') = (head_oid IS NOT NULL)),
    CHECK (head_state <> 'unknown' OR head_ref IS NULL),
    CHECK (worktree_state <> 'clean' OR dirty_coverage = 'complete'),
    CHECK (worktree_state <> 'unknown' OR dirty_coverage = 'unknown'),
    CHECK (
        (dirty_digest IS NOT NULL) = (worktree_state = 'dirty' AND dirty_coverage = 'complete')
    ),
    CHECK (
        (unknown_reason IS NOT NULL) = (head_state = 'unknown' OR dirty_coverage = 'unknown')
    )
);
