# Configuration and storage selection

The complete key/type authority is the generated
[config schema](./codex-rs/core/config.schema.json), backed by
[ConfigToml](./codex-rs/config/src/config_toml.rs) and
[runtime resolution](./codex-rs/core/src/config/mod.rs). A schema property without
a `default` does **not** imply a runtime default. The table distinguishes them.
Examples were parsed as TOML and validated against this schema on Windows;
native configuration loading is **UNVERIFIED** in this pass.

## Common keys

| Key | Type / omission behavior |
| --- | --- |
| `model` | String; no universal schema default. Availability depends on provider/account. The Python launcher separately supplies `gpt-6-luna` unless a model flag is present. |
| `model_provider` | String; runtime chooses the provider. Do not assume every bridge supports every role/tool. |
| `forced_login_method` | `chatgpt` or `api`; unset imposes no explicit method restriction. Gateway supplies `chatgpt` at launch. |
| `cli_auth_credentials_store` | `file`, `keyring`, `auto`, `ephemeral`; type default `file`, with runtime/package/managed-policy resolution. File credentials live in the selected Codex home. |
| `sqlite_home` | Absolute path; explicit/managed config precedes `CODEX_SQLITE_HOME`, then the Codex home. It selects native SQLite files, not the working project. |
| `model_context_window` | Optional integer token count; no fixed schema default, normally model metadata. |
| `model_auto_compact_token_limit` | Optional integer threshold; no fixed schema default. It is not a guarantee of exact recall. |
| `model_auto_compact_token_limit_scope` | `total` or `body_after_prefix`; runtime default `total`. |
| `model_post_turn_compact_threshold_percent` | Integer 0–100 at runtime; omission resolves to 0 (disabled). |
| `approval_policy`, `sandbox_mode` | Policy-controlled enums; use organization-approved values. Disposable storage does not bypass system/cloud requirements. |
| `[memories] generate_memories` | Boolean, runtime default `true`; upstream memory pipeline, not a switch for all Stateful blackboards. |
| `[memories] use_memories` | Boolean, runtime default `true`; upstream memory-use instructions, not a global Forget/purge switch. |
| `[memories] version`, `dual_write`, `dedicated_tools` | Runtime defaults `v1`, `false`, `false`. This pipeline is distinct from project intelligence. |

Memory defaults come from
[MemoriesConfig](./codex-rs/config/src/types.rs); compaction scope from
[protocol config types](./codex-rs/protocol/src/config_types.rs).
Unknown keys can be refused with native `--strict-config`. No Stateful feature
should be activated by inventing a TOML key named `stateful`.

Schema-validated minimal `config.toml` for a private scratch home:

```toml
cli_auth_credentials_store = "file"
forced_login_method = "chatgpt"

[memories]
generate_memories = false
use_memories = false
```

This reduces upstream memory-generation/use paths for the walkthrough. It does
not disable project intelligence or establish complete non-retention. Native
threads and rollouts can still persist. Do not put authentication bytes in TOML.

## Four different selections

1. **Working directory/project:** `-C`/`--cd` selects a directory.
   Stateful `--stateful-project <id>` disambiguates already-known projects when
   necessary; it does not move storage.
2. **Codex home:** `CODEX_HOME` selects config, authentication and other native
   home state. It must already exist if explicitly set. Otherwise it is
   `~/.codex`. Direct CLI/gateway use does not automatically choose `.stateful`.
3. **Native database home:** `sqlite_home`, then `CODEX_SQLITE_HOME`, then
   `CODEX_HOME`. Managed requirements remain authoritative.
4. **Launcher/neutral-reader override:** `STATEFUL_STORE_ROOT` is consumed by
   [scodex.py](./scripts/scodex.py) and the
   [neutral native reader](./codex-rs/stateful-memory/src/native.rs).
   It is **not** a general environment setting understood by the native CLI.

The launcher resolves a Git subdirectory to its Git worktree root, otherwise to
the chosen plain directory. Its default root is `<project>/.stateful`; a relative
`STATEFUL_STORE_ROOT` is relative to that project, and an absolute override is
used as-is. It refuses a project/ancestor/shared-template home and tracked home
content. Ignore policies precede optional missing config/auth seeding from
`~/.codex-stateful`. It passes both native home variables and a `sqlite_home`
CLI override, without importing old knowledge.

For strict isolation from any existing template, use the explicit-home
walkthrough in the [client guide](./clients/stateful-codex/README.md#native-startup-unverified),
not the template-seeding launcher.

## Native versus canonical stores

The landed launcher uses unchanged native files such as
`project_intelligence_1.sqlite`, `stateful_runtime_1.sqlite`, `state_5.sqlite`,
`thread_history_1.sqlite` and `sessions/`. File creation is lazy and not every
home contains every file. Per-project placement is not a one-file conversion.

The separate [project-store library](./codex-rs/project-store/src/location.rs)
resolves `.stateful/memory.sqlite` with identities/domain manifests. Those
components are present but native conversion/automatic migration is **inactive**.
Do not initialize that store and expect it to replace the native databases.

The optional reader's `native-root` / `native-read` operations inspect an
existing native database without migration. `root` is layout-sensitive; prefer
`native-root` when you intend native inspection. Canonical `read` is a bounded
attributed historical read, not the same as effective native memory.

## Web-only environment

| Variable | Meaning |
| --- | --- |
| `CODEX_BIN` | Selected CLI for the gateway; otherwise local debug CLI, then `codex` on PATH |
| `STATEFUL_CODEX_PORT` | Loopback port, default `4173` |
| `CODEX_HOME`, `CODEX_SQLITE_HOME` | Must be selected **before** the gateway/app-server starts |

The gateway strips API-key environment variables and forces ChatGPT login.
It does not erase other configuration or override managed requirements.
Use one gateway per disposable home/project; do not switch valuable project
homes in a running process. Keep private stores out of Git and search indexes.
