# Stateful Codex development

Canonical build, configuration, browser, test and contribution guide.
Start at [README](../../README.md) and keep [AGENTS](../../AGENTS.md) intact.
Commands run from the repository root unless stated otherwise.
Historical execution evidence is in [RESULTS](RESULTS.md), not implied by the
presence of a command below. Linux/macOS/WSL newcomer provisioning and native
login/model/memory journeys remain **UNVERIFIED**.

## Tools and build admission

| Tool | Requirement / previously observed version |
| --- | --- |
| Rust | [Pinned 1.99.1](../../codex-rs/rust-toolchain.toml), rustfmt, clippy, rust-src |
| Node | >=22; Windows checks previously used 22.19.0; no client npm dependencies |
| Python | Python 3; launcher checks previously used 3.13.7 |
| pnpm | [Pinned 10.34.5](../../package.json); not required for client tests |
| just / nextest / insta | Previously 1.57.0 / 0.9.140 / 1.48.0 |
| Formatting | `uv`, DotSlash, pinned buildifier in `tools/buildifier`, ripgrep |
| Native builds | C/C++ compiler/linker, SDK, CMake, Cargo/native artifact access |

Install missing tools through trusted distributions. Tool installation itself
was not requalified. Example:

```text
rustup toolchain install 1.99.1 --component rustfmt --component clippy --component rust-src
cargo +1.99.1 install --locked just --version 1.57.0
cargo +1.99.1 install --locked cargo-nextest --version 0.9.140
cargo +1.99.1 install --locked cargo-insta --version 1.48.0
```

Run one build-producing command at a time. Check all checkouts for Cargo/rustc;
on a shared machine coordinate with other users, not merely a process snapshot.
Check RAM and disk; use one Cargo job when memory is limited. Never kill Rust
processes or delete locks/caches to bypass waiting.

```powershell
Get-Process -Name cargo,rustc -ErrorAction SilentlyContinue | Select-Object Id, ProcessName
```

On Unix, inspect `ps -A -o pid=,comm=` for Cargo/rustc. No matching process only
describes that moment. Historical RAM floors were run-specific admission rules,
not universal contributor requirements. Full-suite approval is separate.

Windows needs VS 2022 Build Tools (Desktop development with C++), Windows SDK,
CMake and the real MSVC linker, not Git's `link.exe`. Linux needs a C/C++
toolchain, pkg-config, OpenSSL headers and CMake; use Linux storage/tools under
WSL rather than mixing target trees. macOS needs Xcode Command Line Tools and
CMake with the matching Intel/Apple Silicon target. These are prerequisites,
not tested provisioning recipes.

## Windows V8 archive and binding

Use a fresh, short LF checkout in an **unused** destination. Replace the
`<unused-short-destination>` placeholder with your chosen short checkout path:

```powershell
git -c core.autocrlf=false -c core.longpaths=true clone --origin stateful --single-branch --branch main --no-tags https://github.com/dl1683/stateful-codex.git "<unused-short-destination>"
cd "<unused-short-destination>"
```

Network clone is **UNVERIFIED**. The successful October 21 build used a fresh
short local clone at `3e831c98ecfc7136f500feb1f7426c931aad1fde`, not copied outputs.
`core.longpaths` does not repair Windows command-line length.

The current registry crate `v8` 152.2.0 is locked to checksum
`a10fe1a92da5c32c7c7f838ce36c0ccfcfd5edf0865b58bdde820aa64cea9888`. Code mode enables the V8 sandbox.
Use the public archive **and matching binding** selected by
[Cargo CI](../../.github/actions/setup-rusty-v8/action.yml); keep the sandbox enabled.

The earlier 150.4.0 default Windows sandbox URL failed with 404 in the recorded
onboarding build. That historical failure and the successful earlier override
remain in [RESULTS](RESULTS.md). They do not qualify the current native build.
`RUSTY_V8_MIRROR` retains the default publisher's version-directory layout and
cannot by itself select Codex's release-tag layout. A cached library alone is
not a matched binding.

