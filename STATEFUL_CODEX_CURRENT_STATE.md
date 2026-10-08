# Stateful Codex current state

Updated 2026-10-07. This page says what the published code does, what has and
has not been measured, where unmerged work lives, and how to operate the
system safely. It is the only current-state page; it is not a chronological
handoff.

Reading order:

1. [`README.md`](./README.md): what Stateful Codex is and how to start it.
2. [`STATEFUL_CODEX_PRODUCT_INTENT.md`](./STATEFUL_CODEX_PRODUCT_INTENT.md):
   the product requirements. These are intent, not demonstrated results.
3. [`STATEFUL_CODEX_MASTER_PLAN.md`](./STATEFUL_CODEX_MASTER_PLAN.md): the
   canonical build plan, agreed 2026-10-07. Its reference design is
   [`STATEFUL_CODEX_MEMORY_PLAN.md`](./STATEFUL_CODEX_MEMORY_PLAN.md). Where an
   older plan conflicts with the master plan, the master plan wins.
4. This page.
5. [`STATEFUL_CODEX_EVALUATION.md`](./STATEFUL_CODEX_EVALUATION.md): the single
   evidence ledger, including failed, stopped, invalid and retired work.
6. The evaluation references:
   [`LONGITUDINAL_PROTOCOL.md`](./clients/stateful-codex/eval/LONGITUDINAL_PROTOCOL.md),
   [`bixbench/README.md`](./clients/stateful-codex/eval/bixbench/README.md) and
   [`stateful_harbor/README.md`](./clients/stateful-codex/eval/stateful_harbor/README.md).

Older reader documents (the clean-build plan, the restart handoff, the
product-testing handoff and the client README) were folded into this page and
the ledger on 2026-10-07. Their text is recoverable from Git at
`46d9071051445583c00b21e0163d373372d1a966`.

## 1. Which code this describes

The fork began as `feature/stateful-codex` on upstream `origin/main` at
`48f897e4ce94152818ce0563b3a8aac3a70ce9b8`. The published branch is
`stateful/main` at `5eacf03387` (2026-09-30), the
upstream sync that carries the fork at `4ea34e7d10`. Everything in sections 3
to 6 is read from that source. This page is a source description, not a fresh
build or test run: the 2026-10-07 cleanup audit ran only `npm test` in
`clients/stateful-codex` (46 reported tests, of which two are discovered
fixture modules) and the BixBench adapter unit tests (17 pass, fixtures and
mocks only). No Rust suite, live model run or browser session was run for this
page.

The October memory work is **not** in this code. The frozen builds measured in
SC-EVAL-035 (`cont_0e160ee0f6`, `cont_9a3c2109d8`, `cont_27aa1988f8`,
`cont_c466898af7`, `cont_e9ee21021e`) came from the unmerged branches below.
Do not attribute their behavior to `stateful/main`.

## 2. Unmerged work on the `stateful` remote

### Integration on `main` (in progress, 2026-10-07)

The paused branches are being integrated directly on `stateful/main`, one reviewed
checkpoint at a time (plan: `sc_dogfood/integrate/INTEGRATION_PLAN.md`; log:
`sc_dogfood/integrate/EXEC_LOG.md`). Landed so far:

- **Step 1, foundation and direct control** (`052741b9c`..`6839ad4f5`): the
  shared continuity base `52694362a1` merged once; transactional capture writer
  and journal; explicit unscoped `/memory add|correct|forget` with `ID@REV`
  targets, expected-project admission and client result fences; bounded review
  and exact reads. Verdict: landable with named limitations after the repair cap.
  **Cut** from this slice, acceptance rows OPEN: automatic word-for-word rule
  capture from messages, quotation-backed user decisions, investigations and
  scoped rules (historical scoped entries are held back and readable), model
  editing or replacement of user-provenance memory, and historical scope titles
  in review. A narrower automatic-capture contract is scheduled around Step 3.
  Linux: 0 new failures against the 83eee86b6 baseline.

Step 2's final cut additionally refuses model lifecycle operations when any recorded
revision of the target has non-Agent provenance, including Agent entries produced by
older binaries from imported memory. Explicit user correction and Forget remain available.

