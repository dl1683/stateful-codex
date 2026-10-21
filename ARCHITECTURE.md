# Architecture

Source map at `997007527c5bb1d03c4d6b67cd5f8786b37b3e17`, 2026-10-20.
**Landed** means implemented, **inactive** means present without native
activation, and **held** means unqualified/not authorized. Read
[current state](./STATEFUL_CODEX_CURRENT_STATE.md) for verification scope.

## Request to durable knowledge

```text
CLI/TUI or loopback web client
  -> native CLI / app-server, selected home and project
  -> thread + extension hooks
  -> project-intelligence: hierarchy, routes, blackboards, eligibility
  -> stateful-runtime: runs, obligations, acceptance, steering, measurements
  -> bounded World State / explicit evidence reads
  -> inference request
```

The web client talks to the Node gateway, which launches `codex app-server`
over stdio. It does not read/write SQLite directly. The CLI and code-mode host
are a matched package. Source:
[gateway](./clients/stateful-codex/server.mjs),
[extension registration](./codex-rs/ext/stateful/src/lib.rs),
[lazy services](./codex-rs/ext/stateful/src/services.rs),
[app-server processors](./codex-rs/app-server/src/request_processors).
The browser's allowed RPC surface is narrower than the app-server API.

## Hierarchy: project → directory → file → anchored region

[Hierarchy types](./codex-rs/project-intelligence/src/hierarchy.rs) distinguish
Project, Directory, File and Region. Each belongs to a project, with revision
and lifecycle (`active`, `missing`, `replaced`). Child nodes carry a project-root
identity and normalized relative path. A region adds a bounded domain-neutral
anchor `(scheme, locator)`, not an assumption that all sources are code lines.
Fingerprints identify source bytes; an anchor alone does not prove freshness.

[Indexer](./codex-rs/project-intelligence/src/indexer.rs) and
[checkout reconciliation](./codex-rs/ext/stateful/src/checkout.rs) publish
hierarchy/routes and account for changes. Partial scans must not treat unseen
files as deleted. Source-aware reads and explicit project selection remain
necessary after moves, worktrees and external edits.

## Blackboard versus context map

| Blackboard | Context map |
| --- | --- |
| What the project has retained: instructions, facts, claims, decisions, questions, contradictions, failures and rejected approaches | Where relevant evidence may be found: bounded descriptions, routing terms, source fingerprints and coverage |
| Revisioned entries, evidence/premise links, provenance, verification, importance, root promotion and retirement | File/region routes with complete/partial coverage and current/stale/unavailable source state |
| A stored claim is not necessarily true or applicable | A matching route is not proof of a claim or completeness |

See [blackboard types](./codex-rs/project-intelligence/src/blackboard.rs),
[context-map types](./codex-rs/project-intelligence/src/context_map.rs) and
[evidence reader](./codex-rs/project-intelligence/src/evidence.rs).
Route search results lead to bounded evidence reads. Verification concerns the
actual source/range and conclusion; fresh bytes alone cannot prove a conclusion
still controls the answer.

## Provenance, authority and lifecycle

[Knowledge context](./codex-rs/project-intelligence/src/knowledge.rs) records
category, authority, scope, source ordering and validity for an entry revision.
Provenance (`user`, `agent`, `maintenance`, `import`) is distinct from authority
(`HumanDirect`, `AssistantReported`, `ReportedThirdParty`, `HostObserved`,
`LegacyUnknown`) and verification. An agent label or confidence score is not
authentication or permission to promote a rule.

Direct [add](./codex-rs/ext/stateful/src/memory_add.rs),
[correct/Forget](./codex-rs/ext/stateful/src/memory_controls.rs), and
[proposal/Undo controls](./codex-rs/ext/stateful/src/memory_receipts.rs)
do not require a model turn. API operations bind the client's project;
correction/Forget also use the observed revision, while additions use a client
action identity. See the [memory protocol](./codex-rs/app-server-protocol/src/protocol/v2/stateful_memory.rs)
and [memory processors](./codex-rs/app-server/src/request_processors/stateful_memory.rs).

Entries move from active to superseded or tombstoned. Corrections create
successors while preserving history. Forget is **effective retirement**, not
erasure of transcripts, historical revisions or already-sent requests. Eligibility
must be enforced at read time, including
[native effective reads](./codex-rs/project-intelligence/src/blackboard_storage/native_read.rs).
Historical scopes are held unsupported; merely listing retained rules does not
assert application in the current thread.

