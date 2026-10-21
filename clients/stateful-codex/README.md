# Stateful Codex web client

Node.js 22 or newer is required. This client has no npm dependencies.
Run commands below from the repository root.

## Native gateway

Build the CLI and its companion from the **same checkout**:

```powershell
cd codex-rs
cargo build -p codex-cli -p codex-code-mode-host
.\target\debug\codex.exe login
cd ..
$env:CODEX_BIN = (Resolve-Path .\codex-rs\target\debug\codex.exe).Path
npm --prefix clients/stateful-codex start
```

On Linux/macOS, use `codex` instead of `codex.exe` and
`export CODEX_BIN="$PWD/codex-rs/target/debug/codex"`.
These build commands describe the source configuration; native builds and login
were **not qualified by the P3b/P3c web-only checks**.

The gateway selects `CODEX_BIN` when set, otherwise the local debug CLI, otherwise
`codex` on PATH. Keep `codex-code-mode-host` (with `.exe` on Windows) beside it.
An explicitly selected existing CLI without its companion is refused.
The app-server uses the cached ChatGPT login, not API-key environment variables.

Open **http://127.0.0.1:4173**. Choose a project and create, continue or fork a
thread. Ask does not create a Stateful run; Work modes retain the existing run
workflow. Set `STATEFUL_CODEX_PORT` to change the loopback listener's port.

## Gateway security

The gateway accepts only its own `127.0.0.1` / `localhost` Host and Origin.
API requests require a random, port-named `HttpOnly; SameSite=Strict` cookie.
Open the page first to establish that cookie. POSTs also require the page's
Origin; cross-site browser requests are refused. No session token appears in
HTML, script-readable storage or EventSource URLs.

Only browser-used RPC methods are exposed. Execution and memory controls accept
exact parameter shapes, not arbitrary model/config/sandbox/authority overrides.
Approval and question replies are checked against the stored request, owning
thread, original typed ID and expected answer shape.

This is a local gateway, not a remote-access service. Do not expose or proxy it
onto a network. The cookie does not protect against a hostile local process.

## Project memory

In a workspace, scroll to **Project memory**:

- Browse rules, historical task-limited rules, background, decisions and knowledge.
  These are retained entries, not a claim that every entry currently applies.
- **Correct** opens a revision-pinned draft. **Save correction** replaces its
  entry; correcting an unverified rule makes the new words your rule.
- **Forget** retires an entry without deleting history.
- **Add** saves a rule, background, decision (with optional reason), or note.
  Retrying unchanged words keeps one action ID. Changing words starts a new action.
- **Show more** follows the cursor. Refresh re-reads the pages already opened.
  If memory changes between pages, the client starts again instead of skipping entries.

All four memory APIs bind `expectedProjectId`; writes also carry the applicable
revision or action ID. They do not start a model turn. Unsupported historical
scope controls are not exposed.

Drafts survive refresh, live events and reload in the same tab, keyed by project
and thread in session storage. Failed or delayed saves do not discard newer
words. A stale correction keeps its original revision; copy/discard and reopen
to correct the latest version. Drafts whose entries disappeared remain visible.
Clearing browser session storage loses these drafts. Unavailable/full storage
retains drafts only for the current page lifetime.

Text is limited to 2,000 UTF-8 bytes, including ` Reason: ` and the reason for a
decision. Longer drafts are refused whole, never shortened. A truncated read is
an excerpt and cannot be used to open or submit a correction.

## Tests and disposable browser check

```powershell
npm --prefix clients/stateful-codex test
node clients/stateful-codex/eval/gateway-smoke.mjs
```

The second command starts the **production HTTP gateway with a fixture bridge**,
prints its ephemeral loopback URL, and creates a disposable project beneath
`clients/stateful-codex/eval/artifacts/`. Stop it with Ctrl+C. It does not load a
real account, launch a native app-server, or qualify native store behavior.
Its synthetic memory pages are intentionally small so paging is easy to check.

Tests cover HTTP admission, replies, current memory call shapes, project and
revision refusals, UTF-8 boundaries, excerpts, paging and delayed-save/reload
reconciliation. Contract fixtures do not replace native integration tests.
HTML render snapshots are in `test/snapshots/`; comparisons normalize Windows
line endings. Set `UPDATE_SNAPSHOTS=1` when intentionally updating the memory
render test, then review the snapshot diff. Never accept snapshots blindly.
