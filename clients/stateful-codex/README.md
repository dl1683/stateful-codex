# Stateful Codex web client

Node.js 22 or newer is required. This client has no npm dependencies.
Run commands below from the repository root.

See the root [reading map](../../README.md#reading-map), [development](../../DEVELOPMENT.md)
and [configuration](../../CONFIGURATION.md) guides first.
Baseline: `997007527c5bb1d03c4d6b67cd5f8786b37b3e17`, 2026-10-20.
Review-fix pin: `b8d095182c703849c1371b35f9232d2cd4867d78`, 2026-10-21.
Windows web suite: **132/132 PASS**. Launcher tests: **10/10 PASS**.
Windows paired build and isolated CLI/app-server initialization: **PASS** with
the [matched V8 overrides](../../DEVELOPMENT.md#windows-v8-archive-and-binding).
The earlier default Deno download failed with HTTP 404. Native browser startup,
login/model/memory and WSL/Linux/macOS remain **UNVERIFIED** in this pass.
The runnable fixture check below needs no Rust build or native binaries.

## Native startup, UNVERIFIED

Build the CLI and its companion from the **same checkout**, after following
[development's process and resource checks](../../DEVELOPMENT.md#before-any-rust-build-or-rust-test).
On your own machine, wait for existing Cargo/rustc builds; only on a shared
maintainer machine, coordinate with its other users. On Windows,
first complete the [V8 archive and binding recipe](../../DEVELOPMENT.md#windows-v8-archive-and-binding)
in the same VS Developer PowerShell; the default Deno URL returns 404:

```powershell
cd codex-rs
cargo build --locked -j 1 -p codex-cli -p codex-code-mode-host
cd ..
$env:CODEX_BIN = (Resolve-Path .\codex-rs\target\debug\codex.exe).Path
$scratch = Join-Path ([IO.Path]::GetTempPath()) ("stateful-web-" + [guid]::NewGuid())
$project = Join-Path $scratch "project"
$scratchHome = Join-Path $scratch "home"
New-Item -ItemType Directory -Path $project,$scratchHome | Out-Null
git init --quiet $project
$env:CODEX_HOME = $scratchHome
$env:CODEX_SQLITE_HOME = $scratchHome
@'
cli_auth_credentials_store = "file"
forced_login_method = "chatgpt"
[memories]
generate_memories = false
use_memories = false
'@ | Set-Content -Encoding utf8 (Join-Path $scratchHome "config.toml")
& $env:CODEX_BIN login
npm --prefix clients/stateful-codex start
```

The fresh home intentionally has no copied credentials or old databases.
Authenticate only through the native login flow. Managed requirements can still
refuse routing; do not bypass them. The gateway's process-wide storage is chosen
at startup. Selecting a project on the page does **not** relocate its home.

Bash equivalent after building, **UNVERIFIED**:

```bash
export CODEX_BIN="$PWD/codex-rs/target/debug/codex"
scratch="$(mktemp -d)"
project="$scratch/project"
mkdir "$project" "$scratch/home"
git init --quiet "$project"
export CODEX_HOME="$scratch/home"
export CODEX_SQLITE_HOME="$CODEX_HOME"
printf '%s\n' 'cli_auth_credentials_store = "file"' \
  'forced_login_method = "chatgpt"' '[memories]' \
  'generate_memories = false' 'use_memories = false' > "$CODEX_HOME/config.toml"
"$CODEX_BIN" login
npm --prefix clients/stateful-codex start
```

The gateway selects `CODEX_BIN` when set, otherwise the local debug CLI, otherwise
`codex` on PATH. Keep `codex-code-mode-host` (with `.exe` on Windows) beside it.
An explicitly selected existing CLI without its companion is refused.
The app-server uses the cached ChatGPT login, not API-key environment variables.

Open **http://127.0.0.1:4173**. Choose a project and create, continue or fork a
thread. Ask does not create a Stateful run; Work modes retain the existing run
workflow. Set `STATEFUL_CODEX_PORT` to change the loopback listener's port.
For this walkthrough select `$project` (the Bash variable on Unix), create a
thread, choose Ask, and ask a harmless question about the empty project.
Do not mistake a setup/health response for a successful model turn.
Stop with Ctrl+C, retain scratch evidence, then close the shell so its private
environment selection does not affect later work.

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

## Memory walkthrough

Use only disposable data. This walkthrough was exercised through the
fixture-backed gateway on Windows, **not qualified against native storage**.
In a workspace, scroll to **Project memory**:

- Browse rules, historical task-limited rules, background, decisions and knowledge.
  These are retained entries, not a claim that every entry currently applies.
- Add a **note**: `Disposable walkthrough note.` Click **Add** and wait for the
  returned entry. A retained draft is not a saved entry.
- **Correct** opens a revision-pinned draft. Change it to
  `Corrected disposable walkthrough note.` **Save correction** replaces its
  entry; correcting an unverified rule makes the new words your rule.
- Refresh, reopen the entry and check the successor text. **Forget** retires it;
  Refresh should no longer list it. This does not delete transcripts, historical
  revisions, browser drafts or previously sent model context.
- Repeat with a harmless **rule**, then Forget it. **Add** also supports
  background and decisions (with optional reason).
  Retrying unchanged words keeps one action ID. Changing words starts a new action.
  If a lost Add response is retried after that entry was forgotten or corrected,
  the retry refuses and keeps the draft; it never restores the retired entry.
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
This protects whole drafts, not universal exact-byte storage: current native
correction code trims input, and its archived exact-input repair is not landed.
See [decisions](../../STATEFUL_CODEX_DECISIONS.md).

For refusal checks, keep a correction open while another tab changes that
entry; saving the old revision must refuse and retain your draft. Refresh should
show the orphan rather than silently applying it to another revision.
In the fixture, the excerpt entry has no Correct button. A 2,001-byte ASCII
draft is refused rather than shortened. Page through **Show more**, then
Refresh to check retention of opened pages. Fixture state resets when its
server restarts; that is not native cold-restart evidence.

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
Select **Disposable gateway check**, its disposable thread and Ask. Fixture
mutation behavior is deliberately simplified; do not use it to qualify
post-Forget replay or migration/authority semantics.

Tests cover HTTP admission, replies, current memory call shapes, project and
revision refusals, UTF-8 boundaries, excerpts, paging and delayed-save/reload
reconciliation. Contract fixtures do not replace native integration tests.
HTML render snapshots are in `test/snapshots/`; comparisons normalize Windows
line endings. Set `UPDATE_SNAPSHOTS=1` when intentionally updating the memory
render test, then review the snapshot diff. Never accept snapshots blindly.
Snapshot regeneration is a maintenance operation, **not run in this docs pass**.
