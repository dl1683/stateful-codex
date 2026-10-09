-- Host-observed facts behind the read-only exemption: side-effecting actions the host saw for
-- the run (any one ends the exemption), and the exemption the terminal transaction recorded.
ALTER TABLE stateful_acceptance_ledgers
ADD COLUMN side_effects INTEGER NOT NULL DEFAULT 0 CHECK (side_effects >= 0);

ALTER TABLE stateful_acceptance_ledgers
ADD COLUMN exemption TEXT CHECK (exemption IS NULL OR exemption = 'readOnly');

-- 1 when the host observed the run's effects since the run began; 0 for runs that began
-- before effect observation existed, whose earlier effects are unknown. New ledgers set 1.
ALTER TABLE stateful_acceptance_ledgers
ADD COLUMN observation_version INTEGER NOT NULL DEFAULT 0 CHECK (observation_version >= 0);

-- Every run still able to complete that has no ledger yet began before effect observation.
INSERT INTO stateful_acceptance_ledgers (
    run_id, revision, workspace_generation, observed_executions, stalled_completions,
    stalled_fingerprint, verification_attempt, updated_at_ms, observation_version
)
SELECT id, 0, 0, 0, 0, '', 0, updated_at_ms, 0
FROM stateful_runs
WHERE status NOT IN ('completed', 'cancelled', 'failed')
  AND id NOT IN (SELECT run_id FROM stateful_acceptance_ledgers);
