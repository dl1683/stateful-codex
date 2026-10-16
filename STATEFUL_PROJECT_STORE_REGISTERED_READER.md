# S1 registered read connection

Add an inert ProjectStoreReader in project-store. It opens only an explicitly
selected, qualified project-local database, validates the full supplied compiled
format2 manifest under a read snapshot, and owns one read-only SQLite connection.
Reuse bootstrap inspection's path/sidecar/filesystem checks. No pool, reconnect,
creation, migration, engine home or native routing is introduced.

A domain owner supplies its own complete compiled manifest when consuming this
reader into an owned identity/connection pair. Domain name, actual versions, SQL
bytes and transactional/simple migration flags must match the admitted manifest;
an unknown/partial/changed owner receives no connection. This is a trusted Rust
boundary for a future domain facade, not a foreign SQL interface or authorization
boundary. A single connection cannot silently reconnect to a different project.

Metadata identity records admission, not knowledge truth or current filesystem
freshness. Domain readers must retain their existing eligibility, retirement,
snapshot and scope checks. Close and reselect after a move. Namespace replacement
by the same filesystem user is not a security boundary. Writable pools and atomic
completion remain S2; populated import/native cache activation remain S3.

Tests exercise real local placement and Rust domain initialization, read-only
queries/write refusal, missing/bootstrap/changed-schema refusal without creation
or repair, compiled-domain mismatch, independent identities and close/move/reopen.
Existing filesystem/mount/redirect tests remain in the same crate gate. No new
dependencies or build-time data are needed. Local serialized Windows/WSL just
tests, scoped fix/final fmt, then fresh exact Droid review; no post-cleanup tests.
Effective knowledge CLI/MCP reads and native first-milestone acceptance do not
follow from this connection factory alone.
