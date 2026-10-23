# Stateful Codex evaluation

Canonical protocol and reproduction guide. [RESULTS](RESULTS.md) is the single
evidence ledger; [FAILURES](FAILURES.md) records decisions and do-not-repeat lessons.
These commands are not authority to start paid, cloud, authenticated or external
benchmark runs.
Historical September protocols do not override current held acceptance gates.

## Experimental integrity

- Freeze source/model/effort, prompts, corpus hashes, task selection, budgets,
  retry policy, tools, permissions, scorer, rubric and exclusions before outcomes.
  Preserve every selected case, interruption, failed attempt and repair.
- For causal A/B, change only the Stateful condition. Isolate project state and
  disable both upstream memory generation and use to prevent hidden prior state.
  Fresh-thread transfer, continuous threads, feedback-conditioned repeats,
  total-context stress and `body_after_prefix` diagnostics are different experiments.
- A project trajectory is the clustered unit, not each correlated question.
  The historical cohort selected six projects for domain/workload coverage,
  five complete trajectories plus a reserved replication. Frozen development
  procurement cases are not held-out samples. The template manifest is unexecuted
  placeholder tooling, never evidence that a cohort ran.
- Questions should include learning, reuse, decisive connections, contradictions,
  source revision, unknowns, natural compaction and separately labelled restart.
  Freeze order; do not replace an unfavorable project.
- Match model, source/build, originator, effort, auth mode, approvals/sandbox,
  permission profile, roots, directory and normalized prompts. Paths may differ
  only as declared isolated copies. Cached login use must not expose credentials.
- Freeze blinded answer packets separately from arm mapping and state grading.
  Seal answer grades before grading persisted state. Independent semantic review
  is necessary; literal terms/numbers are diagnostics, not semantic truth.
- Report full/cached/uncached/output/reasoning tokens, retries, compaction,
  capture/maturation/maintenance, wall time, TTFT and user effort. Reconcile
  response usage with turn/thread totals; never sum cumulative totals per question.
  Separate **MEASURED**, **ESTIMATE**, synthetic priced units, list-price
  equivalents and actual bills. Projected break-even is not observed saving.
- Preserve original failed results. Prospective repairs get new attempts; no
  post-outcome rubric tuning, normalization, discarded attempts or denominator swaps.
  Published unlike harness scores are descriptive, not causal or equivalent.

## Longitudinal tooling

Scripts live in [clients/stateful-codex/eval](../../clients/stateful-codex/eval);
commands here run from `clients/stateful-codex`. Uppercase values and ellipses
are placeholders, not runnable evidence or secret values.

```text
npm run eval:prepare -- --manifest SUITE.json --projects-root PROJECTS --output SNAPSHOTS
npm run eval:run-arm -- --manifest SUITE.json --project PROJECT --arm baseline --snapshot-root SNAPSHOTS --output RESULTS --codex CODEX_EXE --sqlite-home BASELINE_SCRATCH_DB --codex-home BASELINE_SCRATCH_HOME --auth-home ADMITTED_AUTH_HOME
npm run eval:run-arm -- --manifest SUITE.json --project PROJECT --arm stateful --snapshot-root SNAPSHOTS --output RESULTS --codex CODEX_EXE --sqlite-home STATEFUL_SCRATCH_DB --codex-home STATEFUL_SCRATCH_HOME --auth-home ADMITTED_AUTH_HOME
npm run eval:longitudinal -- --manifest SUITE.json --pair PROJECT=ordinary-rollout.jsonl,stateful-rollout.jsonl --observations PROJECT=ordinary-observations.json,stateful-observations.json
npm run eval:prepare-grading -- --manifest SUITE.json --result-root RESULTS --snapshot-root SNAPSHOTS --output PUBLIC_PACKETS --mapping-output PRIVATE_MAPPING --seed FROZEN_SEED --sessions-root SESSIONS
npm run eval:prepare-state-grading -- --manifest SUITE.json --result-root RESULTS --snapshot-root SNAPSHOTS --output STATE_PACKETS
```

