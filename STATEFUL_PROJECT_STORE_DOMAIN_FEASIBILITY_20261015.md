# Domain composition and incomplete-copy preparation

Measured 2026-10-15 against the candidate built from integrated source 922ee4509.
This is disposable source-SQL preparation, not an implemented domain installer,
legacy converter, effective-read interface or native cutover qualification.

The candidate first initialized a fresh qualified scratch project. Python then
executed all 25 checked-in intelligence and eight runtime SQL migrations in that
same database. Together with bootstrap metadata the result had 67 tables including
SQLite/FTS internals, 49 triggers, integrity_check=ok and no foreign-key violations.
This establishes composition of these source SQL files in the recorded Python
SQLite runtime. It does not establish SQLx history compatibility, transactional
domain installation, minimum supported SQLite versions or populated conversion.
The source inventory retains every migration's SHA-256; histories were not merged
into a shared _sqlx_migrations table or registered as supported project domains.

Two synthetic projects received hierarchy nodes, entries/revisions and runtime
runs/thread links through direct SQL. These rows are fixture construction, not
supported foreign writes or proof of application-level eligibility/authority.
A deliberately incomplete in-memory transfer copied only project-a's three parent
tables: hierarchy_nodes, blackboard_entries and stateful_runs. Each selected parent
count was 1 in both databases; integrity and foreign-key checks still passed.
The selected entry revision and runtime thread link were each present in the
source and absent from the partial copy. Consequently, parent counts plus those
SQLite checks cannot substitute for verified dependency closure and effective
reads. This demonstrates a failure of those acceptance checks, not a new converter.

The current CLI then read the composed but unregistered store. It returned exit 1,
unsupportedStore, and no automatic repair; the database SHA-256 stayed unchanged.
Fail-closed registry validation therefore remains in force. Neither extra tables
nor a successful schema composition silently activate application domains.

The first attempt omitted required stateful_runs.strategy_revision during fixture
seeding and failed before the counterexample/reader checks. Its directory and
failure note remain retained. A distinct v2 fixture supplied revision 0 and
produced the observations above; no failed attempt was counted as a passing gate.

Receipts: external sc_dogfood/lsr_drivers/reports/stateful_rebuild_domain_composition.py
and stateful_rebuild_domain_composition_20261015/{result.json,attempt1_failed.md},
plus the retained integrated candidate and final source/hash manifest. No engine
home rows were read or migrated. No native writer barrier, migration-59 cohort,
rollback, actual killed-writer/power-loss, domain contention, Forget/completion or
first company-brain milestone qualification follows. Implement the explicitly
reviewed registry/domain/read boundaries in STATEFUL_PROJECT_STORE_READS_NEXT_20261015.md.