| Acceptance row | Status |
| --- | --- |
| Model lifecycle of entries with any non-Agent revision history | **OPEN** — cut from the retained model mutation, promotion, retirement, supersession and committed replay surface. |

The table below describes the branch tips as paused; rows are consumed as their
checkpoints land.

All of these branches start from `stateful/main` (merge base `5eacf03387`).
None is merged. Master plan priority 1 is to integrate them into one build
with one migration map and rebuild from a clean tree. Status lines come from the
builder logs (`sc_dogfood/fix/*_LOG.md`) at the 2026-10-03 pause.

| Branch | Tip | Status |
| --- | --- | --- |
| `fix/continuity` | `d10057bac0` | **Consumed by Step 1 (curated, cuts above).** Continuity line (base `3da78a47c4`): trimmed packet, exact delivery floor, standing rules, deferred tools, lean protocol and window budget, each reviewed clean; frozen builds `cont_0e160ee0f6`, `cont_9a3c2109d8` and `cont_c466898af7` came from it. Also carries items 1-3 (`/memory`, `codex memory`, investigations), which hit the repair cap and stay open; on-demand indexing stopped at the cap with a zero-entry once-cell defect; two post-cap commits (`9b09858e40`, `d10057bac0`) are unreviewed fixes. |
| `h1done/foundation` | `9de1ef0052` | **Consumed by Step 1 (curated, cuts above).** `fix/continuity` plus one unreviewed `wip` commit extracting the items 1-3 foundation (action-ID fence, root projection module, explicit-only investigation join/end, host background capture off). When saved it left 4 app-server Stateful tests failing; the rubric review was not run. |
| `h1done/item4` | `7be002b288` | Item 4 reduced contract (base `52694362a1`): 2 of 4 slices committed, unreviewed (canonical identity index; capture judged by canonical identity inside the commit). Parser and recall slices must be redone. Its migration `0018` may clash with other branches. |
| `h1done/item5file` | `4418ddc737` | File-level "needs check" replacement for item 5 (base `52694362a1`): project-intelligence jobs and `paths_changed_between` committed and tested but unreviewed, plus a `wip` worker commit. Read adapters, the remaining tests and the review are left. The rejected regex-confirmation pivot stays on `h1done/item5` (`0b458025a2`). |
| `h1done/item6` | `d265021d3e` | Execution-waste item (base `52694362a1`): the final scoped Codex check rated it safe to land as partial. Open: thin 2 KiB opening packet, topic card, per-response attribution, recipe executor identity, and the other named limits in its log. |
| `h1done/item7` | `d0c7ea9c8a` | Visibility item: the Rust head reviewed before the reduced contract (`12a3dc65ad`) was rejected (2 P1, 7 P2). The reduced contract (recap cut, `memory_add` revert, reconnect fencing) is committed as unreviewed `wip`; one TUI reconnect snapshot test was failing at the pause. |
| `h1done/insession` | `f7aebbc01` | In-session memory (base `52694362a1`): 25 reviewed commits. Step 1 (smaller tool schemas) is landable. Step 2 (render mode) is capped with 3 open findings. Steps 3 (window journal) and 5 (run admission) are STOP/CUT. Step 4 (task capsule) has its second repair committed as unreviewed `wip` with one review round left. |
| `fix/web` | `dfff18633c` | Web client line: gateway origin check, long-project glance, memory panel, item 7 web (approved at `28a971c4c0`). The tip ("turn the return card off") is an unreviewed pure cut; `npm test` 195/195. |
| `h1done/web-items123` | `067272525f` | Web side of items 1-3 (About you, Add form, paging, revision-keyed drafts), from `fix/web` at `e753c36676`; repair round 1 applied; `npm test` 198/198. Kept off `fix/web`; the integrator merges it. |

Other remote branches (`dogfood/*`, `fix/phase1`, `bench/horizon`,
`harness/scbench`, `checkpoint/stateful-region-routing-unsafe-20260925`) are
historical or evaluation-only. The checkpoint branch holds the rejected
bare-line-range routing prototype at `a6cdb157de`; it must not be merged.

## 3. Architecture and source reading path

Ordinary Codex runs unchanged unless a Stateful mode is selected. The Stateful
layer is split across crates so that `codex-core` does not become the product
database.