Repeat `--pair` / `--observations` for every manifest project. Replace each
arm's scratch placeholders with new disposable database and native-home
directories outside report/output directories. Keep those homes stable across
that arm's longitudinal questions, but distinct between arms and independent
repetitions. `ADMITTED_AUTH_HOME` is an explicitly admitted existing login source,
not a report destination. Do not copy credentials into output or Git.

These runner flags are syntactically optional but necessary for this isolation:
without `--sqlite-home`, the runner uses inherited `CODEX_SQLITE_HOME`, then
`CODEX_HOME`, then `~/.codex`. Without `--codex-home`, it uses `eval-codex-home`
beneath that database home; without `--auth-home`, it uses inherited `CODEX_HOME`
or `~/.codex`. These defaults do not isolate arm databases. The runner hard-links
admitted auth into the selected native home, refuses an unrelated existing
credential destination and strips API-key variables. This remains credential use.
Windows's frozen runner selects the unelevated sandbox for its read-only case.
Do not broaden that historical policy for a new experiment by implication.

Intervention records bind predecessor/successor corpus revisions, old/new file
SHA-256 and manifest-relative replacement payloads. Only existing files can be
replaced. Validate the whole chain before running, verify old bytes at the boundary,
apply atomically, verify successor and persist before inference. Unplanned changes
invalidate the arm. Resume is idempotent; failed attempts remain in run-state.
`--reconcile-failed REASON` records reconciliation and starts a new numbered
attempt, never overwrites the old result. Stateful resumed questions explicitly
reselect mode to start a new run while retaining project/thread.

The comparator attributes append-only rollouts by turn ID, pins exact session
paths rather than guessing shared directories, counts canonical compactions
without duplicate legacy signals, and records actual first/last model-visible
project fragment, root size, included/omitted entries and freshness.
Missing turns/config parity/usage reconciliation/durable completion or required
observations invalidate the comparison; a performance loss does not.

After each successful Work turn, `eval:export-state` captures
`v1-work-run-snapshot` with run/obligation/steering/hierarchy/context map/
blackboard/evidence/relationship records and artifact hash plus corpus/project/run
revisions. It is **not** complete current state: knowledge context, capture
sources/groups/exclusions, memory journal and acceptance are omitted. It derives
project from a run and cannot export run-less Ask.

```text
npm run eval:export-state -- --sqlite-home PATH --run ID --output PATH --corpus-revision HASH
```

Independent case observations cover source-read audit and quality; rollouts cannot
prove deep-state size, every source region or semantic correctness.
Grade six 0–4 dimensions: correctness, evidence traceability, decisive detail,
uncertainty calibration, contradiction/freshness and usefulness. Grade visible
answer, submitted narrative, persisted result, semantic obligation and intelligence
separately. Missing observations remain missing, never reconstructed from prose.

## Single pairs, short series and lexical probes

```text
npm run eval -- --baseline ordinary.jsonl --stateful stateful.jsonl --expect "8 regular files" --expect "styles.css"
npm run eval:series -- --manifest eval/manifests/licensing-series.json --maturation-rollout maturation.jsonl --pair economics=ordinary.jsonl,stateful.jsonl --pair territory=ordinary2.jsonl,stateful2.jsonl --pair termination-risk=ordinary3.jsonl,stateful3.jsonl
npm run eval:state -- --project PROJECT_ID --gateway http://127.0.0.1:4173 --manifest PROBES.json
```

Pair comparator parity failure exits 2; missing expected terms exits 3.
Read-bearing calls are rollout proxies, not exact OS reads. Series tools are
one-rollout-per-question diagnostics, not continuous-thread measurement.
Supply every actual maturation rollout; the licensing manifest's default
649,173 full / 94,677 uncached / 15 responses is historical SC-EVAL-004,
not a cost-free default for a new study. Break-even is projected only when
observed marginal savings are positive.

