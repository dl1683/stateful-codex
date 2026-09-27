# Stateful Codex restart handoff

Updated: 2026-09-27

## Mandatory restart order

1. Read `STATEFUL_CODEX_PRODUCT_INTENT.md` in full before making any product,
   architecture, UI, or implementation decision.
2. Read the final two sections of `STATEFUL_CODEX_BUILD_PLAN.md`, beginning at
   `Open design problem: trusted reuse and state-aware compaction`.
3. Read this handoff and refresh GitHub issues #15, #16, and #17.
4. Reconcile this document with live Git, source, tests, and processes. This is
   a restart aid, not authority over newer evidence.

## Product priority

Issues #15, #16, and #17 are one coupled intelligence loop:

```text
source work
  -> precise semantic capture
  -> continuity across turns, threads, and compaction
  -> active retrieval when captured understanding matters
  -> exact routing for genuinely missing or consequential detail
  -> point-of-use authority and freshness checks
  -> less rereading, fewer requests, and better decisions over time
```

SC-EVAL-032 showed why this remains the priority. Stateful used 2.0x the input
tokens, 2.7x the uncached input, and 2.3x the tool-output characters of ordinary
Codex while aggregate quality was effectively level. The stateful/ordinary cost
ratio worsened from 1.72x in q01-q05 to 2.58x in q06-q10. q06 had a decisive
finding in state but did not use it; q10 lacked a decisive fact in state and the
old file-only context map could not route to its body lines.

Do not treat a database, prompt packet, component test, or faster search as the
product outcome. The gate is a better longitudinal work trajectory.

## Git and protected state

- Development branch: `feature/stateful-codex`
- Published branch: `stateful/main`
- Current branch head: `19222f07f331`; the migration/backfill, corrected
  trajectory, and durable-measurement work described below is still in the
  working tree. The session filesystem allows source edits but not writes to
  `.git`, so `git add` cannot create `index.lock`; do not describe this state as
  committed or pushed.
- These untracked experiment directories are read-only and must never be
  modified, staged, deleted, or regenerated:
  - `clients/stateful-codex/eval/results/pramana-ab-2026-09-24/`
  - `clients/stateful-codex/eval/results/codex-vs-droid-2026-09-25/`

The rejected bare-line-range prototype remains only as a recovery artifact on
`checkpoint/stateful-region-routing-unsafe-20260925` at `a6cdb157de`. Do not
merge it.

## Pause handoff checkpoint (2026-09-27)

The current working tree is a coherent continuation of `19222f07f331`, not a
finished release. It contains four coupled implementation slices:

1. migration and first-run rollout-backfill reliability, including line-ending
   equivalent migration checksums, retryable insert-only seeding, filesystem
   fallback while incomplete, and owner-token fencing;
2. app-server-owned turn trajectory accounting and durable per-turn Stateful
   measurements, including arrival-order-independent attribution/terminal
   merging, graceful-shutdown draining, stable pagination, exact turn-local
   provider usage, and the experimental `statefulMeasurement/list` and
   `statefulMeasurement/summary` methods;
3. headless output cleanup that removes exactly one Core duplicate for each
   startup warning already emitted during app-server initialization; and
4. a browser “Measured work” surface that exposes the bounded summary without
   presenting partial coverage as all-time data or inventing monetary cost.

The final browser review found two correctness defects and both are fixed in
the working tree. A state-change event received during an active workspace
refresh now queues exactly one follow-up refresh, so the last persisted
measurement cannot remain hidden behind an older in-flight request. A summary
with no terminal trajectory now renders trajectory totals as unavailable rather
than measured zero; a partially merged summary labels its trajectory values as
a recorded subtotal.

Fresh handoff validation after those fixes:

- `node clients/stateful-codex/test/refresh-policy.test.mjs`: 2/2 passed;
- `node clients/stateful-codex/test/workspace-view.test.mjs`: 5/5 passed;
- `just test -p codex-stateful-runtime`: 5/5 passed;
- `just test -p codex-state -p codex-rollout`: 331/331 passed;
- `just test -p codex-app-server-protocol`: 310/310 passed, one configured
  skip;
- `just test -p codex-exec`: 127/127 passed;
- focused app-server terminal-before-attribution merge: passed;
- focused public active-run measurement persistence/query integration: passed;
- focused content-free trajectory notification integration: passed;
- `just fmt` passed with `UV_CACHE_DIR` redirected to the repository-local
  cache; and
- `git diff --check`: passed before this documentation update and must be run
  once more after the final handoff edit.

Do not inflate this evidence. A normal-home live smoke is still required to
prove startup recovery, non-zero trajectory counters, exact turn-local usage,
and agreement between the raw rollout and both public measurement methods. The
repository-wide Rust suite was not run because it remains approval-gated. A
fresh rendered browser pass was not available in this session. GitHub issue
refresh and publication were also blocked during preparation by the sandbox's
forced `127.0.0.1:9` HTTPS proxy. A final `git add -u` attempt also failed with
`Unable to create '.git/index.lock': Permission denied`. Therefore HEAD and the
published `stateful/main` ref remain at `19222f07f331`; the validated source
work described here is still uncommitted in this checkout. Reconcile the final
commit and remote state against live Git rather than assuming this paragraph is
current, and never stage the two protected experiment directories while doing
so.

When Git metadata and network access are writable again, publish only after
rechecking the diff. `git add -u` stages the tracked product changes without
touching the protected untracked directories. Then add only these untracked
implementation paths:

```text
codex-rs/app-server-protocol/schema/json/v2/TurnTrajectoryUpdatedNotification.json
codex-rs/app-server-protocol/schema/typescript/v2/StatefulMeasurementSummary.ts
codex-rs/app-server-protocol/schema/typescript/v2/StatefulTurnMeasurement.ts
codex-rs/app-server-protocol/schema/typescript/v2/StatefulTurnStatus.ts
codex-rs/app-server-protocol/schema/typescript/v2/TurnTrajectory.ts
codex-rs/app-server-protocol/schema/typescript/v2/TurnTrajectoryUpdatedNotification.ts
codex-rs/app-server-protocol/src/protocol/v2/turn_trajectory.rs
codex-rs/app-server/src/stateful_store.rs
codex-rs/app-server/src/stateful_store_tests.rs
codex-rs/app-server/src/turn_trajectory.rs
codex-rs/app-server/src/turn_trajectory_tests.rs
codex-rs/exec/src/startup_warning_deduper.rs
codex-rs/exec/src/startup_warning_deduper_tests.rs
codex-rs/exec/tests/suite/startup_warnings.rs
codex-rs/rollout/src/state_db_backfill.rs
codex-rs/state/migrations/0056_backfill_owner_token.sql
codex-rs/stateful-runtime/migrations/0004_turn_measurements.sql
codex-rs/stateful-runtime/src/measurement.rs
codex-rs/stateful-runtime/src/measurement_storage.rs
```

