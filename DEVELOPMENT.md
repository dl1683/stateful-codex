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

**Windows native:** tool discovery, Cargo metadata, web tests and launcher tests
verified. Visual Studio 2022 Build Tools with x64/x86 C++ tooling was detected.
**Native build FAILED** at the pinned V8 artifact download (HTTP 404);
**native run UNVERIFIED**. The earlier resource refusal is historical.
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

Coordinate **one build-producing command machine-wide**, not one per checkout.
Use one Cargo build job. The resumed documentation task requires **more than
12 GiB free RAM** throughout the operation, with extra headroom for the
compiler/linker. Its admission check reserves another 4 GiB before starting.
The earlier documentation pass used a stricter 20 GiB floor and refused a
cold build at 20.11 GiB free; that is historical, not the current task's gate.
B0 has an additional historical command-specific 23,221,225,472-byte admission
gate; it is not a replacement for current resource inspection.

Check available memory, disk, and the shared build-slot owner before starting.
Do not kill Rust processes to release a Cargo lock. Wait for their owner; never
delete lockfiles or target trees as a workaround. The complete test suite
requires separate owner approval and is prohibited for this documentation task.

## Build both binaries

The Windows command was executed with one job in VS Developer PowerShell and
**failed** because the required V8 archive is unavailable. It produced no paired
CLI/companion. WSL/Linux/macOS execution remains **UNVERIFIED**:

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
- The Windows attempt requested
  `rusty_v8_ptrcomp_sandbox_release_x86_64-pc-windows-msvc.lib.gz`.
  That exact v150.4.0 release asset returns **HTTP 404**; release metadata lists
  Windows non-sandbox/SIMDUTF variants, not this required variant. The downloader
  also selected a Microsoft Store `python3` alias; use an actual Python
  executable via `PYTHON` if needed, but that cannot fix a missing archive.
  No sandbox weakening, dependency replacement or V8 source build was attempted.
- Toolchain/version, proxy/certificate access and native artifact availability
  must be recorded with any build result. Offline appliance readiness is held.

These platform prerequisites are source-informed, **not tested provisioning
recipes**. Report an actual missing library/archive with its command and error
rather than weakening policy.

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
| Rust lock wait or memory pressure | Ask the build-slot owner; wait. Do not kill Rust, delete caches, or start a second build. |
| V8 archive download fails | Record the build failure and exact artifact/target; check trusted network/proxy and cache access. Native execution remains UNVERIFIED. |
| V8 invokes the Microsoft Store Python alias | Set `PYTHON` to an absolute working Python executable in the build shell. Check the archive separately; the observed 404 still blocks this Windows build. |
| Store migration/checksum refusal | Preserve the store and error. Use a fresh disposable home; never edit checksums, delete databases or rebuild valuable history. |
| No project memory in browser | Verify home routing before gateway startup. Selecting a project in the page does not relocate the app-server home. |
| Login required | Gateway forces ChatGPT login in the selected home. A separate disposable home does not inherit your ordinary login. |
| Tracked home / ignore-policy refusal | Do not force the launcher through it. Use an untracked private directory; inspect why `.gitignore`/`.ignore` differs from `*`. |
| Web snapshot differs only by CRLF | Tests normalize CRLF. Inspect the actual snapshot diff; do not blindly regenerate. |
| Port already used | Choose another `STATEFUL_CODEX_PORT`; use the exact printed loopback origin. |

See [testing](./TESTING.md) for validation and final formatting order.