Current correction code trims input. The archived original-input/exact-byte
repair is **not landed**; universal exact correction/replay claims are held.
Generic public blackboard APIs exist, but foreign-agent write activation remains
held. A neutral read projection is not a foreign write contract or an access
control boundary.

## Storage: landed native placement, inactive canonical conversion

[scodex.py](./scripts/scodex.py) resolves the project and places an unchanged
native Codex home in `<project>/.stateful/` or `STATEFUL_STORE_ROOT`. It sets
`CODEX_HOME`, `CODEX_SQLITE_HOME` and explicit `sqlite_home`; ignores precede
optional template config/login seeding. Native databases remain separate:
project intelligence, runtime, state, thread history and other native files.
Direct binary/gateway startup must select its own home explicitly.

This is **landed routing**, not a one-file rebuild/import. Legacy shared homes
are historical, not automatic import sources. Native locks/migrations remain.
Per-project placement does not solve same-project writer contention.

[Project-store](./codex-rs/project-store/src/lib.rs) separately implements
`.stateful/memory.sqlite` resolution, identities/registry, filesystem admission,
domain manifests/upgrades, owned snapshots and guarded transactions. Native
knowledge/runtime currently use their ordinary stores through lazy services,
not an automatic conversion into that canonical store.
These canonical components are **inactive for native cutover**.
Do not confuse their component tests or canonical historical reads with native
whole-store qualification. Populated migration 59/60 compatibility is **held**.

## Runs and terminal truth

Ask has no run. Work modes are autonomous, collaborative and socratic.
[Runtime types](./codex-rs/stateful-runtime/src/run.rs) model budget, revisions,
status and obligation packets; extension
[run context](./codex-rs/ext/stateful/src/run_world_state.rs) and
[completion](./codex-rs/ext/stateful/src/completion.rs) integrate them.
`Answered` means an unverified answer, not acceptance-certified `Completed`.
Paused/blocked/recovery and typed partial outcomes need their own accounting;
process exit 0 alone does not establish a correct deliverable.

The [tool-call checkpoint nudge](./codex-rs/ext/stateful/src/checkpoint.rs) is a
volatile pacing mechanism, not a durable compaction checkpoint. Further ordinary
policy/installation/Work-v3/PartB activation remains held.

## Bounded context, compaction and checkpoints

The [extension](./codex-rs/ext/stateful/src/lib.rs) budgets aggregate fresh-window
Stateful developer content at **12 KiB**. Its
[model-item cap](./codex-rs/ext/stateful/src/limits.rs) is **9,000 bytes**;
[project World State](./codex-rs/ext/stateful/src/world_state.rs) subtracts
markers, and deltas are capped at **8 KiB**. These are separate byte/token
guards, not a promise that every retained entry fits. Omission is explicit.
Root rules, run context and continuity compete within the aggregate bound.

The [core World State framework](./codex-rs/core/src/context/world_state/mod.rs)
tracks snapshots and emits changes without repeatedly replacing history.
Context fragment types live in
[core/context](./codex-rs/core/src/context/mod.rs) and use
`ContextualUserFragment`. Preserve incremental history and stable cache prefixes;
new injected content needs a hard bound, and potentially >1k-token individual
items require the extra manual review specified in AGENTS.

[Conversation packets](./codex-rs/history/src/conversation_packet.rs) retain
selected whole original messages with host-owned position/role and coverage.
Candidate admission is capped at 64 records / 512 KiB before budgeted packing.
[Source assembly](./codex-rs/history/src/conversation_packet_source.rs) and
[live checkpoint handling](./codex-rs/core/src/context_manager/history_conversation_packet.rs)
are host continuity machinery. Persistence is not proof of selected-checkpoint
restoration, exact historical recall or model-visible survival.

The held archived stack addressed capture/provenance/restoration and an
installed-native compaction journey. It is **not merged**. Its failing suites
remain recorded in [decisions](./STATEFUL_CODEX_DECISIONS.md).
Do not infer an exact history/E2 lookup route, automatic quotation or reliable
post-compaction recall from host packet structs. Offline capsules are likewise
[inactive experimental components](./codex-rs/ext/stateful/src/working_set/mod.rs),
not current model-context delivery.
