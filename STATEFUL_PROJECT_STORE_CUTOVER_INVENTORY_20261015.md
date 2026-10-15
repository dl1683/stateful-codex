# Project-store cutover inventory - 2026-10-15

Source-only preparation; not importer/transaction/native-cutover design approval.
Both component inventories received fresh independent review with explicit limits.
Retained raw schema/hash receipts live at:
C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/
The revision4 migration review did not independently attest source-hash equality;
the earlier revision3 review checked all33 hashes and pinned migration trees.
The attachment revision2 review verified cited source at5d3 against the checkout.
No populated SQLx conversion or native activation was run for these inventories.
Recheck every source seam after upstream integration; historical pins are explicit.

# Legacy closure inventory and cutover questions - revision 4

Inventory at main00b62fe32; Python SQLite3.50.4 disposable schema only.
No actual-home content was opened or converted by this inventory. See
migration_inventory.json and migration_shapes.json for exact source hashes,
schema objects, columns, FKs and empty-schema integrity checks. The JSON
`creates` arrays are partial historical syntax matches: they omit virtual-table
creation and include temporary tables later renamed. They are not complete
inventories or final closure specifications; migration_shapes.json describes
final objects.

Counts include SQLite/FTS implementation tables: knowledge51, runtime15.
Only sqlite_sequence overlaps. This does NOT mean all rows can be copied by
project_id, or that independent `_sqlx_migrations` histories may be combined.

| Family | Selection / preservation requirement |
|---|---|
| hierarchy_nodes | Select project identity and complete parent closure; preserve node IDs, root/path identity, lifecycle and revisions |
| blackboard_entries / revisions | Preserve ALL versions, including superseded and retired history; revision ownership follows entry IDs |
| evidence / premise links | Preserve composite entry/revision references and source/context-map endpoints; never rewrite missing evidence as verified |
| relations | Normal insertion requires both endpoints to match the relation's project (migration0006). A mismatched source row blocks import; never expand the selected closure into another project to satisfy it. Deliberate transfer is separate |
| context_map_entries / routing terms | Select project then dependent entry IDs; retain fingerprints and freshness rather than silently revalidating |
| capture groups/actions/members/outcomes | Preserve group IDs, idempotent fingerprints, admitted member set and completed action receipts |
| capture identity aliases/current words | Preserve retired aliases and anti-resurrection identity, not only current entries |
| capture_identity_coverage | Preserve the fail-closed presence marker. after_rowid/watermark address hidden revision rowids, which logical IDs alone do not preserve. Current coverage_available returns false for ANY coverage row, even equal cursors; do not clear/certify it by replay |
| capture_projection_coverage | Separately account for after_source, after_byte, complete and generation; imported progress is not newly observed complete coverage |
| capture sources/chunks/terms/entry links | Preserve exact stored bytes, half-open byte ranges, source/part IDs, hashes, source revision and observed sequence |
| exclusions/omissions | Preserve exclusions even when their source bytes are unavailable; keep missing coverage explicit |
| knowledge_context/scopes/bindings/source_sequences | Preserve authority, category, applicability, binding and source clocks separately |
| memory_changes / project revisions | Preserve journal sequence values/gaps and events with no entry, including scope-ended/capture-incomplete. Project revision triggers have import side effects; counters and cursor eligibility need explicit disposition |
| project_index_refresh_status | Preserve/invalidate/re-establish completeness, truncation, counts and timing explicitly; imported status is not a fresh filesystem observation |
| project_index_refresh_generation | Keep distinct from project revision and capsule eligibility. Old background publishers must remain excluded; current require_current compares their claimed generation |
| repository observations / root observations | Select observations by project then observation_id; root observations lack project_id |
| attribution metadata | One global attribution-enabled timestamp has no project key; preserve its semantics explicitly in import manifest |
| capsule eligibility | Copy/advance monotonic eligibility consistently; import triggers can increment it during row insertion, so count preservation needs deliberate ordering |
| working-set projections/samples/tickets/invalidations | Migration0024 calls these derived/disposable, but retirement/freshness boundaries still govern reuse; do not import an installed ticket as fresh publication proof |
| FTS virtual/shadow tables | Not independent truth. Rebuild derived search from validated domain content and verify retirement/effective reads; do not transplant raw SQLite shadow pages |
| stateful_runs | Select project; preserve IDs, mode/status, strategy, result, budgets and revision |
| obligations / steering / measurements | Preserve project/run agreement and original sequence values/gaps, acknowledgement and provenance; a run_id FK does not prove the child's project_id agrees |
| run threads / continuations / leases | Dependency closure follows run_id. A migrated lease is not proof of a live owner; live-work recovery requires quiescence and a reviewed policy |
| acceptance ledger / criteria / evidence / pending / request parts / steering | Preserve complete composite keys, original check identity, failure/revision history and terminal accounting; never manufacture a current pass |
| host answers | Preserve answer record and exact basis bytes with the run. Raw SQL completed plus record presence reads as Answered; the Partial basis distinguishes Partial. Never normalize the basis or manufacture certified Completed. This is not a stream-publication archive |