The state probe's schema 2 names are `lexicalProbeRecall`,
`storedSupportLexicalProbeRecall` and `storedSupportEntryRatio`.
Schema 1 misleadingly called them semantic recall/precision. They inspect stored
active/sourceVerified/current/evidence-linked fields and substring groups, not
entailment. Truncated 50-entry snapshots, missing probes or prohibited conclusions
fail; this does not establish held-out semantic precision or production truth.
The `eval:state` command above is historical and incompatible with the current
gateway: it requires a retired HTML session-token meta element and POST
header/envelope, while the gateway now uses an HttpOnly cookie and strict RPC
shapes. It fails before querying current state. Do not weaken gateway admission
to restore it. Other old live-gateway scripts also need compatibility checks;
a documented historical command is not a fresh execution.

## Harbor adapter and portable bundle

Adapter: [stateful_harbor](../../clients/stateful-codex/eval/stateful_harbor).
Pinned Harbor commit: `15da91c18580a25489f5bdf2ee71029f3ff3bb2e`.
Use a clean checkout and frozen lock; the adapter validates the active installation,
not only a commit string copied into metadata.

From a clean Stateful source tree with Bash and Docker:

```bash
bash clients/stateful-codex/eval/stateful_harbor/build-portable-linux-bundle.sh
bash clients/stateful-codex/eval/stateful_harbor/preflight-linux-bundle.sh <bundle.tar.gz>
```

The Docker builder starts from `rust:1.95-bullseye` (glibc 2.31), but copies the
current source and invokes unqualified Cargo in `codex-rs`. Rustup therefore
selects the workspace-pinned Rust 1.99.1, not the base image's Rust 1.95.
The scripts package paired binaries and pinned OpenSSL runtime behind launchers
and write a deterministic archive/hash sidecar. Current bundle production with
this toolchain is unqualified; these commands do not establish a Rust-1.95 build.
Preflight verifies the digest and starts both binaries in the exact task image
before authentication/model usage. Generated bundles are not committed.

Set `PYTHONPATH` to the absolute eval directory, `STATEFUL_CODEX_BUNDLE_PATH`,
`STATEFUL_CODEX_BUNDLE_SHA256` and `CODEX_AUTH_JSON_PATH` to admitted local inputs;
remove API-key environment variables. `BundledCodex` runs ordinary mode;
`StatefulCodex` adds `--stateful autonomous`. Keep every other condition matched.
Example after installing the pinned Harbor environment:

```text
harbor run -d terminal-bench/terminal-bench-2-1 -i terminal-bench/fix-git -k 1 -n 1 -a stateful_harbor.stateful_codex:BundledCodex -m openai/gpt-5.6-luna --effort max
harbor run -d terminal-bench/terminal-bench-2-1 -i terminal-bench/fix-git -k 1 -n 1 -a stateful_harbor.stateful_codex:StatefulCodex -m openai/gpt-5.6-luna --effort max
```

Validate one pair first. A full TB2.1 result requires 89 tasks × 5 trials; excluded
broken verifiers need both scorable and all-task bases. September's published
Codex 0.144.1/Luna comparator was 75.73% ±1.32 pp over 445 trials.
Historical leaderboard submission availability is not a current publication right.
No new Harvey LAB runs are authorized by this guide.

Retain native logs, ATIF v1.7 trajectory, bundle/Harbor metadata, initial/final
commits, final patch including committed changes and checksummed SQLite exports.
Task containers isolate trials; the SQLite home survives within-trial resumes.
Foreign state cannot leak between independent trials.

### Feedback-conditioned repeats

From `clients/stateful-codex`:

```text
npm run eval:harbor-feedback -- --cohort COHORT.json --state-root STATE --jobs-dir JOBS --harbor HARBOR_EXE --bundle BUNDLE.tar.gz --bundle-sha256 DIGEST --auth AUTH_JSON --python-path EVAL_DIR --dataset DATASET --job-prefix UNIQUE --attempts 5 --concurrency 8 --cwd CHECKOUT
npm run eval:harbor-feedback-summary -- --cohort COHORT.json --state-root STATE --jobs-dir JOBS --attempts 5 --output SUMMARY.json
```

Fresh task container per attempt, persistent state only for that task, sequential
attempts within a task. Later attempts receive bounded authoritative prior
outcomes, including failures. Repeat successes to detect regressions.
Pre-agent infrastructure retries are distinct; failures after invocation remain
outcomes. Persistence misses do not erase valid verifier results. Summaries
require all controller records unless explicitly incomplete for monitoring.
This is feedback-conditioned learning, not independent pass-at-five or proof of
memory causality without an ordinary repeated control.

## BixBench adapter

[Runner and manifests](../../clients/stateful-codex/eval/bixbench):

| Input | Pin |
| --- | --- |
| BixBench v1.5 | `49311180bdacb324c596f2e07596c126f2004008` |
| Dataset | `f8cc3bdcc6357c88b8c3648306522b9c422dc95a` |
| Metadata SHA-256 | `0d1204dcdae7193a9132ced5a3502008f6b3b163debc1b65b2aa2d86cb132dc9` |
| data-analysis-crow v1.5.0 | `953a6b13c5f8e15354525bca08636f620994f954` |

`smoke-bix18.json` selects one range-verifier question; `bix18-deterministic.json`
freezes five state-isolated questions. `breadth10-deterministic.json` freezes
five range/five string questions across ten capsules, chosen for coverage and
archives <=10 MiB rounded, not representativeness. `repair2-deterministic.json`
is a post-hoc Excel/openpyxl and Docker-EOF repair, not another breadth sample.

From root, with an admitted content-addressed Linux bundle:

```text
python clients/stateful-codex/eval/bixbench/run_bixbench.py --manifest clients/stateful-codex/eval/bixbench/smoke-bix18.json --output-dir OUTPUT --cache-dir CACHE --bundle BUNDLE.tar.gz --bundle-sha256 DIGEST
```

The official image was ARM64-only when checked September 24. An AMD64 pinned
Dockerfile rebuild is source-pinned, not byte-identical official-image use.
Executed local gates instead used truncated debug-base-derived image
`sha256:a07ae515ef9601d48f08bc50d00a6bed7d8f7269e123e1bbfad0b8c709fdb746`;
protocol-v2 repair added `openpyxl==3.1.5` and produced
`sha256:2c5cc1f0c546e90732a2a7fed79f14839ed2565311d179f01c4de69c56a79072`.
The full upstream Dockerfile already had openpyxl.

Preserve question/capsule/answer contracts. Mount only capsule `Data`, never
reference notebooks, answer keys or distractors. Pin/hash downloaded archives,
bundle, image/environment inventory, prompts/schema/manifests and inputs.
Use fresh state per question (`--arm ordinary` is the matched optional control).
Disable search, upstream memories and benchmark-source hosts; audit actual logs.
Runtime package installation violates protocol. Native JSONL is not Aviary format.

Protocol v3 replay starts from untouched inputs and submitted notebook only,
same image, networking disabled, and requires exactly one
`BIXBENCH_ANSWER=<answer>` matching the structured answer. Helper artifacts cannot
rescue replay. Earlier v1/v2 gates predate this marker rule.
Correctness and operational validity stay separate. Operational validity needs
completed structured answer, valid notebook, offline replay, audit/hashes and
terminal integrity-valid Stateful state. Timeouts/nonzero/malformed answers are
incorrect; positively identified pre-invocation infra/provider/container outages
are invalid, not silently dropped. `llm_verifier` remains ungraded without a
frozen judge. Range/string `local_metadata_verifier` diagnostics are not official
LLM-judged scores. Persistent same-capsule studies must be labelled longitudinal.