Review `git diff --cached --check` and `git diff --cached --stat`, commit the
coherent handoff, then push `feature/stateful-codex` to `stateful/main`. Do not
use `git add .`, `git add -A`, or a directory-wide add under
`clients/stateful-codex/eval/results`.

## Authentication incident resolved

On 2026-09-26, another Codex run reached the ChatGPT Codex responses endpoint
but received `401 Unauthorized: Incorrect API key provided`, with a redacted
service-key prefix. The user completed `codex login` again.

The host had no `OPENAI_API_KEY`, `CODEX_API_KEY`, `CODEX_ACCESS_TOKEN`, base
URL, alternate `CODEX_HOME`, or workload-identity override at process, user, or
machine scope. The current auth cache was updated by the login, identified
itself as `chatgpt`, contained usable access and refresh tokens, and had no
non-empty legacy API key. User configuration also forces the `chatgpt` login
method.

The failing interactive Codex pair had been running since 2026-09-25, before
the refreshed login, and retained the obsolete API credential in memory. Only
that exact pre-login process pair was stopped. This current Codex session and
the managed app-server daemon, both started on 2026-09-26, were left running.

Recovery was proven from a separate newly started `codex-cli 0.157.1` process:
`codex login status` reported `Logged in using ChatGPT`, and a real read-only
`codex exec --json` model request completed with the exact response `AUTH_OK`.
The issue was stale process state, not a current global credential or Stateful
Codex launcher leak. Any other Codex process that was already alive before the
login must be restarted; newly started processes use the ChatGPT login.

## Exact-region routing work now pushed

The current sequence after the original region experiment is:

1. `3fd48bdf16 fix(stateful): preserve context routes under scan limits`
   - Exhausting the region budget no longer erases later file routes.
2. `9a27507d90 feat(stateful): consume fingerprint-bound evidence routes`
   - Context-map routes carry the fingerprint-bound range consumed by evidence
     reads instead of passing bare coordinates.
3. `e0c66eb5f7 test(stateful): reject shifted context routes end to end`
   - An edit that shifts lines between query and read fails closed and cannot
     issue a receipt for unrelated content.
4. `4242a7a9df fix(stateful): keep region routes truthful and stable`
   - Regions split at both 64 lines and 4 KiB of searchable text.
   - A route never claims a complete range whose matched text was truncated.
   - Cell identity is stable across ordinary growth/shrink while the exact line
     anchor remains mutable source state.
5. `f9b3e37602 fix(stateful): publish each indexed file atomically`
   - File node, file context entry, region nodes, region entries, and retirement
     of old cells publish in one SQLite transaction.
   - Missing-file lifecycle updates file and regions atomically.
   - A forced mid-publication failure proves the complete previous generation
     remains visible and the failed generation remains absent.
6. `e90eaed41d fix(stateful): keep context routes fast and diverse`
   - FTS ranks a bounded candidate page before hierarchy validation, avoiding
     the prior full join/rank scan.
   - Bounded paging skips prior-generation regions rather than allowing them to
     hide a current route.
   - Per-source and top-level-directory limits prevent one file or review swarm
     from monopolizing the result set.
   - Multi-token queries use literal tokens rather than broad prefixes; a
     single discovery token retains prefix matching.
   - Region descriptions no longer duplicate per-token routing-term rows.
7. `d5b35490c6 test(stateful): prove exact region knowledge reuse`
   - At the app-server/model tool boundary, a region query returns a canonical
     fingerprint-bound route, `evidence_read` reads that exact cell, the model
     records source-verified knowledge with the host receipt, and the same
     region query reports the linked root knowledge.
8. `f1e7002acd feat(stateful): center route previews on query matches`
   - Query results return a UTF-8-safe preview bounded to 240 bytes and centered
     on the body window covering the most query terms.
   - Filename/path matches do not hide a later matching fact in the same cell.
   - Refresh inventory retains the stable leading summary because it has no
     query-specific match.

Focused validation before the final `just fix` and `just fmt`:

- `just test -p codex-project-intelligence`: 37/37 passed.
- `just fix -p codex-project-intelligence`: passed.
- `just fmt`: passed.
- Targeted `codex-app-server` region route/read/record/requery integration test:
  passed.
- `just test -p codex-stateful-extension`: 24/24 passed after the preview
  change; the targeted app-server test also proved a fact near the end of a
  64-line region appears within the 240-byte model-visible preview.

No full Rust suite was run; it still requires explicit user approval.

## Frozen q10 mechanism evidence

The exact two `context_map_query` inputs were recovered read-only from the
protected q10 rollout:

1. `STRATEGY_MAP earliest missing link final artifact end-to-end proof certificate integration`
2. `PLAN.md Production integration merge winning src main proof migration next action`

Measured frozen corpus:

- 1,537 indexed files and 25,096 regions.
- Source scan alone: 12.362 seconds.
- SQLite database: 107,503,616 bytes at the measured checkpoint.
- Full debug indexing completed but varied from about 169 to 248 seconds. This
  remains too slow and was confounded by local build/system contention; do not
  claim an indexing improvement yet.
- Before the FTS pre-limit, the two exact queries took about 50.107 and 27.998
  seconds.
- After the pre-limit, max-10 probes took about 111.6 and 71.3 milliseconds.
  Later max-20 probes were about 145-180 milliseconds.

Recall is mixed and must be described precisely:

- Query 1 did not put the decisive passages in the top 10. With max 20, the
  decisive `PLAN.md:116-168` route ranked 15 and `SCORECARD.md:48-76` ranked 19.
- Query 2 put those same exact routes at ranks 2 and 6, so the actual two-query
  q10 trajectory can reach both facts within a bounded top 10.
- In raw FTS for query 1, the decisive passages ranked 50 and 65. The words in
  the broad query do not identify the later project conclusions. Artificially
  promoting those two cells would be benchmark-specific overfitting.

The conclusion is not “q10 solved.” Exact routing and latency are materially
better, and the second real query recovers both facts. The first query still
demonstrates why accumulated blackboard understanding and query refinement must
drive routing: text ranking cannot invent a strategic relationship absent from
the query and state.

