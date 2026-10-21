# Testing

Read [AGENTS.md](./AGENTS.md) and [development gates](./DEVELOPMENT.md#before-any-rust-build-or-rust-test)
first. Commands below run from the repository root unless stated otherwise.
Use **`just test`**, never direct `cargo test`.

## Verified without a Rust build

```powershell
npm --prefix clients/stateful-codex test
python -m unittest discover -s scripts -p test_scodex.py -v
```

Windows/Node 22.19.0: **132/132 PASS** at review-fix pin
`b8d095182c703849c1371b35f9232d2cd4867d78`, 2026-10-21.
Python 3.13.7: **10/10 PASS**.
Node's count includes discovered fixture modules; it is not 132 independent
product assertions. Tests include real loopback HTTP listeners with fixture
bridges, gateway refusals, memory contracts, paging and draft reconciliation.
They do not qualify native writes, models or a production store.
The review follow-up detects six individual guard-removal mutations: cookie
name, extra envelope keys, the 1 MiB body limit, typed IDs, submission-time
excerpts and retired Add actions. HTTP/controller checks also preserve lost
Add-response drafts when a later Forget or correction retires their entry.
Python's command is `python3` on Unix, **UNVERIFIED here**.

For a disposable manual web check:

```text
node clients/stateful-codex/eval/gateway-smoke.mjs
```

Use the printed URL and [client walkthrough](./clients/stateful-codex/README.md#memory-walkthrough).
The runner starts a fixture-backed production gateway, not a native app-server.

## Scoped Rust tests, execution UNVERIFIED

The [justfile](./justfile) selects nextest's `local` profile, 8 MiB Rust stack
and `--no-fail-fast`. These recipe expansions were checked with `--dry-run`;
they were **not executed**. The earlier cold-build reserve was unavailable;
the resumed paired-binary build failed at its required V8 download (HTTP 404).
No Rust source changed, so these crate suites were not needed for the review fix.
Run only the package(s) affected by your change:

```text
just test -p codex-project-intelligence
just test -p codex-stateful-extension
just test -p codex-stateful-runtime
just test -p codex-project-store
just test -p codex-stateful-memory
just test -p codex-app-server-protocol
just test -p codex-app-server
just test -p codex-tui
```

Do not run all these merely because they are listed. Set `CARGO_BUILD_JOBS=1`
in the shell, reserve the single build slot and keep free RAM above 12 GiB
with the additional admission reserve in development. Never kill Rust processes
or work around a Cargo lock by launching another build. Avoid routine
`--all-features`.

After scoped checks pass, changes in common/core/protocol normally require the
complete `just test`, **only with separate explicit owner approval**.
The documentation executor was explicitly forbidden to run that suite.
If approval is absent, stop that gate and report it; do not quietly substitute
scoped results for full-suite qualification.

## Tests to add with a behavioral patch

Prefer integration tests exercising public APIs. Agent-logic changes require
integration coverage, with major behavior changes listed in the PR.
App-server tests should use JSON-RPC; core tests should use `test_codex` and
response fixtures. Follow the auto-environment builders and platform guidance
in AGENTS. New Rust test modules belong in sibling `*_tests.rs` files.
Use whole-object comparisons and `pretty_assertions::assert_eq`.

For memory changes include project mismatch, stale revisions, whole UTF-8
limits, retired-entry replay and effective reads. For context changes include
bounded rendering, first requests after compaction, checkpoint/reset/fork and
origin/authority safety as applicable. A fixture cannot prove native semantics.

## Snapshots

User-visible TUI changes require insta snapshots. Source-checked commands,
**regeneration UNVERIFIED/not run in this docs pass**, from `codex-rs`:

```text
just test -p codex-tui
cargo insta pending-snapshots -p codex-tui
cargo insta show -p codex-tui path/to/file.snap.new
cargo insta accept -p codex-tui
```

The path in `show` is a placeholder. Read every generated `.snap.new` before
accepting; `accept -p` accepts all pending snapshots in that crate, so use it
only when every change is intended.

Web render snapshots live in
[`clients/stateful-codex/test/snapshots`](./clients/stateful-codex/test/snapshots).
Comparisons normalize CRLF. For intentional memory-render updates on PowerShell:

```powershell
$env:UPDATE_SNAPSHOTS = "1"
node --test clients/stateful-codex/test/memory-view.test.mjs
Remove-Item Env:UPDATE_SNAPSHOTS
git diff -- clients/stateful-codex/test/snapshots
```

This maintenance sequence is **UNVERIFIED/not run** here; it changes snapshots.
The removal only clears the process environment variable, not files.
Review the complete render diff, including escaping, excerpt controls and stale
drafts. Do not regenerate snapshots to hide an unrelated failure.

## Generated artifacts

Source-checked recipe names below are **UNVERIFIED regeneration** in this pass.
Run only when the corresponding source shape changes:

| Trigger | Command from repository root |
| --- | --- |
| `ConfigToml` or nested config types | `just write-config-schema` |
| App-server wire payload/API | `just write-app-server-schema` |
| Experimental app-server fixtures | `just write-app-server-schema --experimental` |
| Rust Cargo dependencies/lock | `just bazel-lock-update` |

Review and commit generated outputs with the source change. API changes also
need `just test -p codex-app-server-protocol`. Do not hand-edit generated files.
For `include_str!`, `include_bytes!`, `sqlx::migrate!` or other compile-time
file reads, update the crate's `BUILD.bazel` compile/build/test data too.
Cargo success alone does not prove Bazel can see those inputs.

## Final cleanup ordering

1. Finish behavior, tests, schema/lock generation and snapshot review.
2. Run the scoped tests and record results.
3. For a large Rust change, run `just fix -p <changed-crate>` inside `codex-rs`.
   This is the repository's Clippy fix recipe, not permission for a workspace
   build. Check edits it makes.
4. Run **`just fmt` inside `codex-rs`**, even for documentation-only changes.
   It formats Just, Rust, Bazel/Starlark and Python, not Markdown.
5. Inspect formatting output, diff and status. Do **not** rerun tests after
   fix/fmt merely for reassurance, per AGENTS. Disclose any unresolved concern.

Never delete user work, generated evidence, migration history or target trees
as “cleanup.” Windows formatter line-ending churn must be inspected, especially
inside multiline Bazel strings. A formatter exit 0 is not a test result.
