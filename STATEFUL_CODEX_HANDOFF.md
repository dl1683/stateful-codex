# Stateful Codex restart handoff

Updated: 2026-09-25

## Mandatory restart order

1. Read `STATEFUL_CODEX_PRODUCT_INTENT.md` in full before making any product,
   architecture, UI, or implementation decision.
2. Read the final two sections of `STATEFUL_CODEX_BUILD_PLAN.md`, beginning at
   `Open design problem: trusted reuse and state-aware compaction`.
3. Read this handoff and refresh GitHub issues #15, #16, and #17.
4. Do not resume Stateful implementation until the Codex ChatGPT-login blocker
   below is resolved by a successful request from a newly started Codex process.

## Current pause reason: Codex authentication is broken

Stateful product work is deliberately paused. A newly launched Codex process
reports `Logged in using ChatGPT` but a real model request retries and then fails
with HTTP 401 because a service-style API key is being sent to
`https://chatgpt.com/backend-api/codex/responses`.

Evidence collected without printing credential values:

- No `OPENAI_API_KEY`, `CODEX_API_KEY`, `OPENAI_BASE_URL`, organization, project,
  or `CODEX_HOME` override is set at process, user, or machine scope.
- `codex login status` reports ChatGPT login.
- The original saved auth record contained both ChatGPT tokens and a legacy
  API-key field. Logout removed it.
- A normal browser-based `codex login` recreated credentials, but a fresh
  `codex exec` still failed with the same wrong-credential 401.
- The official-documentation connector failed with the same 401, confirming a
  shared authentication problem rather than a project-specific command failure.
- The installed executable is the global npm launcher
  `C:\Users\devan\AppData\Roaming\npm\codex.ps1`, version `0.157.0`.
- `~/.codex/config.toml` currently places `forced_login_method = "chatgpt"`
  inside `[mcp_servers.blackboard]`. Codex explicitly warns that
  `mcp_servers.blackboard.forced_login_method` is unrecognized and ignored. If a
  forced login method is retained, it must be a top-level setting.

The blocker is resolved only when all three checks pass:

1. `codex login status` reports ChatGPT.
2. A brand-new process completes a real no-tool request.
3. That process neither sends an API key to the ChatGPT endpoint nor relies on
   the already-running Codex process's in-memory credentials.

Do not accept login-status output alone as proof.

## Git and recovery state

- Main development branch: `feature/stateful-codex`
- Published product branch: `stateful/main`
- Current safe branch head: `f1bc8b1082`
  (`docs(stateful): prioritize longitudinal intelligence loop`)
- The tracked working tree is clean at this checkpoint.
- Two protected experiment directories remain untracked and must never be
  modified, staged, deleted, or regenerated:
  - `clients/stateful-codex/eval/results/pramana-ab-2026-09-24/`
  - `clients/stateful-codex/eval/results/codex-vs-droid-2026-09-25/`

The rejected, uncommitted exact-line-range prototype was preserved exactly on a
separate remote checkpoint branch instead of contaminating `stateful/main`:

- Branch: `checkpoint/stateful-region-routing-unsafe-20260925`
- Commit: `a6cdb157de`
- Status: recovery artifact only; do not merge as written.

That checkpoint contains changes only to:

- `codex-rs/ext/stateful/src/tools/context_map.rs`
- `codex-rs/app-server/tests/suite/v2/stateful_project_context.rs`

It added `source.lineRange`, instructed the model to pass the range to
`evidence_read`, increased the displayed headline limit, and added a scripted
two-turn transport test. The test passed, but Droid review showed that the
prototype proves JSON plumbing rather than safe route consumption or product
behavior.

## Product priority and evidence

Commit `f1bc8b1082` makes the highest priority explicit in the build plan:
issues #15, #16, and #17 are one coupled intelligence loop:

```text
semantic capture
  -> continuity across turns, threads, and compaction
  -> active retrieval
  -> exact routing for missing detail
  -> point-of-use source authority and freshness
  -> less rereading, fewer requests, and better decisions over time
```

This ordering is constrained by SC-EVAL-032:

