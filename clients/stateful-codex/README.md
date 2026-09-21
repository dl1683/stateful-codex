# Stateful Codex client

This local browser client talks to the real Codex app-server. It starts a
loopback-only gateway and reuses the ChatGPT account cached by `codex login`;
it does not accept or store an API key.

Build the branch CLI and its code-mode companion once, then start the client:

```powershell
cd codex-rs
cargo build -p codex-cli -p codex-code-mode-host
cd ..\clients\stateful-codex
node server.mjs
```

Open `http://127.0.0.1:4173`. Set `CODEX_BIN` only when the branch CLI is in a
different location. Both executables must be installed beside each other, as
they are in a normal Codex package. The gateway removes `OPENAI_API_KEY` and
`CODEX_API_KEY` from its child environment and forces the Codex ChatGPT login
method.

## Rollout comparison

Compare an ordinary Codex rollout with a Stateful rollout only after running the
same prompt through the same entry point, model, working directory, and
permissions. The scorer verifies the model, reasoning effort, originator,
source, approval and sandbox policy, permission profile, workspace roots,
directory, and normalized prompt before treating the pair as comparable:

```powershell
npm run eval -- --baseline <baseline.jsonl> --stateful <stateful.jsonl> `
  --expect "8 regular files" --expect "styles.css"
```

The report keeps full lifetime tokens separate from uncached tokens, checks the
expected answer terms, and reports read-bearing tool calls plus Stateful
retrieval and persistence calls. A read-bearing call is a rollout-level proxy,
not an exact count of operating-system reads. A comparison that fails parity
exits with status 2; a run that misses an expected term exits with status 3.
