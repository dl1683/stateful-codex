# Runtime legacy-write quiescence, 2026-10-17

The first project-owned, engine-neutral database milestone remains **OPEN**.
This slice adds an opt-in runtime row-write exclusion primitive, source commit
`b17440e6372e18fdec8e473a9cd6e25bd6c1e2be`, with no automatic/native routing
caller. It does not stop execution, migrate populated knowledge/runtime/metadata,
attach native app-server operations to a project store, or perform lock cutover.

Evidence packet: [native cutover admission](../sc_dogfood/lsr_drivers/reports/stateful_rebuild/native_cutover_admission_20261017/).
The relative directory above is outside this checkout; authoritative local path:
`C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/native_cutover_admission_20261017`.

## Source behavior and boundaries

`LegacyRuntimeQuiescence::open` opens an existing runtime file using a dedicated
connection, without creating/migrating it. `quiesce_project` takes writer admission,
compares the complete native migration ledger and compiled schema, atomically
installs42 runtime row guards plus3 registry guards, and commits a project marker.
All14 runtime tables are covered, including direct OR referenced-run ownership,
OLD/NEW ownership and conflicting UNIQUE/rowid victims under REPLACE/UPSERT.
Shared thread IDs do not establish project ownership. Existing identical read-only
creation replays remain permitted. Protected-key conflicts and the documented
`rowid = -1` conservative refusal can block an otherwise unrelated insert.

The connection becomes close-only before the first uncertain await and becomes
ready only on acknowledged commit. Awaited close drains the SQLx worker; error,
cancellation or uncertain commit requires fresh-owner reconciliation. Five-second
SQLite waits are per-lock waits, not a whole-operation deadline. The128-marker
and256-byte project-ID bounds are enforced. DML cannot alter/replace/delete an
existing marker; direct marker insertion is not an admission receipt. Arbitrary
DDL/schema tampering and malicious file access remain outside the row-DML contract.

The shared native pool is not closed. This primitive cannot be installed as an
execution-stop substitute: run_cancel persists Cancelled before interrupting, and
already-claimed continuations may still dispatch. Operational stopping precedes
live cutover. Knowledge/PI/metadata exclusion, FTS behavior, populated import,
coherent multi-database snapshots, cutover/recovery and a guardian remain gates.

## Verification and actual journey

Windows and WSL each passed116/116 targeted runtime tests, zero skips/retries.
Six new behavioral tests cover populated tables and pre-opened/pinned handles,
ownership conflicts and unrelated progress, registry/schema/history refusal,
concurrent/blocked writers, and deterministic cancellation/close/reconciliation.
The first SQLx compile refusal and pre-Cargo CRLF runner failure are retained.
`just fix -p codex-stateful-runtime` and `just fmt` passed; no Rust tests were rerun
after cleanup. The pinned-format/one named test-only closure-fix proof reproduces
final bytes from tested sources. Fresh Droid source approval binds all29 inputs,
exact base/head and799-line diff. The coherent atomic installer meets the800-line
ceiling; splitting off inert preparation is the smallest alternative stage.

The original literal-copy Luna experiment remains FAIL_RETAINED (18.087s):
one model call changed quoting, JavaScript did not parse, and zero underlying
commands/native fixtures ran. No retry or regrade of that packet.

A distinct retained-handle transport received fresh prospective Droid approval
(54 witness/29 source bindings). It held87 existing-file handles, preserving
bytes and native file IDs through all postflight checks and checked close. The
source controls include37 negative oracles, three rule membership/content changes,
two fake failure paths, six inherited JavaScript stub cases, and a separate child
write/delete/replacement denial control. These controls are not native results.

Its sole original-launcher gpt-6-luna journey remains **FAIL_RETAINED** (23.125s).
The exact raw code and allowlisted command matched, but the underlying command
exited1: Python cp1252 stdout could not encode Marathi after saving its report.
The generic score message combines command/exit checks; the failure is exit/status,
not command identity. The model honestly refused the requested success final;
exact Marathi final delivery is therefore NOT qualified. No actual retry/regrade.
No encoding repair was applied to either frozen packet. The next protocol version
must explicitly establish UTF-8 output and test a captured non-ASCII stdout path
before a new prospective review/journey; keep the current failure unchanged.

The separately delimited native fixture assertions passed before that output
failure: warmed native target updates and retained SQLx writes were refused,
read-only replay and unrelated B update succeeded, whole rows/rowids and exact
Marathi goal strings were preserved, all45 guards/integrity/FKs checked. Retained
Popen worker41244 exited0, installer63780 acknowledged close and exited23, both
reaped without emergency stop. This is runtime component evidence, not a passing
hosted journey. The native helper source pairs to all9,637 Git blobs at b174;
original launcher/binary5e4767 remained unchanged. All84 new and69 old frozen
input hashes still match;87 before/after retained identities agree.

Authoritative second packet:
`C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/native_locked_transport_20261017`.
Read `failed_journey_diagnosis1.json`, `luna1/score.json`, raw events/rollout,
`witness_review1.txt` and the first packet's `scratch/locked_transport1` native
receipts. The diagnosis opens no DB and does not regrade either journey.
Neither helper-only evidence nor model text qualifies native app-server routing,
historical-binary writer exclusion or the first database.

## Next coherent stage

Read `NEXT_KNOWLEDGE_BOUNDARY.md` and its bound source inventory in the evidence
packet, alongside the prior `native_attachment_20261017/NEXT_NATIVE_JOIN.md`.
The PI canonical reader validates an already-project-owned store; it is not a
legacy dependency selector. Two FTS5 virtual tables and ten shadow tables make
copying the runtime trigger generator insufficient. Freeze the full mutation and
ownership closure before implementing PI exclusion; prove all FTS transaction
paths, conflict victims and unrelated-project search/write progress. Preserve
ambiguous/dangling/cross-project refusal and all original failed evidence.

HAL source inspection is read-only, recorded in `hal_inventory/READONLY_NOTES.md`.
The existing owner queue is authoritative. A source-bound eleven-gap diagnostic
is in `hal_inventory/HAL_EMPTY_OVERLAY_DIAGNOSIS.md`:3 tables/8 cells, all empty
partition dispositions;8 exact-bbox replacement candidates have no origin IDs.
This is source diagnosis, not accepted identity edges or reproduced304/315 output
qualification. Two source pages were viewed, all five source PNG hashes agree;
all315 human items remain OPEN and date UNRESOLVED. No frozen translation asset
or human disposition changed. Offline, all-capability and customer qualification remain
open. This runtime component discharges none of those acceptance gates.