## Replay constraints and logical-edge questions

This is not an ordinary final-schema row copy. Migration0004 permits inserting a
revision only when it equals the entry's CURRENT revision. Migration0002 requires
an inserted context-map source to be currently active and fingerprint-matching.
Historical evidence/premise inserts (0009/0011) require current valid sources or
specific predecessor/state conditions. Legitimate retained history can therefore
fail against the final retained source/entry state. A reviewed staged-load/replay
mechanism is unresolved. Do not alter historical verification, lifecycle or
source fingerprints to satisfy today's insertion triggers, or treat FK checks
and insertion order alone as a preservation proof.

Project revision triggers include 0005, relation effects in0006, and scope/binding
effects in0018. Capsule eligibility includes0024 plus source observation in0025.
Identify all their effects before restoring counters;
do not expose a half-imported generation or synthesize new saved/action events.
Data backfills also affect preservation: PI0014 generates an attribution-enabled
timestamp; runtime0011 creates missing nonterminal acceptance ledgers with
observation_version=0. Reapplying initialization around imported rows can change
these facts while satisfying FKs. Preserve original absence and legacy unknown
observation semantics rather than manufacturing fresh attribution or observations.

Declared FKs are only part of the closure. Explicitly verify these logical edges:

- Context scope/group IDs and member/outcome entry-revision references.
- blackboard_entry_revisions.superseded_by must refer to the same project; its
  FK proves target existence, not matching project ownership.
- Journal entry/group/source identities, including records without an entry.
- Acceptance steering/request-part identities, criterion dependencies/dismissal
  references and pending effects; retain original request-span meanings.
- Historical agent_run_id attribution and steering affected_obligation_ids_json;
  preserve meaning without importing another project or replaying an action.
- Source binding_generation versus authoritative thread-project admission:
  threads.project_binding_generation lives in engine state, and source linking
  checks it against the seal. Old attribution is not fresh admission. This is
  not permission to change upstream state migrations.
- Project containment for hierarchy/context/entry links and all project-tagged
  runtime children whose FK proves only run existence, not project agreement.

Block mismatches and dangling required references instead of importing another
project to repair them. Preserve original IDs and references; no renewed action
execution or fresh authority is awarded by import.

## Clocks, rowids and exact-byte questions

Inventory allocator high-water marks per table. sqlite_sequence can exceed the
retained MAX(sequence); its shared implementation-table name does not authorize
wholesale transplantation. Preserve obligation/evidence sequence VALUES, not just
relative order, and distinguish memory-journal sequences, source allocation/
observation sequences, project revisions, capsule eligibility, and acceptance
workspace_generation/verification_attempt. Specify continuation cursor namespace,
interrupted replay/idempotent resume, and next-write allocation tests.

Identity coverage's rowid cursors need a separately witnessed mapping/disposition.
Current identity.rs deliberately rejects historical alias replay: coverage-row
presence means unavailable, including previously certified upgraded stores.
Projection rebuild must not remove that restriction or declare identity coverage
complete. Refresh generation/status and source/project clocks are separate facts.

Source chunks deliberately overlap (4096-byte windows, approximately256-byte
overlap with UTF-8-adjusted boundaries in source_observation.rs). Concatenation
duplicates bytes. Verify offset-based overlap agreement, no gaps, UTF-8/CRLF byte
preservation, chunk hashes, whole-part digest/length and consistency between
columns and serialized SourceSeal (origin, original length/digest, first-observation
sequence, contract version, locator and observation fields). Missing/conflicting
bytes remain unavailable, never reconstructed from search_text, previews or
changed files. Current source retirement/coverage restrictions still govern every
effective read; copying archival bytes cannot make a retired source eligible.
In particular, source_retirement.rs rejects automatic ranges from parts longer
than4096 bytes when the project contains retired aliases. Reconstructing archival
bytes successfully must retain that expected automatic-read refusal.

## Transaction and authority gates

