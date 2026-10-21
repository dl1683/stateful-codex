# Development

Start at the [reading map](./README.md#reading-map). These instructions describe
source baseline `997007527c5bb1d03c4d6b67cd5f8786b37b3e17`, 2026-10-20.
Verification resumed 2026-10-21 at the test/fixture review-fix pin recorded in
[current state](./STATEFUL_CODEX_CURRENT_STATE.md).
Do not confuse the workspace package version, `0.159.2`, with a tested release.

## Tools and platform evidence

| Tool | Repository requirement / documentation environment |
| --- | --- |
| Rust | **1.95.0**, pinned in [rust-toolchain.toml](./codex-rs/rust-toolchain.toml), with rustfmt, clippy and rust-src |
| Node | `>=22`; verified **22.19.0** for the dependency-free web client |
| pnpm | **10.34.5**, pinned with integrity hash in [package.json](./package.json); not needed for the web client |
| Python | Python 3; verified **3.13.7** for launcher tests and recipes |
| just | Verified **1.57.0** |
| cargo-nextest | Verified tool availability **0.9.140** |
| cargo-insta | Verified tool availability **1.48.0** |
| Formatting | `uv` and DotSlash; DotSlash resolves the pinned buildifier manifest in `tools/buildifier` |
| Build tools | Git, C/C++ compiler/linker, CMake, platform SDK, network access to Cargo and native build artifacts |

**Windows native:** use the matched OpenAI V8 archive **and binding** in
[the Windows recipe](#windows-v8-archive-and-binding). The default Deno download
returns 404 for the required sandbox variant. Tool discovery, Cargo metadata,
web tests and launcher tests are verified. A fresh native paired build and
disposable CLI/app-server initialization **PASS**; login/model/memory acceptance
is separate and remains **UNVERIFIED**.
**WSL2/Linux/macOS: UNVERIFIED** here. Earlier component witness documents are
historical evidence, not verification of these newcomer instructions.

Install Rust through your organization's trusted rustup distribution. The
following setup commands are **UNVERIFIED installations** in this pass because
the tools were already present:

```text
rustup toolchain install 1.95.0 --component rustfmt --component clippy --component rust-src
cargo +1.95.0 install --locked just --version 1.57.0
cargo +1.95.0 install --locked cargo-nextest --version 0.9.140
cargo +1.95.0 install --locked cargo-insta --version 1.48.0
```

Install `uv`, DotSlash and ripgrep from trusted distributions if missing.
Use pnpm 10.34.5 for root/SDK maintenance, not an arbitrary newer pnpm.
`pnpm install --frozen-lockfile` is **UNVERIFIED/not needed** for the
dependency-free client. Do not change dependency locks merely to run its tests.

## Before any Rust build or Rust test

On your own machine, ensure no other Cargo/rustc build is running before
starting. Check all checkouts, not just this one:

```powershell
# Windows PowerShell:
Get-Process -Name cargo,rustc -ErrorAction SilentlyContinue | Select-Object Id, ProcessName
```

```bash
# WSL/Linux/macOS:
ps -A -o pid=,comm= | grep -E '(^|[[:space:]/])(cargo|rustc)$'
```

No matching rows means no Cargo/rustc process was found at that moment; the
Unix check exits 1 when there are no matches. If a build is running, wait for it
to finish, then check again. Run one build-producing command at a time.
Check available RAM and disk; use `-j 1` (or `CARGO_BUILD_JOBS=1`) if memory is
limited. The recipes below use one job conservatively.

Only on a shared maintainer machine, coordinate with whoever else uses it
before building so that one build-producing command runs machine-wide.
A process check does not replace that shared-machine coordination. Contributors
on their own machines do not need an internal owner, queue or reservation.

Never kill Rust processes to release a Cargo lock. Wait for the existing build
or coordinate with its owner; never delete lockfiles or target trees as a
workaround. The complete test suite requires separate maintainer approval;
this documentation task permits scoped checks only.

## Build both binaries

On Windows, first follow the short-clone and V8 setup below in the same shell.
The earlier build without those overrides failed. WSL/Linux/macOS execution
remains **UNVERIFIED**:

```powershell
# From the repository root, in a VS Developer PowerShell on Windows:
cd codex-rs
$env:CARGO_BUILD_JOBS = "1"
cargo build --locked -j 1 -p codex-cli -p codex-code-mode-host
cd ..
```

```bash
# WSL/Linux/macOS, UNVERIFIED:
cd codex-rs
export CARGO_BUILD_JOBS=1
cargo build --locked -j 1 -p codex-cli -p codex-code-mode-host
cd ..
```

Default artifacts are `codex-rs/target/debug/codex[.exe]` and
`codex-rs/target/debug/codex-code-mode-host[.exe]`. Keep the pair together and
build both from the same checkout. The client and Python launcher expect this
debug layout; `CARGO_TARGET_DIR`, another profile or a cross-target build changes
it. The web client can instead use an absolute `CODEX_BIN` pointing at a matched
pair. The Python launcher has no equivalent binary-path override.

Optional neutral reader, **UNVERIFIED build**:

```text
cargo build --locked -j 1 -p codex-stateful-memory
```

Run that command inside `codex-rs`. Its executable is `stateful-memory[.exe]`,
not `codex-stateful-memory`.

### Native dependencies and V8

- Windows: use VS 2022 Build Tools, Desktop development with C++, a Windows SDK,
  and CMake. A `link.exe` found in Git's Unix tools is not the MSVC linker.
  Use a Developer PowerShell so the correct compiler/SDK are available.
- WSL2/Linux: install a C/C++ toolchain (`build-essential` on Debian-family
  systems), `pkg-config`, OpenSSL development headers and CMake. Build on the
  Linux filesystem using Linux tools; do not mix Windows and Linux target trees.
- macOS: install Xcode Command Line Tools and CMake, and use the host's matching
  Rust target. Intel and Apple Silicon builds are distinct artifacts.
- [Cargo.toml](./codex-rs/Cargo.toml) pins rusty V8 to **150.4.0**.
  Code-mode runtime enables the V8 sandbox. A cold build can download a large
  platform-specific prebuilt V8 archive; Cargo cache availability alone does
  not prove that archive is available. Do not disable the sandbox or substitute
  another V8 version to make a build pass.
- The default Windows build requests
  `rusty_v8_ptrcomp_sandbox_release_x86_64-pc-windows-msvc.lib.gz`.
  The default URL is
  `https://github.com/denoland/rusty_v8/releases/download/v150.4.0/rusty_v8_ptrcomp_sandbox_release_x86_64-pc-windows-msvc.lib.gz`.
  It returns **HTTP 404**. Deno's v150.4.0 Windows release matrix does not enable
  pointer compression/sandbox; current release metadata lists non-sandbox and
  SIMDUTF variants. There is no evidence here of a moved/removed sandbox asset.
  This is a missing feature variant at the default publisher, not a wrong
  Cargo.lock version or a Python problem.
- Toolchain/version, proxy/certificate access and native artifact availability
  must be recorded with any build result. Offline appliance readiness is held.

These platform prerequisites are source-informed, **not tested provisioning
recipes**. Report an actual missing library/archive with its command and error
rather than weakening policy.

### Windows V8 archive and binding

Use a **short checkout path**, for example `C:/src/stateful-codex`, with LF
checkout bytes. Choose an unused destination; do not overwrite an old clone:

```powershell
git -c core.autocrlf=false -c core.longpaths=true clone --origin stateful --single-branch --branch main --no-tags https://github.com/dl1683/stateful-codex.git C:/src/stateful-codex
cd C:/src/stateful-codex
```

The network clone itself was not tested; verification used a fresh local clone
of intake `3e831c98ecfc7136f500feb1f7426c931aad1fde`, no copied target tree.
`core.longpaths` addresses Git filesystem paths, not Windows command-line limits.

The registry crate is **`v8` 150.4.0** (rusty_v8 is the upstream project name),
locked with checksum
`42a978ff11f15b24e5c05a7123cf2b68f41e763546699781a924ef4e2cf43a49`.
Its build script accepts `RUSTY_V8_ARCHIVE` as an exact HTTPS URL or local
gzip/raw archive path and `RUSTY_V8_SRC_BINDING_PATH` as a local Rust binding.
Both are needed: the crate's packaged `gen/` has no matching Windows sandbox
binding. `RUSTY_V8_MIRROR` only replaces the base, retaining `/v150.4.0/`;
it cannot directly select OpenAI's `rusty-v8-v150.4.0` tag layout.
Without overrides it also checks Cargo home's `.rusty_v8/<escaped-URL>` cache;
`gn_out/obj/rusty_v8.lib` alone is not proof of a matched build.

The following download and verification were executed. They use the same
publisher, names and trusted manifest as
[Cargo CI setup](./.github/actions/setup-rusty-v8/action.yml), not an arbitrary
third-party binary. Run from the repository root:

```powershell
$ErrorActionPreference = "Stop"
$repo = (Get-Location).Path
$artifacts = Join-Path ([IO.Path]::GetTempPath()) ("stateful-v8-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $artifacts | Out-Null
$base = "https://github.com/openai/codex/releases/download/rusty-v8-v150.4.0"
$stem = "rusty_v8_ptrcomp_sandbox_release_x86_64-pc-windows-msvc"
$manifest = "$stem.sha256"
Invoke-WebRequest "$base/$manifest" -OutFile "$artifacts/$manifest"
$trusted = Join-Path $repo "third_party/v8/rusty_v8_150_4_0_release_manifests.sha256"
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
```

Verified SHA-256 values, 2026-10-21:

| Input | SHA-256 |
| --- | --- |
| Manifest | `a4d6221dddb4b5724b23411eaac47caf6095489fbf9d126f65b33cef96a0a8ef` |
| `.lib.gz` archive | `732ec5da4243aa166799780c8519a5eea6f32f6e47657a323342794dc3c239d6` |
| `src_binding_ptrcomp_sandbox_release_x86_64-pc-windows-msvc.rs` | `dabf78ba1faac127660db9862b1d0354175c71b8db2d4fcb5bacbd9c93576b16` |
| Decompressed library | `00baffcf54b30fe198c3e5cc40ec88674ff99cac2b8b60f8e325efce3ce771bb` |

Earlier successful-build caches contained the same archive/binding and identical
decompressed libraries. They were only read and hashed, not reused as build
outputs or changed. Fresh public downloads matched them, so the recipe does not
depend on private machine artifacts. No source build or dependency fallback is
needed for this archive. Keep the sandbox enabled.

Enter the installed VS Developer environment explicitly if this is an ordinary
PowerShell, preserving the overrides in the same process:

```powershell
$vswhere = "${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
$vs = & $vswhere -latest -products "*" -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$vs) { throw "Install VS Build Tools with Desktop development with C++ first" }
Import-Module "$vs/Common7/Tools/Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments "-arch=x64 -host_arch=x64"
cd "$repo/codex-rs"
cargo build --locked -j 1 -p codex-cli -p codex-code-mode-host
cd $repo
```

#### Verified Windows build and startup, 2026-10-21

Fresh short local clone at intake
`3e831c98ecfc7136f500feb1f7426c931aad1fde`, no copied target directory.
Executed **`cargo build --locked -j 1 -p codex-cli -p codex-code-mode-host`**
inside `codex-rs`, with Rust 1.95.0, the VS Developer environment and both
verified V8 overrides above. Cargo reported **success**, `Finished dev profile`
in **60m 02s**; the recording wrapper captured Cargo exit **0** after 3,603.93s.
The background tool's outer status was unavailable at its one-hour boundary;
the completed Cargo output, wrapper record and both binaries establish this
result, not that outer status. Two-second RAM samples had minimum **17.87 GiB**
free; sampling does not prove an instantaneous continuous minimum.

| Fresh debug binary | Bytes | SHA-256 |
| --- | ---: | --- |
| `codex.exe` | 364251136 | `985e1d234638e35e1b07155d72fe3eb9674bd896bd231b6e5ae5b15b315ebd13` |
| `codex-code-mode-host.exe` | 103872000 | `c20fac923914074055826ea7eede685eaac2055b54d8b4cd74bc62d8610391dd` |

These identify this local debug build, not reproducible release-binary hashes.
After building, startup used a new empty Git project and empty native home,
file-only ChatGPT credential storage and disabled global memories. No old home,
login, template credential or target output was copied. Exact native commands:

```text
codex.exe --version
codex-code-mode-host.exe --help
codex.exe -C <disposable-project> login status
codex.exe app-server
```

The last command ran with cwd `<disposable-project>` and `CODEX_HOME` plus
`CODEX_SQLITE_HOME` pointing to `<disposable-home>`. `OPENAI_API_KEY` and
`CODEX_API_KEY` were removed from the child environment. Python's
`subprocess.run(..., input=json.dumps(request) + "\n", capture_output=True)`
sent this initialize request, then EOF for normal shutdown:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"docs3-smoke","version":"1"},"capabilities":{}}}
```

Results: version **`codex-cli 0.159.2`, exit 0**; companion help **exit 0**;
login status **`Not logged in`, exit 1**, as expected for the empty home;
app-server returned initialize result **id 1**, reporting Windows and the
disposable home, then exited **0**. It also warned that historical-rollout
backfill was pending and filesystem fallback was active. This verifies native
CLI startup, not backfill completion, a model answer, interactive TUI/browser
use, V8 cell execution, memory persistence or Forget/replay. Real authentication
and those product journeys remain **UNVERIFIED**. WSL/Linux/macOS remain
**UNVERIFIED**; they are not needed as a fallback for this Windows build.

### Windows formatting

The cold-follow error 206 was reproduced: Cargo fmt batched **342 Rust roots**
into one **38,636-character** rustfmt command. The longest individual root was
only **142 characters**. This exceeds Windows' 32,767-character process command
limit, not MAX_PATH for an individual source. Git's `core.longpaths=true`
cannot fix that. A short checkout reduced the equivalent command to about
22,562 characters; full `just fmt` exited **0**.

Separately, a CRLF checkout made buildifier turn carriage returns *inside*
multiline strings into literal `\r` escapes in four files. Formatter concurrency
did not cause those bytes: buildifier was processing CRLF strings. The
[attributes](./.gitattributes) now force LF for Bazel/Starlark inputs. A fresh
short clone with `core.autocrlf=true` plus these attributes passed `just fmt`
with **no content diff**, including those four files. Git status can retain
stat/line-ending noise; inspect `git diff`, not only exit status.

Prefer a fresh short LF clone over rechecking out a dirty tree. Changing
`core.autocrlf` after checkout does not rewrite existing CRLF bytes. In an
existing long checkout the tested scoped Rust alternative is, from `codex-rs`:

```powershell
cargo fmt -p codex-cli -- --config imports_granularity=Item
```

Substitute the changed crate and run its other applicable formatter groups.
This is not a full-format pass. Stable rustfmt warns that the pinned
`imports_granularity` option requires nightly; that warning is not error 206.
Keep the required final `just fmt` gate in a supported short checkout.

## Disposable CLI on Bash, UNVERIFIED

From the repository root after the build:

```bash
repo="$PWD"
scratch="$(mktemp -d)"
project="$scratch/project"
mkdir "$project"
git init --quiet "$project"
python3 "$repo/scripts/scodex.py" -C "$project" login
python3 "$repo/scripts/scodex.py" -C "$project" --stateful ask \
  "Describe this empty disposable project."
