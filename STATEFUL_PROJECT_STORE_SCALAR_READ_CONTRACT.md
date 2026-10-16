# S1 bounded exact-content read contract (design freeze)

This is a proposed next boundary, not implemented behavior. The effective-hit
extraction is separate. Native pools, writes, migration and root remain unchanged.
The first neutral operation returns attributed historical entry content, not an
effective BlackboardHit or a standing-rule/root projection. Full semantic reads
and all nested support remain later work. This limited operation must disclose
those omissions in its capability and each successful response.

## Ownership and snapshot

An additive intelligence facade consumes a ProjectStoreReader and calls its
into_domain_reader with the intelligence crate's exact compiled migration manifest.
This checked transition is required before constructing the facade. A mismatched
manifest refuses and awaits connection release. It has no public
unchecked connection/pool constructor and does not accept a project_id or reader
policy. Each call begins a validated snapshot, derives the canonical UUID string
as the fresh domain project_id, checks ownership, clocks and coverage, then reads.
It always finishes the snapshot before returning an ordinary result/refusal;
SQL/admission failures or cancellation may leave the owner close-only. Explicit
close awaits connection release. No model inference or filesystem scan is held
inside the transaction. No reconnect, home fallback, repair or native attachment.

Fresh-store binding is conservative: every non-null project_id in every known
intelligence table must be TEXT and equal the kernel UUID. Null project_id is
unsupported where the schema permits it. Fixed, reviewed table names are compiled
from the known schema inventory; no caller SQL/identifier interpolation. Unexpected
ownership refuses the whole operation even when the requested entry is absent.
Domain child dependencies must belong to their owning row, with foreign/orphan
links refused before delivering requested text. This must be frozen against the
actual schema, not inferred from absence of coverage. Populated legacy IDs need
S3 mapping; choosing a first row or rewriting IDs is forbidden.

Requests carry expected project/store UUIDs plus an entry ID (UTF-8 <=512 bytes).
Mismatch refuses before domain content. IDs must be canonical UUID strings; entry
IDs use the existing parser. A moved store retains IDs; independent writable
copies and duplicate-location policy are not solved by this operation.

Project revision comes from project_intelligence_revisions and eligibility
generation from capsule_eligibility_generations, sampled in that same snapshot.
Absent clock row means decimal "0" (existing query/migration fresh-store behavior).
Present project revision must have SQLite typeof=integer and be 0..i64::MAX;
present eligibility generation and entry revision must be integer 1..i64::MAX.
Returned timestamps must be integer 0..i64::MAX. Present NULL/TEXT/REAL/BLOB values
refuse with invalidClock, never share an absent-row default or undergo coercion.
All clocks/revisions/timestamps are decimal strings in JSON; units are named.

Coverage is checked using the existing coverage_available policy before treating
a result as a miss. Return coverageIncomplete distinctly, with no entry text.
Before policy eligibility, perform a bounded structural existence check: absent
entry means missing; malformed revision type/range refuses invalidClock; an
existing entry with no matching current revision row refuses invalidEntry;
foreign ownership or orphan child dependencies refuse invalidOwnership. An inner
join filtering away a corrupt current pointer is not that structural check.
Then use the same entry_source_eligible_on ModelTool predicate as native reads.
Policy ineligibility yields a uniform missing result; archival get_entry is never
the model read path. Every call takes a fresh snapshot, so committed Forget or
exclusion applies next call. A completed read describes its snapshot and cannot
retract bytes already delivered before a later retirement.

## Content projection and bounds

Select only the current entry/revision scalar fields, never SELECT * or the
full load_hit. Before Rust allocation, a SQL CASE/check must bound and validate
storage types for every returned TEXT field: entry/node IDs <=512 UTF-8 bytes,
content <=2048, kind/state/provenance enum <=32 and provenance source ID <=512.
Return an explicit unsupportedContent refusal for overlong/malformed scalars;
never truncate original bytes or fall back to a prior revision. Existing enum
parsers reject unknown values. Empty, missing or inconsistent revision rows refuse.
Do not allocate structured values, evidence links, premise trees, relations,
source bytes or context payloads to decide their omission. Those fields are
always omitted and named as omissions, whether present or absent.

Return exactly: projectUuid, storeUuid, snapshotRevision, eligibilityGeneration,
coverage=complete, projection=attributedContent, entry {id, nodeId, revision,
createdAtMs, updatedAtMs, kind, state, content, provenance {kind, sourceId}},
verification=notEvaluated, and omissions=[structuredValue, evidence, premises,
relations, context, applicability]. A missing result has entry=null and the same
snapshot metadata/projection/omissions. Do not expose stored/effective verification
or promote omitted support to Current/SourceVerified/UserConfirmed. Provenance
describes recorded origin, not authenticated ingress or current authority.
No rule-application, semantic completeness or exact-source claim follows from
this projection. The native full-hit behavior stays unchanged.

