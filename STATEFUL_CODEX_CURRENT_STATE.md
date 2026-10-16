# Stateful Codex current state

## Rebuild status pointer — 2026-10-16

The dated sections after this pointer preserve the October12 snapshot. Current
execution follows the owner-invoked
[Codex/Droid rebuild strategy](C:/Users/devan/sc_dogfood/lsr_drivers/council/STATEFUL_REBUILD_STRATEGY_CURRENT.md).
The first company-brain milestone remains **NOT FULFILLED**. Consult the latest
appended entries in [the evaluation ledger](./STATEFUL_CODEX_EVALUATION.md).

Reviewed components now provide project-local resolution, bootstrap identity and
registry, filesystem admission, closed-stage publication, compiled intelligence
and runtime schema installation, owned readonly snapshots, bounded attributed
content reads and the `stateful-memory` CLI. CLI reads disclose historical
attribution and omitted fields; they do not authenticate production ingress or
assert effective truth. Native knowledge/runtime still use the legacy stores.
The owner priority is one project-owned authority with verified conversion and
lock handling; these components do not yet deliver that complete milestone.

The [project-local writer packet](./STATEFUL_PROJECT_STORE_WRITER_WITNESS_20261016.md)
records the new trusted short-transaction primitive, exact source review and local
Windows/WSL51-test gates, plus separate real-schema rollback/commit/contention/
owned-helper recovery evidence. It is an inactive library component, with no
native lock removal or attachment. The sole actual Luna check failed its exact-cell contract on added trailing LF;
separate diagnostics matched both tool/final JSON outputs. It remains FAIL,
without retry or normalization. This is source/direct-harness qualification only.
The separate runtime-completion adapter remains local on
`rebuild/project-runtime-transaction` at `78989cce288a7b95fa56f137f0a04d8d72168012`:
Droid source-approved and Windows/WSL75 runtime+51store tests passed, but no
separate hands-on Luna witness or promotion. Native completion certification must
also validate knowledge, eligibility, steering and source bindings in one guard.

Earlier metadata/snapshot/effective-reader helper journeys remain scoped evidence,
linked from their packets and the ledger. Bounded attributed CLI source is now
implemented; older next-read plans saying otherwise are historical. The full
Linux PI qualification hold was resolved by the later149-pass run recorded in the
[platform qualification packet](./STATEFUL_PROJECT_PLATFORM_QUALIFICATION_20261016.md).
The opt-in Windows GNU CLI Bazel lane passed7cases/2targets; default Windows/MSVC,
ARM64 and full native Codex are not qualified by it. The associated native journey
failed because code-mode-host=false made its tool unavailable; exit0 was not PASS.

Native exact Marathi transport still fails405-to329bytes. A separate attributed
CLI journey preserved both tool results but failed final nested JSON escaping.
Both failures remain OPEN; the bounded ASCII CLI and narrower refusal witness do
not repair them. See [native qualification](./STATEFUL_HAL_NATIVE_QUALIFICATION_20261016.md),
[attributed CLI witness](./STATEFUL_PROJECT_ATTRIBUTED_CLI_WITNESS_20261016.md),
[HAL preservation](./STATEFUL_HAL_PRESERVATION_BASELINE_20261015.md) and the
[restricted-console mechanism](./STATEFUL_HAL_RESTRICTED_CONSOLE_WITNESS_20261016.md).
The latter qualifies only a separate mechanism, not production policy/transport.

The separate upstream executor retains dirty B0 accounting/recovery custody;
production59/60 collision and remaining gates block its promotion. No approved
upstream batch is available. Historical refs and its worktree remain preserved.
No home conversion, populated importer, old-writer barrier, native project routing,
whole-store completion certification, compaction survival, verified edit/Forget
ingress, MCP, offline/HAL/customer acceptance is claimed. The
[cutover inventory](./STATEFUL_PROJECT_STORE_CUTOVER_INVENTORY_20261015.md)
identifies cached pools, queued publishers, state-DB admission coupling and legacy
authority readers that must be reconciled before activation.

Updated 2026-10-12 (a snapshot: landings of 2026-10-13 and 2026-10-14 are summarised in the note at the top of `STATEFUL_CODEX_EVALUATION.md`, which is the current pointer until this page is refreshed). This page says what the published code does, what has and
has not been measured, where unmerged work lives, and how to operate the
system safely. It is the only current-state page; it is not a chronological
handoff.

Reading order:

1. [`README.md`](./README.md): what Stateful Codex is and how to start it.
2. [`STATEFUL_CODEX_PRODUCT_INTENT.md`](./STATEFUL_CODEX_PRODUCT_INTENT.md):
   the product requirements. These are intent, not demonstrated results.
3. [`STATEFUL_CODEX_MASTER_PLAN.md`](./STATEFUL_CODEX_MASTER_PLAN.md): the
   canonical build plan, agreed 2026-10-07. Its reference design is archived
   unchanged as
   [`STATEFUL_CODEX_MEMORY_DESIGN_2026_10_06.md`](./STATEFUL_CODEX_MEMORY_DESIGN_2026_10_06.md).
   Where an older plan conflicts with the master plan, the master plan wins.
4. This page.
5. [`STATEFUL_CODEX_EVALUATION.md`](./STATEFUL_CODEX_EVALUATION.md): the single
   evidence ledger, including failed, stopped, invalid and retired work.
   SC-EVAL-001 to 037 are archived verbatim in
   [`STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md`](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md).
6. The evaluation references:
   [`LONGITUDINAL_PROTOCOL.md`](./clients/stateful-codex/eval/LONGITUDINAL_PROTOCOL.md),
   [`bixbench/README.md`](./clients/stateful-codex/eval/bixbench/README.md) and
   [`stateful_harbor/README.md`](./clients/stateful-codex/eval/stateful_harbor/README.md).

Older reader documents (the clean-build plan, the restart handoff, the
product-testing handoff and the client README) were folded into this page and
the ledger on 2026-10-07. Their text is recoverable from Git at
`46d9071051445583c00b21e0163d373372d1a966`.

## 1. Which code this describes

This page describes `stateful/main`, read at `e2249ad43` (2026-10-12). Its
code is identical to the pushed head `f097a8008` (2026-10-11); the later commits
change docs only. The paused October branches were integrated onto `main` one
reviewed checkpoint at a time from 2026-10-07; the landings are summarized in
section 2. The fork began
as `feature/stateful-codex` on upstream `origin/main` at
`48f897e4ce94152818ce0563b3a8aac3a70ce9b8`.

This page is a source description, not a fresh build or test run. The most
recent test results are in the landing records below. The frozen builds measured
in SC-EVAL-035 (`cont_0e160ee0f6`, `cont_9a3c2109d8`, `cont_27aa1988f8`,
`cont_c466898af7`, `cont_e9ee21021e`) came from the then-unmerged branches; do
not attribute their measured behavior to the current `main` without a rerun.

The September source pin, the integration log for each step and the 2026-10-03
inventory of paused branches are archived verbatim in
[`STATEFUL_CODEX_PLAN_HISTORY.md`](./STATEFUL_CODEX_PLAN_HISTORY.md), part 3.

## 2. What has landed on `main`

Integration plan: `sc_dogfood/integrate/INTEGRATION_PLAN.md`; log:
`sc_dogfood/integrate/EXEC_LOG.md`. The full landing records for the earlier
checkpoints are in the plan history, part 3.

| Checkpoint | Commits | Outcome |
| --- | --- | --- |
| Step 1, foundation and direct control | `052741b9c`..`6839ad4f5` | Landable with named limitations after the repair cap |
| Cleanup K1 | `fd89dfca5`..`f5f9fea96` | Eval and test hygiene; the backfill-warning JSON fix was reverted |
| Step 2, migration map and store classification | `716e3ae5e`..`e334727cb` | Landable with named limitations |
| Capture C2, source, identity and storage foundation | `7ae0a43b5`..`a7e0bb127` | Landed with cuts |
| Capture C3, model proposals and exact source | `3fe03dfa6`..`42370b992` | Landed with cuts |
| Cleanup K2, portable tool schemas | `738e1c401`..`68638f6e6` | Landed with an open gap |
| Checkpoint E, evidence-bound completion | `fb148cb38`..`0a83b1f58` | Not shippable alone; followed by E1b, E1c and E1d |
| Checkpoint E1b, real-repository check qualification | `056912335`..`179be19c4` | Landed; its read-only exemption was cut |
| Checkpoint E1c, no-tool completion exemption | `0df6aedbd`..`668602bb8` | CUT; code restored to E1b's behaviour |
| Capture slice C4-C6, capture-to-use | `11a08f976`..`92efdff9f` | Landed with a cut (record below) |
| Checkpoint E1d, plain answers in Autonomous runs | `ec7910a06`..`e009fa83f` | Landed as the fallback (record below) |
| Stack pushed 2026-10-11 | from `ebac34e91`; pushed head `f097a8008` | Pushed on local evidence; the later cloud gate found 0 new failures caused by the stack (record below) |
| Step 3b, deliverable guidance and warning-only diagnostic | head `64e6176ed`; follow-on `9eddc1af4` | Closed: R1-R4 not triggered; R1 only after post-outcome adjudication (record below) |
| P1, exec bounded history reads | `862d3115d`..`b8dcee46f` | Landed with named limitations L-R1..L-R5 and L3-3 (record below) |

