# S1 domain installation: explicit format transition

Review units: first manifest preparation/snapshot admission; then explicit atomic
installation and its rollback/contention witnesses. Neither activates native use.

Authority: the owner-invoked rebuild strategy and
STATEFUL_PROJECT_STORE_READS_NEXT_20261015.md. This slice is inert: no native
home, application attachment, legacy import, or completion-fence change.

Project-store will accept a trusted, compiled domain manifest (domain name and
SQLx migration list), without depending upward on either application crate.
Preparation builds the expected schema in an isolated in-memory SQLite database
before taking the destination writer lock. Domain/version/SHA-256 records use
actual migration versions, including gaps; legacy _sqlx_migrations is never used.
SQL supplied through this Rust API is trusted application code, never a CLI,
MCP, or foreign-agent input. Admission is not a filesystem security boundary.

An explicit installation transaction validates the exact existing bootstrap,
applies all requested domain migrations, checks the resulting schema, and records
format/min-reader/min-writer version 2 with an immutable kernel transition digest.
The original bootstrap SQL and migration digest remain unchanged. UUIDs survive.
Failures roll back all DDL, registry and identity changes. Repeated installation
accepts only the identical complete manifest and schema; domain upgrades or
partial-domain addition require a later reviewed transition. Old bootstrap APIs
continue to refuse application stores rather than adopting or upgrading them.

New read admission validates format 2, complete migration history and schema
against that manifest under one snapshot. Extra tables/triggers, missing records,
changed SQL, newer formats and downgrades refuse. Bounded schema/history queries
prevent unbounded Rust projections; this does not qualify hostile SQLite CPU/heap.
Callers own qualified connection placement and bounded SQLite busy timeouts.
No filesystem traversal, model inference or another database commit occurs while
the destination writer lock is held. Migration DDL is initialization work, not
the separately budgeted background-index or interactive completion path.

Fixtures: preserved identity and idempotence, two domains with overlapping/gapped
version numbers, read-only reopen, changed/unknown/partial history and schema
refusal, failure rollback, independent connections and bounded busy refusal.
Real application-schema installation and safe injected handles follow separately;
synthetic manifests do not establish populated conversion or effective recall.
Local tests use the shared build mutex/jobs2/RAM floor, scoped fix then final fmt;
fresh Droid reviews exact final changes before stateful-only promotion.
