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

## User-confirmation boundary

`Confirm this understanding` is a trusted-client action, not a model tool. The
browser sends only the displayed entry ID and revision; the app-server
preserves the entry's meaning and evidence and issues the `userConfirmed`
grade. Model-facing blackboard record/update tools cannot select that grade,
and the generic upsert API cannot manufacture it.

This is an application authority boundary, not proof against a process with
arbitrary host control. The gateway stays loopback-only and normal sandboxed
agent commands cannot use the network. A process explicitly granted
unsandboxed host access could impersonate local clients or edit the state store
directly, so work performed with that authority is outside the provenance
guarantee. Clients must invoke `blackboard/confirm` only in direct response to
an explicit user action.

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
prohibited conclusions. Cases may require the same semantic coverage in the
Stateful completion result and returned completion basis, preventing a correct
visible answer from hiding lost project knowledge. A live API read remains the
authoritative persisted-result check. The report aggregates follow-up usage,
adds every recorded project-maturation rollout, and reports only a projected
break-even when the observed average follow-up saving is positive:

```powershell
npm run eval:series -- --manifest eval/manifests/licensing-series.json `
  --maturation-rollout <licensing-maturation.jsonl> `
  --pair economics=<ordinary.jsonl>,<stateful.jsonl> `
  --pair territory=<ordinary.jsonl>,<stateful.jsonl> `
  --pair termination-risk=<ordinary.jsonl>,<stateful.jsonl>
```

Repeat `--maturation-rollout` for each independently matured project in a
multi-project distribution. Omitting it uses the frozen aggregate recorded in
the manifest. A manifest may set `expectedMaturationRollouts` to reject an
accidentally incomplete lifetime-cost calculation.

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