The CLI operation is `read --project-uuid <uuid> --store-uuid <uuid> --entry <id>`
under existing `stateful-memory --project <dir> --json`. It is auth-free/read-only.
The application assembles actual migration versions/SQL/flags for both domains;
never a second hand-maintained migration history. Bootstrap capabilities/root/init
retain their old meaning. Read capabilities identify this narrower projection.
Stream-preflight at most32 arguments before collecting/converting them for clap.
Charge each argument's UTF-8 bytes plus64 bytes fixed overhead against a16KiB
global cap; reject non-UTF8 arguments without lossy conversion. These bounds apply
before additional application allocation, not before OS/runtime argument storage.
Bound each identity/ID before allocating/parsing it; path bytes fit the global cap.
Bound the entire
escaped ASCII-safe JSON read envelope, newline included, to8192 bytes. Existing
bootstrap envelopes stay4096. If escaping/indentation exceeds the cap, emit one
bounded responseTooLarge error and no partial entry. No raw database error string
or path with unbounded bytes enters a foreign reply. Errors use fixed codes/messages.

## Required separate review units

1. Bounded private scalar loader/contract tests, preserving native behavior and
   checking ModelTool eligibility. No public CLI or domain dependency yet.
2. Schema-reviewed ownership/clock admission and facade with exact compiled
   manifest. Include all direct/reachable ownership cases and known schema inputs.
3. CLI plus lock/Bazel maintenance, subprocess fixtures, bounded envelope snapshots
   and frozen local helper/Luna journey. No promotion before fresh Droid approval.

Tests cover current exact content, missing/foreign entry, tombstone, superseded
user, applied/unapplied proposals, source exclusion, incomplete coverage,
oversized/malformed scalars/clock types, precision above2^53, escaping expansion,
cross-store identical entry IDs, mixed ownership, orphan/cross-project children,
WAL snapshot/Forget between calls and explicit close/move/reopen. Compare equivalent
content projection with native get_hit where eligible; compare native coverage
None separately. No result can improve verification by omitting support.
This freeze grants no effective root, source read, MCP/write, S2/S3/native cutover,
offline/HAL/customer or first-milestone acceptance.

## Exhaustive intelligence ownership matrix

Inventory is the final schema produced by the 25 checked-in migrations at baseline
98c3e63c4; no runtime/private database was read. The implementation must test that
its inventory still matches the compiled schema. All checks run in the admitted
read snapshot and return only bounded flags, not omitted row payloads. Every
declared FK must pass a snapshot-local foreign-key check (including projectless
rows); additional semantic edges below must be checked explicitly. Any invalid
required child edge yields invalidOwnership, with the current-revision exception
and deterministic precedence below. A literal project_id field is always TEXT,
non-null and equal to the kernel project UUID, including derived table rows.

