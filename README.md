# Stateful Codex

**Start a new thread. Keep what your project learned.**

Stateful Codex is a research fork of OpenAI Codex that carries project
understanding across threads: decisions and their reasons, standing rules,
failures, source references and uncertainty. Spend less time rebuilding context
and more time moving the work forward.

You get a native CLI, a local browser workspace, and project memory you can
inspect, correct and Forget. **Research preview:** useful task-specific results,
with important limits still open.

The current upstream integration has [unresolved integration checks](docs/stateful/RESULTS.md#current-integration-checks-october-22),
with a baseline-reproduced timeout and unresolved broad-run timing attribution.
Compaction/scope repairs passed rechecks; see the
[final classification](docs/stateful/RESULTS.md#final-integration-regression-classification-october-23).
Use disposable projects.

[Try it](#quick-start) · [See it work](#see-it-work) ·
[Results](#results-at-a-glance) · [Contributor guides](#where-to-go-next)

Canonical repository: [dl1683/stateful-codex](https://github.com/dl1683/stateful-codex).
Contributions target this fork's `main` branch.

## See it work

The [recorded data-room study](docs/stateful/RESULTS.md#fresh-threads-within-thread-losses-and-accounting-disagreements)
asked for analysis across 14 documents over five sessions. Fresh Stateful threads
scored **18.2/48**, versus **13.5/48** for an ordinary continuous thread. A careful
hand-kept notes control scored **16.8/48**: close enough that Stateful's lead over
notes was within run-to-run noise. These are historical measurements, not a demo
rerun on today's checkout.

To explore the controls yourself, use the documented
[disposable browser walkthrough](docs/stateful/DEVELOPMENT.md#browser-startup-and-memory-walkthrough):

1. Start the fixture command below and open its printed loopback URL. Select its
   disposable project/thread, choose **Ask**, enter `Describe this disposable
   project.`, and select **Open workspace**.
2. In **Project memory**, add `Disposable walkthrough note.`
3. Correct the note, refresh, and check the successor text.
4. Forget it and refresh again; it should leave effective memory listings.

This fixture uses the production HTTP handler with synthetic memory. Restarting
it resets its state. It demonstrates the interface; native persistence and model
behavior require separate qualification. Forget does not erase transcripts.

## Results at a glance

All numbers below come from the [single results ledger](docs/stateful/RESULTS.md).
Historical measurements use different builds and protocols; they are not current
release scores or proof that memory caused a benchmark result.

| Recorded evaluation | Result | What to keep in mind |
| --- | --- | --- |
| Fresh-thread data room, SC-EVAL-033 | Stateful fresh **18.2/48**; ordinary continuous **13.5/48**; hand NOTES **16.8/48** | Three runs per arm, proxy judging. The lead over NOTES is within noise. Stateful used more tokens than ordinary continuous. |
| Standing rule across fresh sessions, SC-EVAL-035, build `27aa1988f8` | Stateful **20/20**, ordinary **1/20**; compactions **3 vs 15** | One run per arm/build; ordinary needed conversation-only re-asks. Within-run results were mixed or negative. |
| Historical SWE-bench Verified, Luna | **458/500 (91.6%)** | Historical bundle `83eee86b6`, before a later completion-gate regression; no matched Stateful-off arm. |
| Historical TB2.1, Luna | **341/435 (78.4%)** scorable; **341/445 (76.6%)** on all-task basis | Five trials; broken qemu cases excluded from the scorable denominator. No ordinary arm. |
| Same-day changed facts, SC-EVAL-034 | Stateful **0/10**, ordinary **7.5/10** first-answer correctness | Stale-current claims remain a failure, despite useful fresh-thread reuse. |
| Recorded Windows onboarding checks | Web **132/132**; launcher **10/10** | Fixture/mocked checks, not independent product assertions or native memory acceptance. |

Later completion-gate regressions, failed hosted journeys, within-thread losses
and higher lifetime costs remain in the ledger. Exact historic recall, reliable
Work completion and universal savings are not established.

## Quick start

Run commands from the repository root. The fixture needs Node 22+ and no npm
installation or Rust build:

```text
node clients/stateful-codex/eval/gateway-smoke.mjs
```

Open the exact URL it prints, select the disposable project/thread and choose
**Ask**. Enter `Describe this disposable project.` and select **Open workspace**.
Stop with Ctrl+C. This is the fastest way to explore the browser.

For native use, install the
[build prerequisites](docs/stateful/DEVELOPMENT.md#tools-and-build-admission),
then build both binaries:

```text
cd codex-rs
cargo build --locked -j 8 -p codex-cli -p codex-code-mode-host
cd ..
```

**Windows:** first follow the
[matched V8 archive and binding setup](docs/stateful/DEVELOPMENT.md#windows-v8-archive-and-binding).
The default sandbox archive URL failed in the recorded build. Keep the sandbox
enabled and use the documented verified artifact pair.

Use a disposable project and home for the
[native CLI walkthrough](docs/stateful/DEVELOPMENT.md#paired-builds-and-disposable-cli).
Its launcher command is:

```text
python scripts/scodex.py -C <disposable-project> --stateful ask "Describe this empty disposable project."
```

For a native browser session, follow the
[disposable login and browser setup](docs/stateful/DEVELOPMENT.md#browser-startup-and-memory-walkthrough),
then run:

```text
npm --prefix clients/stateful-codex start
```

Open `http://127.0.0.1:4173`. The native login/model/memory and cold-restart
journeys remain unverified; the recorded fresh Windows build and initialization
smoke do not establish those behaviors. Linux/macOS/WSL newcomer setup and
independent clean-clone acceptance also remain unverified. Preserve valuable
history and credentials; use disposable storage for experiments.

## Landed and held

| Status | Capability |
| --- | --- |
| Landed in source | Project knowledge hierarchy, blackboards, context maps, bounded reads and direct memory controls |
| Landed in source | Project-local native home routing; several databases remain |
| Landed in source | Ask/Work surfaces, loopback gateway protections, memory paging and revision-keyed drafts |
| Inactive | Single-file memory conversion and capsule/console components |
| Held | Exact recall after repeated compaction, universal correction/Forget replay, foreign-writer exclusion, populated migrations, reliable Work and full native/platform acceptance |

A component pass is not product acceptance. Read the results ledger before
relying on any capability in a real workflow.

## Where to go next

| You want to... | Read |
| --- | --- |
| Build, run, configure storage, test or contribute | [Development](docs/stateful/DEVELOPMENT.md) |
| Understand contracts and find the implementation | [Architecture](docs/stateful/ARCHITECTURE.md) |
| Check measurements, failures and qualification limits | [Results](docs/stateful/RESULTS.md) |
| Understand prior decisions and recover historical work | [Failures](docs/stateful/FAILURES.md) |
| Reproduce an evaluation with its original protocol | [Evaluation](docs/stateful/EVALUATION.md) |

Agent contributor rules and the repository map: [AGENTS.md](AGENTS.md).

Built on [OpenAI Codex](https://github.com/openai/codex). Licensed under
[Apache-2.0](LICENSE); preserve upstream notices and security guidance.