### Open items and cuts carried from the earlier landings

Each item is quoted or condensed from its landing record in the plan history,
part 3.

- **Step 1, acceptance rows OPEN** (cut from that slice): quotation-backed user
  decisions, investigations and scoped rules (historical scoped entries are held
  back and readable), model editing or replacement of user-provenance memory,
  and historical scope titles in review. Automatic word-for-word rule capture,
  also cut there, has since landed in C4-C6.
- **Step 2, acceptance row OPEN:** model lifecycle of entries with any non-Agent
  revision history. The model can change or retire only memory whose every
  revision is assistant-authored; explicit user correction and Forget remain
  available.
- **C2 cuts:** root predecessor-content publication, legacy alias replay on
  upgraded stores, historical-findings run completion, and existing-entry reuse
  for new source groups.
- **C3 cuts:** automatic exact-source and source-search recall, and automatic
  proposal recall.
- **K2 gap (OPEN):** providers whose bridge puts very long data in call IDs
  (LiteLLM with Gemini thought signatures) are refused rather than supported,
  and there is no host-enforced cap on a model that never produces a valid
  completion.
- **Checkpoint E:** checker adequacy is not independently judged.
- **E1b:** non-Git roots and files inside embedded repositories fail closed.
- **E1c:** migration `0012` stays as applied history, unused.

### Recent landing records

- **Capture slice C4-C6, capture-to-use: LANDED WITH A CUT** (`11a08f976`..`92efdff9f`):
  stated project rules are saved word for word at ingress with a receipt and
  Undo. A model-proposed decision or ruled-out approach can be applied with
  `/memory apply`, but only for the user's whole single message of at most 240
  bytes. Forget and Undo hold across restarts and are never silently restored,
  and a cold new thread receives the rules and decisions in its root, with the
  time the user said them.
  After four reviews and three repairs, the automatic tool-result publication
  fence was removed. Cut with it: model tools no longer return the user's own
  memory (rules, decisions, corrections), and the model can no longer read past
  conversation (`conversation_read` is removed, and so is `memory_read`'s
  earlier-turn route).
  **Usefulness loss, until the turn-boundary Forget contract restores it:** the
  model sees past conversation and user memory only through thread-start
  continuity and the root.
  Hands-on (SC-EVAL-038): rules, decision, cold recall and Forget pass. A
  rejected hypothesis is still never proposed by the model (OPEN).
  Inherited, OPEN: a held read of an agent finding can still be delivered after
  another thread's model retires it.

- **Checkpoint E1d, plain answers in Autonomous runs: LANDED AS THE FALLBACK**
  (`ec7910a06`..`e009fa83f`): the host-ended Completed answer failed review
  (10 blockers) and was removed. Instead, a final message that ends with the
  four-line `[stateful-outcome]` block (disposition answer, no open issues) ends
  the run **answered, not verified**. That is a terminal status, neither
  Completed nor Blocked, and it satisfies no acceptance criterion. It shows as a
  calm label in the TUI, exec and Web. An interrupt before the commit wins.
  **OPEN:** the hands-on check fails. With gpt-6-luna, one of two trivial
  questions still ended Blocked because the model took the completion-tool route.
  The outcome block is shown raw, and no answered label appeared. A follow-up
  that strengthens the guidance, the refusal hint and the rendering is next.
  **OPEN debt:** an answered run is stored as `completed` plus a
  `stateful_host_answers` record, which every in-repo reader projects to
  answered. The journal and epoch foundation step replaces this with a
  first-class status through an additive migration.

