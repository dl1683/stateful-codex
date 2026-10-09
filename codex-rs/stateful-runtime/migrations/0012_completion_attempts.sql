-- Completion calls the host released to dispatch for the run. The no-tool exemption admits
-- only the one completion that terminally succeeds: a run with any earlier attempt owes the
-- ledger. Existing ledgers start at 0, which never admits the exemption.
ALTER TABLE stateful_acceptance_ledgers
ADD COLUMN completion_attempts INTEGER NOT NULL DEFAULT 0 CHECK (completion_attempts >= 0);
