-- Current-field eligibility probes retired aliases without scanning active identities.
CREATE INDEX capture_alias_retired_project ON capture_identity_aliases(project_id, retired);
