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
- Last product-work head: `5f855b3651`
  (`docs(stateful): close route preview gate`); authentication handoff commits
  follow it.
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
gate must remove the explicit cue, exercise relevant deeper/non-root knowledge,
and then test the stored-but-unused q02 -> compaction -> q06 decision case.

The first disposable attempt was falsely invalidated by an evaluator phrase
that crossed a source line break (`six\npercent`). Commit `66f9831b31` now
normalizes whitespace in evidence assertions and covers the wrapped-source
case. This was an evaluation defect; the underlying attempt had already
created source-verified entries and evidence links.

The branch CLI was rebuilt from the current source. Rebuilding the code-mode
companion in the same command could not download the prebuilt V8 archive on
this host, but the already-present companion executed every canary tool call
successfully. Treat a future companion rebuild as an infrastructure task, not
as evidence about Stateful behavior.

## Remaining blockers and smallest gates

Do not launch another broad benchmark yet. Close these small gates first:

1. **Uncued and deeper-state reuse.** Repeat the small decision canary without
   telling the model to use state first, and use relevant non-root knowledge so
   route-reported coverage must trigger deeper retrieval before any reread.
2. **Semantic capture and active retrieval.** Use q02 -> compaction -> q06 to
   prove the reset mismatch is captured, survives, and changes the later action
   without an unjustified reread.
3. **Changed authority.** A newer controlling source must invalidate confident
   reuse even if the old cited bytes remain unchanged.
4. **Indexing cost.** Profile publication before optimizing it. Atomicity is now
   correct, but the frozen debug index is still far too slow for a product
   claim.
5. **Interrupted refresh.** Add a deterministic canary for transient scan/read
   failure so reconciliation cannot silently mark an unread file missing.

For every mechanism gate, record answer quality, repeated source ranges, input
and uncached tokens, model requests, tool-output volume, wall time, state writes
and queries, and whether the decisive prior conclusion was actually used.

## Recommended next action

Run one stricter uncued, non-root reuse probe to distinguish prompt compliance
from default policy. If it passes, move directly to semantic capture and the
q02 -> compaction -> q06 gate rather than continuing to tune FTS against q10.