The current completion fence PREVENTS intervening knowledge writes while validation
and separate runtime persistence execute. Its limitations include database-wide
contention and lack of a single shared knowledge/runtime commit, not a demonstrated
unguarded knowledge-write gap. Merely pointing both handles at one file would
make the runtime's separate BEGIN IMMEDIATE contend with the held fence.
S2 must prepare candidates outside the transaction, then revalidate knowledge,
eligibility, run, obligations and steering before one short terminal commit.
Race Forget, steering, completion, competing writers, cancellation and uncertain
commit. Bootstrap tests establish none of those application-level invariants.
Quiescence/recovery must cover both autonomous run leases and acceptance
verification_owner/verification_lease_expires_at_ms, plus pending command effects.
Also inventory ThreadProjectAdmission: state/thread_project_admission.rs opens an
immediate state-DB transaction to prevent rebinding, while PI source linking
accepts that guard and opens its own writer transaction (blackboard_storage/source.rs).
Audit actual guard lifetimes/call sites and preserve rebind protection; replacing
CompletionFence alone must not be reported as eliminating every cross-DB writer
lock. How native binding admission participates in short project-owned writes
remains an S2 design/qualification question, not a proposed state-migration edit.

Before native attachment, qualify self-exclusion of .stateful, artifacts, backups
and automatic exports in Git AND non-Git roots. Current indexer/scan.rs72-85 uses
hidden(false) and explicitly filters only .git/node_modules/target directories.
The new ignore policy alone is not evidence that every native capture/read route
excludes its own private store. This is a preactivation test question, not a
reproduced disclosure or a change to the currently inert library.

Current legacy files contain multiple projects. A cutover of one project must not
silently drop unrelated rows or leave old clients writing the retired authority.
A new local file and a launcher pin are not themselves an old-writer barrier.
Already-open legacy connections also cannot be dismissed by a startup-only check.
Quiescence, consistent WAL-inclusive backups and per-project canaries remain gates.

Candidate to review in S3, NOT approved/implemented here: a new append-only PRIVATE
legacy-domain retirement migration could make older clients refuse unknown history,
without reusing/deleting migration numbers or rewriting checksums. This would be
a global barrier for those shared files, so every unmigrated project needs an
accounted path from the quarantined snapshot. It does not stop already-open old
clients; they must be quiesced before activation. No such migration was added.
Do not alter upstream state migrations or append guessed records to their ledger.

The verified importer must stage one selected dependency closure, compare canonical
row hashes/counts, IDs, retired behavior, effective reads and unrelated-project
canaries, then activate all native attachments under one barrier. Keep original
home files as inactive archives. Rollback after new writes needs accounted reverse
transfer preserving retirement, not restoration of an older truth snapshot.

UUID creation currently means a NEW project/store. Import must separately review
legacy project identity adoption/alias policy; do not rewrite original row IDs
merely to fit freshly generated metadata. Ambiguous legacy roots are explicit
input blockers, not permission to choose an unrelated ancestor or home fallback.


# Native attachment seams and preactivation questions - revision2

Source-only inventory at main5d3cfe4a19ad671111b758db8ca5397d22c24660.
No native routing change, actual-home inspection, runtime qualification, or S2/S3
approval. This supplements MIGRATION_CLOSURE.md; it is not a conversion design.
Paths below are relative to C:/Users/devan/sc_main. Recheck after M0/upstream merge.

