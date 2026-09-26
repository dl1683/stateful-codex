# Stateful Codex restart handoff

Updated: 2026-09-26

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
- Last pushed head before this handoff update: `b47e85b470`
  (`fix(stateful): read exact region evidence`).
- The tracked working tree was clean at this checkpoint.
- These untracked experiment directories are read-only and must never be
  modified, staged, deleted, or regenerated:
  - `clients/stateful-codex/eval/results/pramana-ab-2026-09-24/`
  - `clients/stateful-codex/eval/results/codex-vs-droid-2026-09-25/`

The rejected bare-line-range prototype remains only as a recovery artifact on
`checkpoint/stateful-region-routing-unsafe-20260925` at `a6cdb157de`. Do not
merge it.

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

The second trust defect remains open: model-facing record/update schemas still
offer `userConfirmed`, and the generic API still relies on caller-supplied user
provenance. Do not replace this with a cosmetic provenance check. Define the
host-observed user action or receipt that earns `userConfirmed`; model tools
must not self-award it, and model revisions of confirmed meaning must either
retain an exact valid confirmation or downgrade/reject the grade.

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
overlap and are bounded to 55 minutes. The first complete four-stage smoke
passed. A later script revision writes Codex progress events separately from
the final critique so raw CLI event streams are not fed back as peer analysis.

This closes the narrow changed-authority safety gate. It does not establish
efficient repair, automatic semantic cleanup of every dependent claim, or
freshness behavior at large-corpus scale.

## Remaining blockers and smallest gates

Do not launch another broad benchmark yet. Close these small gates first:

1. **Trusted user confirmation.** Close the remaining `userConfirmed`
   authority gap with a host-bound user action. Generic callers and model tools
   must not be able to manufacture the grade by selecting an enum or claiming
   user provenance.
2. **Derived premise provenance.** Direct source-citation enumeration is now
   implemented, but the frozen repair also needs honest reuse of current
   source-verified blackboard premises. Design bounded revision-pinned premise
   references that distinguish a derived conclusion from direct source
   verification and let changed-premise dependents be enumerated without the
   host making semantic authority decisions.
3. **Ordinary retrieval snapshots.** Root projection and affected-source pages
   have transaction-consistent snapshots. Audit and, where necessary, give
   ordinary blackboard and context-map queries the same consistency guarantee
   before relying on them during concurrent refresh or mutation.
4. **Indexing cost.** Profile publication before optimizing it. Atomicity is now
   correct, but the frozen debug index is still far too slow for a product
   claim. GitHub issue #19 records the current Pramana footprint: 25,096 regions,
   a 107.5 MB database, and roughly 169-248 seconds of debug indexing.
5. **Interrupted refresh.** Add a deterministic canary for transient scan/read
   failure so reconciliation cannot silently mark an unread file missing.
6. **Thread-view continuity.** Repeat the now-passing compaction mechanism with
   a fresh thread attached to the same project, because threads must not be
   project-memory boundaries.

For every mechanism gate, record answer quality, repeated source ranges, input
and uncached tokens, model requests, tool-output volume, wall time, state writes
and queries, and whether the decisive prior conclusion was actually used.

## Recommended next action

Do not rerun the model yet. First close the smaller authority invariant:
`userConfirmed` must be issued only from an exact host-observed user action and
must never be self-awarded by a model or generic caller. Falsify both creation
and revision paths and preserve legitimate user instructions. Then design the
smallest honest revision-pinned dependency contract for derived knowledge and
test it with the changed-amendment case plus an unchanged-source premise. Only
after those deterministic contracts survive review should a small Luna replay
test whether the repair trajectory actually becomes shorter.