| Concern | Source |
| --- | --- |
| Startup and identity | `codex-rs/app-server-client/src/stateful.rs` binds an explicit project and root and starts the run; exec and the TUI reuse it. Root CLI flags (`--stateful <mode>`, `--stateful-project`) propagate into `codex exec`. Autonomous runs default to 24 continuations and 14,400 seconds (`DEFAULT_STATEFUL_MAX_CONTINUATIONS`, `DEFAULT_STATEFUL_MAX_ELAPSED_SECONDS`), not unlimited activity. |
| Project intelligence | `codex-rs/project-intelligence`: hierarchy, context map, blackboard, evidence links, relations and premises (migrations `0001`-`0014`); `indexer.rs` scans, publishes and tracks refresh generation and health. |
| Runs, steering, measurement | `codex-rs/stateful-runtime`: runs, obligations, steering, autonomous leases and per-turn measurements (migrations `0001`-`0004`). |
| Agent integration | `codex-rs/ext/stateful`: context, tools, tool policy, turn and thread lifecycle. `tools/mod.rs` registers 13 tools: `blackboard_query`, `blackboard_record`, `blackboard_record_batch`, `blackboard_update_batch`, `blackboard_relate`, `context_map_query`, `context_map_refresh`, `evidence_read`, `obligation_update`, `stateful_run_update`, `stateful_run_read`, `steering_query`, `steering_reconcile`. `world_state.rs`, `run_world_state.rs` and `outcome_world_state.rs` own the incremental, bounded model context. |
| Trust and completion | `source_freshness.rs`, `read_receipts.rs`, `visible_root.rs`, `completion.rs`, `autonomy.rs`, `socratic.rs`, `checkpoint.rs`: read receipts, point-of-use freshness, revision-bound completion aliases, owner fencing and mode gates. |
| API | `codex-rs/app-server/src/request_processors/stateful.rs` and `stateful/api.rs`, `blackboard.rs`, `context_map.rs`, `project_intelligence.rs`, `extensions.rs`, `stateful_store.rs`, `turn_trajectory.rs`; app-server protocol v2 types and the generated TypeScript and Python models. All Stateful methods are experimental v2. |
| TUI and exec | `codex-rs/tui/src/stateful_ui.rs` and the app-server event modules; `codex-rs/exec` Stateful startup, attribution (`stateful_attribution.rs`) and `startup_warning_deduper.rs`. |
| Persistence compatibility | `codex-rs/state/src/migrations.rs`, `codex-rs/rollout/src/state_db.rs` and `state_db_backfill.rs` (section 6). |
| Browser client | `clients/stateful-codex`: `server.mjs` (loopback gateway and app-server bridge), `public/` (setup, workspace, RPC and SSE). |
| Evaluation harness | `clients/stateful-codex/eval`: see the longitudinal protocol's command index. |

Model-visible size limits come from `codex-rs/ext/stateful/src/limits.rs`
(`MAX_MODEL_ITEM_BYTES` is 9,000 bytes) and the renderers. A byte cap is not a
token guarantee. Exact evidence has its own bounded response contract
(12,288-byte reads).

## 4. Design contracts

These contracts come from the original clean-build plan and still describe the
code. Future work follows the master plan.

- **Project, not prompt, is the scope.** A project is the user-selected
  directory with its ordered roots and thread membership, extending Codex's
  existing project identity. Threads are views; start, resume and fork attach
  the same project intelligence when the user selects the same project. Nothing
  infers a project from a prompt or recent activity.
- **Blackboard and context map stay separate.** A blackboard entry is
  understanding (kind, content, confidence, verification, importance, root
  promotion, evidence and relations, supersession). A context-map entry is a
  source location, revision and retrieval description. Neither substitutes for
  exact source. The hierarchy mirrors project, directory, file and optional
  anchored region, with one project node, containment under the roots and
  unique active path or anchor.
- **Mutable records** carry a stable ID, project ID, revision, timestamps and
  provenance, and change only by compare-and-swap or an idempotency key.
- **Model context enters through typed World State contributions.** Each
  fragment is incrementally diffed, bounded below 10K tokens, stable in
  ordering, carries the project identity and knowledge revision, separates
  verified evidence, derived understanding, hypotheses and stale material, and
  survives compaction without rewriting history. The original cap targets were
  1 KiB identity, 24 KiB (under 8K tokens) root, 16 KiB per deeper-state or
  context-map response and 8 KiB per obligation; they were iteration variables,
  and `limits.rs` is the current authority. Truncation is deterministic and
  disclosed.