```

The [PowerShell equivalent](./README.md#native-cli-unverified) and
[web walkthrough](./clients/stateful-codex/README.md) use the same pairing rule.
Keep the scratch directory until you have reviewed results; no cleanup/deletion
is required by these instructions. A new shell restores ordinary environment
selection. Never copy private homes into a repository or PR.

## Troubleshooting

| Symptom | Check before changing anything |
| --- | --- |
| Missing code-mode host | Build/install the companion beside the selected CLI. Inspect `CODEX_BIN`; do not accidentally run released upstream Codex. |
| Rust lock wait or memory pressure | Wait for the existing build; coordinate with other users only on a shared machine. Use one Cargo job when memory is limited. Never kill Rust, delete caches, or start a second build to bypass a lock. |
| V8 archive download fails | Use both verified Windows archive/binding overrides above. Record exact artifact/target and any hash/network failure; never substitute a non-sandbox archive. |
| V8 invokes the Microsoft Store Python alias | Set `PYTHON` to an absolute working Python executable. Interpreter selection does not repair the default 404; use the matched archive/binding recipe. |
| Store migration/checksum refusal | Preserve the store and error. Use a fresh disposable home; never edit checksums, delete databases or rebuild valuable history. |
| No project memory in browser | Verify home routing before gateway startup. Selecting a project in the page does not relocate the app-server home. |
| Login required | Gateway forces ChatGPT login in the selected home. A separate disposable home does not inherit your ordinary login. |
| Tracked home / ignore-policy refusal | Do not force the launcher through it. Use an untracked private directory; inspect why `.gitignore`/`.ignore` differs from `*`. |
| Web snapshot differs only by CRLF | Tests normalize CRLF. Inspect the actual snapshot diff; do not blindly regenerate. |
| Port already used | Choose another `STATEFUL_CODEX_PORT`; use the exact printed loopback origin. |

See [testing](./TESTING.md) for validation and final formatting order.