- **Stack pushed 2026-10-11 on local evidence only** (stateful/main `ebac34e91` to
  this commit; the user approved pushing without the cloud gate while gcloud
  auth was unavailable). It contains:
  - capsule stage (a): an eligibility generation, a projection store and a
    publication ticket, inactive;
  - the structural working-set floor and its offline driver, inactive;
  - the piggyback stages and their revert, so their net effect is zero;
  - reads slice E1, under this contract: "Forget removes the entry from memory
    reads, the session-start memory and the digest. Text the agent already wrote
    into its own run notes, strategy or findings before the Forget, and earlier
    messages in existing threads, are derivatives and are not removed.";
  - the Forget receipt hint;
  - a continuity fix that reports withheld turns as withheld rather than as
    unreadable history;
  - the plan docs.

  Local tests at the frozen head (Windows):

  | Crate | Result | Failures |
  | --- | --- | --- |
  | project-intelligence | 144/144 | none |
  | stateful-extension | 235/236 | known fan-in timeout |
  | exec | 134/136 | load timeouts |
  | tui | 5,579/5,583 | Windows-local, outside the stack |
  | app-server | 1,830/1,864 | Windows-local base set; no Stateful test failed |

  **Cloud gate on the pushed head (`f097a8008`):** 0 new failures caused by the
  stack (3 known base core failures; one app-server login-callback flake that
  passes on rerun).
  **Named limitation:** the agent's own copies of user words survive Forget, as
  derivatives. Example: a rule containing "credential MARA-QX704" is forgotten,
  but a strategy it wrote, "Continue release checks with credential MARA-QX704.",
  still shows.
  **OPEN defect:** capture seals host text as the user's words. That covers
  realtime delegation, heartbeat prompts and /init.
  **Origin plumbing: CUT (2026-10-11).** Three review rounds; the final
  review returned STOP/CUT. The server, core and capture half passed static
  review (per-input origin, downgrade-only `hostGenerated`, capture seals only
  User). The TUI half did not: the composer-level fail-closed rule still let
  host or restored text reach a User submission through slash-command
  arguments and queued slash fallbacks, kill-buffer reinsertion, completion
  insertion and long recovered question drafts. The patches are archived in
  `sc_dogfood/integrate/wip/origin_final/` (14 patches, base `73d97824a`) and the
  review is in `sc_dogfood/integrate/reviews/origin_rework_review.md`. A future
  attempt should carry provenance per inserted span, not per draft, and can land
  the additive server half first with capture unchanged.

- **Parked: the capsule working-set scaffold.** Parked 2026-10-12 (entropy
  pass); the code deletion lands in the entropy code slice. It is the inactive
  capsule stage (a) storage and publication scaffold listed in the stack above.
  - Tag: `parked/capsule-scaffold-2026-10-12`, on `e2249ad43`.
  - To revive, restore from the tag
    `codex-rs/project-intelligence/src/working_set_storage.rs`,
    `working_set_storage/publication.rs`, their tests
    (`working_set_storage_tests.rs` and `working_set_storage/publication_tests.rs`),
    and the shared file-sample types and constants, which the entropy code
    slice moves into the extension.
  - Migrations `0024_working_set_capsule.sql` and
    `0025_capsule_source_observation.sql` stay in the tree.
  - Why it would be revived: the structural floor results in SC-EVAL-041.
    "The floor holds both hard gates on within3: zero wrong-active rules and
    zero rejected-working routes over 104 capsules." Its tuned-set row
    (`c96429fa1`, 512 tok) scored 5/5 plain and 5/5 Stateful; on within3, now
    a regression set, 2/5 and 2/5.

- **Step 3b, deliverable guidance and warning-only diagnostic: CLOSED, stays**
  (`64e6176ed`, with the follow-on `9eddc1af4`: warn once per run;
  status-phrase sequences). None of the predeclared revert criteria triggers
  (SC-EVAL-044): R2 B=14 vs C=19; R3 ms4 ctl−3b +0.258, q36a3 −0.204; R4 0/22;
  R1 0/317 after adjudication. **Disclosure:** the file-deliverable
  interpretation used for R1 was adopted after outcomes were visible; the
  first checker's 39/317 (12.303%) is retained (FINDINGS "3b R1 ADJUDICATED
  and 3b CLOSED"; `sc_dogfood/integrate/reviews/adj_hunt.md`). Usefulness is
  not established by these criteria. **OPEN:** q36a3 critical errors 3.7% to
  9.3%, to be checked in the slice-5 matched release rerun.