| Table(s) | Ownership and required edge checks |
|---|---|
| hierarchy_nodes | Direct project_id; non-null parent_id resolves to same-project hierarchy node. Root parent may be null. Parent cycles refuse. |
| blackboard_entries | Direct project_id; node_id resolves same-project hierarchy; integer current revision resolves exactly one (entry_id,revision) row. |
| blackboard_entry_revisions | entry_id resolves an entry; non-null superseded_by resolves same-project entry. Historical revisions retained. agent_run_id is an opaque attribution string, not a local runtime FK. |
| blackboard_evidence_links | Owner (entry_id,revision) and context_map_entry_id must exist and derive the same project. No evidence content is returned. |
| blackboard_premise_links | Owner and premise (entry_id,revision) pairs must exist and derive same project. Check every stored edge rather than allocate recursive payloads. Cycles do not grant verification; this projection evaluates none. |
| blackboard_relations | Direct project_id and both entry endpoints resolve same project. agent_run_id remains opaque. |
| context_map_entries | Direct project_id; node_id resolves same-project hierarchy. |
| context_map_routing_terms | entry_id resolves context_map_entries. |
| capture_sources | Direct project_id. original_event_id, authoritative_thread_id and turn_id are recorded external identities; no engine-home lookup. |
| capture_source_chunks | source_id resolves capture_sources. Byte content is omitted and never allocated. |
| capture_source_terms | Direct project_id; (source_id,start_byte) resolves chunk and its source derives same project. |
| capture_entry_sources | entry_id and source_id both exist and derive same project; missing source cannot disappear from exclusion checking. |
| capture_source_exclusions | Direct project_id. entry_id and digest are durable retirement identities, intentionally allowed after corresponding entry/source removal; any extant entry or digest-matching source must derive same project. Never discard unknown exclusions. |
| capture_source_omissions | Direct project_id; original_event_id is opaque unavailable-source identity. |
| capture_identity_aliases | Direct project_id; entry_id exists and derives same project. scope_id may be empty or unknown; retain conservative existing alias behavior, never treat unknown scope as independent authority. |
| capture_current_words | entry_id exists; (entry_id,revision) resolves the entry's current revision. scope_id may be empty/unknown under existing conservative eligibility policy. |
| capture_identity_coverage, capture_projection_coverage | Direct project_id. after_rowid/after_source are progress cursors, not required live-row FKs. Presence of capture_identity_coverage means incomplete, preserving existing coverage policy. |
| knowledge_context | Direct project_id; owner (entry_id,revision) exists and derives same project. scope_id/group_id may legitimately refer to unknown recorded context; no invented FK or authority from payload references. All payload/applicability omitted. |
| knowledge_scopes | Direct project_id. opened_source/ended_source are opaque attribution locators. |
| knowledge_scope_bindings | Direct project_id; (project_id,scope_id) resolves knowledge_scopes. thread_id is external. |
| knowledge_source_sequences | Direct project_id; project clock, no external reference. |
| capture_groups | Direct project_id. thread_id/turn_id are external; no transcript-copy lookup. |
| capture_group_members | Direct project_id; group resolves same-project capture_groups. Non-null entry_id/revision occur as a pair and must resolve same-project revision. Both null allowed only for omitted/failed/not_restored outcomes; no phantom saved/already_present/pending entry. |
| capture_group_actions | Direct project_id; (project_id,group_id) resolves capture_groups. action_id/request_fingerprint are ingress identities. |
| capture_action_outcomes | Direct project_id; (entry_id,revision) exists and derives same project. |
| memory_changes | Direct project_id; non-null entry_id/revision must occur as a pair and resolve same-project revision. action_id/thread_id/turn_id/group_id are historical attribution, not mandatory local FK references. |
| repository_observations | Direct project_id. |
| repository_root_observations | observation_id resolves repository_observations; project_root/git_worktree_root are historical paths, never followed for this projection. |
| project_intelligence_revisions, capsule_eligibility_generations | Direct project_id plus exact clock rules above. |
| project_index_refresh_generation, project_index_refresh_status | Direct project_id; no hidden engine-home ownership. |
| working_set_projections, working_set_file_samples, working_set_publication_tickets, working_set_invalidations | Direct project_id even though disposable. Binding/thread/executor/window/run/version/revision/watermark fields are recorded external or derived identities; no runtime run required and no payload used. Tickets need not have a surviving projection. |
| blackboard_attribution_metadata | Global singleton feature metadata, no project meaning; never returned as entry authority. |
| blackboard_search | Derived FTS virtual table; not read by this exact scalar operation. entry_id must resolve an owned entry if inspected, but this projection never trusts FTS content. |
| context_map_search | Derived FTS virtual table; not read. Its logical project_id follows the direct rule; entry_id resolves owned context entry if inspected. No FTS content is trusted. |
| blackboard_search_config/content/data/docsize/idx, context_map_search_config/content/data/docsize/idx | SQLite-owned FTS shadows (suffixes name each table). Never read/interpret as authorities; no synthetic project_id ownership is assigned to encoded shadow blocks. |
| sqlite_sequence | SQLite-owned AUTOINCREMENT metadata, not project knowledge. |

Required semantic checks must not be weakened because the requested scalar omits
those child payloads: source exclusions/proposal eligibility depend on some of
these edges. This conservative whole-store admission may refuse damaged data
outside the requested entry; no repair or claim of populated legacy compatibility.
Read-only inspection cannot authenticate arbitrary same-user filesystem writes.

Refusal precedence is fixed: invalid request/expected identity, kernel admission,
then invalidClock for any sampled clocks/entry revision/timestamps, invalidEntry
for any existing entry missing its matching current revision row, invalidOwnership
for foreign project or orphan child edges, coverageIncomplete, then requested
entry absence/policy miss, then unsupportedContent for eligible scalar content.
Whole-store admission uses those same clock/current-pointer classifications; the
matrix's current-revision edge is invalidEntry, not invalidOwnership. Each phase
returns only bounded flags. Within one phase, codes do not depend on query order.
No later phase runs after an earlier refusal. Valid unrelated corrupt scalar
payloads are not loaded by this exact projection; structural checks remain global.