## Small behavioral reuse canary

A two-turn, read-only Luna canary on a fresh isolated copy of the licensing
fixture passed on 2026-09-26. It used one continuous thread, disabled host
memories, retained ChatGPT authentication, and captured a project-state
artifact after each turn.

- Turn 1 asked for the controlling royalty as of 2026-03-15. The model made
  five exact evidence reads, correctly concluded 6% of net sales effective
  2026-03-01, and persisted current source-verified knowledge. The artifact
  reached intelligence revision 45 with two current root entries and four
  evidence routes.
- Turn 2 asked the same unchanged question and explicitly told the model to
  start from verified project intelligence. It inherited revision 45, queried
  active blackboard state, made zero raw-source or evidence reads, produced the
  same correct answer, and left intelligence revision 45 unchanged rather than
  duplicating state.
- Model responses fell from 5 to 3, tool calls from 4 to 2, and source reads
  from 5 to 0. Input tokens fell from 104,781 to 87,787; uncached input fell
  from 27,981 to 7,659 (72.6%); output fell from 2,283 to 753; and measured
  model-turn duration fell from 52.8 to 20.8 seconds.
- State inheritance and the source-evidence assertion both passed after each
  canonical turn. Both runs completed durably with no rejected tool result.

This is directional evidence for the basic reuse policy, not a broad product
claim. The follow-up prompt explicitly cued state-first behavior, the fact was
root-promoted, the source did not change, and the fixture is small. A stricter
gate therefore removed the explicit cue and root promotion.

That stricter two-turn Luna canary also passed. Turn 1 persisted one complete
source-verified conclusion at `executed-amendment-1.md` file scope: the records
audit is quarterly and binding under an executed amendment, not merely
proposed. The root remained empty. Turn 2 asked the same authority/frequency
question without mentioning memory, state, reuse, citations, or rereading. It
queried active blackboard state, made zero source reads, answered correctly,
and left intelligence revision 43 and entry revision 1 unchanged.

- Model responses fell from 7 to 3, tool calls from 6 to 2, and source reads
  from 1 to 0.
- Input tokens fell from 135,455 to 75,342; uncached input fell from 26,399 to
  6,478 (75.5%); output fell from 1,422 to 592; and measured model-turn
  duration fell from 41.4 to 18.0 seconds.
- The follow-up began at the preceding intelligence revision, preserved the
  exact non-root entry and evidence link, completed durably, and made no
  redundant state write.

This closes the small root and deeper-state behavioral reuse gate. It still
does not establish compaction continuity, changed-source behavior, performance
on a large corpus, or an aggregate advantage over ordinary Codex.

The first disposable attempt was falsely invalidated by an evaluator phrase
that crossed a source line break (`six\npercent`). Commit `66f9831b31` now
normalizes whitespace in evidence assertions and covers the wrapped-source
case. This was an evaluation defect; the underlying attempt had already
created source-verified entries and evidence links.

A second disposable deeper-state probe exposed another evaluator defect: raw
rollouts correctly emitted project revision deltas `0 -> 42 -> 43`, but the
summarizer carried only the original full packet at revision 0 into the next
turn. Commit `4a5558fbda` now folds `<stateful_project_update>` deltas into the
effective project state. The frozen rollout then passes inheritance: turn 1
ends at revision 43 and turn 2 begins at revision 43. The model had in fact
queried the non-root entry; the earlier inheritance failure was not a product
continuity failure.

The branch CLI was rebuilt from the current source. Rebuilding the code-mode
companion in the same command could not download the prebuilt V8 archive on
this host, but the already-present companion executed every canary tool call
successfully. Treat a future companion rebuild as an infrastructure task, not
as evidence about Stateful behavior.

## Compaction continuity canary

A deliberately small q02 -> compaction -> q06 mechanism canary passed on
2026-09-26. It used a five-file synthetic UART certificate corpus modeled on
the reset-boundary failure observed in the protected Pramana evidence. The
project copy was isolated and read-only, host memories were disabled, Luna ran
at medium effort, and automatic compaction was forced at a low token limit.

The q02-style turn compared production RTL, the formal bridge, firmware entry,
and certificate scope. It persisted five current, source-verified root findings
at intelligence revision 35, including the decisive contrast: production
resets both synchronizer stages low, while the formal model initializes both
high and has no production reset branch. It completed durably and passed the
source-evidence assertion. Two canonical compactions occurred during that
turn; the post-compaction project packet contained all five findings and their
current evidence routes.

The q06-style prompt was intentionally uncued:

> What would a skeptical certificate reviewer attack first in RX-EXACT?
> Construct the strongest concrete attack and explain why it is stronger than
> the excluded host-throughput concern. Cite exact files and lines. Do not edit
> files.

The follow-up began at the exact preceding revision 35, retained 16 hierarchy
nodes, 10 context-map entries, all five blackboard entries and revisions, and
all current evidence. It made zero read-bearing calls, blackboard queries, or
context-map queries because the relevant root findings were already injected.
It selected the reset-to-first-frame production/formal mismatch as the first
and strongest attack, explained that it occurs before the explicitly excluded
host/FIFO boundary, preserved the caveat that the reviewed sources alone do
not prove an unconditional extra push, and completed durably without writing
duplicate state. Two more canonical compactions occurred during the follow-up.

Directional trajectory measurements were:

- model responses: 8 -> 4;
- tool calls: 5 -> 1;
- read-bearing calls: 1 -> 0;
- input tokens: 167,724 -> 87,480;
- uncached input tokens: 43,564 -> 17,848;
- output tokens: 11,207 -> 4,837; and
- measured runner duration: 218.5 -> 97.4 seconds.

This closes the narrow semantic-capture-through-compaction mechanism gate. It
does not establish a broad quality or cost advantage: the corpus had only five
files, findings were root-promoted, the source was unchanged, the thread was
continuous, compaction was forced aggressively, and there was no ordinary
baseline arm. Its value is causal and architectural: an uncued later decision
was controlled by source-verified project state after four real compactions
without source rereading.

Two disposable precursor attempts produced the same substantive finding but
were rejected by an over-specific bag-of-words assertion because the model used
semantically equivalent phrases such as `different`, `unsound`, and `do not
agree`. The final gate asserts the stable invariant instead: one current
source-verified entry must co-locate production, formal, reset, low, and high,
and must cite the production lines containing both synchronizer registers and
their low reset assignments. Treat exact-synonym evidence assertions as an eval
fragility, not as product evidence.