- **P1, exec bounded history reads: LANDED WITH NAMED LIMITATIONS**
  (`862d3115d`..`b8dcee46f`, pushed 2026-10-12). Exec's completion backfill
  and Autonomous reconciliation no longer call `thread/read` with
  `include_turns: true`; an experimental app-server opt-in that only exec sets
  retries the history projection before listing turns. Named limitations
  (`sc_dogfood/integrate/reviews/p1_impl_report.md`):
  - **L-R1:** no witness drives the follow-deadline and `Lagged` recovery
    branches around both `reconcile_autonomous_wait` call sites.
  - **L-R2:** the turn-list request-error case of reconciliation is
    unwitnessed.
  - **L-R3:** the autonomous end-to-end test neither forces a recovery branch
    nor checks hydration notices, so it does not close L-R1.
  - **L-R4:** with nothing delivered, a stale page holding an earlier
    AgentMessage fills the turn; the backfill read has no projection barrier.
  - **L-R5 (deferred):** a final answer delivered only by stream, followed by
    a stale successful backfill holding an earlier AgentMessage, can still
    overwrite it in both `-o` files.
  - **L3-3:** payload bytes are unbounded; there is no byte cap.

## 3. Architecture and source reading path

Ordinary Codex runs unchanged unless a Stateful mode is selected. The Stateful
layer is split across crates so that `codex-core` does not become the product
database.

| Concern | Source |
| --- | --- |
| Startup and identity | `codex-rs/app-server-client/src/stateful.rs` binds an explicit project and root and, for the Work modes, starts the run; exec and the TUI reuse it. A bare `--stateful` is Ask, which starts no run (`StatefulModeCliArg` in `codex-rs/utils/cli/src/stateful_mode_cli_arg.rs`). Root CLI flags (`--stateful <mode>`, `--stateful-project`) propagate into `codex exec`. Autonomous runs default to 24 continuations and 14,400 seconds (`DEFAULT_STATEFUL_MAX_CONTINUATIONS`, `DEFAULT_STATEFUL_MAX_ELAPSED_SECONDS`), not unlimited activity. |
| Project intelligence | `codex-rs/project-intelligence`: hierarchy, context map, blackboard, evidence links, relations and premises, repository observations, knowledge context and capture sources (migrations `0001`-`0025`); `indexer.rs` scans, publishes and tracks refresh generation and health. |
| Runs, steering, measurement | `codex-rs/stateful-runtime`: runs, obligations, steering, autonomous leases, per-turn measurements, the acceptance ledger and host answers (migrations `0001`-`0004` and `0010`-`0013`). |
| Agent integration | `codex-rs/ext/stateful`: context, tools, tool policy, turn and thread lifecycle. `tools/mod.rs` registers 14 tools. Every Stateful thread gets the 8 project tools: `blackboard_query`, `blackboard_record_batch`, `blackboard_update_batch`, `blackboard_relate`, `memory_read`, `context_map_query`, `context_map_refresh`, `evidence_read`. A thread with a run also gets the 6 run tools: `obligation_update`, `stateful_run_update`, `stateful_acceptance_update`, `stateful_run_read`, `steering_query`, `steering_reconcile`; an Ask thread does not (`RunTools::Hidden`). `world_state.rs` and `run_world_state.rs` own the incremental, bounded model context. |
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

Ask is the default. A bare `--stateful` means `--stateful ask`, which starts no
run (`default_missing_value = "ask"` in `codex-rs/tui/src/cli.rs:30` and
`codex-rs/exec/src/cli.rs:37`), and the browser client's setup form starts in
Ask (`mode: "ask"`, `clients/stateful-codex/public/setup-controller.mjs:19-20`).
`autonomous`, `collaborative` and `socratic` start a Work run.

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
  history cell (`stateful_ui.rs`, covered by a snapshot test). The memory-visible
  TUI measured in SC-EVAL-035 was on then-unmerged branches; the later TUI
  journeys are SC-EVAL-038, 039 and 042.

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

**Newest evidence** is SC-EVAL-038 to 044 (2026-10-09 to 10-12): the capture,
Ask and restored-read journeys, the restricted-read within1 baseline, the
capsule offline tests, the open-model ladder (measured on bundle 83eee86b6,
before the acceptance-ledger completion gate) and the three-way
harness regression check with its gate-bypass causal check.

**Cross-session campaign** (SC-EVAL-035, 2026-10-02/03, unmerged candidate
builds, every test n=1 per arm):

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
- The superseded master priorities recorded a first-session premium of 1.7-1.9x; see `STATEFUL_CODEX_PLAN_HISTORY.md`, part 1. This is a historical observation, not a fresh measurement of this snapshot.

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