The [October 22 artifact-verification record](RESULTS.md#v8-artifact-verification-october-22)
covers the current archive, matching binding and manifest. Download/hash
verification does not establish compilation or native execution acceptance:

```powershell
$ErrorActionPreference = "Stop"
$repo = (Get-Location).Path
$artifacts = Join-Path ([IO.Path]::GetTempPath()) ("stateful-v8-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $artifacts | Out-Null
$base = "https://github.com/openai/codex/releases/download/rusty-v8-v152.2.0"
$stem = "rusty_v8_ptrcomp_sandbox_release_x86_64-pc-windows-msvc"
$manifest = "$stem.sha256"
Invoke-WebRequest "$base/$manifest" -OutFile "$artifacts/$manifest"
$trusted = Join-Path $repo "third_party/v8/rusty_v8_152_2_0_release_manifests.sha256"
$expected = ((Get-Content $trusted | Where-Object { $_.EndsWith("  $manifest") }) -split '\s+')[0]
if (!$expected -or (Get-FileHash "$artifacts/$manifest").Hash.ToLowerInvariant() -ne $expected) {
    throw "V8 manifest hash mismatch"
}
foreach ($line in Get-Content "$artifacts/$manifest") {
    $hash, $name = $line -split '\s+', 2
    Invoke-WebRequest "$base/$name" -OutFile "$artifacts/$name"
    if ((Get-FileHash "$artifacts/$name").Hash.ToLowerInvariant() -ne $hash) {
        throw "V8 artifact hash mismatch: $name"
    }
}
$env:RUSTY_V8_ARCHIVE = "$artifacts/$stem.lib.gz"
$env:RUSTY_V8_SRC_BINDING_PATH = "$artifacts/src_binding_ptrcomp_sandbox_release_x86_64-pc-windows-msvc.rs"
$env:PYTHON = (Get-Command python.exe).Source
$env:CARGO_BUILD_JOBS = "1"
$vswhere = "${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
$vs = & $vswhere -latest -products "*" -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$vs) { throw "Install VS Build Tools with Desktop development with C++ first" }
Import-Module "$vs/Common7/Tools/Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments "-arch=x64 -host_arch=x64"
cd "$repo/codex-rs"
cargo build --locked -j 1 -p codex-cli -p codex-code-mode-host
cd $repo
```

Expected inputs from the [recorded verification](RESULTS.md#v8-artifact-verification-october-22):

| Input | SHA-256 |
| --- | --- |
| Manifest | `e16bf2d7bc4b64a96a89e87048d61b9c466040e4c82ae64cfe9f7f2052ef3179` |
| Gzip archive | `cd829415695b30ad61b306b23a381e11276f14bb0b46815ac5ed8b96711d002d` |
| Matching binding | `572ed16ff09bb7944f4b35c659ff62dcd3faf43a0351e3f94f011e01b2f92a62` |

No private cache or new dependency is required by this recipe. In a new shell,
repeat setup; environment changes do not persist automatically.

## Paired builds and disposable CLI

On non-Windows platforms, build the same pair from `codex-rs` after admission:

```text
cargo build --locked -j 1 -p codex-cli -p codex-code-mode-host
```

Keep `codex[.exe]` and `codex-code-mode-host[.exe]` together in
`codex-rs/target/debug/`. Another profile/target/`CARGO_TARGET_DIR` changes that
layout. Gateway `CODEX_BIN` accepts an absolute paired CLI; the Python launcher
has no corresponding binary override. The optional
`cargo build --locked -j 1 -p codex-stateful-memory` produces `stateful-memory`,
not `codex-stateful-memory`; its build is not requalified here.

Native model walkthrough, **UNVERIFIED**, after the paired build, in a new
PowerShell. The explicit scratch store overrides any inherited
`STATEFUL_STORE_ROOT`. The launcher can still copy missing config/auth from
`~/.codex-stateful`; review that template first. This is disposable storage,
not an unseeded setup. For an unseeded home use the direct native browser
recipe below:

```powershell
$repo = (Get-Location).Path
$scratch = Join-Path ([IO.Path]::GetTempPath()) ("stateful-intro-" + [guid]::NewGuid())
$project = Join-Path $scratch "project"
$env:STATEFUL_STORE_ROOT = Join-Path $scratch "home"
New-Item -ItemType Directory -Path $project | Out-Null
git init --quiet $project
$launcher = Join-Path $repo "scripts/scodex.py"
python $launcher -C $project login
python $launcher -C $project --stateful ask "Describe this empty disposable project."
```

Headless equivalent is `python $launcher exec -C $project --stateful ask "..."`
(add `--skip-git-repo-check` outside Git). In a new Bash shell, the equivalent
disposable setup is below; the same template inheritance applies. Bash
execution is **UNVERIFIED**.

```bash
repo="$PWD"
scratch="$(mktemp -d)"
project="$scratch/project"
export STATEFUL_STORE_ROOT="$scratch/home"
mkdir -p "$project"
git init --quiet "$project"
python3 "$repo/scripts/scodex.py" -C "$project" login
python3 "$repo/scripts/scodex.py" -C "$project" --stateful ask "Describe this empty disposable project."
```

Close the walkthrough shell afterwards to discard its storage override.
Explicit Ask avoids starting Work. The launcher defaults to `gpt-6-luna`; select
an available model explicitly if necessary. Retain scratch evidence.

## Configuration and storage

Authority is the [generated schema](../../codex-rs/core/config.schema.json),
[ConfigToml](../../codex-rs/config/src/config_toml.rs) and
[runtime resolution](../../codex-rs/core/src/config/mod.rs). Schema omission
does not imply a runtime default. No invented TOML `stateful` key activates this fork.

| Selection | Meaning |
| --- | --- |
| `-C` / `--cd` | Working directory/project, not database placement |
| `--stateful-project <id>` | Disambiguates known projects, not a storage move |
| `CODEX_HOME` | Existing config/auth/native home; otherwise `~/.codex` |
| `sqlite_home` → `CODEX_SQLITE_HOME` → Codex home | Native database precedence, subject to managed requirements |
| `STATEFUL_STORE_ROOT` | Launcher/neutral-reader override only; not understood generally by native CLI |
| `CODEX_BIN` | Gateway CLI selection; otherwise local debug, then PATH |
| `STATEFUL_CODEX_PORT` | Gateway loopback port, default 4173 |

Launcher default is `<project>/.stateful`; relative overrides are project-relative.
It refuses project/ancestor/shared-template homes, tracked home content and
repository Git errors. Private ignore policies precede optional missing config/
login seeding from `~/.codex-stateful`. Review that template privately; no knowledge
is imported. Use explicit empty homes below to avoid template seeding entirely.

Native files can include `project_intelligence_1.sqlite`,
`stateful_runtime_1.sqlite`, `state_5.sqlite`, `thread_history_1.sqlite` and
`sessions/`; creation is lazy. Canonical `memory.sqlite` is a different, inactive
conversion path. Never test a new build against valuable populated history.

| Key | Type / omission behavior |
| --- | --- |
| `model`, `model_provider` | Strings; provider/account resolution, not one universal model |
| `forced_login_method` | `chatgpt` or `api`; gateway forces `chatgpt` |
| `cli_auth_credentials_store` | `file`, `keyring`, `auto`, `ephemeral`; type default `file`, policy/runtime resolution applies |
| `model_context_window` | Optional token integer, normally model metadata |
| `model_auto_compact_token_limit` | Optional threshold, not exact-recall guarantee |
| `model_auto_compact_token_limit_scope` | `total` / `body_after_prefix`; runtime default `total` |
| `model_post_turn_compact_threshold_percent` | 0–100; omission 0 disables |
| `approval_policy`, `sandbox_mode` | Organization-approved policy, never bypassed by disposable storage |
| `[memories] generate_memories`, `use_memories` | Runtime defaults true; separate upstream pipeline |
| `[memories] version`, `dual_write`, `dedicated_tools` | Runtime defaults `v1`, false, false |

`--strict-config` can reject unknown keys. Disabling upstream memory flags does
not disable project intelligence or guarantee non-retention; rollouts may persist.
Credentials never belong in TOML, Git or a report.

## Browser startup and memory walkthrough

Fixture route needs only Node: `node clients/stateful-codex/eval/gateway-smoke.mjs`.
Open its exact printed ephemeral loopback URL. Select its disposable project/thread,
choose **Ask**, enter `Describe this disposable project.`, and select **Open
workspace** to reach Project memory. This uses the production HTTP handler with
synthetic memory, not native storage, an account or a model. Stop with Ctrl+C.

Native browser/model route is **UNVERIFIED**. After building, in a new PowerShell:

```powershell
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
Write-Output "Project directory: $project"
npm --prefix clients/stateful-codex start
```

Authenticate only through native login. Gateway strips API-key environment
variables; it does not erase managed requirements or other config. Open
`http://127.0.0.1:4173`. Under **Existing project**, choose **Create from a
directory**, enter a disposable **Project name**, and set **Project directory**
to the absolute path printed by `$project`. In **Thread view**, choose **Create**;
choose **Ask**, enter `Describe this empty disposable project.`, then select
**Open workspace** to reach Project memory. The fresh home has no registered
projects; this form registers the directory before starting the thread.
Selecting a project in the page does not relocate the process's home.
Use one gateway per disposable home; do not switch valuable homes while running.
Close the shell afterwards to discard its environment selection.

In **Project memory**:

1. Browse retained rules, background, decisions and knowledge. Retained historical
   scope does not mean currently applicable.
2. Add `Disposable walkthrough note.` and wait for an actual returned entry.
3. Correct it, refresh/reopen and verify successor text. Forget it and refresh;
   it should disappear from effective listings, not from transcripts/history.
4. Repeat with a harmless rule, background and a decision/reason. Unchanged Add
   retries keep one action ID; changed words start a new action. A retry after
   retirement refuses and keeps the draft.
5. Use **Show more**, then Refresh; opened pages should remain covered. A changed
   memory revision restarts paging rather than skipping entries.
6. Keep a correction open while another tab changes its entry. Old-revision Save
   must refuse and preserve the draft/orphan. Excerpts have no Correct action.
   A 2,001-byte ASCII draft refuses whole.

Drafts are keyed by project/thread in tab session storage; delayed saves cannot
discard newer words. Unavailable/full storage retains only page-lifetime drafts.
Text is limited to 2,000 UTF-8 bytes including ` Reason: ` and decision reason.
An excerpt cannot open or submit a correction. Whole-draft protection is not
universal exact-byte storage: current native correction trims input.
Fixture restart resets fixture state, not proof of native cold restart.

Gateway admission binds its loopback Host/Origin and a random port-named
HttpOnly/SameSite=Strict cookie; visit the page first. POST also requires Origin.
Only browser-used exact RPC shapes and thread/typed-ID-bound replies are exposed.
This is not a network service or hostile-local-process/multi-user boundary.
Never expose/proxy it onto a network.

## Tests and final formatting

Dependency-free checks:

```powershell
npm --prefix clients/stateful-codex test
python -m unittest discover -s scripts -p test_scodex.py -v
```

Use `python3` on Unix. Fixture HTTP tests do not qualify native writes/models.
For Rust, use **`just test -p <changed-crate>`**, never direct `cargo test`.
Examples: `codex-project-intelligence`, `codex-stateful-extension`,
`codex-stateful-runtime`, `codex-project-store`, `codex-stateful-memory`,
`codex-app-server-protocol`, `codex-app-server`, `codex-tui`.
Run only affected crates, not every example. The root
[justfile](../../justfile) uses nextest `local`, 8 MiB stack and no-fail-fast.
After scoped checks, common/core/protocol changes need full `just test` only
with explicit separate approval. Avoid routine `--all-features`.

Agent behavior changes require public integration tests. Memory coverage includes
wrong projects, stale revisions, UTF-8 bounds, retirement replay and effective reads.
Context coverage includes first requests after compaction, reset/checkpoint/fork,
bounded rendering and authority. Follow upstream auto-environment builders and
sibling `*_tests.rs` conventions in AGENTS.

TUI changes require reviewed insta snapshots. From `codex-rs`, run the affected
tests, `cargo insta pending-snapshots -p codex-tui`, then
`cargo insta show -p codex-tui <snapshot.snap.new>`. Accept a whole crate only when
every pending change is intended. Browser snapshots are in
[test/snapshots](../../clients/stateful-codex/test/snapshots); comparisons normalize
CRLF. For intentional memory-render updates set `UPDATE_SNAPSHOTS=1`, run
`node --test clients/stateful-codex/test/memory-view.test.mjs`, clear the variable,
and inspect the entire diff.

| Source change | Generator from root |
| --- | --- |
| Config types | `just write-config-schema` |
| App-server API | `just write-app-server-schema` |
| Experimental fixtures | `just write-app-server-schema --experimental` |
| Rust dependencies/lock | `just bazel-lock-update` |

Commit generated outputs with their source changes. Compile-time files also need
appropriate Bazel data; Cargo success does not establish Bazel visibility.

Finish tests/generators/snapshot review, then scoped `just fix -p <crate>` for
large Rust changes, then **`just fmt` inside `codex-rs`**, including documentation
changes. Do not rerun tests after fix/fmt merely for reassurance.
Inspect all formatter changes; never delete evidence or user work as cleanup.

**Windows formatting:** long checkouts previously produced a 38,636-character
rustfmt command over 342 roots, exceeding the 32,767-character process limit
(error 206), despite individual paths fitting. Short LF clones passed full fmt.
CRLF inside multiline Bazel strings previously became literal `\r`; LF attributes
now protect Bazel/Starlark inputs. `core.longpaths` alone fixes neither issue.
Scoped `cargo fmt -p <crate> -- --config imports_granularity=Item` is a tested Rust
fallback, not a full-format pass; stable-rustfmt option warnings are separate.

## Contributing and upstream policy

Target [Stateful main](https://github.com/dl1683/stateful-codex), not OpenAI's
repository. Main is the only maintained product branch; short-lived contributor
PR heads are fine, archives are recovery refs. Keep changes bounded, state
invariants and exact validation, inspect staged paths and exclude private data.
Preserve configured Git author/committer identity.

Document the validation performed and its limits. A contract change does not
convert an earlier failed acceptance into a pass. Independent clean-clone
newcomer review remains outstanding.

Upstream changes enter main as pinned reviewed/tested batches, preserving Stateful
context/retry/delivery/history/API behavior. Checked, fetched, merged, validated
and promoted are distinct. B0 and populated migration 59/60 remain held.
Never overwrite main, rewrite applied migration checksums, merge whole failing
archives behind flags or publish historical/private tags without authority.

Fresh state databases apply upstream migrations through 61, then the fork's
unchanged owner-token and project-binding SQL as 62 and 63. This avoids the
upstream 59/60 collision. Existing fork 59/60 databases refuse rather than
silently rewriting their applied ledger; use a disposable home while populated
conversion remains unqualified. Fresh/reopen and historical-store refusal
checks do not qualify that conversion.

For missing companions, inspect pairing/`CODEX_BIN`; for lock waits, wait; for V8,
verify the matched archive/binding; for migration refusal, preserve the old store
and use fresh scratch; for missing browser memory, inspect startup home routing.
These are diagnostics, not permission to bypass a guard.
