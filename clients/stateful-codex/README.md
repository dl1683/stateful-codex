# Stateful Codex client

This local browser client talks to the real Codex app-server. It starts a
loopback-only gateway and reuses the ChatGPT account cached by `codex login`;
it does not accept or store an API key.

Build the branch CLI once, then start the client:

```powershell
cd codex-rs
cargo build -p codex-cli
cd ..\clients\stateful-codex
node server.mjs
```

Open `http://127.0.0.1:4173`. Set `CODEX_BIN` only when the branch CLI is in a
different location. The gateway removes `OPENAI_API_KEY` and `CODEX_API_KEY`
from its child environment and forces the Codex ChatGPT login method.
