# Private request parameters: legacy parity, not project activation

Source `cfdb20ccbd` / `97411af572` extracts the private helper signature
`acceptance_request(connection, run_id, goal)`. All five production callers pass
the same existing run's ID and goal. Original SQL, ledger timing, sort/append
order, byte spans, errors, public legacy wrapper and terminal behavior remain.
The caller contract is explicit: these parameters are not a binding certificate.
No public API, dependency, schema, native route, request cap or writer is added.

Fresh minimal-design and exact-source reviews APPROVE their limited stages.
Windows/WSL runtime 97/97 PASS each, zero skips/retries. Whole tuples/persisted
ordinal-ID rows cover Unicode/quotes, reopen and stable earlier spans when a
lexically earlier instruction is later applied; all non-applied statuses remain
excluded, and cancelled-run materialization does not change the run.
Both test and helper complete 9,606-file frames match at `97411af572`; all leaves
verify against retained Git objects. Scoped fix/final fmt PASS; fix made no
source edits. Final formatting only changes layout; no Rust retest afterward.

The external helper calls the rebuilt library's EXISTING PUBLIC LEGACY API in
two explicitly disposable legacy homes, not canonical `.stateful/memory.sqlite`.
Direct2 passes whole text, exact allowed durable deltas, complete ledger defaults,
stable ordinals, idempotent process reopen, schema/headers/all typed durable cells,
status filtering and note/home/file-tree checks. Raw post-terminal fixture edits
do not establish host steering admission. This is not OS confinement.
Fresh helper/controller review APPROVE. ONE actual unchanged-launcher gpt-6-luna
session PASS: 14.405 seconds, one inspector command, six materializations,
exact whole tool/final JSON and durable postflight, 30 archived bindings.
It qualifies the `9741` helper only, not a later cleanup or installed native binary.

Fresh offline lock generation failed on a cached yanked dependency. Reusing the
prior approved lock preserves all 621 dependency package definitions; only the
helper root identity/declared edges change. Locked/offline build PASS; no root
dependency/version change, online fallback or native installation occurred.

The wider project request writer remains DESIGN REVISE / DEFERRED. A separately
reviewed raw Python SQLite3.50.4 experiment shows missing-run VM steps growing
1,517/15,017/150,017 with unrelated history, versus 11 using a fixture-only run
index. A project filter hides a foreign binding. Evidence is LIMITED_TO_PYTHON,
fixed target cardinality/empty parts; it is not Rust/SQLx or universal performance
qualification. Logical table hashes match, not physical database pages.
No production index/migration or project-domain upgrade is implemented.

Next: review a run-prefix access path and explicit domain transition; account
foreign/unmaterialized scope, both count+new and max-ordinal+new, complete checked
byte/newline/metadata budgets and inherited trusted-schema limits. No silent
filtering, renumbering or caps on native legacy callers. See external
`request_20261017/NEXT_BOUNDING_GATE.md` and retained wider-design review.

Exact closeout/promotion receipts determine landing. B0 is separately owned and
unqualified. Native one-database/import/old-writer, real acceptance/source/learning,
Forget/compaction/MCP/offline/customer gates remain open. Evidence is under
`sc_dogfood/lsr_drivers/reports/stateful_rebuild/request_20261017`.