- **Modes are explicit and durable.** Autonomous keeps working until
  completion, pause or cancel, an exhausted budget, a real authorization
  boundary or a blocker, with lease state for crash recovery. Collaborative
  works normally, emits semantic updates at meaningful changes and accepts
  steering without turning updates into approval gates. Socratic questions and
  synthesizes first and executes only after an explicit transition. The system
  never infers the mode.
- **Obligations and steering are typed records.** The UI never reconstructs
  progress, evidence, readiness or steering from assistant prose or tool text,
  and never labels a run ready, complete or verified from row counts, tool
  success, prose or an exit code.
- **The app-server v2 API is the product boundary.** The experimental methods
  are `projectIntelligence/status` and `/tree`; `blackboard/query`, `/upsert`,
  `/relate` and `/confirm`; `contextMap/query` and `/refresh`; `evidence/read`;
  `statefulRun/start`, `/read`, `/pause`, `/resume`, `/cancel` and `/setMode`;
  `obligation/list`; `steering/submit` and `/list`; and
  `statefulMeasurement/list` and `/summary`, plus `*/updated` notifications.
  List methods use cursor
  pagination; mutations take idempotency keys or expected revisions;
  notifications carry project, run and entity revision; clients recover gaps
  by reading state.
- **The frozen prototype is not a source.** Its code, schemas, APIs and data
  were not transplanted and there is no promise to read prototype databases.
- **Fail visibly.** A missing or corrupt store prevents evidence-backed
  readiness and leaves ordinary Codex usable.

## 5. Supported operation

### Build and start

Build the CLI and its code-mode companion from the same source and profile;
they must sit beside each other, as in a normal Codex package:

```powershell
codex login
cd codex-rs
cargo build -p codex-cli -p codex-code-mode-host
.\target\debug\codex.exe --stateful collaborative "Map this project and start resolving its open questions."
```

`codex exec --stateful <mode>` runs the same startup headlessly;
`--stateful-project` selects an existing project. SC-EVAL-033 found that
`codex --stateful` requires a prompt at launch.

The browser client starts a loopback-only gateway (default
`http://127.0.0.1:4173`, override with `STATEFUL_CODEX_PORT`):

```powershell
cd clients/stateful-codex
npm start
```

Set `CODEX_BIN` only when the branch CLI is elsewhere. The gateway removes
`OPENAI_API_KEY` and `CODEX_API_KEY` from its child and forces the ChatGPT login
method; it neither accepts nor stores an API key. Its content security policy
is `style-src 'self'`.

### Trust boundary for user confirmation

"Confirm this understanding" is a trusted-client action, not a model tool. The
browser sends only the entry ID and revision to `blackboard/confirm`; the
app-server keeps the meaning and evidence and issues the `userConfirmed`
grade. Model record and update tools cannot select that grade, and the generic
`blackboard/upsert` API rejects both `userConfirmed` and a caller-chosen
`sourceVerified` grade. This is an application authority boundary, not
protection against a process with arbitrary host control: such a process could
impersonate a local client or edit the store directly. Clients must call
`blackboard/confirm` only on an explicit user action.

### Measured work panel

The workspace reads `statefulMeasurement/summary` for the newest 100 records of
the selected project. It reports `hasMore` when older records fall outside the
window, keeps terminal and token coverage explicit, shows trajectory totals as
unavailable until a terminal trajectory has merged and as a recorded subtotal
when partially merged, and infers no monetary cost (the stored records have no
provider price, unit or currency). Refreshes are coalesced but keep one queued
follow-up, so an event during an in-flight read causes one final read.

### Known limits of the published browser and TUI

- Hierarchy indentation uses an inline `style="--depth:…"`, which the
  `style-src 'self'` policy blocks, so the tree renders flat. Fix with external
  CSS classes, not by weakening the policy.
- The workspace appends message deltas and clears live text on `turn/started`
  without checking the selected thread, so concurrent sessions on one gateway
  leak into each other's pages (SC-EVAL-034).
