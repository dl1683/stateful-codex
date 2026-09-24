# BixBench evaluation adapter

This adapter runs the native Stateful Codex CLI as a custom agent on BixBench
v1.5. It preserves BixBench's question, capsule data, scientific environment,
and answer contract while replacing only the agent harness.

The first gate is deliberately small: `smoke-bix18.json` selects one question
from a 22 KB *P. aeruginosa* swarming-analysis capsule with a deterministic
metadata range verifier. Once that produces a valid executed notebook,
structured final answer, isolated Stateful database, and local verification,
`bix18-deterministic.json`
pre-registers all five independently state-isolated questions from that capsule
before a broader sample. `breadth10-deterministic.json` then pre-registers one
question from each of ten capsules: five range and five string verifiers across
imaging, transcriptomics, epigenomics, sequence analysis, clinical modelling,
network biology, machine learning, and genomic-variant analysis. Capsules were
selected before outcomes for verifier/category coverage and an operationally
manageable first-pass archive size (at most 10 MiB, rounded); this is a breadth
gate, not a representative estimate of the complete benchmark.

## Pinned inputs

- BixBench source: `FUture-House/BixBench` commit
  `49311180bdacb324c596f2e07596c126f2004008`.
- Dataset revision: `f8cc3bdcc6357c88b8c3648306522b9c422dc95a`.
- Dataset metadata SHA-256:
  `0d1204dcdae7193a9132ced5a3502008f6b3b163debc1b65b2aa2d86cb132dc9`.
- Scientific environment source: `Future-House/data-analysis-crow` tag
  `v1.5.0`, commit `953a6b13c5f8e15354525bca08636f620994f954`.

FutureHouse's published `futurehouse/bixbench:aviary-notebook-env` manifest is
currently ARM64-only. On an AMD64 host, rebuild the environment from its pinned
Dockerfile rather than substituting a generic image. This is a source-pinned
rebuild, not the byte-identical published image:

```powershell
git clone --branch v1.5.0 --depth 1 https://github.com/Future-House/data-analysis-crow.git <fhda-root>
docker build --platform linux/amd64 `
  --tag stateful-bixbench-env:fhda-v1.5.0-amd64 `
  --file <fhda-root>/src/fhda/Dockerfile.pinned `
  <fhda-root>/src/fhda
docker build `
  --tag stateful-bixbench-agent:fhda-v1.5.0-amd64 `
  --file clients/stateful-codex/eval/bixbench/Dockerfile.agent `
  clients/stateful-codex/eval/bixbench
```

The derived compatible layer adds Git, ripgrep, jq, current CA certificates,
and `openpyxl==3.1.5`. The Excel reader is explicit because the upstream AMD64
source build cannot otherwise read `.xlsx` capsules. This is a practical local
environment for directional evaluation, not the official byte-identical
BixBench image; report results accordingly.

## Run the smoke

Build the content-addressed Stateful Linux bundle as described in
`../stateful_harbor/README.md`, then run:

```powershell
py -3.12 clients/stateful-codex/eval/bixbench/run_bixbench.py `
  --manifest clients/stateful-codex/eval/bixbench/smoke-bix18.json `
  --output-dir "$env:LOCALAPPDATA/Temp/stateful-bixbench-bix18-smoke" `
  --cache-dir "$env:LOCALAPPDATA/Temp/stateful-bixbench-cache" `
  --bundle clients/stateful-codex/eval/artifacts/stateful-codex-ef3a1ff83886-x86_64-unknown-linux-gnu-compat2.tar.gz `
  --bundle-sha256 a321910f2cae0ccfed8e3b412f4f3ef7a7ec22f62f683444655be6c76c6841ad
```

The runner uses the normal cached `~/.codex/auth.json`; it does not accept or
forward an API key. Raw data, agent and container diagnostics, executed
notebook, an official postprocessing-shaped record, isolated Stateful database,
token usage, hashes, and local verifier result remain under the selected output
directory. Native
Codex JSONL is retained as the action/event evidence; it is not mislabelled as
BixBench's Aviary trajectory format.

`--arm stateful` is the default. `--arm ordinary` runs the same question,
container, model, prompt, and output contract without Stateful mode, permitting
a matched control when one is explicitly useful; it is not required for every
Stateful run.

## Integrity boundary

- Standard runs use a fresh workspace and fresh Stateful database per question.
- The agent sees only the contents of the capsule's single `Data` directory;
  reference notebooks and other capsule files never enter its workspace.
- Answer keys and distractors remain on the host and are never mounted into the
  agent container.
- Cached and live web search, Codex memories, and known benchmark-source hosts
  are disabled. The raw event log is also audited for prohibited search or
  benchmark-source download attempts.
- Capsule archives and metadata are downloaded from the pinned dataset revision
  and rejected unless their SHA-256 values match the manifest.
- The machine-readable run and workspace manifests record the exact image ID,
  full Conda/Pip environment inventory and hash, agent bundle, prompt/schema
  hashes, harness and selection-manifest hashes, input file hashes, model,
  effort, timeout, state scope, and task list. Native rollout sessions are
  retained beside logs.
- Agent timeouts, ordinary nonzero exits, and missing or malformed answers
  count as incorrect. Failures before the Codex invocation boundary, detected
  provider/authentication outages, and positively identified container-engine
  disconnects are reported as invalid rather than incorrect.
- Submitted notebooks are checked structurally, then replayed from the
  untouched capsule inputs plus only the submitted notebook in the same image
  with Docker networking disabled. Agent-created helper artifacts cannot make
  a replay pass. Runtime package installation is an integrity violation; all
  dependencies must already exist in the recorded image. Replay validity
  remains separate from answer correctness.
- Stateful runs retain both SQLite databases. The result records database
  integrity, terminal run status, continuation count, and hierarchy, context-map,
  blackboard, relationship, and obligation counts.
- `llm_verifier` questions remain ungraded until an explicit judge protocol is
  selected. They are never silently scored with a substitute judge.
- String/range results are labelled `local_metadata_verifier`. The string
  diagnostic preserves upstream's punctuation-insensitive normalized match but
  rejects numeric sign or punctuation collisions that would change the value.
  BixBench's
  current official postprocessing uses an LLM judge for open answers, including
  range questions, so these local checks are diagnostics rather than official
  BixBench scores. The exported official-shaped trajectories can be judged
  later under a pinned official protocol.
- Result summaries keep local answer correctness separate from operational
  validity. A run is operationally valid only when Codex completes, returns the
  structured answer, submits a valid notebook, passes offline replay and the
  protocol audit, records its hashes, and (for the Stateful arm) leaves a
  terminal integrity-valid durable run. A correct local answer cannot hide a
  broken or irreproducible run.
- A future same-capsule persistent-state study must be labelled longitudinal;
  it cannot be mixed into the question-isolated BixBench score.
