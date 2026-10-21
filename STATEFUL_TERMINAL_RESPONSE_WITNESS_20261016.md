# Inactive terminal composition and HAL response codec, 2026-10-16

> **Archived historical record, not current operating instructions.**
> Use [current state](./STATEFUL_CODEX_CURRENT_STATE.md) for status and holds.
> Historical external evidence is not distributed in this repository.
> Paths and personal identifiers are redacted; original bytes and any
> historical digest claims refer to the version preserved in Git history.

Two disjoint slices extend the inactive foundations at main `4b87191f85b0`.
Neither attaches project storage or a console worker to a native user session.
Source commits are `b6c645a8fc` (434 insertions, 2 deletions) and `ddfd2a9d89`
(412 insertions, 1 deletion), each a separately reviewed slice under 500 changed
lines. Conflict-free merge `9fb1a6070ec0` preserves all ten changed blobs and all
four involved crate trees exactly; `merge_identity.json` records static evidence.

The extension fixture uses the actual compiled project-intelligence and runtime
migrations in one project-owned `.stateful/memory.sqlite`. It prepares a
no-learning token, binds run/start, revalidates PI and immediately commits terminal
state under the same writer guard. Durable comparisons cover schema, every table,
rowids and the verification lease. Success consumes the lease atomically; runtime,
knowledge and run-binding refusals preserve the durable image, including a late
trigger rollback after admission. Acceptance rows are synthetic, not real checker
receipts. This adds test coverage, not production routing or completion behavior.

The Windows console protocol adds an inactive, strict closed-buffer response
codec for ready, spawned, rootExited and failed events. Version, nonce, object
shape, duplicate fields, integer bounds, nonzero PIDs, exact framing and the
existing 1 MiB cap are enforced. Nonces provide correlation, not authentication.
No stream reader, worker launch, handle admission or lifecycle claim is added.

## Validation and retained failures

- Current Windows extension: 338 run, 337 passed, one Git-marker permission
  preflight failure. Current Linux extension: 339 run, 338 passed, one existing
  memory-history test timed out at the unchanged 60-second threshold. Both
  problems reproduced on clean main. All three new fixtures pass on both OSes.
  Neither complete extension suite is green.
- Response protocol: all 14 tests passed on Windows and Linux; no skips.
- The earlier Windows aggregate passed 162 PI tests with one intentional skip
  and 80 runtime tests. Its extension executable was stale and is excluded as
  current-source evidence. Copy-Item preserved an old mtime; executable literals
  proved the mismatch. A package clean and real recompile produced the current
  Windows evidence. Initial fixture failures and the Linux relation-column
  failure remain retained, not regraded.
- `just bazel-lock-update` succeeded and left MODULE.bazel.lock unchanged.
  Cargo.lock records the two added dev-dependency edges. Local builds used one
  shared build mutex, two jobs and the unchanged RAM admission floor. No cloud
  builds, Rust PID termination, native installation or policy changes occurred.
- Scoped `just fix -p codex-stateful-extension` and `just fix -p
  codex-windows-console-protocol` succeeded. Extension source was unchanged, with
  an existing large-enum warning retained. Protocol cleanup removed one redundant
  clone from a test. Both worktrees passed `just fmt`; a final formatter pass on
  the composed tree succeeded with no changes. No Rust tests followed cleanup.
  Formatter differences and the clone removal require exact final-source review;
  the component/model source archives remain the executed source authority.

External evidence: `sc_dogfood/lsr_drivers/reports/stateful_rebuild/terminal_20261016`
and sibling `response_20261016`; exact source reviews, designs, source frames,
binary captures, baseline failures and controller review revisions are retained.
One frozen unchanged-launcher `gpt-6-luna` journey passed all seven exact custom
exec cells in 34.610 seconds, with no retries, other tools or judge. Both complete
final outputs retained `windowsSuiteClean:false`, `linuxSuiteClean:false`,
`nativeRouting:false`, `syntheticAcceptance:true`. Fresh raw/public call and output
identity checks passed; native, provider and login jobs exited naturally with
zero descendants before cleanup. Independent closed audit rechecked 116 inputs,
the 9,585/9,586 archive leaves, actual events, final outputs and cleanup. The
manifest is `e1885a96156a23ab06ffec3ba0c61569ac8535d0d6abb1d51c1c87a1b6921ec5`.
This is `PASS_LUNA_INACTIVE_FIXTURES_ONLY`, not native acceptance. Full source
frames become historical after final formatting and documentation; the captured
archives remain the exact test/model source evidence. Final cleanup and merge
verdicts are recorded separately and do not requalify the whole native product.

## Remaining boundary

The first project-owned database milestone is NOT FULFILLED. Native routing,
populated import, old-writer barrier, real checker/source freshness, full atomic
native completion and fence removal, Forget, compaction, engine-neutral reads,
aggregate steering bounds, and native Unicode/Marathi/offline/HAL/customer
acceptance remain open. Prior PI/stream Luna final-contract failures remain
failures. The protected upstream B0 branch remains owned by its parallel executor
and unqualified; no blind upstream merge is authorized by these component results.
