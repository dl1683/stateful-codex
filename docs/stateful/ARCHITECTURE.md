# Stateful Codex architecture

Stateful baseline
`43a2c15951b1fd93d4c9f6be2545712634bdaecf`, integrated with pinned upstream
`a3d93e1e69`. Documentation consolidated 2026-10-22; integration checks do not
establish product acceptance. Start with the
[README](../../README.md). Measurements belong only in [RESULTS](RESULTS.md);
rejected designs and recovery belong in [FAILURES](FAILURES.md).

## Product goal

Let a person select a project, choose a thread and mode, and delegate substantial
work without rebuilding context every session. Retain decisions with reasons,
rules, rejected approaches, uncertainty and decisive cross-source connections.
The intended progression is blackboard-first work with selective source checks,
not chat with an ever-growing transcript. Source files remain ground truth.

The user selects the project and whether to create, continue or fork a thread.
**Ask** uses project context without a run or acceptance obligations.
**Work** explicitly starts an autonomous, collaborative or socratic run.
Autonomous work should proceed without unnecessary checkpoints while remaining
steerable. These are product requirements, not universal reliability claims.

The obligation packet should explain what was learned, why it matters, changed
reasoning, remaining obligations and uncertainty, rather than narrating tool calls.
System-derived hypotheses may evolve; user goals and binding constraints may not
be silently rewritten. Roughly 500 useful words was a design guide, not a measured
optimum or an exemption from hard context bounds.

## Source map and data flow

```text
CLI/TUI or loopback browser
  -> native CLI/app-server, selected home/project/thread
  -> Stateful extension hooks
  -> project-intelligence: hierarchy, context maps, blackboards, eligibility
  -> stateful-runtime: runs, obligations, steering, acceptance, measurements
  -> bounded World State and explicit evidence reads
  -> inference request
```

| Responsibility | Entry point |
| --- | --- |
| Agent integration / lazy services | [extension](../../codex-rs/ext/stateful/src/lib.rs), [services](../../codex-rs/ext/stateful/src/services.rs) |
| Hierarchy and indexing | [hierarchy](../../codex-rs/project-intelligence/src/hierarchy.rs), [indexer](../../codex-rs/project-intelligence/src/indexer.rs), [checkout](../../codex-rs/ext/stateful/src/checkout.rs) |
| Knowledge and source navigation | [blackboard](../../codex-rs/project-intelligence/src/blackboard.rs), [context map](../../codex-rs/project-intelligence/src/context_map.rs), [evidence](../../codex-rs/project-intelligence/src/evidence.rs) |
| Authority and freshness | [knowledge](../../codex-rs/project-intelligence/src/knowledge.rs), [capture admission](../../codex-rs/ext/stateful/src/capture_admission.rs), [freshness](../../codex-rs/ext/stateful/src/source_freshness.rs) |
| Direct memory controls | [add](../../codex-rs/ext/stateful/src/memory_add.rs), [correct/Forget](../../codex-rs/ext/stateful/src/memory_controls.rs), [receipts](../../codex-rs/ext/stateful/src/memory_receipts.rs) |
| Public memory boundary | [protocol](../../codex-rs/app-server-protocol/src/protocol/v2/stateful_memory.rs), [processor](../../codex-rs/app-server/src/request_processors/stateful_memory.rs) |
| Execution and completion | [run](../../codex-rs/stateful-runtime/src/run.rs), [run context](../../codex-rs/ext/stateful/src/run_world_state.rs), [completion](../../codex-rs/ext/stateful/src/completion.rs) |
| Browser and gateway | [server](../../clients/stateful-codex/server.mjs), [admission](../../clients/stateful-codex/gateway-policy.mjs), [event router](../../clients/stateful-codex/event-router.mjs) |
| Native placement / neutral reads | [launcher](../../scripts/scodex.py), [reader CLI](../../codex-rs/stateful-memory/src/main.rs), [native reader](../../codex-rs/project-intelligence/src/blackboard_storage/native_read.rs) |
| Inactive canonical storage | [project-store](../../codex-rs/project-store/src/lib.rs) |
| Inactive console transport | [windows-console-protocol](../../codex-rs/windows-console-protocol) |

The gateway launches `codex app-server` over stdio; it does not access SQLite
directly. Its exposed RPC surface is narrower than the full app-server API.
The CLI and code-mode host must come from the same build.

## Hierarchy, blackboard and context map

