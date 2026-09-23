# Harbor matched-agent evaluation

This adapter compares the branch binary with itself. `BundledCodex` runs
ordinary Codex and `StatefulCodex` adds only `--stateful autonomous`. Both use
the same model, task container, content-addressed bundle, Codex login, Codex
configuration, and Harbor trajectory converter.

The integration is pinned to Harbor commit
`15da91c18580a25489f5bdf2ee71029f3ff3bb2e`. A different Harbor revision is a
protocol change and requires revalidation before a scored run.

## Build the Linux bundle

Build with Docker from any checkout where Bash is available. The worktree must
be clean so the SHA-256 can identify an exact source commit. The builder uses
Rust 1.95 on Debian Bullseye (glibc 2.31), rather than inheriting a potentially
newer host glibc.

```bash
bash clients/stateful-codex/eval/stateful_harbor/build-portable-linux-bundle.sh
```

The script builds `codex` and `codex-code-mode-host`, creates a deterministic
package under `clients/stateful-codex/eval/artifacts/`, and writes its SHA-256
sidecar. Docker BuildKit caches the Rust and V8 inputs between builds. Binaries
are deliberately not committed.

Before Harbor can invoke a model, run the archive in the exact task image:

```bash
bash clients/stateful-codex/eval/stateful_harbor/preflight-linux-bundle.sh \
  clients/stateful-codex/eval/artifacts/stateful-codex-<commit>-x86_64-unknown-linux-gnu.tar.gz
```

The preflight verifies the digest, extracts the archive inside the task image,
and starts both packaged binaries. A compatibility failure therefore stops the
evaluation before authentication or model usage.

## Configure Harbor

Make the adapter importable and point it to the exact bundle and the normal
Codex login. Do not set an API key for this comparison.

```powershell
$env:PYTHONPATH = "$PWD\clients\stateful-codex\eval"
$env:STATEFUL_CODEX_BUNDLE_PATH = "<absolute path to the .tar.gz>"
$env:STATEFUL_CODEX_BUNDLE_SHA256 = "<64-character digest from .sha256>"
$env:CODEX_AUTH_JSON_PATH = "$env:USERPROFILE\.codex\auth.json"
Remove-Item Env:OPENAI_API_KEY -ErrorAction SilentlyContinue
Remove-Item Env:CODEX_API_KEY -ErrorAction SilentlyContinue
```

Use `stateful_harbor.stateful_codex:BundledCodex` for the ordinary arm and
`stateful_harbor.stateful_codex:StatefulCodex` for the Stateful arm. Keep the
task list, trial count, model, reasoning effort, timeout, and every other job
setting identical. For example:

```powershell
harbor run -d terminal-bench/terminal-bench-2-1 -i terminal-bench/fix-git -k 1 -n 1 -a stateful_harbor.stateful_codex:BundledCodex -m openai/gpt-5.6-luna --effort max
harbor run -d terminal-bench/terminal-bench-2-1 -i terminal-bench/fix-git -k 1 -n 1 -a stateful_harbor.stateful_codex:StatefulCodex -m openai/gpt-5.6-luna --effort max
```

Run one paired task first to validate that the only behavioral difference is
the Stateful flag. After that control succeeds, broader local runs may use only
`StatefulCodex` and compare descriptively with the published Codex result. Do
not describe that as a causal A/B result: the public run used a released Codex
binary rather than this branch build.

The current published GPT-5.6 Luna comparator is Codex 0.144.1 at max effort:
445 trials, 75.73% accuracy with 1.32 percentage-point standard error. The
content-addressed source is the Terminal-Bench 2.1
[`2026-07-11-openai-gpt-5-6-luna-max-codex.json`](https://github.com/harbor-framework/terminal-bench-2-1/blob/main/leaderboard/submissions/2026-07-11-openai-gpt-5-6-luna-max-codex.json)
submission, backed by Harbor job `4860a28f-bc1a-5367-9885-57ff9ccc3a15`.
Terminal-Bench 2.1 requires all 89 tasks and five trials per task for a full
score. Community leaderboard submissions are currently closed, so local runs
are evidence, not official leaderboard entries.

Each attempt preserves native Codex logs, an ATIF v1.7 trajectory, adapter
metadata containing the bundle and Harbor hashes, and `final.patch` when the
task workspace is a Git repository. The SQLite state home is outside the
transient Codex session home, so state survives resume or multiple steps inside
one trial. Harbor's fresh task container prevents state from crossing trial
boundaries.