## Changed-authority canary

A two-turn Luna canary passed the narrow changed-authority safety gate on
2026-09-26. The isolated project contained a base policy with an executed
threshold of 10, a draft amendment with a threshold of 6, and an assessment
count of 8. Turn 1 correctly concluded that launch was permitted because the
draft amendment was not binding, persisted four source-grounded entries, and
ended at intelligence revision 25.

Between turns, the base policy remained byte-identical and only the amendment
changed: it became executed effective 2026-03-01 and expressly replaced the
base-policy threshold with 6. The second user prompt did not announce a source
change. It asked for the controlling authority, threshold, and assessment as of
2026-03-15, and for an explanation of any prior conclusion that no longer
controlled. Before the first substantive response, the point-of-use freshness
audit advanced the project from revision 25 to 26 and marked the old root
decision and amendment route stale. The model then read the decisive changed
source, correctly concluded that launch was not permitted because 8 exceeds 6,
explicitly superseded the old root decision, and completed at revision 35.

The frozen canonical rollout therefore proves the mechanism that matters:

- an unchanged old citation did not make the old conclusion safe to reuse;
- the newly controlling source was detected before the answer, even though the
  user did not announce the change;
- the prior root decision became stale and was superseded by a current,
  source-verified decision; and
- the final answer identified the new authority and explained why the old
  conclusion no longer controlled.

The trajectory was safe but inefficient. Turn 1 used 157,121 input tokens,
40,129 uncached input tokens, 7,664 output tokens, eight model responses, five
tool calls, two compactions, and 161.0 seconds. Turn 2 used 320,041 input tokens,
52,265 uncached input tokens, 14,495 output tokens, 14 model responses, nine
tool calls, four compactions, and 288.6 seconds. It made two blackboard queries,
two context-map queries, and one read-bearing outer call. One completion was
rejected because supersession advanced the old entry from revision 1 to 2 while
the model still submitted revision 1; the model recovered on retry. This is a
tool-contract and repair-efficiency signal, not a correctness failure.

Some non-root cleanup debt also remains. The earlier draft-status entry is
still active in raw storage even though its evidence fingerprint is stale, and
the earlier conditional base-authority fact remains active because its cited
bytes did not change. The root decision is safely superseded and queries expose
freshness, so this did not corrupt the answer, but mature project intelligence
should make dependent stale claims easier to find and repair without forcing a
long exploratory trajectory.

The eval summarizer initially misreported the second turn as beginning at
revision 25. The raw rollout showed a `world_state` snapshot at revision 26
before the first agent response; a pre-prompt compaction usage record had been
mistaken for that response. The evaluator now folds canonical `world_state`
root revisions into the effective state and ignores pre-prompt usage when
locating the first response. State inheritance permits a monotonic pre-response
advance caused by freshness reconciliation but still rejects revision
regression or a starting revision newer than the captured artifact. Frozen
replay now reports revision 25 at turn start, 26 at first response, and 35 at
completion, with all prior hierarchy, context-map, and blackboard records
retained.

A response-by-response decomposition of the frozen repair trajectory identified
two concrete tool-contract costs:

- The first context-map query already isolated `amendment.md` as stale and the
  other three controlling sources as current, but a stale hit did not expose a
  copyable refresh locator. The model queried again and then reread all four
  sources even though only the amendment had changed.
- `blackboard_update_batch` returned the old decision at its new superseded
  revision 2, but completion instructions allowed historical selections only
  from `blackboard_query`. The model first submitted the obsolete revision 1,
  was rejected, queried the same historical entry again, compacted, and retried
  completion with revision 2.

Three bounded fixes are now pushed:

- `f2fbe0a390` adds `refreshInput` to stale context-map hits. It is directly
  consumable by `evidence_read`, refreshes only the changed file, withholds the
  invalid guarded route, and tells the model to reuse adequate current
  knowledge rather than reread it. The Stateful extension suite passed 24/24;
  an app-server integration proves the stale route exposes the refresh input
  while the shifted guarded route remains rejected.
- `74bc2e949b` makes successful supersede/retire mutations return a copyable
  `historicalFinding` at the new revision. Run World State and completion tool
  instructions now direct the model to use that result without another query.
  The Stateful extension suite passed 24/24, and a real app-server tool
  integration proves superseding revision 1 returns the exact historical
  selection at revision 2. Scoped Clippy and formatting passed; the pre-existing
  `RootBlackboardStatus` large-enum warning remains.
- `a78f394b8b` adds direct changed-source dependency enumeration. A file-route
  seed expands to the file plus its current and retired region routes, so a
  refreshed file can still find active entries citing an older region route.
  Results contain current active entry revisions and direct-citation freshness,
  support lifecycle scope and stable-ID pagination, and are materialized in the
  same SQLite transaction that selects them. The first page returns the current
  project-intelligence revision; continuation requires that revision and fails
  closed if project state changes. Unknown, non-source, or cross-project seed
  route IDs are rejected instead of producing a misleading partial answer.
  Pages omit evidence locators and relations, report their counts explicitly,
  and byte-budget the complete response including the continuation cursor.
  Storage tests cover two-page traversal and both invalid-seed and
  concurrent-revision failures. A real app-server integration proves that a
  changed file route finds a stale active finding whose direct citation used a
  guarded region route. The project-intelligence suite passed 37/37, the
  Stateful extension suite passed 24/24, and the targeted app-server integration
  passed. Scoped Clippy and formatting passed; the same pre-existing large-enum
  warning remains.

These changes give a six-call direct repair path in place of the frozen
nine-call path: one route query, one changed-source read, one affected-knowledge
query, one coherent record batch, one lifecycle update, and one completion.
That is a displacement theory, not a measured cost result. No new model run has
yet shown that Luna follows the shorter path.

The affected-source query is deliberately mechanical. It reports entries with
direct citations to the selected file/regions and their current freshness; it
does not infer that an unchanged-source conclusion semantically depends on a
changed authority. The model still decides whether each result should be
revised, superseded, retired, or retained. A stale citation that remains true is
not itself a reason to supersede a finding. Revision-pinned provenance between
derived conclusions and premise entries remains a separate design problem.

