# Project-store rebuild baseline and custody: 2026-10-15

> **Archived historical record, not current operating instructions.**
> Use [current state](./STATEFUL_CODEX_CURRENT_STATE.md) for status and holds.
> Historical external evidence is not distributed in this repository.
> Paths and personal identifiers are redacted; original bytes and any
> historical digest claims refer to the version preserved in Git history.

This is S0 evidence and preparation for the owner-directed project-store rebuild.
It does not activate a project-local store or qualify migration, rollback, locking,
compaction survival, foreign-agent operations, or HAL delivery.
Authority is the converged `STATEFUL_REBUILD_STRATEGY_CURRENT.md` and
`STEWARD_STATEFUL_REBUILD.md` in `sc_dogfood/lsr_drivers`.

## Branch custody

Initial main: `00b62fe3208d529fa663b6673c5fe6b1fd12d7de`.
The sync executor owns `sc_wt_upstream`, `sync/upstream-20261015`, at
`04a51b086571b1a057b7c585843b14fa709671eb` when inspected. Its evaluation and
six accounting source paths remain dirty. Its B0 report is resource-held and
its production/recovery gates are unresolved. Component approval is not approval
to import or install the entire batch. No sync worktree mutation was performed.

Two independently launched rebuild executors initially selected `sc_wt_rebuild`.
Concurrent untracked crate files were detected; both versions were archived in
`reports/stateful_rebuild_concurrent_2129/`. Initial reciprocal notices disagreed:
the uppercase notice yielded the worktree to the original executor, while
`reports/stateful_rebuild_custody.md` yielded it to this location executor.
Fresh Droid review of the initial S0 document caught that unresolved handoff.
At21:36Z this executor explicitly accepted the later handoff in BOTH notices:
this executor owns `sc_wt_rebuild`; the other owns `sc_wt_rebuild_registry` and
migration-registry work. Its untouched root prototype is retained externally;
this resolver uses the location implementation. Gates from those different
implementations cannot be combined into one approval.

This evidence/identity work uses `sc_wt_rebuild_bootstrap` on
`rebuild/project-store-bootstrap-2129`, created from main. No resolver approval is
claimed here. The original executor archived its own root prototype; this session
has neither staged nor deleted that prototype. Resolver implementation and
migration registry receive separate exact-diff reviews before integration.

Both rebuild worktrees ran `git merge --ff-only main`: already up to date.
A history-preserving feature-to-main fast-forward was attempted from the historical
feature worktree. Git refused because an untracked `STATEFUL_CODEX_MEMORY_PLAN.md`
would be overwritten. Its object hash differs from main's tracked version; it was
left untouched. Historical maximum lag was 1134 commits at the initial snapshot.
The branch map and complete local pairwise counts are external receipts; no origin
operation, force push, branch deletion, or archived experiment import occurred.

## Storage and migration inventory

`reports/stateful_rebuild_s0_20261015/migration_inventory.json` contains exact SQL
SHA256 values and independently constructed in-memory schema objects, columns and
foreign-key relationships for both domain migration directories. This executed
SQLite schema construction, not Rust migration or a populated-project conversion.

Project intelligence has 25 SQL files and 51 resulting tables, including SQLite
FTS shadow tables and `sqlite_sequence`; semantic runtime has eight SQL files and
15 resulting tables. Their sole table-name overlap is SQLite's `sqlite_sequence`.
Both application openers nevertheless own separate SQLx migration ledgers, so
combining the ledgers or simply changing `SqliteConfig.home()` is unsafe.
Existing native routing remains home-scoped in intelligence/runtime storage,
extension `services.rs`, and app-server attachments. The knowledge completion
fence holds `BEGIN IMMEDIATE` across runtime persistence in another database.
Project placement alone cannot establish atomic completion or bounded contention.

The importer must preserve project dependency closure, revisions, sources,
retirement/exclusions, journals, receipts, runtime obligations and steering.
S2 must qualify one short atomic completion transaction; S3 must qualify
inspect/dry-run/copy/verify/activate/rollback and prevent legacy dual writes.
No native fallback or store cutover is authorized by this baseline receipt.

## Actual launcher and Luna observations

External evidence root: `reports/stateful_rebuild_s0_20261015/`.
The installed launcher targets `sc_main/codex-rs/target/debug/codex.exe`, version
0.159.2. Its executable SHA256 is
`5e476700701b28394d319863498a6e6789a2f64317d9756b69d12be8818043c7`.
The companion host and launcher hashes are in `baseline_manifest.json`.
These identify installed bytes; source matching has not been established.

The short `scodex exec ...` invocation failed argument parsing before inference.
With explicit `--stateful collaborative`, the same launcher ran Luna high effort
in a plain non-Git scratch folder. It answered 17 + 25 with exact `42`, exit0.
A separate session created and read back exactly `rebuild-baseline` plus LF in
`note.txt`, exit0. The edit log contains two tool-router errors (an invalid
acceptance-ledger addition and completion-only fields on a noncompleted update).
Successful file bytes do not make this an error-free semantic journey.
Independent file bytes and SHA256 are recorded in
`baseline_user_check.json`; this was exec, not TUI or candidate-binary testing.

After the ordinary turn, native memory add saved scratch canary RB2129; a fresh
CLI process listed it. A direct row query found its ID and project ID in the
home-scoped `project_intelligence_1.sqlite`. The scratch project had no
`.stateful/memory.sqlite`, including after save. Forget produced revision2 and
an independent CLI list returned `Nothing saved yet.` This witnesses active CLI
retirement only, not restart recall, digest exclusion or derivative deletion.
The scratch file and private retired history were retained.

Before these journeys, SQLite backup API copied state, intelligence, runtime and
thread-history databases, including committed WAL contents, to local inactive
archives. All four quick checks returned `ok`. Binary/host/launcher copies and
hashes are retained alongside them. They are per-file online snapshots, not a
quiescent cross-database barrier, a proved source-matched pair, or a rehearsed
rollback. Archives may contain private history: never stage or upload them.

## HAL preparation boundary

Preserve exec image input, goal diagnostics, Responses bridge routing,
Answered/Partial, A1/A2, non-Git identity, exact IndicTrans2 strings/provenance,
and offline dependencies through every storage/merge gate. H1 preparation starts
alongside S1; later implementation retains its roadmap dependency gates.

Inspected HAL receipts still describe CIDCO as a 54-page rendered draft with
315 review flags; the top-ten queue points to concrete source/page/block disputes.
Human adjudication and Codex delivery review remain outstanding. Five English to
Marathi packs are drafts. Seven original envelope rig causes and zero original
scored vertical-slice sessions are not HAL acceptance or capacity measurements;
later broker/observer receipts are separate narrow evidence. No cloud command,
new model judgment, customer source edit or offline inference ran in this slice.
Developer/publisher controls model eligibility; teacher lineage is informational.

Next source gate is a coherent, freshly reviewed S1a resolver, followed by S1b
bootstrap/identity and S1c bounded neutral reads. Migration, native cutover and
removal of the cross-database fence stay gated on S2/S3. This report's completion
is only an S0 preparation boundary; the owner milestone remains undelivered.
