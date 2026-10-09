-- Host-observed facts behind the read-only exemption: side-effecting actions the host saw for
-- the run (any one ends the exemption), and the exemption the terminal transaction recorded.
ALTER TABLE stateful_acceptance_ledgers
ADD COLUMN side_effects INTEGER NOT NULL DEFAULT 0 CHECK (side_effects >= 0);

ALTER TABLE stateful_acceptance_ledgers
ADD COLUMN exemption TEXT CHECK (exemption IS NULL OR exemption = 'readOnly');