Independent Astra, Claude Code, and Droid reviews challenged this slice before
it was finalized. Their concrete findings drove region-route closure, bounded
pagination, transaction-consistent materialization, compact pages, cursor byte
budgeting, project-revision pinning, invalid-seed rejection, and the explicit
direct-citation boundary. They independently agreed that semantic dependency
provenance should remain a separate next layer rather than being inferred by the
host.

The same review exposed a defect in longitudinal evidence-read measurement,
now fixed through `649a89e116` and `7f94840695` and closed as GitHub issue #18.
The evaluator pairs tool results with their originating calls and normalizes
completed reads to a source fingerprint plus exact returned range. Guarded
routes remain attributable as attempts from their route identity when a result
is unavailable; distinct ranges in one file stay distinct; explicit and guarded
forms for the same resolved read share one identity; and unresolved or failed
calls remain attempts rather than being counted as completed reads. Dynamic
code-mode calls are reconciled with every returned evidence object, so one
syntactic invocation that performs multiple reads is no longer undercounted.
Truncated reads contribute one completed record for the exact returned subset.
Exact-repeat accounting is longitudinal: it distinguishes prior-turn repeats
from duplicates within the current turn. All 41 Stateful client tests passed. A
read-only replay of the protected ten-turn SC-EVAL-032 rollout produced 178
completed reads: 143 new exact identities, 22 prior-turn repeats, and 13
within-turn repeats, plus 2 failed and 7 unresolved syntactic attempts; 6
attempts had no concrete identity. That difference means the earlier
syntactic-call reread counts must be regenerated before reuse; no protected
result was modified. This is a telemetry repair, not evidence that any repeat
was unnecessary or that Stateful is cheaper.

The first recurring Droid review also found that root projection assembled
counts, selected entry IDs, per-entry data, and the reported project revision
through separate autocommit reads. A concurrent mutation could therefore pair
aliases from one snapshot with the revision of another. `c4067d433b` now builds
the complete root projection in one SQLite read transaction, matching the
snapshot boundary used by affected-source enumeration. The focused
project-intelligence suite passed 37/37, scoped Clippy passed, and formatting
passed. GitHub issue #20 is closed. This removes the source-confirmed mixing
mechanism; it does not claim that a mixed projection was observed in a real run.

That review also found two public trust/verification defects. Generic
`blackboard/upsert` previously accepted a caller-selected `sourceVerified`
grade without the host-issued read receipt required by the model tool path.
Commit `f0c9366f50` rejects that grade at the generic API boundary. Commit
`9d0b2b113e` strengthens the public JSON-RPC regression: it uses a real current
indexed route and fingerprint, rejects both create and update attempts, and
proves the rejected update did not advance state because the legitimate
revision-guarded update still succeeds. The targeted app-server test passed,
then scoped Clippy and formatting passed.

The second trust defect is closed across three commits. `44bf6dcc58` removes
`userConfirmed` from model record/update schemas, rejects out-of-schema
self-awards and generic API attempts even when the caller claims user
provenance, and requires a downgrade before changing confirmed meaning.
`6aa45e7f92` adds the experimental `blackboard/confirm` RPC: it accepts only an
exact active entry ID and expected revision, preserves the existing meaning and
evidence, and lets the host issue user provenance and the confirmed grade.
`59ea293d75` exposes that exact action in the browser workspace. The client
cannot submit replacement content, evidence, verification, or provenance; it
refreshes after confirmation and removes the action once the effective grade
is `userConfirmed`. The client suite passed 42/42. A real-browser check proved
the exact RPC payload, the post-confirm badge and action removal, and no console
errors. The maximum-reasoning peer review then forced an explicit trust-boundary
audit: neither code mode nor any model-facing tool references the confirm RPC,
and the normal restricted sandbox blocks loopback network access. Confirmation
is authority delegated to a trusted app-server client, not cryptographic proof
against a process granted arbitrary host control; such a process could also
edit the state database directly. The protocol and local-client documentation
state this boundary explicitly. GitHub issue #23 was reopened during that audit
and its acceptance gate is complete only under this now-recorded limitation.

Commit `b47e85b470` closes the browser exact-evidence break recorded as GitHub
issue #22. `evidence/read` now reconstructs the guarded route from the current
indexed region, rejects a supplied range that differs from the typed anchor,
reads through the fingerprint-checked evidence reader, and preserves the
region anchor in its response. Public JSON-RPC coverage passed for the exact
region and mismatch paths. The existing file-level bounded/stale-source test
also passed after its stale expectations were corrected to include the region
node and route already produced by the indexer. Scoped Clippy and formatting
passed; no full Rust suite was run.

An hourly read-only peer-review loop is installed outside the repository as the
Windows Scheduled Task `StatefulCodex-Hourly-Droid-Review`. Its script is
`C:\Users\devan\.codex\automations\stateful-droid-hourly\run-review.ps1` and
its timestamped artifacts are under
`%LOCALAPPDATA%\StatefulCodex\hourly-droid-reviews`. Each cycle runs a broad
maximum-reasoning Droid repository/issue review, a Codex source-grounded
critique, a Droid rebuttal, and a final Codex decision record. Runs cannot
overlap and are bounded to three hours. Every run reviews a detached worktree
of the committed head, so concurrent implementation edits cannot invalidate
the review snapshot. Each Droid stage has bounded retries and the worktree is
removed even after failure. The first complete four-stage smoke passed. A later
script revision writes Codex progress events separately from the final critique
so raw CLI event streams are not fed back as peer analysis.

At the latest checkpoint the scheduler remains enabled, but Factory rejected a
fresh CLI probe before inference with HTTP 402: both the standard and Droid
Core five-hour usage windows were exhausted and Extra Usage was not active for
the authenticated CLI account. Droid was updated to `0.228.0`; the same
account-side response remained. The task will retry on its next hourly trigger
and needs no repository change when the usage window resets.

The first maximum-reasoning cycle completed all four stages successfully and
produced the decision record under
`%LOCALAPPDATA%\StatefulCodex\hourly-droid-reviews\20260926-110328`. It forced
the explicit confirmation threat-model audit above and identified repeated
same-file hashing as a smaller deterministic cost mechanism. Commit
`fc671dedb9` now reuses one live source check within each root audit or
point-of-use observation when file and region routes share the same indexed
source location. Every route still compares the live result against its own
stored fingerprint and reconciles its own node. The focused regression proves
one file route plus one real region route hashes exactly one file length; all 26
Stateful extension tests passed, followed by scoped Clippy and formatting. This
does not solve refresh/index publication cost or cross-call hashing, so GitHub
issue #19 remains open.

