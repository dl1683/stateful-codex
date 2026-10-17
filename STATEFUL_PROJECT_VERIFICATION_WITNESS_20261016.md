# Inactive project-bound verification lease component

This adds two guard-consuming runtime APIs for starting and ending a lease in
the admitted project's `.stateful/memory.sqlite`. They share the legacy SQL
policy, require the compiled runtime domain and exact project-owned run, and
commit prior staged writes with the lease operation. Refusal drops the guard;
its owner becomes close-only. Cancellation during COMMIT remains uncertain:
close and reconcile. A lease does not establish checker or acceptance evidence.

Evidence directory: `C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/verification_20261016/`.
Design: `DESIGN.md`; parity boundary: [reviewed design](./STATEFUL_PROJECT_ACCEPTANCE_PARITY_DESIGN_20261016.md).

## Source and local gates

- Initial source `4146c3dd9d`, admission coverage `c382f05c64`; fresh source
  reviews `review1.txt` / `review2.txt` approved their exact scope. C382 passed
  85/85 runtime tests on Windows and WSL.
- Follow-up `f4faf76da0` restores legacy overflow validation before pool
  admission and makes the test observer preserve SQLite storage classes,
  complete TEXT/BLOB bytes and REAL bits. Both regressions first failed in
  `regressions_red.log`; the deliberately filtered run skipped 85 other tests.
- `windows87.log` and `linux87.log`: 87/87 each, zero skips/retries. Complete
  9,592-file before/after snapshots match each other; see
  `pre_cleanup_source_check.json`. New cases cover rollback/close-only refusal,
  domain/project/run admission, takeover, expiry, stale/exact release and prior
  domain writes. Cancellation handling was inspected, not dynamically exercised.
- `merge2_source_custody.json`: conflict-free merge `eabed33af5` with approved
  main `eb767f926b`; exact runtime/extension/protocol component trees preserved.
  Source cleanup normalizes line endings, adds a narrow closed-pool test lint
  expectation, and formats. Scoped `fix_final2.log` passes without warnings;
  `fmt_final.log` passes. No Rust tests ran after final fix/format.

## Frozen launcher journey and limits

`component_trial7/luna/score.json`: PASS, one actual gpt-6-luna session through
the unchanged installed launcher, one command, 14.504 seconds. It invokes an
external helper built from C382, not an installed candidate product binary.
Two fresh, distinct project/store identities exercise attempts 1/2/3, competing
rollback, stale release, exact release and prior domain writes. All tables and
absolute lease fields are checked with lossless, consistent SQLite snapshots.

`helper_review7.txt` approved the proposed session. Two positive controls and
13 rejected false-PASS controls are in `control_checks7.json`. Final output,
execution lifecycle, attribution counters, durable state and all frozen inputs
pass postflight; `component_trial7/frozen_archive` retains 9,620 bindings,
including the entire 9,591-file C382 source frame. Build5 custody is recorded,
not cryptographic build attestation. Earlier helper/controller failures and
direct-only trials are retained; they are not successful Luna sessions.

The frozen journey does not cover later overflow/test-observer cleanup as a
new binary. Native routing, populated migration, legacy-writer exclusion,
real acceptance/source freshness, Forget/compaction, neutral MCP writes and
the first project-database milestone remain open. No native binary was installed.