- Within-run cost: the bounded within1 run in the master plan's canonical
  order. The reference is the SC-EVAL-040 restricted-read baseline's plain arm
  (1.94M units, 10 compactions), which replaced the earlier 3.70M-unit,
  18-compaction reference.
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

### Benchmark position (summary)

- Benchmark reruns are suspended.
- The positioning is one general harness competitive with the best model- or benchmark-specific harnesses, not a plain-Codex parity study.
- Real-task comparisons (the open-model ladder, SC-EVAL-043, measured on bundle 83eee86b6, before the acceptance-ledger completion gate; the later three-way check and gate-bypass causal check are SC-EVAL-044, final) are the current evidence.
- Selection rules and earlier benchmark results: "External benchmark program" in [`STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md`](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md).

### Harness robustness backlog (from the open-model ladder)

Added 2026-10-11. The open-model ladder (SC-EVAL-043) ran one Stateful Codex
bundle (83eee86b6, Autonomous, 108 sessions per model) across open models.
Failures on small and mid-size models are read here as **harness signals**:
each one is a place where the harness, not only the model, decides whether the
user gets an answer. Paths are under `sc_dogfood/dsg/ladder/` unless stated.
Counts are from `runs/<arm>/sessions.jsonl` and `runs/<arm>/logs/`.

> **Label: the ladder evidence in items 2-8 was measured on bundle 83eee86b6, before the acceptance-ledger
> completion gate.** Item 1 comes from the later three-way check and gate-bypass check (SC-EVAL-044).