Three commits now close the next deterministic authority gap. `b37d91d360`
adds bounded semantic-premise links from one blackboard entry revision to exact
revisions of other entries. Premises are not ordinary `dependsOn` navigation
relations, do not count as direct source evidence, and never promote a derived
entry to `sourceVerified`. They preserve which trusted findings a conclusion
actually depended on. Storage rejects self-links, missing, inactive, stale, or
non-current premise revisions. Freshness walks the revision-pinned dependency
graph, and changed-source enumeration recursively includes current conclusions
whose unchanged direct source depends on an affected premise. The focused
regression proves that a changed amendment makes an unchanged-source conclusion
stale and discoverable while its direct citation remains current.

`cae829c436` makes that contract usable by the model. Record and update tools
accept at most 16 exact `{entryId, revision}` premises. Before reuse they audit
the premise's direct and transitive source-verified evidence against live
filesystem bytes, rather than trusting only the last indexed fingerprint. Root
context and blackboard queries expose premise identity and freshness, and
completion fails closed when a material root finding has stale, unavailable, or
unchecked premise support. Updates preserve premise provenance when omitted;
confirmed meaning still requires an explicit downgrade before it can change.
All 27 Stateful extension tests and all 38 project-intelligence tests passed.

`00e61132de` exposes premise provenance through the experimental app-server v2
blackboard API and regenerated Rust, TypeScript, Python, and schema artifacts.
The public integration covers a current user-confirmed premise, a dependent
derived entry, the premise's later revision/downgrade, and the resulting stale
premise/effective-verification response. All 310 app-server-protocol tests
passed with one skipped test, and the targeted app-server integration passed.
Scoped Clippy and formatting passed for all affected crates. The generic API
reports and validates the transactionally stored project-intelligence state;
only the model-facing record/update/completion paths have project roots and
therefore perform the stronger live-filesystem audit. Do not describe a generic
API query as proof that source bytes were re-read at query time.

The recurring review's latest decision was written against `b37d91d360` because
its local shell could not start. Its main falsification target—premise authority
across storage, model query/root/completion, and the public API—is now covered by
the commits and tests above. Its still-valid next criticism is that ordinary
blackboard and context-map queries select candidates and materialize hits across
multiple autocommit reads. Root projection and affected-source enumeration
already use one read transaction; ordinary retrieval must receive the same
snapshot guarantee before concurrent refresh behavior is trusted.

Commit `c6266e5559` closes that snapshot gap. Ordinary blackboard search now
validates scope, selects candidate IDs, and materializes full entries,
relations, direct-evidence freshness, and premise freshness in one SQLite read
transaction. Context-map search does the same across every FTS candidate page,
diversity selection, and hit load. Project listing, multi-hit path lookup,
multi-statement entry/hit loads, and guarded-hit validation also use one read
snapshot. Single-statement aggregate reads remain single statements. Two
deterministic WAL regressions select candidates, commit a concurrent source
mutation, prove the open reader still materializes the pre-mutation/current
view, then prove a new query sees the changed/stale view. All 40
project-intelligence tests passed, followed by scoped Clippy and formatting.
This proves the storage snapshot boundary, not application-level freshness of
the indexed data.

Commit `dc51e7b320` closes the queued interrupted-refresh canary without adding
a second refresh state machine. The existing scanner already marks an inventory
incomplete after any walk or file-read error, and missing-file reconciliation
runs only for a complete inventory. The new deterministic test injects a
`PermissionDenied` file read through that scanner, obtains one skipped file and
an incomplete/truncated inventory, then runs the normal publication phase. It
proves no file is marked missing, the previously published hierarchy node is
unchanged, and the last complete route remains current and queryable. All 41
project-intelligence tests passed, followed by scoped Clippy and formatting.
This protects the last published generation from a transient unread file; it
does not make a genuinely deleted file distinguishable from every host-specific
filesystem anomaly outside the scanner's error contract.

The first controlled index-cost diagnostic then falsified an overly simple
explanation for issue #19. In temporary local debug fixtures with a fixed 496
regions, initial publication rose from roughly 277 ms at 16 files to 450 ms at
124 files and 828 ms at 496 files; unchanged publication rose from roughly 110
ms to 155 ms and 332 ms. A 160-file/4,960-region fixture measured about 1.26 s
of scan work, 3.43 s of initial publication, and 1.33 s of unchanged
publication. Current code was approximately linear in this bounded range.
Extrapolation remains far below the protected Pramana observation of 1,537
files, 25,096 regions, and 169–248 seconds, so the current per-region SQL loop
alone does not explain that historical result. Source filesystem/OneDrive/AV
behavior, database growth, environmental contention, or the older code path may
dominate. The temporary diagnostic was removed; no indexer rewrite was made on
an unfalsified guess.

Commit `99fb9ce290` makes the next real refresh diagnostic. Project-wide and
single-file reports now include `regionsIndexed`, `scanDurationMs`, and
`publicationDurationMs`. The bounded model tool and experimental
`contextMap/refresh` v2 response expose the same fields, and the tool response
budget accounts for them. Project-intelligence passed 41/41, Stateful extension
27/27, app-server protocol 310/310 with one skipped, and both affected public
app-server integrations passed. Scoped Clippy and formatting passed; only the
pre-existing root-blackboard large-enum warning remains. Issue #19 stays open
until a real repository refresh records the phase split and index cost is
compared with routes actually used and longitudinal savings.

This closes the narrow changed-authority safety gate. It does not establish
efficient repair, automatic semantic cleanup of every dependent claim, or
freshness behavior at large-corpus scale.

Commit `f2af68d17b` and a two-thread live canary now close the narrow
thread-view-continuity gate. The deterministic app-server integration completes
a project outcome in one thread, opens an independent thread on the same
project, proves that the completed result and learning enter the new model
request, and proves that a private transcript marker does not.

The live run used cached ChatGPT login with API-key environment variables
removed and `gpt-5.6-sol` at high reasoning. First thread
`01a0dec1-0f8a-70b0-a470-97c02d9fdfee` read `DEPLOYMENT.md:L1-L3` once,
promoted the exact conjunctive release gate, and completed correctly. Fresh
thread `01a0dec2-29d8-7382-bd14-d2c7a69740a9` reused the current root finding
and completed outcome, gave the same exact `C7-42` plus passed-rollback answer,
and made no evidence-read, context-map, shell, or code-mode filesystem call.
Its only tool calls queried or mutated durable blackboard/run state through the
code-mode host. The first
and second turns used 149,106 versus 130,142 total tokens, 28,786 versus 15,070
uncached-input-plus-output tokens, six versus five model requests, and 6,630
versus 4,988 bytes of tool output. Wall time was about 34 seconds for each.
This is positive evidence for demonstrated fresh-thread use and zero repeated
source ranges; the 47.64% uncached-token reduction is directional evidence from
one tiny fixture, not a general cost claim.

