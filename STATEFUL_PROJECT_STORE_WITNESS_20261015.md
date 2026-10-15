# Independent project-store reader witness - 2026-10-15

This is a bootstrap-metadata witness, not delivery of the owner first milestone.
No native attachment, project knowledge import/read, application lock removal,
atomic completion, MCP, offline, macOS or HAL acceptance is established here.

The reviewed reader is781c8e1e4fd7245e5b1cc0637d5db6d5a1efa504, based on the
approved concrete-path opener5d3cfe4a19ad671111b758db8ca5397d22c24660.
Fresh source review approved the reader including its Bazel target-inference and
feature-dependent snapshot-order repairs. The later initializer is separate.

The independent executor fast-forwarded only its own registry worktree to781,
made no source edits, and rebuilt a leaf under the shared Local\StatefulB0LocalBuild
mutex, host available-memory floor23,221,225,472 bytes and CARGO_BUILD_JOBS=2.
Build exited0. Tracked project-store/reader/workspace-input hashes and HEAD were
unchanged across the build. The executable was copied while holding the mutex,
marked read-only, and hashed; this is not cryptographic source-to-binary attestation.
No candidate was installed into the native launcher. No tests were repeated after
source cleanup; earlier component gates remain separately recorded.

Frozen binary SHA256:
70b782778f89a71dc4c7879aefc8b7b85720235fba430cbf370bd8d85acdfa79

Evidence directory (external, never stage binaries/private session archives):
C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/

| Receipt below that directory | Result / limits |
|---|---|
| reader_snapshot_781c8e1e4/manifest.json, source_before.json, source_after.json, build.log | Frozen executable and local build provenance; build0 |
| neutral_cli_harness_review8.txt | Fresh APPROVE HARNESS WITH LIMITS; exact read-only contract; no empirical claim from review itself |
| neutral_cli_20261015T233534023950Z/receipt.json | One candidate run PASS, every registered child collected, no final fixture errors |
| luna_reader_781c8e1e4/manifest.json, stdout.jsonl, answer.txt, score.json | Actual Luna launcher check PASS with two completed successful tool reads and exact deterministic equality |
| migration_inventory_review4.txt, MIGRATION_CLOSURE.md | Source-only migration inventory approved with limits; empty-schema Python checks are not populated SQLx conversion |

The independent harness uses Python SQLite3.50.4 to seed two disposable metadata
stores from the pinned committed bootstrap SQL. This does not exercise the Rust
initializer. It checks distinct project/store UUIDs, Unicode/non-Git roots,
identity preservation across a project move, physical file identity, exact schema
digest and SQLite integrity, plus missing/foreign/empty/future-format refusals.
All surviving project/profile/temp snapshots and expected-absent paths are checked
after child collection. Canonical bytes and persisted files/sidecars remain
unchanged at the observed snapshots. This is not continuous filesystem auditing
or OS-wide isolation. Child lifetime overlap was observed; it is not a forced
SQLite critical-section race or crash qualification. Init was NOT RUN.

Luna session01a141ec-3118-7090-ad15-3f43ad38efba used the existing installed
scodex.cmd launcher with the separate frozen reader. Both root commands completed
with exit0. The final UTF-8 JSON exactly equals the independently seeded UUIDs and
the direct/tool reader paths, including the Marathi directory. The deterministic
scorer uses explicit UTF-8 decoding and compares complete objects; no model judge.
Fixture files and the frozen reader hash stayed unchanged. The installed native
baseline was not rebuilt, and ordinary session writes to its engine home were
expected; no claim of zero engine-home writes or home conversion is made.

Preserve the earlier failed native file-edit trial in S0: its model finalized
while tool work remained running and the requested file was absent. This later
read-only pass does not repair that result. HAL H1 read-only CLM Marathi transport
remains FAILED. ASCII escaping in the new CLI is not a fix for native source-byte
transport. Knowledge/native/legacy/transaction/customer gates remain open.

The next implementation sequence is still M0 prerequisites, then S2 unified
knowledge/runtime transactions and S3 verified import/native cutover. A source-only
attachment inventory is being reviewed separately; it is not activation approval.
