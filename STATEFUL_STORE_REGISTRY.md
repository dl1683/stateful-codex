# S1b1 design brief: project-store identity and bootstrap registry

Authority: converged STATEFUL_REBUILD_STRATEGY_CURRENT.md, sections 3-6.
This is an inert foundation complementary to the other rebuild executor's
ProjectStoreLocation resolver. Custody: sc_wt_rebuild_registry on
rebuild/store-migration-registry; resolver stays in sc_wt_rebuild.

Add a small codex-project-store kernel that initializes an explicitly supplied
SQLite connection under one BEGIN IMMEDIATE transaction. It creates only fixed,
bundled bootstrap SQL: one project UUID, one store UUID, format/capability version,
and the immutable (domain,version,SHA256) migration registry. No application-domain
migration, legacy `_sqlx_migrations` merge, path routing or native cutover occurs.
SQLite connections remain caller-owned; this slice does not open any file.

Initialization on an empty database creates the identity once. Repeated opens
validate it and the complete known registry rather than resetting or repairing
unknown state. Unknown schema, partial state, malformed UUIDs, extra bootstrap
records or digest/version mismatch refuse. Unknown domains are not supported
until their owner registers and validates them in a later coherent slice.
Schema SQL is fixed compile-time input and is listed as Bazel compile_data.
The registry does not accept or execute caller-provided SQL.

The caller must set a bounded SQLite busy timeout. SQLite serializes competing
initializations through BEGIN IMMEDIATE; no process-local lock is relied on for
database correctness. Tests use independent connections to a disposable file,
plus malformed/partial/foreign-format and allocation-failure rollback fixtures.
No writer lock spans model inference, filesystem traversal, another database
commit or background work in this kernel. Busy outcomes are propagated honestly.

This does not establish qualified filesystem storage, migration compatibility,
concurrent native operation, exact answer retention, retirement behavior or the
company-brain milestone. Future path-opening policy must accept only qualified
local storage and revalidate resolver paths. Native activation remains S2/S3.
Bounded result projections do not qualify SQLite engine CPU/heap use on arbitrary
hostile or corrupt files; that resource-admission gate remains separate.

Gate: local just test -p codex-project-store under Local\StatefulB0LocalBuild,
CARGO_BUILD_JOBS=2 and 23,221,225,472 free bytes; then scoped fix/final fmt, no
postcleanup tests. Dependency changes require just bazel-lock-update. Fresh
headless Droid reviews the exact final commit, and nontrivial integration is
reviewed again. Existing HAL behavior has no call sites into this inert crate.
