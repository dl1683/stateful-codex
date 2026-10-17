CREATE INDEX stateful_steering_run_status_revision
ON stateful_steering (run_id, status, resulting_strategy_revision, created_at_ms, id);