- Stateful used 2.0x the input, 2.7x the uncached input, and 2.3x the tool-output
  characters of ordinary Codex while aggregate quality was effectively level.
- The stateful/ordinary cost ratio worsened from 1.72x in q01-q05 to 2.58x in
  q06-q10.
- q06 had the decisive reset-mismatch finding in state but did not use it.
- q10 did not have the decisive routed-result fact in state and could not route
  to the relevant body lines.
- Forty of 81 later evidence reads overlapped an earlier read, but only a small
  fraction of requested lines was represented in promoted knowledge. Retrieval
  ranking alone therefore cannot solve the capture deficit.
- Completed outcomes are bounded deterministic continuity and have a useful q06
  counterfactual, but they should not be enlarged without measured benefit.
- Byte-identical cited lines are not sufficient freshness authority because an
  edited qualifier elsewhere can change their meaning. The range-rebinding
  experiment was correctly reverted.

Issue #15 contains the full evolving evidence and the planning checkpoint:
https://github.com/dl1683/stateful-codex/issues/15

## Pushed routing work before the pause

1. `710d3ec6cb fix(stateful): report context coverage honestly`
   - A file is complete only when the searchable representation is complete,
     not merely because all bytes fit the scan excerpt.
2. `777dfb2901 feat(stateful): index bounded source regions`
   - Added generic line-grid region nodes and context entries, lifecycle refresh,
     hard per-file/project caps, and source fingerprints.
3. `5e85663054 feat(stateful): diversify context region results`
   - Suppresses a redundant file hit when matching regions exist and limits a
     source path to three returned regions.
4. `f1bc8b1082 docs(stateful): prioritize longitudinal intelligence loop`
   - Records the coupled failure model, negative results, execution order, and
     measurement gates.

Validation completed before the pause:

- `just test -p codex-project-intelligence`: 31/31 passed after the pushed
  region and diversity slices.
- `just test -p codex-stateful-extension`: 24/24 passed for the checkpointed
  line-range prototype.
- Targeted app-server integration
  `model_can_verify_the_exact_line_range_returned_by_context_routing`: passed.
- Scoped fixes and formatting passed for the pushed slices.

These results establish deterministic component behavior only. They do not
establish q10 recall, safe route identity, lower cost, or improved quality.

## Droid review: blockers in the current region design

Two independent read-only Droid reviews were completed before the pause. Their
findings materially reject the current slice as merge-ready.

### 1. The frozen q10 routing gate still fails

Regions are bounded to 64 lines but their searchable description is truncated
to 4 KiB. `SCORECARD.md:1-64` is more than 11 KiB, so the decisive line 59 is
outside the indexed text even though the returned anchor claims lines 1-64.

A focused replay reported these ranks:

| Frozen query | decisive PLAN region | decisive SCORECARD region |
|---|---:|---:|
| strategy / earliest missing link query | 12 | 23 |
| production integration query | 3 | 89 |

The result limit is 10. The mechanism therefore would not have prevented the
documented q10 miss.

Required direction: create contiguous regions bounded by both line count and
searchable bytes, never silently truncate the represented range, and rerun the
exact frozen queries before ranking work or product claims.

### 2. Route identity is not safely consumable

The checkpoint tells the model to reuse a bare line range. If the file changes
and lines are inserted above that range, `evidence_read` refreshes the file and
can replay the old coordinates against new content. Unrelated shifted bytes can
then receive a fresh read receipt.

Required direction: consume a route identity bound to its fingerprint and exact
range. On fingerprint mismatch, fail closed and return or require a refreshed
route. Do not silently replay coordinates after refresh.

### 3. Region routing loses the reuse signal

Region search returns region context-map entry IDs, while `evidence_read`
currently resolves and receipts the parent file route. Later region queries ask
for blackboard knowledge linked to the region ID and therefore omit the parent
file's `knownKnowledge`. This can recreate the rereading behavior the feature is
supposed to reduce.

Required canary: query region -> read -> record a source-verified conclusion ->
repeat the same query. The second query must report the durable knowledge once,
with the exact range and no route-identity ambiguity.

### 4. The project region cap can erase file routes