The first fixture refresh indexed four diagnostic stdout/stderr artifacts
because those files were initially redirected inside the selected directory.
It opened only `DEPLOYMENT.md:L1-L3` as evidence, and the second thread's logs
were outside the project. This makes the token totals conservative and prevents
treating them as a clean benchmark; it does not create an alternative second-
thread source path because that thread performed no filesystem read.

Third independent thread `01a0dec6-c7cc-7603-86d5-61988c48dbdf` removed the
evaluation-specific instruction to persist a continuity meta-finding. It reused
current E1, made no filesystem or context-map call, and needed only one
`stateful_run_update` completion call across two model requests. The turn used
50,457 total tokens, 11,417 uncached input plus output, 2,554 bytes of tool
output, and 17.1 seconds. Versus the cold first turn, total tokens fell 66.16%
and uncached input plus output fell 60.34%. This control falsifies the suspected
need for record/query/promote during ordinary fresh-thread reuse; those calls in
the prior turn followed its explicit evaluation prompt. The result is still a
tiny repeated-answer control over already-rich state, not representative
project-lifetime proof.

The canary also preserves its limitations. The second prompt explicitly asked
the model to persist a continuity conclusion, causing a record/query/promote
sequence that ordinary answer reuse should not need. Failed pre-runs with
`gpt-6-luna` and `gpt-6-sol` were rejected before inference because those model
aliases are unavailable through this ChatGPT-account endpoint. The successful
local CLI reports version `0.0.0` and predates the newest observability/test-only
commits; rebuilding the current branch was blocked before linking by the `v8`
150.4.0 archive download and unavailable local Python fallback. Deterministic
current-source coverage and the older-binary live trajectory are therefore
separate evidence. The live run did not exercise the new index phase fields.

Commit `fb3b60a55a` closes a bounded route-diversity failure from issue #17.
The prior context-map query stopped after collecting `limit * 16` queryable
candidates, so one busy top-level directory could consume the candidate set
before directory diversity was applied. A deterministic counterexample with
160 matching files under `reviews/` and the decisive 161st match under `docs/`
failed before the change: the `docs/guide.md` route was absent.

Context-map search now performs one bounded FTS read of up to
`max_results * 16 * 16` candidates plus one exhaustion-probe row inside the
existing read transaction, then applies source and top-level-directory caps.
The storage result carries an exact `truncated` signal: it is true when another
diversified result exists or when the bounded candidate scan did not exhaust
the search. The model tool maps that signal to `mayHaveMore`, and the
experimental v2 `contextMap/query` response exposes it directly instead of
guessing from `data.len() == limit`.

The 160-plus-one regression now returns the decisive `docs/guide.md` route and
reports `truncated: false` when the search is exhausted; a three-result query
reports `truncated: true`. All 41 project-intelligence tests, all 27 Stateful
extension tests, all 310 app-server-protocol tests with one skipped test, and
the two affected app-server integrations passed. Scoped Clippy and formatting
also passed; only the pre-existing root-blackboard large-enum warning remains.
This proves bounded diversity and truthful exhaustion reporting for the frozen
counterexample. It does not prove semantic ranking quality, representative
large-corpus latency, or lower token cost, so issue #17 remains a broader
trajectory gate rather than a closed product claim.

Commit `b85c1dbe64` closes the exact file-to-region knowledge handoff identified
in issue #17. Before the change, context-map search suppressed a matching file
route when it returned a child region, while `route_knowledge` counted only
blackboard evidence attached to the returned route ID. A source-verified finding
cited to the file therefore produced no `knownKnowledge` signal on its child
region. The new deterministic regression failed with an empty result before the
change.

Route knowledge now treats a file and its direct region children as one physical
source family at the storage boundary. Each returned route receives counts for
active findings linked anywhere in that family; `COUNT(DISTINCT entry.id)` keeps
multi-route evidence from inflating the count, and the hierarchy file identity
prevents cross-file leakage. Exact-route callers retain the same result shape.

The regression passes, all 42 project-intelligence tests pass, and the affected
app-server integration passes after reading and recording evidence from a file
route, then receiving the finding's promoted-knowledge signal on a distinct
child-region result without a second `evidence_read` call. Scoped Clippy and
formatting also pass. This proves the storage and model-tool handoff and a
deterministic no-reread trajectory. It does not prove that an unconstrained live
model will reliably use the signal, that retrieval ranking is broadly adequate,
or that lifetime cost falls on representative work.

Commit `5dc0fd7b14` closes the suppressed audit-transition risk found by the
recurring peer review. The old semantic fingerprint rewrote every
`uncheckedThisTurn` label to `current`. That was harmless for a visible
`sourceVerified` finding because its effective verification also fell to
`unverified`, but it hid the only model-visible change for a `userConfirmed` or
other non-source-verified finding with supplemental evidence, and could hide a
catalog-only route downgrade.

The app-server regression exercises the real next model request. It first loads
a normal evidence file and verifies that the packet reports the user-confirmed
finding and route as current. It then replaces the path with a directory, which
makes the live audit unchecked without changing the stored hierarchy or root
revision. Before the fix, the second request retained no invalidation packet;
after the fix, it contains both `evidence=uncheckedThisTurn` and the route's
`(uncheckedThisTurn)` label.

The fingerprint still normalizes only the project-intelligence revision, so a
pure revision advance keeps its compact update. All 27 Stateful extension tests
pass, the red/green app-server request test passes, and the affected existing
project-context integration passes after correcting a pre-existing stale text
assertion to the production wording already at HEAD. Scoped Clippy and
formatting pass; the pre-existing root-blackboard large-enum warning remains.

Commits `f177cce404`, `197c48ba8c`, and `8884f86384` complete the durable
refresh-health restart and recovery path. Full refresh now persists separate
file-inventory and region-coverage health, counts, skipped and missing paths,
truncation, phase timings, and completion time. Targeted file refresh does not
replace the project-wide status. App-server v2, the model-visible World State,
and the browser expose the distinction; startup retries uninitialized or
inventory-incomplete projects without treating partial region coverage as a
reason for an endless full refresh. Volatile timing fields do not participate
in the semantic fingerprint.