| Observed seam | Source | What a later change must account for |
|---|---|---|
| Extension installation receives one optional SqliteConfig | codex-rs/app-server/src/extensions.rs:137; codex-rs/ext/stateful/src/lib.rs:669-693 | Project selection is per thread; one process-wide home config is not a project-store resolver |
| Native service cache | codex-rs/ext/stateful/src/services.rs:21-78,119-125,165-169 | Independently cached blackboard, context map, hierarchy, repository-observation and runtime handles. Replacing one open path does not replace the others; existing handles survive later configuration changes |
| App-server domain processors | codex-rs/app-server/src/message_processor.rs:532-545 | context-map, blackboard and project-intelligence processors receive cloned state DB SQLite configuration separately from the extension |
| Blackboard API cached stores | codex-rs/app-server/src/request_processors/blackboard.rs:405-435 | Read and mutation routes cache blackboard/context-map/hierarchy. Preserve project admission and error semantics rather than silently falling back to the old home |
| Context-map refresh | codex-rs/app-server/src/request_processors/context_map.rs:56-85,134-143 | Resolves project roots then uses cached hierarchy/context map and ProjectIndexer; self-exclusion and per-project isolation apply to refresh too |
| Project-intelligence status/evidence | codex-rs/app-server/src/request_processors/project_intelligence.rs:371-390 | Independently cached hierarchy/context map; effective reads must retain retirement/freshness restrictions |
| Runtime/measurement handle | codex-rs/app-server/src/message_processor.rs:336-337; codex-rs/app-server/src/stateful_store.rs:25-70,73-109,153-164 | One cached runtime pool and asynchronous measurement work; queue drain, project attribution, reopen and quiescence are distinct gates. Drain can time out and warn; return alone does not establish quiescence |
| Knowledge file constructors | codex-rs/project-intelligence/src/storage.rs:21-51; blackboard_storage.rs:89-103; context_map_storage.rs:63-76; repository_observation_storage.rs:58-71 in the same crate | Each opens project_intelligence_1.sqlite under SqliteConfig.home and runs the historical migrator; they do not consume the new bootstrap registry |
| Runtime file constructor | codex-rs/stateful-runtime/src/storage.rs:25-28,64-77 | Opens stateful_runtime_1.sqlite with its own migrator; merely changing the filename cannot combine its history or terminal transaction with knowledge |
| Shared engine configuration | codex-rs/state/src/sqlite.rs:35-43,145-150 | SqliteConfig also selects upstream engine state/log/goal/memory/queue/history files. Do not globally repoint it to move only the two private Stateful domains |
| Completion guard | codex-rs/project-intelligence/src/blackboard_storage/fence.rs; codex-rs/ext/stateful/src/tools/run.rs:106-118,329-457 | Knowledge writer exclusion spans separate runtime persistence. Unified writes need one short revalidated transaction, not nested independent BEGIN IMMEDIATE on one file |
| Thread-project admission guard | codex-rs/state/src/thread_project_admission.rs; codex-rs/project-intelligence/src/blackboard_storage/source.rs | State-DB writer guard prevents rebind while source link writes knowledge. Preserve admission protection and audit all guard lifetimes; completion-fence replacement alone removes neither this coupling nor every lock |
| Admission callers | codex-rs/ext/stateful/src/capture_sources.rs:47-50; codex-rs/ext/stateful/src/tools/source_proposals.rs:69-74; codex-rs/state/src/thread_project_admission.rs:36-44 | Both pass services.sqlite into admission, which separately opens state_db_path. Keep project-domain resolution separate from authoritative engine-metadata admission; do not move upstream state merely to move private domains |
| Non-native evaluation exporter | clients/stateful-codex/eval/export-project-state.mjs:1-9,19-26; eval/run-longitudinal-arm.mjs:197-202 in the same client | Reads both legacy files directly under sqliteHome. Decide whether this stays a frozen historical fixture or follows new authority. It explicitly omits current domain families and run-less Ask projects; it is not a migration backup |
| Non-native evaluation validity reader | clients/stateful-codex/eval/bixbench/artifact_validation.py:62-65,115-128,153-188 | Direct SQLite reads of both legacy files under state_dir feed operational validity. Native rerouting would not redirect this reader; retained files could become stale authority or missing files. This is a prospective dependency, not a demonstrated failure |
| Bootstrap boundary | codex-rs/project-store/src/lib.rs:1-2; opener.rs:25-36,114-155 in that crate | Inert initialization/inspection is separate from native attachment and domain semantics |
| Filesystem scanning | codex-rs/project-intelligence/src/indexer/scan.rs:72-85 | hidden(false), explicit .git/node_modules/target filtering. The new .stateful ignore file is not proof every scan/capture route excludes private DB/WAL/backups/exports |

This table records observed native Rust store-open sites and separately observed
non-native first-party evaluation readers, not an exhaustive cross-language audit
or every transitive tool/capture/read call or dynamic integration. A source search
found native direct opens concentrated in services.rs and the listed app-server
processors/handle. Public library users may open stores independently. Every
runtime fixture must log the concrete file and store/project identity actually
used; a configured path, main HEAD, or unchanged binary size is insufficient.

A later qualified activation must demonstrate, separately:

- Two projects and two independent engines read the same project-owned authority;
  each project's native, app-server, CLI and eventual MCP routes agree, with no
  inherited home fallback or cross-project cached handle reuse.
- Rebinding and multi-root/moved-root behavior use a reviewed identity policy.
  Existing native project IDs are not silently replaced by bootstrap UUIDs.
- Forget, edit, steering, answer/completion, measurements, leases, delayed
  publishers, pending commands and reopened sessions retain their semantics.
- Old cached pools, background tasks, other processes and already-open legacy
  clients are quiesced before authority changes. Startup refusal alone does not
  stop an admitted writer. Shared old files retain every unrelated project's data.
- Read-only operations do not bootstrap, mutate authority, or claim freshness;
  WAL-inclusive backup, uncertain commits, interrupted import and rollback after
  new writes receive explicit policies and disposable populated fixtures.
- Git and non-Git capture/index/read routes exclude .stateful operational files,
  snapshots, backups and automatic exports; crafted evidence cannot promote those
  private artifacts into project knowledge.
- Offline/customer/HAL, exact source bytes, image output and model support remain
  independent transport/customer gates, not properties established by this source
  map. Bootstrap JSON-envelope changes alone do not qualify native source-byte
  transport. Independent execution evidence is recorded separately in
  STATEFUL_HAL_PRESERVATION_BASELINE_20261015.md; this map adds no HAL runtime result.

No test was run for this map. The first milestone remains unfulfilled until the
strategy's M0 prerequisites and S2/S3 transaction, import, native attachment,
engine-neutral knowledge read, and legacy retirement gates actually pass.