Hierarchy is project → directory → file → optional anchored region. Regions use
a domain-neutral `(scheme, locator)`, so functions, clauses, tables and passages
share one abstraction. Nodes have project identity, normalized relative paths,
revisions and active/missing/replaced lifecycle. Partial scans must not mark
unseen files deleted. Moves, worktrees and external edits need reconciliation.

The **blackboard** stores revisioned facts, claims, instructions, decisions,
questions, contradictions, failures, rejected approaches and relationships.
The **context map** routes to evidence using descriptions, terms, fingerprints
and complete/partial coverage. A route match is neither evidence of truth nor
proof that every controlling source was covered.

The intended access pattern is bounded project root → deeper blackboard →
context-map route → exact source when needed. The root should support unprompted
cross-file connections; automatic salience trimming is not an accepted substitute.
Fresh source bytes do not prove that a conclusion is still controlling. Exact
style/layout work may require original material directly, not a semantic summary.

## Authority and lifecycle

Provenance (`user`, `agent`, `maintenance`, `import`), authority
(`HumanDirect`, `AssistantReported`, `ReportedThirdParty`, `HostObserved`,
`LegacyUnknown`) and verification are separate. Confidence, an agent label,
quoted text or a parser cue cannot confer human authority. Preserve exact scope,
negation, attribution and source order; ambiguous text remains attributed/pending.

Direct add/correct/Forget requires no model turn. APIs bind the expected project;
correction/Forget also bind the observed revision, while Add binds a client
action identity. Corrections create successor revisions. **Forget retires
effective memory, not transcripts, historical revisions or already-sent context.**
Eligibility must apply at read time. Historical rule scopes remain unsupported.
Current correction trims input; the exact-input archive is not landed.

A receipt must distinguish retained, pending, omitted and rejected words.
Lost-response Add retries after Forget/correction must not resurrect the entry.
Read-only neutral projection is not foreign-write authorization or multi-user
access control. Global/personal memory and project knowledge are distinct
pipelines; readable paths do not authorize cross-project export.

## Context and continuity bounds

| Mechanism | Bound and meaning |
| --- | --- |
| Aggregate fresh-window Stateful developer content | 12 KiB, shared by root/run/continuity |
| Model-facing item | 9,000 bytes, with marker allowance subtracted |
| World State delta | 8 KiB |
| Conversation packet candidates | 64 whole-message records / 512 KiB before budgeted packing |

See [limits](../../codex-rs/ext/stateful/src/limits.rs),
[project World State](../../codex-rs/ext/stateful/src/world_state.rs),
[core World State](../../codex-rs/core/src/context/world_state/mod.rs),
[packets](../../codex-rs/history/src/conversation_packet.rs) and
[checkpoint handling](../../codex-rs/core/src/context_manager/history_conversation_packet.rs).
Byte limits and token limits are different. Omission must be disclosed.
Preserve incremental history and stable cache prefixes. New fragments use typed
`ContextualUserFragment` contracts; [AGENTS](../../AGENTS.md) sets item review limits.

Packet persistence does not prove selected-checkpoint restoration, exact historical
recall, first-request root visibility or post-Forget replay safety. The Work
[checkpoint nudge](../../codex-rs/ext/stateful/src/checkpoint.rs) is volatile pacing,
not a durable compaction checkpoint. Capsules remain inactive experiments.

## Native placement versus canonical project store

The landed launcher selects the Git worktree root or explicit plain directory and
routes an unchanged native home to `<project>/.stateful/`, or `STATEFUL_STORE_ROOT`.
It supplies both home variables plus `sqlite_home`, preserves native locks/schema,
publishes private ignore policies before optional missing config/login seeding,
and imports no knowledge. Direct native/gateway startup requires explicit routing.
Several native SQLite databases and sessions remain; this is not one physical DB.

The separate canonical `.stateful/memory.sqlite` kernel owns project/store UUIDs,
filesystem admission, manifests, snapshots and guarded transactions. It is
**inactive for native conversion**. Placement validation alone cannot establish
absence of sync agents or same-user namespace replacement.

- Initialization publishes a closed staged database without replacing an existing
  file. Unknown/empty preexisting canonical files are refused, not adopted.
  Abandoned stages never become authority. Process-crash and power-loss recovery
  are separate qualification gates.
- Compiled domain manifests retain actual migration versions/gaps and SQL hashes.
  Exact schema/history and both UUIDs are checked in the same admitted snapshot.
  Bootstrap history stays immutable; domain upgrade is explicit, not open-time repair.
- A reader owns one read-only connection without reconnect/home fallback.
  Each owned snapshot repeats admission; unfinished/cancelled owners become
  close-only. Finish and await close before reuse/move/reselection.