The deterministic recovery test injects an unread source, proves the
incomplete result survives store reopen, restores the source, adds a new file,
and verifies that an ordinary retry both clears the health warning and indexes
the new source. All 42 project-intelligence tests, 28 Stateful extension tests,
310 app-server-protocol tests with one configured skip, 44 browser tests, and
the four affected app-server integrations passed. Scoped Clippy and formatting
passed; the pre-existing root-blackboard large-enum warning remains. No full
Rust suite was run.

Commit `3578c5e5a0` completes the full-refresh publication-ordering gate. Each
full refresh durably claims a per-project generation before scanning. Project,
root, directory, file, missing-file, and refresh-status mutations all verify
that generation under the same SQLite write transaction as the mutation. A
later claim therefore fences every remaining write from an older scan without
turning the entire index build into one large transaction or weakening the
existing per-file rollback boundary.

The regression opens independent stores, pauses the older generation after its
scan, changes the source, completes a newer refresh, and resumes the older
publisher. The older call is rejected as superseded; the newer status remains
unchanged, the newer fact remains queryable, and the older fact is absent. All
43 project-intelligence tests passed, followed by scoped Clippy and formatting.
Targeted single-file refresh remains outside this full-refresh generation
policy and retains its per-file atomic transaction; general live-edit
coordination is still tracked by issue #16.

Commit `dc2dd91aeb` closes the narrow selective-completion-capture gate. The
five-outcome projection remains bounded. Instead of enlarging it or copying
terminal prose into a parallel memory store, completion now rejects any
non-empty `finalObligation.learning` when neither a current root alias nor an
exact historical blackboard revision was selected. The rejection instructs the
model to record and promote the smallest durable conclusion, then retry.
Outcome-only completion remains available when the run declares no reusable
project learning.

The real app-server tool-call regression submits a material threshold learning
with no blackboard finding. The host rejects it before terminal mutation; the
run remains running and the obligation store remains empty. The 28/28 Stateful
extension suite, both completion-guard integrations, the autonomous
continuation integration, and the unselected-provenance integration passed.
Scoped Clippy and formatting passed with the pre-existing root-blackboard
large-enum warning. The invariant is structural, not semantic: the model still
chooses which current or historical finding actually preserves its learning,
so representative trajectory work must measure that choice.

## Remaining blockers and smallest gates

Do not launch another broad benchmark yet. Close these small gates first:

1. **Finish the migration/backfill gate with a live smoke.** The current working
   tree addresses issues #29 and #31 locally: LF/CRLF-equivalent migrations are
   accepted without rewriting stored checksums, substantive mutations still
   fail, historical metadata backfills outside startup, incomplete reads fall
   back to the filesystem, seeding cannot overwrite live rows, failures cannot
   be skipped, and owner-token fencing rejects stale checkpoints/completion.
   Migration 0056 resets legacy completion once for a safe insert-only reseed.
   `codex-state` plus `codex-rollout` pass 331/331 tests. A small normal-home
   smoke remains required before calling the gate closed operationally.
   Re-check issue #30 against the actual runtime migrator before changing
   version-skew behavior: the current runtime migrator already enables
   `ignore_missing`, so a checksum failure must not be mislabeled as ordinary
   forward-version skew.
2. **Validate and expose comparable run records.** The live licensing run
   recorded after `19222f07f3` had correct Stateful attribution and answer
   content but zero model-response, tool, output, and compaction counters. Raw
   app-server events had been suppressed for the ordinary headless client, so
   the earlier evidence claim is invalid and issue #24 remains open. The
   corrective working-tree implementation now counts before raw-event
   suppression, guarantees terminal trajectory delivery, measures
   failed/interrupted invocations, and uses invocation-local resume tokens.
   Active Stateful turns are durably stored with typed attribution, trajectory,
   exact turn-local provider token usage when available, final status, and
   stable completion time. Missing usage remains absent rather than being
   inferred from lifetime thread totals. Independent Astra and Sol reviews
   found and drove fixes for cross-turn binding loss, two-channel arrival races,
   failed-turn status mismatch, notification loss, destructive replay, mutable
   ordering, and shutdown cancellation. Attribution and terminal trajectory now
   merge in either order; graceful shutdown waits for listener drain before
   clearing listeners. The experimental v2 `statefulMeasurement/list` method
   now exposes a bounded, project-scoped series using an opaque project-bound
   cursor and immutable creation ordering. Runtime 5/5, the terminal-first and
   multi-response usage regressions, the public active-run persistence/query
   integration with exact upstream usage, and four schema-fixture checks pass.
   It still needs a fresh live smoke after the migration/backfill gate, derived
   summaries, and authoritative monetary cost. Cost must
   not be fabricated when the provider amount's unit and currency are
   unavailable.
3. **Real index-cost attribution.** The next suitable repository refresh must
   record the new scan/publication split, database size, and used-versus-indexed
   routes. Do not optimize the historical 169–248 second result by extrapolating
   from the small local fixture.

For every mechanism gate, record answer quality, repeated source ranges, input
and uncached tokens, model requests, tool-output volume, wall time, state writes
and queries, and whether the decisive prior conclusion was actually used.

## Recommended next action

The local migration, first-run rollout-backfill, durable turn-record, and bounded
project-summary implementations now pass their focused suites. The experimental
`statefulMeasurement/summary` method aggregates an explicit newest-first window,
reports its terminal and token-usage coverage, and discloses omitted older rows;
it does not invent dollar pricing or failed-request counts. The duplicate
ignored-config warning in headless item output is also removed by an exec-only,
exact one-for-one deduper with focused unit and real CLI JSONL integration
coverage plus the complete `codex-exec` suite. The browser workspace now reads
that summary and presents bounded measurement, trajectory, reuse, status, and
token-coverage evidence without calling it all-time or monetary cost; its focused
render assertions and checked-in snapshot pass 4/4 in-process.

Run one small normal-home headless fixture next and verify startup recovery,
non-zero trajectory counters, exact turn-local token usage, and the merged
persisted turn record against the raw rollout plus both public measurement
methods before restoring any observability claim. The current sandbox cannot
write a normal Codex home, so this operational gate requires a writable ordinary
runtime rather than another component test. Collect issue #19 diagnostics only
on the next suitable real refresh, and do not launch a broad benchmark merely to
obtain either measurement.
