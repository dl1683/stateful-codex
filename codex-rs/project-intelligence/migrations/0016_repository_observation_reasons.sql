-- Store one unknown reason per observed component instead of one shared reason
-- per root. sqlx runs this migration inside a transaction.
CREATE TABLE repository_root_observations_v2 (
    observation_id TEXT NOT NULL REFERENCES repository_observations(id),
    project_root TEXT NOT NULL,
    git_worktree_root TEXT NULL,
    head_state TEXT NOT NULL CHECK (head_state IN ('commit', 'unborn', 'unknown')),
    head_oid TEXT NULL,
    head_ref TEXT NULL,
    worktree_state TEXT NOT NULL CHECK (worktree_state IN ('clean', 'dirty', 'unknown')),
    dirty_digest TEXT NULL,
    dirty_coverage TEXT NOT NULL CHECK (dirty_coverage IN ('complete', 'unknown')),
    head_unknown_reason TEXT NULL CHECK (head_unknown_reason IN (
        'notGit', 'missingRoot', 'inaccessibleRoot', 'gitUnavailable', 'gitCommandFailed',
        'invalidGitOutput', 'timeout', 'outputLimit', 'unsupportedPath', 'unstableSample',
        'dirtyFingerprintNotCollected'
    )),
    worktree_unknown_reason TEXT NULL CHECK (worktree_unknown_reason IN (
        'notGit', 'missingRoot', 'inaccessibleRoot', 'gitUnavailable', 'gitCommandFailed',
        'invalidGitOutput', 'timeout', 'outputLimit', 'unsupportedPath', 'unstableSample',
        'dirtyFingerprintNotCollected'
    )),
    dirty_unknown_reason TEXT NULL CHECK (dirty_unknown_reason IN (
        'notGit', 'missingRoot', 'inaccessibleRoot', 'gitUnavailable', 'gitCommandFailed',
        'invalidGitOutput', 'timeout', 'outputLimit', 'unsupportedPath', 'unstableSample',
        'dirtyFingerprintNotCollected'
    )),
    PRIMARY KEY (observation_id, project_root),
    CHECK ((head_state = 'commit') = (head_oid IS NOT NULL)),
    CHECK (head_oid IS NULL OR (length(head_oid) IN (40, 64) AND head_oid NOT GLOB '*[^0-9a-f]*')),
    CHECK (head_state <> 'unknown' OR head_ref IS NULL),
    CHECK (worktree_state <> 'clean' OR dirty_coverage = 'complete'),
    CHECK (worktree_state <> 'unknown' OR dirty_coverage = 'unknown'),
    CHECK (
        (dirty_digest IS NOT NULL) = (worktree_state = 'dirty' AND dirty_coverage = 'complete')
    ),
    CHECK ((head_unknown_reason IS NOT NULL) = (head_state = 'unknown')),
    CHECK ((worktree_unknown_reason IS NOT NULL) = (worktree_state = 'unknown')),
    CHECK (
        (dirty_unknown_reason IS NOT NULL) = (worktree_state = 'dirty' AND dirty_coverage = 'unknown')
    )
);

INSERT INTO repository_root_observations_v2 (
    observation_id, project_root, git_worktree_root, head_state, head_oid, head_ref,
    worktree_state, dirty_digest, dirty_coverage,
    head_unknown_reason, worktree_unknown_reason, dirty_unknown_reason
)
SELECT observation_id, project_root, git_worktree_root, head_state, head_oid, head_ref,
       worktree_state, dirty_digest, dirty_coverage,
       CASE WHEN head_state = 'unknown' THEN unknown_reason END,
       CASE WHEN worktree_state = 'unknown' THEN unknown_reason END,
       CASE WHEN worktree_state = 'dirty' AND dirty_coverage = 'unknown'
            THEN unknown_reason END
FROM repository_root_observations;

DROP TABLE repository_root_observations;

ALTER TABLE repository_root_observations_v2
RENAME TO repository_root_observations;