- Begin execution and Resume change run status but do not start a model turn.
- The hierarchy pages through every node (500 per page), but other panels read
  bounded windows (50 blackboard entries; 100 obligations, steering records,
  thread items and measurements; 20 context-map hits), not complete project
  history.
- At the Phase A freeze (SC-EVAL-033, 2026-09-28/29) the TUI showed little of
  the Stateful layer: obligation updates were not visible, only the first
  message of a session became a Stateful run, steering was not durable, a
  Socratic run could not be moved to execution, and `resume --stateful` was
  unusable. The published source does route obligation notifications to a TUI
  history cell (`stateful_ui.rs`, covered by a snapshot test), but no live TUI
  check has been recorded since. The memory-visible TUI measured in SC-EVAL-035
  is on the unmerged branches.

## 6. Persistence and compatibility

- Three stores stay separate: Codex host state (`codex-state`), project
  intelligence and the Stateful runtime, each with its own migration history.
- Migration SQL is pinned to LF by `.gitattributes`. The shared runner accepts
  an applied checksum only when the embedded SQL recomputed with LF or CRLF
  proves it is the same migration, keeps the stored checksum, and retries once
  after a concurrent initializer. Substantive SQL changes still fail.
- Historical rollout metadata backfills in the background. Thread reads use
  the filesystem until the backfill completes; the backfill only inserts
  missing rows, so it cannot overwrite live metadata. A persisted owner token
  fences checkpoints and completion after a lease takeover; the first failure
  keeps the backfill incomplete and retries from the last contiguous
  checkpoint.
- The owner-token migration is `codex-rs/state/migrations/0059_backfill_owner_token.sql`.
  It was first released in the fork as version `0056`, which collided with
  upstream's version-56 creator-identity migration; the runner moves a legacy
  fork row from 56 to 59 after replaying its reset so upstream 56-58 can run.
  `state/testdata/state_v56_from_4ea34e7d10.sqlite` reproduces that collision
  for the regression test. Keep both.
