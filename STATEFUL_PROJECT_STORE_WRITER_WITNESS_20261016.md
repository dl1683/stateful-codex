# Project-local writer qualification â€” 2026-10-16

Source component: `ab8764ef5d8207164396b181a8b60cb8af6acbd3`, based on
`139e3b62c7d3e6676477f36b14f8fa26d9b0d942`. Fresh `source_review1.txt`
approves the exact four-file, 493-added-line diff. Evidence directory:
`C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/writer_20261016`.
This is an inert trusted Rust API, not native activation or a foreign SQL endpoint.

`ProjectStoreWriter` selects only an existing qualified project-local store,
requires expected project/store UUIDs and the prepared compiled domains, performs
readonly admission before writable WAL setup, and uses a two-second busy timeout,
foreign keys and FULL synchronous mode. Each short `BEGIN IMMEDIATE` rechecks the
kernel, identities and complete registered history. The guard commits prior domain
writes together, revalidates before commit and supports explicit rollback.
An unfinished/dropped/cancelled/failed operation makes its owner close-only.
Cancellation during COMMIT needs close and reconciliation; no guaranteed rollback
or power-loss certification is claimed. No native application lock was removed.

## Recorded local checks

- Windows `windows_store3.log` and WSL `linux_store2.log`: 51 passed each,
  zero skips/retries. Independent-process contention and another project remaining
  writable are exercised. Complete 9562-input maps match before/after each gate.
- Scoped `just fix` and final `just fmt` pass; no tests after cleanup. The later
  argument-comment-only repair2c673ae7a9 was separately reviewed with the runtime
  source; it is eligible for promotion but is not part of the frozen ab8764 artifact.
- `direct_trial2/score.json`: PASS_REAL_SCHEMA_TRANSACTION_COMPONENT_ONLY.
  Fresh full intelligence/runtime schemas have 67 tables/35 migration records.
  Independent SQLite logical row/schema/history checks prove explicit rollback,
  drop rollback and cross-domain commit. A separate competitor reports typed Busy
  in 2.365 seconds while another project commits. The owned helper was terminated
  only after its marker confirmed both uncommitted domain writes; recovery kept
  neither write, integrity/FKs passed and a later writer acquired the lock.
- The original `direct_trial1` remains FAIL: the observer tried to hash live SHM
  and Windows denied access. Its owned child was collected and postflight passed.
  Trial2 records operational sidecar access failures explicitly; closed main bytes,
  consistent logical reads, integrity/FKs and all16 fixed bindings are required.
  It does not claim physical immutability of live SQLite sidecars.
- The helper and product CLI were built locally locked/offline with the shared
  global build mutex, jobs2 and RAM floor23221225472 bytes. They bind the same
  reviewed source HEAD and all9562 input leaves. Full archive and auxiliary hashes
  are retained; these are custody receipts, not cryptographic build attestation.

## Luna check

One actual unchanged-launcher `gpt-6-luna` run, `luna_policy_trial3`, is
**FAIL_LITERAL_CELL_CONTRACT_RETAINED** (`failure.json` says unapproved code-mode
cell). Each raw code cell added exactly one trailing LF to the externally approved
literal. The original failure and all raw evidence remain unchanged; no retry,
normalization or weakened scoring was accepted. Earlier controller reviews and
preparations1/2 made no native/model call. Fresh `luna_review3.txt` approved the
56-binding frozen proposal before the sole actual run.

A separate diagnostic `independent_audit.json` confirms two ordered profile-free
exit0 commands, exact whole tool results/final/answer-file JSON, one completed
turn, two authoritative custom calls and zero prohibited calls. Raw inventory has
two calls/two outputs. Both literal hashes differ by the added LF; this diagnosis
does not upgrade the failed trial. Stock-provider/managed-policy/ChatGPT-login and
enabled-host admissions passed. All57 postflight entries (56bindings+manifest),
closed database bytes/logical state and all9562 live source leaves match the frozen
build. Owned Job cleanup reports active0/rootExit0; ordinary native engine-home
session writes remain possible. No judge was used.

The two raw synthetic agent entries lack admitted identity-word projections, so
null entries were required by policy. This cannot establish successful content
recall or the S1 seeded-entry acceptance journey, even apart from the literal
contract failure. Promotion is limited to source-reviewed/Cargo/direct-harness
qualified inactive writer code; native acceptance remains open.

## Separate runtime adapter â€” source approved, NOT promoted

`rebuild/project-runtime-transaction` at
`78989cce288a7b95fa56f137f0a04d8d72168012` adds the guard-consuming runtime
completion adapter in a separate499-line source slice, after the two-line
argument-comment repair `2c673ae7a907740aa433abfcab7548a3c8703b2f`.
It shares the unchanged legacy acceptance body and rejects foreign project-tagged
run/dependent rows. Knowledge, eligibility and source binding validation remain
caller responsibilities in the same transaction. Native callers remain legacy.

Evidence: sibling `runtime_20261016/source_review1.txt` APPROVE SOURCE,
`qualification_audit.json`; Windows/WSL runtime75 and project-store51 each pass,
zero skips/retries. Every recorded gate has identical9564-input before/after maps.
First runtime fixture correctly hit an existing public project-mismatch refusal;
final corrupt-child fixture uses explicit raw SQL. Equivalent final assertion and
argument-comment cleanup was typechecked by scoped fix then formatted, not retested.
Root Bazel lock refresh exited0 with no MODULE drift; the only Cargo edge is an
existing first-party package. This adapter awaits a separately frozen hands-on
Luna witness before promotion. Its clean local commits and reviews are preserved.

## Remaining authority gates

The owner's first company-brain milestone remains NOT FULFILLED. No native project
routing, populated shared-database migration, old-writer barrier, whole-store
completion certification, lock removal, compaction survival, authenticated edit or
Forget ingress, MCP, offline/HAL/customer acceptance is established. The retained
native Marathi405-to329-byte corruption and separate final nested double-escaping
failures remain OPEN. Raw synthetic agent rows without capture identity-word
projections cannot qualify content recall. Protected upstream B0 custody and
historical branches remain unchanged; no separately approved upstream batch exists.

Next: freeze a runtime-adapter harness on its approved source; independently check
success and late-failure whole-store state, lease consumption/restoration, source
bindings and owned cleanup, then one reviewed Luna journey. After that, native
completion must validate knowledge/eligibility/steering/source bindings under the
same guard before any fence replacement. Populated conversion must separately
prove quiescence of cached pools/background publishers/already-open old clients,
WAL-inclusive source custody, unrelated-project preservation and explicit authority
publication/recovery. Current transaction primitives do not establish those gates.