1. **Acceptance-mediated failures in Qwen control.**
   In SC-EVAL-044, Qwen3.6-35B-A3B rc0 fell from 98 (original, 83eee86b6) to 3 (control,
   7c4707230) and 2 (3b, 64e6176ed) of 108. The regression contains directly evidenced acceptance-mediated
   blocked runs and acceptance-repair loops, alongside timeout failures; the gate's individual aggregate effect is
   not identified and environmental contributions are not excluded.
   **Gate-bypass check (SC-EVAL-044; `sc_dogfood/integrate/reviews/gb_hunt.md`):** "The bypass result strongly
   supports the acceptance completion gate decision as the main cause of the q36a3 exit-status regression on these
   20 t1/s1 keys. It does not isolate the effect from load and time, establish recovery across the full ladder, or
   establish usefulness recovery." (rc0 original 16, ctl 0, gb 19 of 20.)
   - (a) **Non-git workspaces record passing checks as "unavailable"** (`777fac767` removed non-Git workspace
     enumeration). Qwen control `csv__t1/s1` runs checks returning exit code zero and `OK`; the ledger records
     `outcome:"unavailable"` ("the check changed files no criterion pins, or the workspace could not be
     identified, so its effects are unknown"). The snapshot contains no Git repository. This does not prove that
     every unavailable receipt has that cause. **Harness-wide for the host-command evidence path, verified with
     gpt-6-luna (n=1):** "the git-only workspace identity failure is real shared host-command evidence behaviour
     (acceptance_workspace.rs:214/160/93 -> acceptance_observation.rs:384), i.e. harness-wide for that evidence
     path" (`sc_dogfood/campaign2/handson_gate/RESULTS.md`, Corrections).
   - (b) **Plain questions end Blocked after acceptance refusals.** Qwen control `qaconcept__t2/s1` exits rc1
     after 211 seconds ("completion refused; the run stays running (refusal 1 of 3 without acceptance progress
     before the host blocks it as partial). Unmet acceptance gates: C1 ... omission proposal awaiting review"),
     with `status:"blocked", revision:20`; its final explains that the answer was already supplied. **With
     gpt-6-luna (n=1):** the plain question incurred one malformed completion rejection and one omission-gate
     refusal because Luna attempted `stateful_run_update`, then the answer footer ended it answered after one
     refusal; repeated completion attempts without acceptance progress can instead cause host blocking. n=1 does
     not isolate instruction-following as the cause of the Luna/Qwen difference (same Corrections, items 5-6).
   - (c) **Acceptance repair produces schema and revision loops.** Control errors include
     "nothing changed: changes[0] needs text for this action" (189 times across control), "nothing changed:
     changes[0] does not take statement for this action" and "openIssues is only valid when status is
     completed", plus failures over missing criteria, checker files, pinned artifacts and incompatible manual
     observations. **With gpt-6-luna (n=1):** the work trial's stderr has 18 "nothing changed" lines, and its
     initial checker admission failed because the stored criterion had an absolute checkCwd (same Corrections,
     item 7). n=1; obedience is not isolated.
2. **Status-only finals.** ms4 (Mistral-Small-4): 40 of 108 session-final
   answers are only a status line, across 25 tasks (30 are `Task completed.`).
   Two more ms4 finals are the raw completion-tool JSON. Other arms: q36a3 1,
   mmm3m 2, every other row 0. This mirrors baseline Codex finishing without
   the output and needing a re-prompt. Pointer-only omissions are counted
   separately: q36a3 has three finals that say "see ... above" (deck t1/s2,
   deck t2/s2, postmortem t2/s2), and memdecide t2/s1 says the Slack summary is
   above without reproducing it.
   Fix: step 3b (guidance plus a warning-only diagnostic; frozen
   `sc_dogfood/integrate/briefs/step3b_freeze.md`), landed and closed with no R1-R4 revert (SC-EVAL-044); surviving real-model omissions stay OPEN. 3b never
   re-prompts. A bounded harness re-prompt for the deliverable stays a candidate
   if the predeclared 3b rerun leaves omissions.
3. **Tool-argument type mismatches.** step37f (Step-3.7-Flash): 1,743
   argument-type rejections in 104 of 108 sessions (`codex_core::tools::router`
   in `s*.err`). 1,279 rejections supplied unsigned decimal digit strings to
   integer fields (`"1"` for a u64). Twenty supplied other strings, including
   numeric representations invalid for the expected integer type, empty
   strings, placeholders and markup. 431 contain valid JSON
   objects or arrays encoded as strings. Four more struct or sequence
   rejections contain malformed JSON, XML or a JSON scalar. 9 are boolean
   strings. 92 of its 108 sessions exit rc=1.
   Among the other ladder arms, numeric-string integer rejections appear only
   in mmm3m (6 lines across 3 sessions). Other arms also have nonnumeric
   string-ID/integer mismatches, for example q35d9h `"7af948"`.
   Fix: lenient schema coercion at the tool boundary (a lossless numeric string
   becomes an integer; a JSON-encoded string becomes the expected object or
   array), with a logged warning and a counter. Anything else stays rejected.
4. **Loops on the completion step.** q36a3 (Qwen3.6-35B-A3B): 10 of 108
   sessions exit rc=1. 9 of the 10 ran to exactly 25 turns without completing
   the run. All 10 carry completion-contract rejections (46 lines, for example
   "completionDisposition noReusableLearning completes with the result only;
   omit rootRevision, ..." and "run revision conflict").
   The tenth exit, memcontinue t2/s1, ended after 4 turns. It followed a
   successful Blocked update at revision 5, and Autonomous exec returns rc=1
   for that status (archived rollout in `runs/q36a3/snap/`).
   Exactly 25 turns (maximum cumulative `stateful_attribution.turns`) is also
   the pattern for ms4 48 of 48 rc=1 sessions, step37f 84 of 92, gr42d8x 36 of
   38 and q35d9h 3 of 4. Another 7 step37f sessions and 1 q35d9h session ran
   26 turns. Rejections are common even in sessions that end rc=0 (q36a3 89 of
   98).
   **Correction:** the 3b freeze (§§7/9) and the predeclared FINDINGS entry
   originally classified these ten exits as hydration failures. Both have now
   been amended: nine sessions ran 25 turns with rejected completion updates;
   memcontinue t2/s1 ended after four turns following a successful Blocked
   update. The hydration message is in 107 of q36a3's 108 sessions, including
   97 of its 98 rc=0 sessions, and is counted separately, not as an exit cause.
   Fix: the completion tool normalises an unambiguous but over-specified call
   (drops the fields the chosen disposition forbids, with a warning) instead of
   rejecting it. After repeated completion rejections in one run, it returns a
   minimal valid example, or ends the turn with the answer and a clear blocked
   status, rather than continuing to the turn ceiling.
5. **Hydration error on every continuation (separate harness defect).** In
   Autonomous exec, `thread/read` is called with `include_turns: true`
   (`codex-rs/exec/src/lib.rs` 1730-1737 in `reconcile_autonomous_wait`, and
   2180-2187). The app-server answers with its paginated-thread deprecation
   notice (`app-server/src/request_processors/thread_processor.rs:42`). Exec
   JSONL then prints that notice as an `error` item, once per continuation (24
   in a 25-turn session). It appears in 94-107 of 108 sessions on the arms
   with long sessions (gr42d8x, mmm3m, ms4, q35d9h, q36a3, step37f), and in
   2-19 of 108 on the others. It is non-fatal, but it is a false error item and
   re-reads the full history on every continuation. The call sites are read
   from source; the per-session link to the reconcile call is inferred from the
   one-per-continuation count.
   Fix: page with `thread/turns/list` (or read only the latest turn), and do
   not surface a deprecation notice as an error.
6. **Chat template rejects the `developer` role.** ms4 needed a bridge-side
   hook (`devrole.py`: leading developer/system messages become `system`,
   mid-thread ones become `user`) and a chat-completions bridge path. Without
   them it failed "Unexpected role 'system' after role 'assistant'" and
   "'input_text' is not a valid ChunkTypes" (`LOG.md` rows for ms4;
   `sc_dogfood/dsg/RUN_LOG.md` 20:15Z). ms4 reports zero cached input tokens
   out of 118.9M recorded input. Other displayed arms report approximately
   44-96% cached input; the cause of ms4's zero is not isolated.
   Fix: a role-fallback adapter in the harness's provider layer, driven by a
   per-model capability, instead of the bridge hook. Mid-thread developer
   injections are themselves a portability cost.
7. **Token use on small models.** Usage counters are cumulative per session,
   so each session's final counter is used once (results.json snapshot
   2026-10-12 02:29Z). An earlier version of this item summed the counters
   across turns and was inflated.
   - Recorded input per launched session, total/108: q38d27x 0.731M, glm53f
     0.531M, mimo26f 0.835M, q36a3 0.669M, q35d9h 2.130M, ms4 1.101M,
     gr42d8x 1.666M and step37f 2.755M. These exclude unknown usage. q36a3 is
     0.80-1.26 times the three listed comparison rows.
   - The unknown-usage sessions are all nonzero exits: ds41fl 12, qfn38b 14,
     mmm3m 7, step37f 4, gr42d8x 13, ms4 4.
   - Among rc=0 sessions, median recorded input is q36a3 0.506M; q38d27x
     0.650M, glm53f 0.354M and mimo26f 0.677M; q35d9h 0.838M; gr42d8x 1.192M;
     ms4 0.302M.
   - Nonzero-exit sessions account for 13.5% of q36a3's recorded input, 84.1%
     of ms4's, 74.1% of gr42d8x's and 95.0% of step37f's. These include
     timeouts, argument failures and completion loops. The input specifically
     attributable to completion retries has not been isolated.
   - A fixed prefix or tool-schema cost is **not verified**: the ladder did not
     record per-request prefix or schema tokens.

   Fix: record prefix and tool-schema tokens per request, and completion-retry
   tokens, in the trajectory (SC-EVAL-040 measured a prefix of about 2.5k
   tokens on Luna). End completion loops (item 4).
8. **Capacity saturation and timeouts on small GPUs.** q35d9 on 2xL4 with 64 concurrent
   chains: 54 of 70 finished sessions hit the 3,600 s session timeout. gr42d8 on
   2xL4: 20 of 20 rc=1 at about 47 min each. gr42d8h on 1xH100: KV cache at 92%,
   8 running against 55-110 waiting requests, aborted at 3 of 108 sessions.
   gr42d8x on 4xH100 still had 31 of 108 rc=124 at 3,602 s, and q38d27 on
   1xH100 had 30 of 108. Sources: `LOG.md` rows 18-19 and
   `sc_dogfood/dsg/RUN_LOG.md` 19:40Z. The L4 logs are in GCS only and were not
   re-read for this entry.
   The runner imposed wall-clock timeouts (3,600 s,
   `sc_dogfood/dsg/realtasks/vm/run_session.sh`). Queue saturation was
   observed directly on gr42d8h and reported for the L4 attempts. The causes
   of the other timeout sessions are not isolated: queueing, slow generation,
   loops or stream trouble. The client retried dropped streams in some logs
   ("Reconnecting... (stream disconnected before completion: idle timeout
   waiting for SSE)"), which documents stream problems too.
   Fix: client-side backoff and retry with jitter on overload responses, plus a
   concurrency signal (queue time and time to first token in the trajectory)
   that a runner or user can use to shed load.

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
