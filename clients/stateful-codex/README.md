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

For a pre-registered sequence of follow-up questions, use `eval:series` with
one named pair per manifest case. The report checks semantic term groups and
prohibited conclusions, aggregates follow-up usage, adds the recorded
one-time maturation cost, and reports only a projected break-even when the
observed average follow-up saving is positive:

```powershell
npm run eval:series -- --manifest eval/manifests/licensing-series.json `
  --pair economics=<ordinary.jsonl>,<stateful.jsonl> `
  --pair territory=<ordinary.jsonl>,<stateful.jsonl> `
  --pair termination-risk=<ordinary.jsonl>,<stateful.jsonl>
```

## Project-state regression probes

Evaluate a mature project's structured blackboard against a versioned semantic
manifest through the live gateway:

```powershell
npm run eval:state -- --project <project-id>
```

The default procurement manifest checks expected decisions, decisive facts,
contradictions, and open questions. It reports semantic probe recall separately
from supported-entry precision. A supported entry must be active,
source-verified, linked to evidence, and current against the indexed source.
The command fails if the 50-entry snapshot is truncated, a probe is missing or
unsupported, an entry lacks current support, or a prohibited affirmative claim
appears. These deterministic probes are regression evidence, not a substitute
for independent semantic review or a held-out precision benchmark.
