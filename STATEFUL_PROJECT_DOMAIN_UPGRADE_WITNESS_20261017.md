# Explicit domain upgrade witness — 2026-10-17

> **Archived historical record, not current operating instructions.**
> Use [current state](./STATEFUL_CODEX_CURRENT_STATE.md) for status and holds.
> Historical external evidence is not distributed in this repository.
> Paths and personal identifiers are redacted; original bytes and any
> historical digest claims refer to the version preserved in Git history.

Inactive library component; first project-database milestone NOT FULFILLED.
PreparedDomainUpgrade requires complete trusted manifests, identical domains and
unchanged per-domain prefixes plus suffix. Ordinary admission stays exact.
The borrowed writer validates predecessor/UUIDs under BEGIN IMMEDIATE, applies
suffix/history, validates successor, commits once then synchronously switches its
manifest. Failure/cancellation is close-only. After awaited close, reconciliation
checks both complete cohorts in ONE qualified read-only snapshot. Observed state
is not an exclusive reservation. Prefix/schema equality does not prove arbitrary
trusted migration SQL preserves data. Held WAL snapshots/raw readers may finish;
fresh old admission refuses. This is not old-client exclusion.

Executed source `e8ec5a4f7f50cc1396284ed8ba20e24195adf399`: fresh review3 APPROVE.
Windows3/WSL1 pass 56/56 each, no skips/automatic retries; all 9,612 source leaves
match Git objects and test/helper frames. Windows1 macro failure and Windows2
55/56 invalid-fixture failure retained; fixes test-only. Private commit-hook test
drops the future after commit entry, awaits close and freshly reconciles actual
successor; the hook marker alone is not durable evidence. Scoped fix/final fmt pass,
fix no edits, later cleanup layout only, no Rust retests afterward.

Locked/offline unique helper, 621 unchanged dependency definitions. Direct1 passes
8 checks across 2 canonical project fixtures, 68 tables each: both real schemas plus
fixture-upgrade domain, synthetic retirement/journals/answers/allocator/NUL/REAL/
BLOB data. Whole typed tables/schema, selected stable header fields, UUIDs and
prior history compared; only fixture index/new history may change. Rollback,
cross-process
busy close-only, reconciliation and old-reader/writer refusal pass. Fresh helper
review APPROVE precedes the sole original-launcher gpt-6-luna PASS: 18.238 seconds,
one command, 44 archived bindings, with full durable controller postflight.

Evidence: historical-evidence/domain_upgrade_20261017/
Exact closeout/promotion separately determine landing. No production runtime
index, native routing/import, arbitrary-migration preservation, native CLI
interoperability for the extra fixture domain, helper cancellation, old-client
exclusion, offline/customer or native HAL qualification. Helper home is isolated;
normal original-launcher session/cache writes are not home invariance. B0 separate.