- A writer admits read-only before WAL, uses bounded two-second lock waits and
  `BEGIN IMMEDIATE`, and rechecks schema/identity at commit. No guard spans
  inference, commands, scans or another database. Uncertain COMMIT requires
  close, fresh durable reconciliation and operation identity, not blind retry.

### Bounded neutral content contract

`stateful-memory read` returns **attributed historical content**, not a full
effective blackboard or standing-rule projection. It requires expected canonical
project/store UUIDs; the same snapshot validates clocks, current pointers, whole
known ownership/required edges, coverage and native ModelTool eligibility.
Foreign/orphan structure refuses globally even if the requested entry is absent.
Missing/ineligible entries share a uniform miss; incomplete coverage is distinct.

Before allocation, scalar IDs fit 512 UTF-8 bytes, content 2,048, enum text 32
and provenance source ID 512. Malformed/oversize values refuse whole.
Clocks/revisions/timestamps are lossless decimal strings, not JavaScript numbers.
Successful projection says `verification=notEvaluated` and omits structured
values, evidence, premises, relations, context and applicability. Omitting support
cannot improve verification. No raw SQL or arbitrary foreign writer is exposed.
Refusal order is request/identity → kernel → clocks → current pointer → ownership →
coverage → miss/policy → supported content.

Bootstrap replies fit 4,096 bytes; read replies fit 8,192, including escaped JSON
and LF. CLI preflight allows 32 OS arguments including program name; UTF-8 bytes
plus 64 per argument fit 16 KiB. Invalid Unicode refuses before clap conversion.
These do not bound OS argument storage or guarantee native Marathi transport.

### Future acceptance parity, not activation

Canonical completion must preserve the existing durable acceptance gate, not a
caller Boolean: ledger/workspace revision, pending commands, lease owner/attempt,
latest obligation/blockers, steering, criterion dependencies and request coverage.
Prepare/hash/observe outside the writer; revalidate run/start/project/store,
knowledge/eligibility and bounded source findings under the same final transaction.
Terminal result and final obligation commit atomically. Positive learning and
negative-learning coverage are separate obligations. Synthetic receipts do not
prove real checker execution or freeze external files through COMMIT.
Keep remote-executor observations distinct from host-shadow paths.

Populated import, identity preservation, native routing, old-writer exclusion,
uncertain acknowledgement recovery and both learning dispositions remain required
before removing legacy fences. `Answered` is unverified; `Completed` is
evidence-bound. Exit zero alone is not delivery correctness.

## Inactive console protocol

The version1 closed request codec binds an explicit nonce, receiving worker
width and bootstrap-control references; these are non-owning handle assertions,
not authentication. Lone UTF-16 surrogates and environments above 32,767 units
round-trip within the 65,536-unit budget. NUL paths, wrong roles/nonce/width,
duplicate/unknown/malformed JSON and incomplete/oversize frames refuse.
Closed stream, response-frame and Ready→Spawned→RootExited ordering components
do not supply a native launcher, trusted worker, deadline, descendant drain or
guardian. Source/codec passes never repair the separate native byte-loss failure.

## Capability inventory

The contracts above describe implemented slices and their limits. They are not
an execution plan or a statement that the complete product is accepted.

| Source capability | Acceptance boundary |
| --- | --- |
| Project-local home selection | Implemented selection; populated conversion and broad portability remain unqualified |
| Bounded neutral reads | Partial read-only projection; no complete authenticated knowledge/history graph |
| Explicit project memory controls | Source controls and fixture coverage; native cold recall and replay-wide correction/Forget require separate evidence |
| Delivery provenance and conversation packets | Host-side records; repeated-compaction exact-answer survival and restoration remain unqualified |
| Work completion | Evidence-bound contract; historical failed journeys and independent execution refusal remain retained |
| Browser isolation | Local Host/Origin/authentication controls; no multi-user or RBAC acceptance |
| Storage and writer admission | Existing fences remain required; populated import, old writers, contention and uncertain acknowledgements require native qualification |
| Restricted console components | Inactive codecs and limited mechanism evidence; no complete production launcher, guardian or lifetime acceptance |
| Build and startup | Only the exact checks in RESULTS are established; no universal clean-install, login/model or platform qualification |

Release and native acceptance remain **unqualified**. Component passes do not
establish native execution, authority, recovery or durable deliverable behavior.

Deployment-specific application and platform capabilities remain **held**.
Historical requirements are preserved in the private recovery records described
in [FAILURES](FAILURES.md).
No customer deployment, disconnected operation, application quality or capacity
acceptance follows from this source inventory or its component tests.