When the next file would exceed 50,000 regions, the scanner currently stops the
entire corpus walk. Later files disappear instead of retaining file-level partial
routes.

Required direction: stop adding regions when the budget is exhausted, continue
indexing every remaining file, and mark omitted region coverage partial.

### 5. Refresh can expose mixed generations as current

The file is updated before regions, regions are written individually, and old
regions are retired last. A query during refresh or after partial failure can
observe internally matching old region entry/node fingerprints and call them
current even though the parent file has advanced. Historical matching regions
can also consume the three-per-source diversity slots.

Required direction: generation-aware or atomic per-file publication. Readers
must see the complete old generation or complete new generation; current active
routes must not be displaced by historical routes.

### 6. Region identity accumulates EOF history

The region ID contains both start and variable end line. Growing 65-70 to 65-71
creates a new identity and leaves the previous node missing. Ordinary file growth
therefore creates unbounded historical rows and can worsen stale-route crowding.

Required direction: stable cell/start identity with the exact current end stored
as mutable route metadata, or another identity scheme that remains stable across
ordinary append/shrink cycles.

### 7. Route headlines can hide the matched fact

The checkpoint merely raises the leading headline from 240 to 512 bytes. A fact
matched near the end of a bounded region can still be absent from the model-
visible headline. The query may rank the right region while giving the model no
reason to select it.

Required direction: return a bounded query-specific FTS snippet or equivalent
match-centered preview. Do not solve this by returning every 4 KiB description.

### 8. Scan and deletion lifecycle truth has older gaps

- A transient per-file scan failure can leave the file unseen and reconciliation
  can mark it missing.
- `refresh_file` canonicalizes before resolving the stored file, so a deleted
  path cannot take the intended single-file missing lifecycle path.
- The existing deletion test exercises full refresh, not `refresh_file`.

These are issue #16 freshness/truthfulness problems and need explicit canaries.

### 9. Refresh inventory and public API semantics are inconsistent

`list_project` returns active file and region entries, so refresh inventory can
inject redundant region metadata. The existing app-server expectation assumes
file-level inventory. Public v2 still exposes only a generic region anchor, and
public `evidence/read` does not accept region IDs. Decide the compact inventory
contract first; stage public parity after the model path has a sound canonical
route type, unless an external consumer requires it sooner.

## Small gates required before another broad run

After authentication is fixed, do not launch a broad benchmark. Use these
small falsification gates first:

1. Exact frozen q10 route gate: both PLAN and SCORECARD facts in the bounded top
   10; record index time, database growth, query time, hashed bytes, and output.
2. Structural reuse gate: route -> read -> persist -> same route query reports
   the linked root/deeper knowledge.
3. Shift-between-query-and-read gate: inserting lines above a returned route must
   reject stale coordinates and must not issue a receipt for shifted content.
4. q02 -> compaction -> q06 gate: the reset mismatch survives and is actually
   used without an unjustified reread.
5. Changed-authority gate: a newer controlling source invalidates confident
   reuse even when old cited bytes remain unchanged.
6. Region-cap and interrupted-refresh gates: all file routes survive the cap and
   queries never observe a mixed generation as current.

Only after those gates pass should a short matched trajectory test whether later
turns use fewer requests and less repeated source output without losing quality.

## Recommended resumption sequence

1. Fix and validate Codex ChatGPT authentication first.
2. Refresh issues #15-#17 and re-read the product intent and this handoff.
3. Ask Droid to finish the interrupted second-round review that converts the
   blockers above into sub-500-line review stages. Challenge its prioritization;
   do not treat its first answer as authoritative.
4. Decide whether to temporarily disable region results or fix forward. Do not
   merge checkpoint `a6cdb157de`.
5. Land the smallest safety slice first: route identity/fingerprint consumption,
   region-cap preservation, and decisive lifecycle canaries.
6. Make searchable regions truthful and run the exact q10 route gate.
7. Connect parent knowledge and add the route/read/record/requery canary.
8. Measure bounded indexing and query cost before default enablement.

The product objective is unchanged. The current region implementation is a
useful experiment that exposed the right failure layer, but it is not yet a safe
or demonstrated solution.