- `ignore_missing` lets a binary tolerate newer migrations it does not know;
  it is not a promise that an old binary can safely use a newer store. An older
  binary can silently lose memory (issue #26).

## 7. What is measured and what is open

The ledger is the authority; this is a pointer.

**Latest evidence** (SC-EVAL-035, 2026-10-02/03, unmerged candidate builds,
every test n=1 per arm):

- Across fresh threads, memory helps: on the 20-session horizon series
  Stateful used 0.73x-0.89x the ordinary input, but the crossover depends on the
  ordinary arm's two re-asks (0.83x-0.97x without them). Rules captured verbatim
  held `Next:` 20/20 against 1/20.
- Within one run it has not met the cost gate: long1 and long2 cost more and
  blind reviews preferred ordinary; within1 cost 1.27x priced units with 23
  against 18 compactions, although its blind review preferred Stateful (about
  80%) and it recalled never-restated facts 2/3 against 0/3. Ratios within about
  0.15 of 1.0 are noise at n=1, and within1's resume turns ran under
  workspace-write in both arms.
- The first session costs more (1.7-1.9x per the master plan).

**Earlier evidence still standing:** SC-EVAL-034 (one run per arm; 13.4%
cheaper over 25 questions, more expensive on small one-shot batches);
SC-EVAL-033 (fresh-thread Stateful best on the data room; fixed overhead on
trivial asks; standing rules not captured deterministically); SC-EVAL-032 (one
continuous thread: 2.0x input and 2.7x uncached, quality level); SC-EVAL-034's
negative findings (stale memory served as current, 0/10 against 7.5/10;
compaction lost numbers told to the user, 0/4 against 3/4). Terminal-Bench 2.1
one-attempt breadth was 70/89 on the original bundle and 68/89 on the Phase A
freeze; neither is an official score.

**Open gates** (each needs evidence, not more components):

- Within-run cost: the master plan's Gate B replay and the within1 replay
  (cost at or below plain, 3.70M units; at most 18 compactions).
- First-session premium and fixed overhead on trivial requests.
- Stale state: point-of-use freshness for changes outside the agent's own
  sources.
- Standing-rule application, not only capture.
- Index cost (issue #19): a real refresh must record the scan and publication
  split, database size and used-versus-indexed routes before any indexer
  rewrite. The historical 1,537-file, 25,096-region refresh took 169-248
  seconds; a small local profile was roughly linear and does not explain it.
- Trajectory counters were last validated by mechanism tests; the first live
  claim was withdrawn (ledger, implementation checkpoints). A normal-home live
  reconciliation of trajectory counters, turn-local usage and both measurement
  methods has not been recorded.
- The repository-wide Rust suite has not been run for the fork; it requires
  explicit approval.
- Whether the full root stays loaded after an in-thread compaction is the
  master plan's open decision.

## 8. Operating rules for testing and evaluation

These rules come from incidents recorded in the ledger. Each one cost real
time.

- **Authentication.** Use the cached ChatGPT login only. Prefix commands with
  `env -u OPENAI_API_KEY -u CODEX_API_KEY`. A Codex process started before a
  fresh `codex login` keeps its old credential in memory; restart it rather
  than editing auth (2026-09-26 401 incident).
- **Isolation.** Give every arm its own copy of the project and its own Codex
  home and state store, with host memories disabled
  (`memories.use_memories=false`, `memories.generate_memories=false`). Never
  run tests against the user's real `~/.codex` state, and do not share an auth
  or store directory between parallel evaluations. Concurrent sessions on one
  SQLite store can starve the pool (issue #37).
- **Binary identity.** Build both binaries from one clean tree and record the
  commit, version and hashes before interpreting a result. On Windows a running
  binary cannot be replaced; build into a separate target directory. A release
  binary built while its tree was being edited matches no commit and must not
  be frozen.
- **Windows builds.** Build test binaries at below-normal priority and never
  while an A/B campaign is running. If the pinned V8 prebuilt download fails,
  point `RUSTY_V8_ARCHIVE` at `<target>/debug/gn_out/obj/rusty_v8.lib` and
  `RUSTY_V8_SRC_BINDING_PATH` at
  `<target>/stateful-v8-artifacts/src_binding_ptrcomp_sandbox_release_x86_64-pc-windows-msvc.rs`
  (the published `rusty-v8-v150.4.0` archive and binding worked);
  set `AWS_LC_SYS_PREBUILT_NASM=1`. After changing `.sql` migrations run
  `cargo clean -p codex-state -p codex-project-intelligence -p codex-stateful-runtime`.
  `git add` in a fresh copy can hit a transient "Permission denied" on
  `.git/objects`; retry and verify the baseline commit before launching.
- **Keep every attempt.** Invalid, failed and interrupted attempts stay in the
  record with their reason. Missing measurements stay missing.
- **Count the right things.** `codex exec` "tokens used" is uncached input plus
  output; cumulative and cached input are in the `--json` usage events.
  Canonical compaction events and their replacement history are read from
  the session rollout (`"type":"compacted"` records); the `--json` stream
  carries only cumulative compaction counters in its trajectory records. Distinguish what the state database captured
  from what the model actually received, and host maintenance from model work.
- **Headless hygiene.** Close stdin for background `codex exec` (`< /dev/null`)
  or it can hang before creating a session. Stopping a campaign means stopping
  its driver loop as well as the Codex processes. Confirm a worker by its
  rollout file, not its PID. Do not time post-run checks as agent time.
- **Project identity is explicit.** Select the project ID or root; do not rely
  on matching directory contents. Use distinct directories per arm and run.
- **Agree the design before running.** State what a test isolates before
  launching it. Tests built from independent units show rule persistence, not
  whether nuanced mid-session facts survive many compactions.
- **Test as a user.** Judge memory by using the CLI, TUI and web UI on real
  work; report quality, cost and operation (rules kept, survival through long
  runs and compaction, speed). Both arms run the same model, so quality should
  be comparable; do not over-invest in arm-fairness confounds.
- **Protected evidence.** The untracked directories
  `clients/stateful-codex/eval/results/pramana-ab-2026-09-24/` (SC-EVAL-032) and
  `clients/stateful-codex/eval/results/codex-vs-droid-2026-09-25/` are read-only
  evidence: never modify, stage, delete or regenerate them, and never use
  `git add .` or a directory-wide add under `eval/results`.
- **Issues** go to `dl1683/stateful-codex`, never `openai/codex`.
- Push only to the `stateful` remote.
