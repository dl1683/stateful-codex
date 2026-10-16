# Next S1 boundary: domain registration and effective knowledge reads

## Superseding checkpoint: 2026-10-16

The source facts and sequence below describe the earlier bootstrap-only boundary.
PreparedDomains preparation/atomic installation and ProjectStoreReader are now
landed: format2 admits exact compiled domain histories/schema, installs both real
schemas in fresh scratch, and supplies one owned read-only connection after exact
domain matching. See DOMAIN_INSTALL, DOMAIN_WITNESS and REGISTERED_READER/READER_WITNESS
packets. The old bootstrap CLI still refuses application stores as designed; no
effective knowledge read command or native attachment has been activated.

Next implement step3's narrow application-domain facade, then step4's effective
CLI reads with existing eligibility/snapshot/retirement logic and escaped envelopes.
The kernel project UUID versus legacy intelligence project_id mapping remains an
explicit design decision; schema installation did not solve populated ownership.
Domain upgrades, M0/B0 cohort qualification, S2 and S3 remain separate gates. The
historical bootstrap observations below are retained rather than silently recast
as current limitations of the new API.

The staged [effective-read design](./STATEFUL_PROJECT_STORE_EFFECTIVE_READ_DESIGN_20261016.md)
selects an admitted snapshot guard as the next source unit, followed by a
behavior-preserving eligibility extraction and separately reviewed bounded
projection/CLI contracts. Design approval does not implement those stages.

This is an implementation handoff, not an implemented feature or permission to
activate native storage. The converged rebuild strategy remains the authority.
The current executable can initialize and inspect bootstrap metadata only.
S1 knowledge reads, M0–M3, S2 atomic completion and S3 conversion remain open.

## Source facts checked in this run

- `codex-rs/project-store/src/registry.rs` accepts exactly the two bootstrap tables,
  one project-store migration record and format/reader/writer version 1. Extra
  tables or domain records currently refuse. Adding an application table without
  a reviewed format transition would break inspection; do not weaken validation
  to accept arbitrary extra objects.
- `project-intelligence/src/storage.rs` and `blackboard_storage.rs` open
  `sqlite.home()/project_intelligence_1.sqlite` and run their SQLx migrator.
  `stateful-runtime/src/storage.rs` separately opens `stateful_runtime_1.sqlite`.
  Redirecting only the home directory would still create two databases, and
  pointing both unchanged SQLx migrators at one file would mix migration numbers.
- Schema-only Python execution of checked-in SQL produced 51 intelligence tables
  (including SQLite/FTS internals), 49 triggers, and 15 runtime tables. There are
  25 intelligence migrations and eight runtime migrations; runtime versions are
  1,2,3,4,10,11,12,13. Preserve their actual versions and bytes, not list positions.
  Only 30 intelligence tables and four runtime tables have a project_id column.
  These are source-schema observations, not populated conversion/Rust proof.
- Existing effective-read logic is substantial: `blackboard_storage/identity.rs`
  applies source exclusions, incomplete-coverage refusal, retirement aliases,
  proposal admission and user-statement rules. `query.rs::get_hit` uses a read
  transaction and ModelTool eligibility. `root_projection.rs` applies eligibility
  before limits. Raw `get_entry` is archival access and is not a neutral model-read
  substitute. `source.rs::read_source_page/read_source_range` recheck exclusions.
- `ext/stateful/src/services.rs` caches home-opened pools for all domain facades.
  App-server has separate caches/openers in request processors and stateful_store.
  `blackboard_storage/fence.rs` holds a database-wide BEGIN IMMEDIATE fence;
  `ext/stateful/src/tools/run.rs` commits runtime state while that fence remains
  held, then releases it. Bootstrap publication does not change this native path.

External reproducible inventory: `sc_dogfood/lsr_drivers/reports/`
`stateful_rebuild_domain_inventory.py` and its JSON output. It opens only fresh
in-memory Python databases and source SQL; it does not inspect home rows.

## Selected implementation order

1. Extend the project-store format through an explicit, separately reviewed known
   transition. Keep bootstrap migration 1 immutable and preserve both UUIDs.
   Pin supported application-domain manifests by actual migration version and
   digest. Unknown, partial, dirty or changed history must refuse atomically.
   Old reader/writer compatibility must be negotiated explicitly; existing init
   must not silently adopt or upgrade an application store. Determine the exact
   transition API and version bump in its code brief before editing the registry.
2. Install intelligence schema into a fresh qualified project store under that
   registry, independent of native attachment. Reuse the checked-in migration SQL
   and register its build-time inputs in Bazel. Do not run the two legacy SQLx
   migrators against a shared _sqlx_migrations table. A fresh-domain fixture is
   not a converted legacy project. Runtime installation follows the same domain
   ownership contract; it must precede any native claim of a single database.
3. Add a narrow injected, validated project-store handle to intelligence facades,
   retaining existing home constructors for the still-unmigrated native path.
   Do not expose a public unchecked from_pool constructor. Keep engine/home/auth
   out of the neutral open path and keep codex-core out of this new ownership.
   Use the same domain eligibility implementation for neutral reads; do not copy
   its SQL into the CLI or create a second permissive knowledge representation.
4. Add seeded effective `read`/root operations through that facade. A separate CLI
   must read a real entry without Codex authentication. Bind every operation and
   continuation to project/store identity, snapshot and eligibility generation;
   return explicit coverage/omissions/freshness. Serialize revision integers
   losslessly and enforce 4 KiB root/8 KiB page envelopes after escaping. Exact
   source bytes must be distinguished from previews and current filesystem bytes.
   Stdio MCP and foreign writes remain S6; no actor string grants human authority.
5. Only after the relevant S1 and M0–M3 gates, implement S2 shared transaction
   participation and S3 inspect/dry-run/copy/verify/activate/rollback. Native cache
   keys must include project/store identity, not just one engine home. All native
   attachment sites need the same cutover barrier, with no home fallback/dual write.

## Required fixtures before expanding claims

Registry transitions need independent-process contention, bounded busy refusal,
known/changed/partial/unknown manifests, rollback after injected failure, downgrade
refusal and preserved IDs. Each committed domain migration must have exact digest
and schema checks; do not infer a successful conversion from a table count.

Effective reads need live, retired, superseded user, unapplied/applied proposal,
source-excluded, incomplete-coverage and oversized entries. Check filtering before
page limits, cross-project IDs/cursors, stale continuation after Forget, source
range hashes/offsets and Unicode expansion. Compare whole responses and snapshot
visible output. Native caches and legacy home bytes must stay untouched in S1.

For later conversion, start with direct project-owned rows and an explicitly
reviewed dependency closure. Revisions/evidence/premises/source chunks and runtime
acceptance children lack direct project_id. Detect cross-project or orphan links
instead of chasing them into unrelated projects. Rebuild FTS/derived indexes;
do not copy shadow tables or sqlite_sequence as project-owned knowledge. Verify
canonical row hashes, foreign keys, integrity, effective reads, retirement and an
unrelated-project canary. Thread/turn references into transport/runtime homes need
an explicit retention or unavailable-source policy, not silent dangling success.

Legacy backups in this run were per-file online consistent copies, not a quiescent
cross-database checkpoint. No old-client write barrier, populated conversion,
recovery rehearsal or production migration-59 cohort qualification was established.
Preserve the sync executor's ownership and unresolved B0 gate. Actual killed-writer,
power-loss, sustained indexing and completion/Forget races remain separate tests.
