# Stateful Codex evaluation record

This record distinguishes implemented behavior from demonstrated product
advantage. [`STATEFUL_CODEX_PRODUCT_INTENT.md`](./STATEFUL_CODEX_PRODUCT_INTENT.md)
defines the outcome being tested; component tests and a runnable interface are
necessary, but are not evidence that Stateful Codex is better than ordinary
Codex. Every entry keeps its failures, invalid or stopped runs, and negative
economics. Entries were compacted on 2026-10-07 without changing any result;
the uncompacted text is recoverable from Git at
`46d9071051445583c00b21e0163d373372d1a966`.

## Index

| ID | Date | Question | Outcome |
| --- | --- | --- | --- |
| SC-EVAL-001 | 09-21 | Trivial directory inventory | Correct; +22.28% full, +45.26% uncached (negative) |
| SC-EVAL-001R | 09-21 | Same, active-run tools | Reliability fixed; +86.34% full, +37.19% uncached (negative) |
| SC-EVAL-002 | 09-21 | Decisive procurement connection | Correct; reopened all 8 files; +225.77% full (fails rereading and cost gates) |
| SC-EVAL-002R | 09-22 | Compact root, batch writes | 149,360 full; still about 2x ordinary (negative) |
| SC-EVAL-003 / 003R | 09-22 | Selective controlling evidence | 4 of 6 files, -17.78% full, +60.84% uncached |
| SC-EVAL-004 | 09-22 | Held-out licensing memory | Lexical 6/10 (manual 10/10); follow-up -15.76% full, -20.89% uncached; maturation 649,173 |
| SC-EVAL-005 | 09-22 | Three-question amortization | -31.95% full, +22.59% uncached; lifetime behind |
| SC-EVAL-005R | 09-22 | Deferred-tool remediation | Rejected and reverted (+35.95% uncached) |
| SC-EVAL-006 | 09-22 | Linked-batch maturation | -46.88% maturation full tokens |
| SC-EVAL-007 | 09-22 | Refresh-route maturation | -19.03% full vs 006 |
| SC-EVAL-008 | 09-22 | Resolved evidence | Mechanisms worked; +49.61% full vs 007 (regression) |
| SC-EVAL-009 | 09-22 | Exact provenance, terminal ordering | Both gates passed; 250,849 full |
| SC-EVAL-010 | 09-22 | Maturation plus three follow-ups | -19.41% follow-up full, +27.90% uncached; one answer dropped a carve-out |
| SC-EVAL-011 | 09-22 | Project-scoped prompt cache | Passed; -45.42% turn uncached |
| SC-EVAL-012 | 09-22 | Final semantic coverage | Prose passed; durable-result gate failed |
| SC-EVAL-013 | 09-22 | Durable material findings | Passed, with one rejected opaque-reference retry |
| SC-EVAL-014 | 09-22 | Revision-bound aliases | Passed |
| SC-EVAL-015 | 09-22 | Two-project distribution | Stopped after maturation (completion contract) |
| SC-EVAL-016 | 09-22 | Same, disambiguated | Stopped after maturations (evidence schema) |
| SC-EVAL-017 | 09-22 | Same, exclusive evidence | Correct and selective; full and lifetime economics failed |
| SC-EVAL-018 | 09-22 | Single-call completion | Partial pass; uncached target failed |
| SC-EVAL-019 | 09-22 | Final-answer fidelity | Passed |
| SC-EVAL-020 | 09-22 | Bounded outcome | Retrieval passed; workflow and economics failed |
| SC-EVAL-021 | 09-22 | Substantive intermediate obligation | Failed |
| SC-EVAL-022 | 09-22 | Same, remediated | Failed; case frozen |
| SC-EVAL-023 | 09-22 | Six-project breadth screen | Directional only; graders preferred ordinary on 5 of 6 |
| SC-EVAL-024 | 09-23 | Source change under total-context stress | Correct; economics failed decisively |
| SC-EVAL-025 | 09-23 | Same, `body_after_prefix` growth | -49.36% total; no compaction occurred |
| SC-EVAL-026 | 09-23 | Explicit historical retrieval | Mechanism shown; +639.47% full; ungraded |
| SC-EVAL-027 | 09-23 | Terminal-Bench `fix-git` control | Stateful 3/3, ordinary 0/1; 2.24x-3.46x cost |
| SC-EVAL-028 | 09-23 | Terminal-Bench smoke and 89-task breadth | Smoke 4/4; breadth 70/89 (78.65%) |
| SC-EVAL-029 | 09-23 | Concurrent project-family smoke | Retired before any result; manifest removed 10-07 |
| SC-EVAL-030 | 09-23 | Feedback-conditioned pilot | 50 attempts; fifth attempts -22.0% cost, 7/8 vs 5/8 |
| SC-EVAL-031 | 09-24 | BixBench gates | 4/5; breadth 3/10 local, 2/10 correct and valid |
| SC-EVAL-032 | 09-24 | Pramana ten-question A/B, one thread | 2.0x input, 2.7x uncached; quality level |
| SC-EVAL-033 | 09-28/29 | Matched behaviour study | Fresh-thread best on data room; overhead on trivial asks |
| SC-EVAL-034 | 09-30 | 25-question model eval | -13.4% cost; same-day negative findings |
| SC-EVAL-035 | 10-02/03 | Hands-on campaign | Helps across fresh threads; not yet within one run |

Unnumbered records: live interface validation (09-21/22), project-state
instrumentation (09-22), fresh rendered browser validation (09-22), the
longitudinal program and turn-aware evaluator (09-22), the external benchmark
ladder (09-23), mechanism canaries and implementation checkpoints
(09-24 to 09-27), hands-on product testing with issues #24-#42 (09-25 to 09-27),
and the external benchmark program (from 09-28, including the planned
SC-PKRO-001 and SC-MEM-REUSE-001).

## Evaluation method

`clients/stateful-codex/eval/compare-rollouts.mjs` compares two rollout JSONL
files. It refuses to treat a pair as comparable unless originator, source,
model, reasoning effort, approval and sandbox policy, permission profile,
working directory, workspace roots, and normalized user prompt all match. It
reports full lifetime usage separately from uncached usage, checks predeclared
answer terms, and counts read-bearing tool calls plus Stateful retrieval and
persistence calls. Read-bearing tool calls are a rollout-level proxy, not an
exact count of operating-system reads; one call may perform several
operations. Token totals are the final cumulative totals recorded by Codex, not
estimates from text length.

**Standard conditions for SC-EVAL-001 to SC-EVAL-022** unless an entry says
otherwise: native branch CLI (TUI or `codex exec`), `gpt-5.6-luna` at `xhigh`,
`danger-full-access` with `never` approval, the same selected directory and
workspace roots and the exact prompt in both arms, cached ChatGPT login with
`OPENAI_API_KEY` and `CODEX_API_KEY` removed, and a comparator that accepted
every parity field. Procedures marked pre-registered were committed before the
run and not changed after it. Corpus copies were SHA-256-checked before and
after and stayed byte-identical; "full" means full lifetime tokens and
"uncached" means uncached input plus output. From SC-EVAL-015 on, Codex's
separate user-memory feature was disabled in both arms because it already held
notes about these fixtures.

## SC-EVAL-001: trivial exact directory inventory (2026-09-21)

Question: does project intelligence reduce work or tokens on a task whose
answer is cheap to verify directly? TUI originator, CLI source. Prompt:

```text
Count-regular-files-in-this-selected-directory-not-recursively-report-exact-total-and-group-every-filename-by-extension-do-not-edit-verify-from-filesystem
```

Rollouts: ordinary `01a0c60b-3489-7d72-96ae-54c1eb632a0a`, Stateful
`01a0c608-945f-7d42-a6d5-e889d4d6f19c`. Both returned the registered exact
count, extension grouping and all eight filenames, and said nothing was edited.

| Measure | Ordinary | Stateful | Remediated Stateful (001R) |
| --- | ---: | ---: | ---: |
| Full input tokens | 68,968 | 84,145 (+15,177) | 128,712 (+59,744) |
| Cached input tokens | 47,360 | 52,736 (+5,376) | 99,328 (+51,968) |
| Uncached input tokens | 21,608 | 31,409 (+9,801) | 29,384 (+7,776) |
| Output tokens | 932 | 1,332 | 1,539 |
| Full lifetime tokens | 69,900 | 85,477 (+15,577, +22.28%) | 130,251 (+60,351, +86.34%) |
| Uncached input + output | 22,540 | 32,741 (+10,201, +45.26%) | 30,923 (+8,383, +37.19%) |
| Model responses | 3 | 3 | 5 |
| Read-bearing tool calls | 2 | 1 | 1 |

Stateful also made one blackboard query, one obligation update and one run
update, and no context-map query. **Negative result:** the saved read was much
cheaper than the added context, tools, reasoning and persistence; the task is
below the break-even point for durable intelligence. One pair cannot attribute
the difference to a fragment or give a stable percentage, but it rejects any
claim that Stateful is cheaper for every task.

**SC-EVAL-001R.** After binding model tools to the selected thread's active run,
rollout `01a0c64c-e23b-7333-beec-32603b4d8eb0` passed full parity and the same
answer checks. `steering_query`, `obligation_update` and `stateful_run_update`
no longer took a model-authored `runId`, and the malformed-ID failure of the
earlier diagnostic run disappeared; it used one read-bearing call and no
blackboard or context-map query. Still negative: uncached improved from 32,741
to 30,923, but separate responses for steering, verification, obligation, run
completion and the answer made full usage worse. One rerun cannot separate
prompt effects from sampling variance.

## Live interface validation (2026-09-21 to 2026-09-22)

The first focused native validation on 2026-09-21 built the branch CLI, passed
five Stateful TUI tests and snapshots, and ran two cached-login CLI runs against
`clients/stateful-codex/public` without source edits; they reused project
intelligence across threads and produced durable obligations (threads
`01a0c5d8-d37a-71c3-9904-8561c286afe8` and
`01a0c5e2-1a12-7603-85fa-08f138dcc93c`). The first rendered Edge/CDP run
(continue, fork, Socratic gate, exact evidence, autonomous pause/resume, reload
recovery) left its machine-readable result and screenshots under
`%LOCALAPPDATA%/Temp/stateful-client-live-1790028346984`. It found and fixed
three client defects: an invalid context-search limit, stale thread-scoped
session keys that reopened the wrong run, and an unsupported fresh-history call
surfacing as a user-visible error. On this Windows host some model-issued shell
commands failed before execution because the Windows Store `pwsh.exe` returned
access denied; Stateful recovery and non-shell tools continued, and that host
failure is not reported as successful command execution.

After the active-run remediation, the native CLI run
`01a0c64c-e23b-7333-beec-32603b4d8eb0` (SC-EVAL-001R) completed with an exact
answer and no source edits, and a rendered browser run on the remediated
package (thread `01a0c646-d8f7-7571-b398-e77f423b429d`) surfaced the Windows
host-shell failure and its approval recovery, then showed the correct
obligation and completed result (screenshots under
`%LOCALAPPDATA%/Temp/stateful-client-smoke-1790032608507`). The browser-client
suite passed 5/5. The code-mode companion was built on Windows with the
matching `v150.4.0` archive and generated binding from the published Codex
`rusty-v8-v150.4.0` release, because the crate's default sandbox archive URL
returned 404; this was a local artifact override, not a dependency or source
change.

**2026-09-22 exec and live client regression.** Root-level `--stateful` had been
silently dropped by `codex exec`, so the first attempted run was ordinary Codex
and is not Stateful evidence. After sharing native startup between TUI and exec,
propagating the selection, and attaching the chosen project before
`thread/start`, rollout `01a0c83a-c55b-75d0-89fa-8ab8b9cbebd5` received a
selected project and durable run, used `steering_query`, bounded
`evidence_read` calls, one obligation update and a terminal run update, made no
shell or file-read call or edit, and correctly selected Cedar with exact file
citations: 116,310 input, 83,200 cached input, 2,920 output, 119,230 full and
36,030 uncached tokens. Its prompt and entry point differ from the registered
baseline, so it is a mechanism check, not a comparative win.

Two real gateway runs followed. Thread `01a0c841-34c6-7d31-a76a-68da3373edaf`
received the mature root and exact-source aliases, explicitly skipped deeper
search, verified the controlling ranges, published one obligation and completed
correctly (124,627 full tokens, 90,880 cached input, 33,747 uncached). Thread
`01a0c851-0974-74f2-92a6-db2ba4c9f3e6` began with a submitted steering
instruction that became `applied` during the turn; the strategy revision became
1 and the durable strategy, obligation and result all used the requested
viability-versus-final-award distinction. The exercise found a terminal-state
defect: a completed run accepted steering that could never apply. The runtime
now rejects it (`cannot submit steering to a terminal run (Completed)`), and
the client replaces steering, mode, maintenance and instruction controls on a
terminal outcome with `Start another outcome`. Runtime test and client suite
(6/6) passed. No screenshot run was possible (in-app browser automation was
unavailable), and the companion rebuild was blocked before compiling project
code (the pinned V8 archive download failed and Python was unavailable); the
gateway used the existing matching companion.

These checks demonstrate operability, not comparative advantage.

## Open release evidence

The Stage 8 release claim (the original plan's evaluation and release-hardening
stage: publish only demonstrated improvements, keep regressions) remains open
until representative paired scenarios measure:

1. knowledge precision and recall after multiple turns and compaction;
2. reduced repeated source reading and lifetime cost in a mature workspace;
3. recovery of a decisive detail and a non-obvious cross-source connection;
4. steering acknowledgement and application latency;
5. unattended autonomous completion and restart recovery; and
6. final-claim correctness, freshness, and traceability to exact evidence.

Each scenario must predeclare its expected facts or decisions, use matched
ordinary and Stateful conditions where comparison is meaningful, keep negative
results, and not turn component success into a product claim.

## SC-EVAL-002: decisive procurement connection (2026-09-21)

Corpus: `clients/stateful-codex/eval/fixtures/procurement`. Stateful first gets
one maturation run whose job is to map the corpus and persist reusable,
evidence-linked understanding; the measured question then runs in a fresh
Stateful thread and a fresh ordinary thread. Maturation cost is reported and
counts toward lifetime cost. Measured prompt:

```text
Determine-which-vendor-is-viable-under-all-binding-criteria-identify-the-decisive-cross-source-details-that-rule-out-each-alternative-cite-exact-files-and-do-not-edit
```

Predeclared answer: Cedar is the only viable vendor. Alder fails the EEA gate
because `security-addendum.md` permits temporary plaintext access by a support
engineer in Virginia, United States, despite the proposal's EU-hosting claim.
Birch fails continuity because its 72-hour queue is shorter than the verified
96-hour outage in `operations-log.md`. Cedar has 120 hours of offline
acceptance, EEA-only plaintext support and an authoritative $285,000 year-one
total. The answer cites `binding-criteria.md`, `security-addendum.md`,
`operations-log.md` and `finance-schedule.md` and reports no edits. A failure to
persist useful knowledge, a stale or unsupported claim, or a cost regression
counts as negative.

**Maturation.** Autonomous rollout `01a0c657-cded-7882-891d-e4ae2911ea33` read
all eight files and persisted the gates, authority order, costs, the verified
outage requirement, vendor outcomes, the Cedar-only gate matrix, the
contradiction with the preliminary ranking, open questions and relationships,
keeping "only viable candidate" distinct from "finally awarded". It had to
recover from an omitted confidence field, a source-fingerprint typo, an
idempotency collision after a partly successful batch, and an invalid
relationship kind. Cost: 994,169 full tokens (971,091 input of which 864,000
cached, and 23,078 output).

**Measured runs.** Ordinary `01a0c660-a7eb-7092-b5b4-11e1213b779f`, fresh
Stateful `01a0c662-8f72-7333-809d-9a1267b5164f` (TUI/CLI). Both passed every
predeclared content check: Cedar, the Virginia plaintext conflict, 72 against 96
hours, Cedar's 120-hour queue and $285,000 total, exact files, viability versus
final award, no edits.

| Measure | Ordinary | Stateful | Stateful change |
| --- | ---: | ---: | ---: |
| Full input tokens | 70,715 | 234,695 | +163,980 |
| Cached input tokens | 54,528 | 194,048 | +139,520 |
| Uncached input tokens | 16,187 | 40,647 | +24,460 |
| Output tokens | 2,176 | 2,764 | +588 |
| Full lifetime tokens | 72,891 | 237,459 | +164,568 (+225.77%) |
| Uncached input + output | 18,363 | 43,411 | +25,048 (+136.40%) |
| Model responses | 3 | 7 | +4 |
| Raw source files reopened | 8 | 8 | 0 |

The comparator counted two ordinary and one Stateful read-bearing call, but the
call contents show both arms opened all eight files, so this is not a 50%
source-read reduction. Stateful used state first (steering, then eight broad
context-map searches), reopened every file for exact citations, published a
semantic obligation and completed without a model-authored run ID.
**Interpretation:** it fails the reduced-rereading and token gates; maturation
makes lifetime economics substantially worse. It does show that a non-obvious
decision and its provenance can cross threads and produce the right
evidence-grounded result. Next steps then: make verification selective (route
from verified root claims to the smallest controlling source set, using the
context map as a locator rather than querying once per known filename) and cut
persistence round trips during maturation.

**SC-EVAL-002R.** Two changes followed: current source routes resolved into the
always-loaded root plus a batch-record tool for up to 16 findings per call with
per-item results (proved through the app-server integration, two records, two
successes, zero failures; not yet measured in a maturation run), and compact root
rendering with `E#`, `R#` and `S#` aliases instead of repeated opaque IDs,
absolute paths, provenance call IDs and endpoint IDs. A route-only intermediate
rollout `01a0c7db-bb7f-7a50-884f-753920172109` used 147,944 full and 37,352
uncached tokens, skipped the context-map query and still opened all eight files:
the old 24 KiB root spent most of its budget on identifiers and paths, truncated
the gate matrix and omitted later findings. The compact-root rollout
`01a0c7e3-6569-71e3-a526-3430f1b82fd7` injected a 6,456-byte section with the
full gate matrix, every current route and every promoted finding.

| Measure | Ordinary | Original Stateful | Compact-root Stateful |
| --- | ---: | ---: | ---: |
| Full lifetime tokens | 72,891 | 237,459 | 149,360 |
| Uncached input + output | 18,363 | 43,411 | 36,720 |
| Model responses | 3 | 7 | 5 |
| Raw source files reopened | 8 | 8 | 8 |
| Context-map queries | 0 | 1 | 1 |

Against the original Stateful run that is 88,099 fewer full tokens (37.10%) and
6,691 fewer uncached (15.41%); against ordinary it is still 104.91% more full
and 99.97% more uncached, with the same answer checks passing. Missing state is
no longer the cause: the model saw the complete decision and routes and
deliberately rechecked all eight files because the prompt asks for every
alternative with exact citations, which sets a high legitimate verification
floor. Reduced rereading should be tested where the answer depends on a small
subset of a larger corpus. Still negative on lifetime cost.

## SC-EVAL-003: selective controlling evidence (2026-09-22)

Pre-registered. Mature procurement project; a question whose controlling
sources are a subset of the corpus (`codex exec` in both arms):

```text
Determine whether Alder satisfies the binding EEA data-residency gate. Identify the controlling evidence that overrides any proposal claim, cite exact project files, distinguish this gate decision from overall vendor viability, and do not edit files.
```

Expected: Alder fails the binding EEA gate; `binding-criteria.md` makes
storage, processing, debugging, emergency support and temporary plaintext
access part of the gate; `security-addendum.md` is executed, supersedes
inconsistent proposal language and lets a Virginia support engineer decrypt and
view plaintext records during emergency support; the failed gate makes Alder
non-viable without re-evaluating Birch or Cedar; exact files, no edits. Reading
unrelated vendor, operations or finance files counts against selective
routing; narrower reading without lower cost is mechanism evidence only.

Ordinary `01a0c860-5a90-7ff1-9542-f673bf38faf3`; first Stateful
`01a0c861-6e0f-7850-b0a9-1beb71ec7140`. That run exposed two avoidable
orchestration turns, so the run-state context was changed to identify complete
pending steering directly and to publish an already-decided obligation and
terminal update sequentially in one code-mode call; optimized rollout
`01a0c875-8a49-7783-9569-bc9ab50102b9` at commit `4154ebcbc9`. All answers were
correct with no edits.

| Measure | Ordinary | First Stateful | Optimized Stateful | Same-binary ordinary (003R) |
| --- | ---: | ---: | ---: | ---: |
| Full measured-turn tokens | 102,914 | 140,567 | 83,056 | 101,020 |
| Cached input tokens | 81,920 | 106,496 | 50,688 | 80,896 |
| Uncached input + output | 20,994 | 34,071 | 32,368 | 20,124 |
| Model responses | 4 | 5 | 3 | 4 |
| Read-bearing outer tool calls | 3 | 1 | 1 | 3 |
| Distinct raw project files opened | 6 | 3 | 4 | 6 |
| Steering-query calls | 0 | 1 | 0 | — |
| Stateful persistence model turns | 0 | 2 | 1 | — |

Ordinary opened `binding-criteria.md`, `security-addendum.md`,
`vendor-alder.md`, `operations-log.md`, `finance-schedule.md` and
`committee-notes.md`. The first Stateful run opened only the three decisive
residency sources (criteria, addendum, Alder proposal), the stricter minimum;
the optimized run opened those plus the finance schedule (one predeclared
non-controlling file) and avoided the operations log, committee notes and other
vendor files: breadth six to four (33.3%), outer read calls three to one. Its
first tool turn was one `evidence_read` batch of four ranges; its second issued
`obligation_update` then `stateful_run_update` in one code-mode call. Against
the first Stateful run that saved 57,511 full (40.91%) and 1,703 uncached
(5.00%). Against the original ordinary run it used 19,858 fewer full tokens
(19.30%) and one fewer response but 11,374 more uncached (54.18%).

**SC-EVAL-003R** (pre-registered): the original ordinary run predates
`4154ebcbc9`, so ordinary was rerun on the same binary as rollout
`01a0c880-4ea5-7911-9b19-065635cb608d`. It reached the right gate and viability
conclusions but did not state that no files were edited (a clean-worktree check
confirmed none); Stateful stated it. It again opened the three controlling
files plus the operations log, finance schedule and preliminary committee
notes. Optimized Stateful used 17,964 fewer full tokens (17.78%) and 12,244
more uncached (60.84%). This reproduces selective routing and the full-token
advantage together with the uncached regression; it does not show lower
lifetime cost after maturation or a stable distribution. Next efficiency work:
reduce fixed injected and tool-schema overhead without losing exact-source
behavior.

## Project-state quality instrumentation (2026-09-22)

`clients/stateful-codex/eval/evaluate-project-state.mjs` reads a live project
through the gateway and scores a versioned manifest, separating semantic
concept recall, recall of concepts backed by current source-verified evidence,
the fraction of returned entries that are active, source-verified,
evidence-linked and current, and prohibited affirmative conclusions. It refuses
a completeness result when the bounded 50-entry response is truncated. The
default procurement manifest covers 11 controlling facts, decisions,
contradictions and open questions. At project-intelligence revision 43 the
mature project returned 14 of 14 currently supported entries, matched and
supported all 11 probes, and had no prohibited conclusion; client suite 7/7.
This is a regression baseline, not an independent precision or recall
estimate: corpus and state had been inspected before the manifest was written.
A held-out corpus must commit its probes before maturation.

## SC-EVAL-004: held-out licensing memory (2026-09-22)

Manifest and procedure committed before the maturation run and unchanged
after. Corpus `clients/stateful-codex/eval/fixtures/licensing`, ten files
created after the procurement evaluator: a proposal, master agreement, two
executed amendments, an executed territory side letter, approved economics,
verified risk evidence, preliminary minutes, a closing checklist and a binding
review policy. The expected manifest is
`clients/stateful-codex/eval/manifests/licensing-state.json`, outside the
project root. Decisive detail: executed amendment 2 removes data-security,
confidentiality and IP-indemnity exposure from the master agreement's
$2,000,000 general cap, yet the deal is not ready to close because Canadian
consent, counsel confirmation and final board approval are pending. Phases:
(1) a native maturation run reporting full cost; (2) the committed manifest
scores the live blackboard before any probe change (ten concepts, current
evidence support, five prohibited conclusions); (3) fresh ordinary and
Stateful threads answer:

```text
Is liability exposure capped at $2,000,000 for a data-security breach under the current licensing documents? Identify the controlling instrument and exact project files, distinguish that liability conclusion from whether the transaction is ready to close, and do not edit files.
```

Expected: no; amendment 2 controls over the master agreement and proposal, and
its carve-out leaves data-security exposure uncapped by Section 7.3; that does
not make the transaction ready to close.

**Maturation.** Thread `01a0c88f-9c66-70b1-8210-fd14b2068f3d` refreshed the
context map (ten files, zero skipped or missing), read each file once, made no
edit, and persisted 18 records (13 root-promoted) with 24 relationships:
authority hierarchy, executed terms, stale-proposal and preliminary-board
contradictions, closing blockers, numbers, implications and open questions.
Cost: 15 responses, 630,185 input (554,496 cached), 18,988 output, 649,173 full,
94,677 uncached. It needed two recoverable retries: an evidence read asked for
`maxBytes: 20000` above the declared 12,288-byte limit, and one relationship
batch used a malformed endpoint ID (the good links stayed durable).

**State score.** 18 entries; 17 currently source-supported (94.44%
supported-entry precision); 6 of 10 exact lexical probes matched and
supported; zero prohibited conclusions; `passed: false`. The one unsupported
entry was a corpus-coverage inventory ("all ten files read") with no evidence,
which should not have been persisted. Manual inspection found all ten concepts
in current source-verified state; the four lexical misses come from the scorer
requiring every term group in one entry (`6%` against `six percent`,
`supersede inconsistent` against `supersedes inconsistent`, `written Canadian
regulatory consent` against `written consent`, and a royalty concept spanning
the executed-term and approved-forecast records). So 6/10 is not 60% semantic
recall; it shows the lexical scorer is not a recall measure, and the failing
result is kept unchanged.

**Fresh CLI comparison.** Ordinary `01a0c898-cab9-7ad1-ac00-aedc316858aa`,
Stateful `01a0c898-c9e2-7670-8f23-57a0f3c1b7f1`, same locally built CLI. Both
answers were correct, named executed Amendment 2, cited the master license and
closing checklist, separated the uncapped conclusion from close readiness and
made no edits.

| Measure | Ordinary | Stateful | Stateful delta |
| --- | ---: | ---: | ---: |
| Full tokens | 173,902 | 146,500 | -27,402 (-15.76%) |
| Uncached input plus output | 45,390 | 35,908 | -9,482 (-20.89%) |
| Model responses | 6 | 5 | -1 |
| Read-bearing outer calls | 4 | 3 | -1 |
| Project files read | 10 | 4 | -6 |

Ordinary listed and reread all ten files; Stateful began from the root and
verified the master license, Amendment 2, the closing checklist and the binding
policy. Its first parallel wrapper forwarded a local `key` field to
`evidence_read` and had to be corrected. The generic scorer marked both
answers failed only because they did not use the literal phrase `not ready to
close`; their meaning was correct (a lexical-evaluator limit). Including
maturation, the first question is not cheaper: cumulative 795,673 full and
130,585 uncached tokens.

**Interactive CLI/TUI smoke.** In a Windows PTY with `RUST_LOG=trace`, the
licensing project, Collaborative mode and cached login, thread
`01a0c8a5-1503-7243-a8bd-f62ad30f2e4f` launched the TUI, rendered the directory
and model, streamed `TUI_STATEFUL_OK`, reported usage, produced a resumable
thread and shut down on Ctrl-C. `just codex` compiled `codex-cli` but could not
replace `target/debug/codex.exe` while the gateway's app-server held it, so the
already current binary was used (no clean relink claimed). After stopping the
gateway, `just test -p codex-cli` ran 448 tests: 447 passed and the unrelated
`sandbox_fetches_and_enforces_cloud_managed_permission_profile` timed out twice
launching a nested Windows sandbox; the root-to-exec selection test passed.
`just test -p codex-tui stateful_ui` passed its four Stateful tests (one leaky-
handle annotation). The full `codex-tui` suite is not green on this checkout
(non-Stateful theme snapshot drift, stack overflows in session and pagination
tests, timeouts and leaks); generated `.snap.new` and `.pending-snap` files were
removed without accepting changes. The gateway was restarted on the relinked
binary: `/health` returned `{"ready":true,"authMode":"chatgpt"}` and
`codex login status` returned `Logged in using ChatGPT`.

## SC-EVAL-005: maturation amortization series (2026-09-22)

Pre-registered. Three new questions on the mature SC-EVAL-004 project, each
needing a different subset (current economics, Canadian territory, termination
notice plus modeled insurance exposure); prompts, semantic term groups,
prohibited conclusions and the one-time maturation usage are frozen in
`clients/stateful-codex/eval/manifests/licensing-series.json`. One fresh pair
per question. The report includes lifetime cost after adding SC-EVAL-004's
649,173 full and 94,677 uncached maturation tokens and projects break-even only
when the average saving is positive. Three pairs show whether the saving
repeats; they are not a workload distribution.

| Question | Ordinary | Stateful |
| --- | --- | --- |
| Economics | `01a0c8c1-aa6a-71f2-ac7a-3b2a0b450f89` | `01a0c8c2-d0f8-75c1-a29f-b47ea7049ac1` |
| Territory | `01a0c8c4-9760-7cd1-9a74-a38270081e86` | `01a0c8c5-d66c-7881-9e72-944812d73eda` |
| Termination and risk | `01a0c8c7-52fa-74c2-aca1-0bfc71831455` | `01a0c8ca-438a-77e1-be0a-1606fcc85e3f` |

Every answer was substantively correct, cited exact files, separated executed
terms from proposals or planning assumptions, and made no edits. Ordinary
opened all ten files every time; Stateful verified four, three and four files
from the root without a deeper or context-map query.

| Measure | Ordinary | Stateful follow-ups | Delta |
| --- | ---: | ---: | ---: |
| Full tokens | 428,784 | 291,780 | -137,004 (-31.95%) |
| Uncached input plus output | 73,456 | 90,052 | +16,596 (+22.59%) |
| Model responses | 16 | 10 | -6 |
| Read-bearing outer calls | 13 | 4 | -9 |
| Raw project files opened across pairs | 30 | 11 | -19 |
| Per-question full-token wins | — | 3 of 3 | — |
| Per-question uncached-token wins | — | 0 of 3 | — |

Full-token savings were 25.49%, 40.14% and 29.73%; uncached costs were 16.62%,
19.24% and 30.48% higher. The frozen lexical scorer returned `passed: false`
and is not repaired: the Stateful economics answer called the proposal
"explicitly subordinate" rather than a registered `does not control`,
`supersed`, `non-binding` or `stale` string; the ordinary territory answer said
the licensee "may not sell" and quoted "excluding Canada"; both risk answers
said the data-security claims were outside or removed from the cap rather than
`uncapped`, `cap does not apply` or `carve-out`. With maturation, Stateful
totals 940,953 full and 184,729 uncached against 428,784 and 73,456: behind by
512,169 full and 111,273 uncached. Full-token break-even would come around
question 15; there is no uncached break-even. Selective routing is stable on
this fixture; the lifetime and uncached gates fail.

## SC-EVAL-005R: deferred-tool remediation (2026-09-22)

Pre-registered. Commit `8aa897c673` moved seven exceptional-path tools behind
Codex's deferred-tool discovery (deeper blackboard and context-map queries,
context refresh, blackboard and relationship mutation, historical steering
query); evidence reads, obligations, terminal updates and active steering stayed
visible. The territory question (the median uncached regression, needing only
the side letter, closing checklist and binding policy) was rerun once on the
rebuilt binary. Ordinary `01a0c8e5-1458-7a11-ad92-ca037549a2a1` reopened all ten
files; Stateful `01a0c8e7-623f-7182-89e3-0efe34244e6b` verified only
`executed-side-letter.md`, `closing-checklist.md` and
`binding-review-policy.md`, completed its obligation and run, and made no
edits. Both passed parity and the three answer checks.

| Measure | Ordinary | Stateful | Stateful delta |
| --- | ---: | ---: | ---: |
| Full tokens | 156,315 | 82,664 | -73,651 (-47.12%) |
| Uncached input plus output | 40,091 | 54,504 | +14,413 (+35.95%) |
| Model responses | 6 | 3 | -3 |
| Read-bearing outer calls | 4 | 1 | -3 |
| Project files opened | 10 | 3 | -7 |

The schema saving was real but small: the first request held 26,062 input
tokens against 26,442 in the original territory run (380 fewer, about 1.4%).
Provider caching dominated: the three Stateful responses got 1,792, 0 and
26,368 cached input tokens, and the second response alone added 27,021 uncached
input. This is not root rereading or failed routing. The mature root fragment
is 11,150 characters (13 source-routed findings and relationships) and the run
fragment 1,311 characters (mode, goal, constraints, steering, completion
contract); removing either would remove the product being measured.
**Decision: the deferred-tool change was rejected and reverted** because a
1.4% first-request saving did not justify making core memory tools less
discoverable. Future efficiency work keeps the rich root and run contract,
measures distributions rather than one cache outcome, and targets stable prefix
construction or maturation efficiency.

## Fresh rendered browser validation (2026-09-22)

Live port-4174 gateway, current branch CLI, cached login. The browser created
Collaborative thread `01a0c904-e60c-7391-9845-a9211c9e3233` on the mature
licensing project with a predeclared read-only task (current executed royalty
and controlling instrument, exact evidence, one obligation, completion). Run
`run-33c385b12d21a4b44b90a9fcdc1587c24a46d8c107cce2bd52bf6b415f685f8a`
completed with zero continuations: 6% of net sales from 2026-03-01, executed
Amendment 1's express replacement of master Section 4.2 controlling over the
stale 8% proposal, with learning, implication and strategy shown separately.
The evidence action returned the exact 296-byte `executed-amendment-1.md`
revision (execution by both parties on 2026-02-05, the Section 4.2 replacement,
six percent). No fixture changed. The first render exposed a layout defect: 18
findings in a narrow right rail made a 5,042-pixel page with large blank areas.
Controls now stay in the rail and findings use a full-width responsive grid
(three, two and one columns at 1440, 768 and 480 px); the same page is 2,615
pixels tall with nothing hidden. Captures are under
`%LOCALAPPDATA%/Temp/stateful-client-render-1790078328677`; the workspace
snapshot changed and the client suite passed 8/8. This is a rendered-browser
pass for that executable, not cost evidence.

## Maturation replications SC-EVAL-006 to SC-EVAL-009 (2026-09-22)

Each is a same-corpus mechanism replication of the SC-EVAL-004 maturation
prompt on a fresh byte-identical copy of the ten-file licensing corpus with a
rebuilt CLI, not a held-out semantic test or an economics claim. Each procedure
was committed before its run and kept even if the model ignored the new path or
cost regressed. Project roots were temporary directories under
`%LOCALAPPDATA%/Temp`; every corpus was unchanged after the run.

| Measure | 004 | 006 | 007 | 008 | 009 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Model responses | 15 | 10 | 8 | 11 | 7 |
| Outer custom tool calls | 14 | 9 | 7 | 10 | 6 |
| Full tokens | 649,173 | 344,870 | 279,247 | 417,784 | 250,849 |
| Uncached input plus output | 94,677 | 75,814 | 69,071 | 75,256 | 68,065 |
| Final blackboard findings | 18 | 17 | 17 | 13 | 13 |
| Relationships | 24 | 27 | 29 | 11 | 19 |
| Failed or retried persistence calls | 1 | 0 | see text | 2 | 0 |
| Literal concept probes (manual review) | 6/10 (10/10) | 3/10 (10/10) | 3/10 (10/10) | 4/10 (10/10) | 7/10 (10/10) |

### SC-EVAL-006: linked-batch maturation

SC-EVAL-004 needed separate turns for a 16-record batch, a relationship batch
using copied entry IDs, a second two-record batch, another relationship batch
and a retry after one malformed endpoint. Commit `093cfc3211` raised the batch
to 24 findings plus up to 48 relationships that reference the records'
idempotency keys (single-record and entry-ID tools remain; an app-server
integration proved one model call persists two findings plus their
relationship, extension 5/5, app-server 1/1). Thread
`01a0c91e-af11-7aa0-a7f9-cd77a9ccb9a2`, root
`%LOCALAPPDATA%/Temp/stateful-maturation-1790080610019`. The model read all ten
files, verified all ten in one parallel `evidence_read`, and committed 17
findings and 27 relationships in one `blackboard_record_batch` with zero
failures, no malformed ID and no retry; the unsupported corpus-inventory record
of SC-EVAL-004 was not recreated. Against SC-EVAL-004: responses -5 (-33.33%),
custom calls -5 (-35.71%), full -304,303 (-46.88%), uncached -18,863 (-19.92%).
The frozen scorer found 17 of 17 active, current, source-verified,
evidence-linked findings, zero forbidden conclusions and 3 of 10 literal
probes; manual review found all ten concepts (authority hierarchy, current
royalty, base cap and executed carve-outs, termination, territory,
risk/insurance gap, preliminary-board contradiction, closing blockers, open
insurance question). The misses come from single-entry string rules (`6%` for
`six percent`, `convenience termination` for `termination for convenience`, one
concept split across linked findings): perfect supported-entry precision and
complete manual coverage, not a machine-scored recall pass. The run still made a
full-corpus shell pass before verification, a context refresh plus two context
queries, and three final blackboard checks in one turn. Directional projection
with SC-EVAL-005's follow-ups: 636,650 full against 428,784 ordinary (207,866
behind; break-even around question eight instead of fifteen) and 165,866
uncached against 73,456 (a 92,410-token deficit; no uncached break-even). These
cross-run calculations are not a matched rerun or a release claim.

### SC-EVAL-007: refresh-route maturation

Commit `88280d1f1d` made `context_map_refresh` return a deterministic,
response-bounded inventory of current routes, with `context_map_query` as the
fallback when the inventory is truncated or insufficient (protocol unchanged;
storage 26/26, extension 5/5, an app-server integration proved a model refresh
over two files receives both routes in its next request). Thread `01a0c93b-f307-7e12-856b-d242cacb5c1b`, project
`01a0c93b-f2ef-7c00-bf49-710fdcd05d14`, root
`%LOCALAPPDATA%/Temp/stateful-refresh-routes-1790082503050`. The only discovery
call was the refresh, which returned all ten routes with
`routesTruncated: false`; the model used them directly and made no context-map
query, shell listing or shell read (two of each fewer than SC-EVAL-006). Against
SC-EVAL-006: responses -2 (-20.00%), outer calls -2 (-22.22%), full -65,623
(-19.03%), uncached -6,743 (-8.89%). Scorer: 17 of 17 supported, zero forbidden,
3 of 10 literal; manual 10/10. Two avoidable retries remained: a parallel
evidence call asked for `maxBytes: 100000` above the 12,288-byte limit, and
the linked batch accepted 16 findings and 26 relationships but rejected one
finding with a malformed copied fingerprint, repaired by a single-record retry
plus its three relationships. An unrelated memory-maintenance patch also failed
after state was complete. Projection with SC-EVAL-005's follow-ups: 571,027 full
against 428,784 (142,243 behind; break-even around question seven); uncached
159,123 against 73,456 (85,667 behind, no break-even).

### SC-EVAL-008: resolved-evidence maturation

Commits `783bc5d4af` (clamp a positive oversized `evidence_read.maxBytes` to
12,288 bytes and report it; zero stays invalid) and `c2708822c0` (blackboard
writes cite current routes instead of model-copied fingerprints; the resolver
stores the authoritative entry and fingerprint; a single-source finding without
a `nodeId` lands on its file node, cross-source findings stay at project level,
root promotion unchanged). Extension 5/5 passed, and focused app-server tests
proved the live oversized request from SC-EVAL-007 and route-resolved,
source-verified, file-level batch persistence.
Intended: one refresh, one evidence turn, one obligation turn, one linked write,
one completion, no retries. Thread `01a0c955-4d4c-7f81-a704-bcb568cc3407`,
project `01a0c955-4d3d-7c83-94e8-eefa910dca54`, root
`%LOCALAPPDATA%/Temp/stateful-resolved-evidence-1790084192908`. Both mechanisms
worked live: `maxBytes: 30000` was clamped without a retry, and the successful
batch carried 30 relative-path evidence references with no entry IDs,
fingerprints or node IDs, correctly placed. **The cost regression is retained**
(against SC-EVAL-007: responses +3, +37.50%; outer calls +3, +42.86%; full
+138,537, +49.61%; uncached +6,185, +8.95%; findings 13, relationships 11). The
first linked batch was rejected because the model copied exact `lineRange`
locators from `evidence_read` that blackboard evidence could not store; the
retry dropped the ranges and committed 12 findings and 11 relationships. A later
single write added a thirteenth finding after the model had already completed
the run, so its second completion failed with no active run; no finding was
lost, but the terminal result may omit the last question. All 13 findings were
active, current, source-verified and evidence-linked; manual review found all
ten concepts and nothing forbidden; the literal scorer matched 4 of 10
(`passed: false`). Placement put the policy instruction and effective-date
question on the policy file, the territory fact on the side letter, and
cross-source conclusions at project scope. Two global-memory reads required by
the environment are included in the cost. Against SC-EVAL-004 it still saved
231,389 full (-35.64%) and 19,421 uncached (-20.51%), but SC-EVAL-007 was the
better result. Next: persist exact line ranges and harden terminal ordering.

### SC-EVAL-009: exact-provenance finalization

Commit `db88bf1784` keeps optional exact line ranges from model evidence
through validation, SQLite revisions, queries, root World State aliases,
app-server v2 and browser evidence reads (one bounded range per source;
whole-source links stay valid); the integration proved `lineRange: {start: 2,
end: 3}` is returned by the next query and rendered as `S1:L2-L3` in the next
request's World State, and the browser API returns only those lines. The completion contract now says
all blackboard, relationship, obligation and verification work must finish
before `completed`, and completion is the final Stateful mutation; terminal runs
stay immutable. Thread `01a0c98a-e574-7912-ac68-9f55011304db`, project
`01a0c98a-e567-7890-9711-3260f8fd1a0d`, root
`%LOCALAPPDATA%/Temp/stateful-exact-provenance-1790087681185`. Six outer
code-mode calls: refresh (combined with the required global-memory lookup; ten
routes, no truncation), one parallel evidence read (`maxBytes: 50000` clamped to
12,288 per request, all ten sources returned once), one obligation update, one
linked batch, one verification call, and a final call that wrote the
verification obligation and then `stateful_run_update(status: "completed")`. No
context-map query and no shell listing or read. The single batch accepted all 13
findings and 19 relationships with 30 ranged evidence links; a live API check
found all 30 ranges intact and all 13 findings active, current, source-verified
and linked. Completion succeeded at run revision 2 as the final mutation, so the
terminal result holds the same findings, relationships, uncertainties and
verification. Against SC-EVAL-008: responses -4 (-36.36%), outer calls -4
(-40.00%), full -166,935 (-39.96%), uncached -7,191 (-9.56%), relationships +8
(+72.73%), ranged links 0 to 30, failed or retried persistence calls 2 to 0. The
two single-source risk findings landed on the risk-assessment file, the
single-source territory finding on the side letter, and the ten cross-source
findings at project scope; no record was unplaced, and root promotion stayed
independent of placement.

**Rendered exact-evidence check.** Edge 153 against the live port-4174 gateway
first showed project status reporting 13 understandings while the findings view
silently filtered out `strategy` and `decision` kinds and showed 11; the UI now
renders every kind under `Project understanding & open signals` (client 8/8).
The second render showed all 13 cards and all 30 range controls; `Open evidence
· lines 5–11` opened only `risk-assessment.md` lines 5-11 (`408/455 bytes ·
lines 5–11`), with no overflow at 1440 by 900 or 480 by 900 and no UI or RPC
error (captures `%LOCALAPPDATA%/Temp/stateful-sc009-final-desktop.png` and
`%LOCALAPPDATA%/Temp/stateful-sc009-final-mobile.png`). The frozen manifest found
13 of 13 supported entries, zero forbidden conclusions and 7 of 10 literal
probes; the misses were `supersede inconsistent` against `supersedes
inconsistent`, a royalty concept split across the 6% finding and the stale-8%
contradiction in numeric notation, and a base-cap finding without the literal
`Section 7.3` label. After `turn.completed` and exit 0 the CLI logged one
`UnknownProcessId` cleanup message for a finished command; it changed nothing
but is kept as an operational signal. SC-EVAL-009 closes the two SC-EVAL-008
failures; it does not establish precision and recall, lifetime savings or
release readiness.

## SC-EVAL-010: maturation plus follow-up series (2026-09-22)

Pre-registered. The SC-EVAL-009 maturation (rollout
`%USERPROFILE%/.codex/sessions/2026/09/22/rollout-2026-09-22T10-35-18-01a0c98a-e574-7912-ac68-9f55011304db.jsonl`;
250,849 full, 68,065 uncached, 7 responses, read from the rollout rather than the
manifest's old SC-EVAL-005 figure) plus the three frozen
`licensing-series.json` follow-ups, each run once ordinary and once
Collaborative Stateful bound to project `01a0c98a-e567-7890-9711-3260f8fd1a0d`.
Lexical checks unchanged; manual review reported separately.

| Case | Ordinary full | Stateful full | Ordinary uncached | Stateful uncached | Ordinary reads | Stateful reads |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| economics | 87,337 | 66,825 | 23,849 | 26,377 | 3 | 1 |
| territory | 101,767 | 111,343 | 21,127 | 28,399 | 3 | 3 |
| termination-risk | 119,196 | 70,288 | 20,380 | 28,816 | 5 | 1 |
| **Follow-up total** | **308,300** | **248,456** | **65,356** | **83,592** | **11** | **5** |

Stateful saved 59,844 full follow-up tokens (19.41%), four responses (15 to 11)
and six read-bearing calls, winning full tokens on two of three questions, but
lost uncached on all three (+18,236, +27.90%). Ordinary searched or opened all
ten files every time; Stateful verified four, three (with a repeated side-letter
verification) and six. With maturation: 499,305 against 308,300 full (+191,005,
+61.95%) and 151,657 against 65,356 uncached (+86,301, +132.05%); full-token
break-even at about 13 questions, no uncached break-even. Manual review: both
economics answers covered all four concepts and both territory answers all
three despite scorer misses; the ordinary termination answer covered all four.
**The Stateful termination answer omitted the executed uncapped-liability
carve-out** while correctly reporting 60 days, the $5.5 million planning
scenario, the $3 million limit, the $2.5 million gross difference, unresolved
coverage and the planning-versus-legal distinction; the fact was in the root and
the obligation. No forbidden claim. This shows continuity, exact routing and
less broad rereading, not Stage 8: maturation is not amortized, uncached cost
regresses consistently, a concept was dropped at the final answer, and three
same-corpus questions are not a distribution.

## SC-EVAL-011: project-scoped prompt cache (2026-09-22)

Passed after pre-registration. In SC-EVAL-010 every ordinary first response
reused 9,984 cached tokens and every Stateful first response zero, including
three consecutive threads on the same project; traces put most of the uncached
regression there rather than in rereads or the approximately 4,500-token root
increment. Commit `c0f120f009` adds a host-owned prompt-cache affinity keyed by
the explicitly selected project (live selection changes update it; thread and
session identity stay in metadata; review and ephemeral-fork overrides keep
precedence; two app-server threads share the outbound cache key in tests). The
exact SC-EVAL-010 economics prompt ran in two fresh Collaborative threads on
project `01a0c98a-e567-7890-9711-3260f8fd1a0d`: warmer
`01a0c9ca-c37b-72c1-b176-0bf5a5420c11`, measurement
`01a0c9cb-dc13-78a1-823e-b014ce67e8ce`. Pass required nonzero cached input and
lower uncached input in the measurement's first response with a correct answer;
a 12,000-token cached prefix and at least 25% first-response uncached reduction
were registered as directional thresholds.

| Measure | Warmer | Measurement | Delta |
| --- | ---: | ---: | ---: |
| First-response input | 20,938 | 20,271 | -667 |
| First-response cached input | 0 | 12,032 | +12,032 |
| First-response uncached input | 20,938 | 8,239 | -12,699 (-60.65%) |
| Turn full tokens | 69,651 | 67,302 | -2,349 (-3.37%) |
| Turn uncached input plus output | 27,155 | 14,822 | -12,333 (-45.42%) |
| Model responses | 3 | 3 | 0 |
| Outer calls | 2 | 2 | 0 |

Both answered all four concepts, verified the same four regions in one call,
and wrote one obligation then completion in a second call; no unsupported claim
and no edits. Both thresholds passed, and the cache win did not come from less
verification, fewer writes, truncation or edits. This closes the cross-thread
cache defect and shows a rich stable root need not be fully uncached on every
adjacent thread;
it does not cover cache expiry, idle periods, root revisions, other models or
representative workloads, and it does not change SC-EVAL-010.

## Completion integrity SC-EVAL-012 to SC-EVAL-014 (2026-09-22)

All three re-ran the exact SC-EVAL-010 termination-risk prompt in one fresh
Collaborative thread on project `01a0c98a-e567-7890-9711-3260f8fd1a0d`. The
product gate in each: the persisted result and the final prose both state that
executed amendment 2 removes data-security and confidentiality exposure (and
IP indemnity) from the master agreement's general cap, and keep the 60-day
notice, $5.5 million planning scenario, $3 million listed cyber limit, $2.5
million gross difference, unresolved coverage, planning-versus-contractual
distinction, exact evidence and read-only boundary. A lexical scorer was not
used as the judge.

**SC-EVAL-012: final semantic coverage.** Commit `4fc80c460c`: a run cannot
complete without a final semantic obligation, and completion returns a bounded
checklist (at most 16 items of 640 bytes) from that packet's learnings,
implications, uncertainties and blockers, with an instruction to reconcile
result and prose; it infers no legal concepts, rewrites no user goal and adds no
second finalization turn. Thread `01a0c9d6-ae8c-7623-bd76-e3aed052708d`,
rollout
`%USERPROFILE%/.codex/sessions/2026/09/22/rollout-2026-09-22T11-58-05-01a0c9d6-ae8c-7623-bd76-e3aed052708d.jsonl`.
Mechanism passed (obligation then completion as the final mutation; 13 checklist
items, none omitted). Final prose passed. **The durable result failed:** it said
only that the binding policy requires uncapped or carved-out exposure to be
identified, not that amendment 2 actually removes the data-security,
confidentiality and IP-indemnity exposure from the cap; the final obligation
also missed it, and a post-completion checklist cannot change an already
terminal result. Four responses; 93,223 input (65,792 cached), 3,984 output.

**SC-EVAL-013: durable material findings.** Commit `0f7684a3f4`: root entries
expose stable compact `K` references; completion must submit the material ones,
which are resolved against the current root (verification, freshness, exact
ranges) and placed ahead of the bounded final-obligation content in the result
before the run becomes terminal; unknown, duplicate or oversized selections fail
first; an empty selection asserts that no root finding is material (extension
6/6; an app-server integration proved a selected root finding is visible in
initial World State, returned in the checklist and stored in the terminal
result; the stricter Autonomous continuation integration passed). Gates: `K` references exposed, the critical carve-out finding selected and
returned as a `rootFinding` with its ranged routes, completion final, corpus
unchanged. Thread `01a0c9f8-5fbb-7a23-ab17-dfaa26a73cfa`, rollout
`%USERPROFILE%/.codex/sessions/2026/09/22/rollout-2026-09-22T12-34-53-01a0c9f8-5fbb-7a23-ab17-dfaa26a73cfa.jsonl`;
gateway read confirmed run
`run-4d7df998518f144b9fe69e173051aa2912e76f41d6dd6a2521cbc4e016f7f908` at
terminal revision 2. All correctness gates passed: seven root findings were
selected and returned first in the 16-item checklist (four lower-priority packet
items omitted from the checklist but kept in the obligation), and the persisted
result states that the $2 million aggregate cap does not govern data-security,
confidentiality and IP-indemnity obligations, with the superseded 30-day
baseline as well. Five responses and four outer calls (a non-project memory
lookup, a five-file evidence batch, an obligation plus failed completion, a
successful retry): 118,455 input (89,088 cached), 3,767 output, 122,222 full,
33,134 uncached. **Negative ergonomics:** the model first submitted two
hexadecimal references not in the rendered root; completion rejected the first
before any mutation, and the retry with seven valid references cost an extra
response and about 3,022 uncached tokens. Failing closed was correct; the fix
is explicit selection by compact display aliases bound to a root revision
(SC-EVAL-014). The result must be a bounded completion basis, not a dump of the
root.

**SC-EVAL-014: revision-bound aliases.** Commit `4efb44bcab` replaces `K` handles
with the root's compact `E` aliases plus the shown project-intelligence
revision; a changed root, malformed, duplicate or out-of-range alias fails
before terminal mutation, and new knowledge from the run is still covered by the
appended final packet (extension 6/6; durable-result and Autonomous app-server
scenarios passed). Thread `01a0ca0b-28a8-7dd2-91b7-730d7c1c54f2`, rollout
`%USERPROFILE%/.codex/sessions/2026/09/22/rollout-2026-09-22T12-55-24-01a0ca0b-28a8-7dd2-91b7-730d7c1c54f2.jsonl`.
The root showed revision 54 and `E` aliases with no `K` handle; completion sent
`rootRevision: 54` with `E1`, `E6`, `E8`, `E9`, `E12` and `E13`, succeeded
first time, returned all six current source-verified findings with ranges, and
stayed the final mutation. Prose and persisted result passed every product gate
(including the superseded 30-day baseline and pending coverage and counsel
review); a gateway read confirmed run
`run-2b309747a13c75404f697c262114b81fa63fc847e1de1432334ef244a817170e` at
revision 2 with a 5,016-character result; the checklist had 15 items and omitted
none. Four responses and three outer calls (a four-file evidence batch, an
obligation update, completion): 94,549 input (66,816 cached), 2,496 output,
97,045 full, 30,229 uncached; against SC-EVAL-013, 25,177 fewer full (20.60%),
2,905 fewer uncached (8.77%), one fewer response and call, and no raw read of
`binding-review-policy.md` because the root carried that relationship and
route. Open: concurrent root revisions, representative precision and recall,
maturation-inclusive economics, and the approval-gated repository-wide Rust
suite.

## Two-project release distribution SC-EVAL-015 to SC-EVAL-017 (2026-09-22)

Frozen manifest `clients/stateful-codex/eval/manifests/release-distribution.json`:
six outcome questions across independently matured procurement and licensing
projects (binding vendor viability, authority and cost, the verified continuity
basis, current licensing economics, Canadian territory, termination and
data-security risk), each with prompt, expected concepts, prohibited
conclusions, project identity, and a requirement that the Stateful completion
result plus returned completion basis keep the same material coverage as the
visible answer. Protocol: fresh hashed copies; one Collaborative maturation per
project with the manifest prompt; each case as a fresh ordinary and a fresh
Stateful thread; same rebuilt binary, login, model, effort, prompt, directory,
roots, approval, sandbox and permission profile; Codex user memory disabled in
both arms (it already held notes about these fixtures and would leak answers);
pair order alternating; both actual maturation rollouts mandatory scorer inputs
(a missing one is an error, not zero cost). Gates: correctness (all twelve
answers carry every concept and no prohibited conclusion; each Stateful run
completes once with a successful durable record, semantic checks pass across
result and completion basis, a live API read confirms the terminal revision,
both corpora byte-identical); retrieval (fewer read-bearing calls and unique raw
files without weaker citations); economics (lower aggregate full follow-up
tokens, with uncached, both maturations, lifetime delta, win counts and
break-even reported). Two small synthetic corpora cannot show large-corpus
scaling, code-editing work, multi-day cache behavior, concurrent root revisions
or production readiness.

Commit `c60f6c969d` prepared the evaluator for this distribution: a case can
require registered concepts in the Stateful completion result and returned
completion basis, not only the visible answer; completion attempts are
counted; and every measured maturation rollout is accepted. Its 11-test suite
passed and reconstructed the real SC-EVAL-014 completion as one attempt with
15 checklist items.

**SC-EVAL-015 (stopped; no matched case run).** Procurement maturation thread
`01a0ca24-35fc-7513-9ec0-6d422e1cae46` persisted 13 source-verified findings and
14 relationships across all eight files (conjunctive gates, authority order,
Alder and Birch failures, Cedar's sole apparent eligibility, chronology, costs,
unresolved final award); source unchanged, no edits. Completion took three
attempts: the model first selected all 13 aliases against the cap of eight,
then copied project-intelligence revision 45 into both `rootRevision` and the
unrelated `expectedRevision` (the run was at revision 1, so it was rejected),
then succeeded with `expectedRevision: 1`, `rootRevision: 45` and eight aliases
at run revision 2. 453,078 full and 68,054 uncached. The rendered root had told
the model to select every material alias despite the cap, and the two revisions
were not distinguished at the call site: a product-contract failure, kept as
negative evidence and excluded from release economics. Commit `6f0f03c981`
renders the exact `expectedRevision`, says not to substitute the
project-intelligence revision, and says to select at most eight
highest-priority aliases and keep the rest in the final obligation (extension
6/6, scoped lint and formatting; CLI rebuilt; the companion rebuild stayed
blocked by the Windows V8 archive download, so the existing matching binary was
kept).

**SC-EVAL-016 (stopped; no matched case run).** Same manifest and protocol from
fresh copies with the `6f0f03c981` CLI, plus an operational gate: at most eight
aliases, run and root revisions kept distinct, first-call completion, any retry
kept as a failure. Procurement thread `01a0ca30-f52a-71b2-b6ca-3e52e7c1d9dc`
selected eight aliases, kept run revision 1 apart from project revision 41,
completed first time, persisted 14 findings and nine relationships, and used six
responses, five outer calls, 159,212 full and 65,004 uncached (full 64.86% below
SC-EVAL-015's failed preflight, with no completion retries). Licensing thread
`01a0ca36-26b2-7ef3-9117-b51c4b9af46b` also completed first time with the
correct revisions and eight aliases, persisting 12 findings and 13 relationships
in seven responses and six outer calls (207,649 full, 58,657 uncached); both
corpora unchanged. Its first atomic batch, however, gave both
`contextMapEntryId` and `relativePath` per evidence item; runtime correctly
rejected the batch (exactly one identity is valid) but the published schema's
`anyOf` allowed both. To avoid mixing a known avoidable cost into the
distribution, execution stopped. Commit `22290f0b98` uses `oneOf` with explicit
mutual-exclusion text (extension 6/6, lint, formatting, CLI rebuilt); commit
`d9a3a970c1` lets the evaluator read the nested completion output of a grouped
obligation-plus-completion code-mode call (11-test suite; reconstructs both live
completions).

**SC-EVAL-017 (completed): correct and selective; full and lifetime economics
failed.** Same manifest and conditions from two new copies with the
`22290f0b98` CLI and no reuse of earlier projects; any invalid-evidence retry in
maturation would stop the run. Procurement thread
`01a0ca3e-3bf3-7ad1-b99e-90b2b78ef2d9` (run
`run-6d5c68a0f1e9ecd3788f58983a232299470658112e0124c3aaa869e60f38e695`) persisted
eight findings and nine relationships; licensing thread
`01a0ca41-cee0-7402-b78f-8a925d3b80bd` (run
`run-86ae359bc9dc850cdde43ab432a1080bbfdbf82d964dfd586eae359e22c67a1e`) fourteen
findings and ten relationships; each completed first time. Together: 447,702
full, 114,134 uncached, fifteen responses. The licensing maturation found a
cross-source closing gap: the executed side letter requires both Canadian
regulatory consent and licensor written acknowledgement, while the closing
checklist tracks only consent. All twelve matched runs completed; every
Stateful follow-up completed first time with no failed script or terminal
mutation; a fresh gateway read confirmed all six Stateful runs `completed` at
revision 2 with nonempty results; all eight procurement and ten licensing files
stayed byte-identical.

The unchanged lexical scorer reports failure: it passes the three licensing
pairs and the visible procurement continuity pair but rejects correct wording
such as "the only vendor shown to pass every binding deployment criterion" (the
alternatives require "only viable" or "sole viable"), complete statements of all
three sub-cap totals, preliminary non-final committee notes, and a durable
continuity result saying Cedar "meets the continuity gate" with 120 hours and 24
hours of headroom rather than "satisfies". These misses are kept unchanged; the
manifest was not tuned. Independent semantic review finds all twelve visible
answers and all six durable results correct, complete, caveated and grounded.

Retrieval improved: read-bearing calls 20 to 7 (65% fewer); per-question unique
raw-file reads 54 to 29 (46.30% fewer; ordinary reopened every file every time);
responses 26 to 21 (19.23%); uncached follow-ups 97,693 to 77,806 (20.36% fewer,
winning five of six pairs). Full follow-ups rose from 381,853 to 430,830
(+12.83%, winning one of six). With both maturations Stateful used 878,532 full
and 191,940 uncached, 496,679 (130.07%) and 94,247 (96.47%) above ordinary.
Uncached break-even projects to about question 35; full tokens have none.
Procurement follow-ups carry the full-token regression; licensing shows one
substantial win and two near ties. The next optimization had to reduce
procurement's repeated cached-context and completion overhead without deleting
the rich root knowledge that enabled the correct cross-source answers. A future replication needs a semantic
evaluator or human rubric fixed before execution; widening this lexical scorer
after the fact would invalidate the result.

## Short-workflow replications SC-EVAL-018 to SC-EVAL-022 (2026-09-22)

All on the mature SC-EVAL-017 procurement project and the unchanged
`release-distribution.json` prompts, same rebuilt binary and conditions, user
memory disabled. Each was pre-registered before implementation; all corpus
files stayed byte-identical and every run was confirmed by a live API read.

| Run | Thread | Responses | Full | Uncached | Result |
| --- | --- | ---: | ---: | ---: | --- |
| 018 authority | `01a0ca7a-4d3d-71f3-9123-6fa1be61bbe9` | 3 | 64,248 | 28,920 | mechanism passed; zero-cache first response |
| 018 continuity | `01a0ca7b-b2a1-72e3-b15d-3edf978b1a45` | 4 | 80,045 | 10,413 | extra adjacent-verification round |
| 019 authority | `01a0ca86-4356-7403-82de-3701e6c14f87` | — | — | — | passed |
| 019 continuity | `01a0ca87-6f61-7da1-a1ba-f5c2a1d25499` | — | 84,033 | 13,377 | passed |
| 020 continuity | `01a0ca8e-d9f8-7ca3-9ec6-ac8b351debd7` | 4 | 81,003 | 15,467 | retrieval passed; redundant obligation round |
| 021 continuity | `01a0ca9f-9c48-7c42-bf59-c760f4af492f` | 5 | 102,833 | 27,057 | failed |
| 022 continuity | `01a0cab8-0dff-7c32-afd4-927ea33683b8` | 5 | 109,325 | 31,501 | failed |

The same-binary ordinary continuity run (`01a0ca7c-e83d-7322-8333-f02a40622fdb`)
used 78,593 full and 14,337 uncached; the two SC-EVAL-018 ordinary runs together
used 136,346 full and 28,058 uncached.

**SC-EVAL-018: single-call completion and outcome-bounded verification.**
SC-EVAL-017 traces localized the procurement regression: authority published
its final obligation and completion in separate rounds (four responses);
continuity verified the four decisive sources, then opened two adjacent
residency and price sources before grouping persistence (five responses); the
completion call also returned 5,842-7,151 characters of checklist output, but
the avoidable 20,000-plus-token rounds dominate. Change: the final semantic
obligation becomes part of the terminal `stateful_run_update` (intermediate
`obligation_update` stays for real mid-course changes), and guidance says
verification is bounded by the requested outcome. Gates: one evidence batch, one
terminal update carrying the obligation, no separate final obligation, one
completion attempt, at most three responses, a completed revision-2 result.
Human rubric: authority keeps all three approved totals, the controlling order,
the conjunctive non-waivable rule and preliminary-not-final; continuity keeps
the verified 88/91/94/96-hour history, North Ridge, Birch's 72-hour queue and
24-hour consequence, Cedar's 120-hour claim and ordering preservation,
verified-history versus vendor-claim, and no final-award implication.
Efficiency gate: below SC-EVAL-017's Stateful 189,003 full and 32,587 uncached
for these two cases (ordinary was 114,445 and 34,573). The change was committed
as `3dc47450a5`. Both runs used one terminal call carrying `finalObligation`,
no separate final obligation, first-attempt completion and no failure marker;
results of 7,923 and 5,466 characters. Authority met three responses (one
five-file batch). Continuity opened finance and security after its first
four-file batch to settle all three viability gates, so the
one-batch/three-response gate failed for it. Total 144,293 full (44,710 below
189,003, 23.66%), responses nine to seven, but 39,333 uncached (20.70% above
32,587) because of the zero-cache authority start; against the same-binary
ordinary pair, +5.83% full and +40.18% uncached. The ordinary authority run
logged a non-fatal missing collaboration-thread host message, not attributed to
Stateful. Both submitted narratives said no final approval was recorded (or
that Cedar's status was viability, not approval), but the later visible prose
softened that to provisional wording: a caveat loss the checklist did not
prevent. Verdict: single-call completion and the full-token target passed; the
complete mechanism gate and the uncached target failed. This two-case
diagnostic is not a release-economics result.

**SC-EVAL-019: submitted-result fidelity.** Completion returns the model's own
submitted narrative as `submittedResult` and requires the visible answer to keep
every conclusion, caveat, uncertainty and blocker (formatting may change).
Stateful only (the change affects only post-completion answer assembly); gates: one terminal call with `finalObligation`, a returned
`submittedResult`, no separate final obligation, revision-2 result, unchanged
corpus, and an answer keeping every material conclusion (all three totals and
the non-waiver order; the 96/72/24/120-hour distinction; verified history
versus vendor claims; viability is not a recorded final approval). The change
was committed as `633de3b08b`. Every gate passed: authority used one evidence
batch and one terminal call, continuity two batches and one terminal call; runs
`run-84a3c6207976ad707a7b82bb0d4276c5efcb1fd56dbf9954228f1b53c624a3a0` and
`run-8d73434252388fb04cf8a2d356eaa36fbaf89e3572ab541817b2aa9e5416697f` completed
at revision 2 with 6,742 and 5,474 characters; both visible answers kept every
registered point with no unsupported award or certainty. Seven responses,
145,312 full and 24,224 uncached: +1,019 full (+0.71%) and -15,109 uncached
(-38.42%) against SC-EVAL-018 because both first requests reused the project
cache; against the same-binary ordinary pair +6.58% full and -13.66% uncached.
The fidelity defect is closed; the remaining full-token gap is continuity's
second evidence round.

**SC-EVAL-020: bounded outcome.** Guidance: do not turn a one-dimension
question into an overall project determination; use verified root knowledge
only as labelled adjacent context. Gates: one evidence batch limited to
`binding-criteria.md`, `operations-log.md`, `vendor-birch.md` and
`vendor-cedar.md` (opening finance, security, committee or Alder files fails the
gate even with a correct answer); one terminal call with `finalObligation` and
`submittedResult`; at most three responses; one completion; revision-2 result;
full and uncached below SC-EVAL-019 continuity's 84,033 and 13,377. The run
opened exactly the four sources, concluded only the offline-acceptance
dimension, kept the verified-history and vendor-claim distinction, Birch's
24-hour loss, Cedar's 24-hour stated margin and the not-final-approval boundary;
run `run-fe5316cfad34b5ecdfce74f6d000a721f93b79cdd1962b68e864ec9e93acb1f9`
completed at revision 2 with a 4,442-character result. The mechanism and
economics gates failed: after the evidence batch the model sent a separate
`obligation_update` with all its final learning but no `next`, blocker or
requested judgment, then the terminal call with nearly the same packet (a fourth
response). 81,003 full and 15,467 uncached: 3.61% fewer full and 15.62% more
uncached than SC-EVAL-019 continuity, and 3.07% more full and 7.88% more uncached
than ordinary.

**SC-EVAL-021: substantive intermediate obligation.** An intermediate
`obligation_update` must carry nonempty `next`, `blockers` or
`requestedJudgment`; answer drafting, formatting and terminal persistence are
not next work, and a final packet with only the answer left belongs directly in
`stateful_run_update.finalObligation`; an empty-future packet is rejected before
persistence. Gates:
one four-file batch, no intermediate obligation or rejected retry, one terminal
call, three responses, one completion, revision-2 result, full below 81,003 and
78,593, uncached below 15,467 (compared with 14,337). Retrieval and answer
passed (exactly the four sources; the 88/91/94/96 history, North Ridge, Birch's
72-hour queue and 24-hour intake loss, Cedar's 120-hour claim and
timestamp/order statement, verified-history versus vendor-claim, not final
approval); run
`run-f4453bd9e75fae9a5a0df210bc0376a8793c523e1ec215710bc9730e5a5a93d5` completed
at revision 2 with 4,478 characters. **Workflow and efficiency failed.** The
conditional schema repeated only `minItems` inside its `anyOf` branch, so the
code-mode surface lost the array shape and a scalar `next` was rejected; the
retry persisted an obligation whose only content was a plan to synthesize
already-reviewed evidence. Five responses (one rejected and one recorded
intermediate call, one completion); 102,833 full and 27,057 uncached, 26.95% and
74.93% above SC-EVAL-020 and 30.84% and 88.72% above ordinary. A forward-looking
sentence is not a meaningful update.

**SC-EVAL-022: meaningful intermediate obligation.** Plain array schemas for
every field; runtime requires both a forward signal (`next`, `blockers` or
`requestedJudgment`) and semantic content (`rationale`, `learning`,
`implication`, `strategy`, `changed`, `uncertainty`, `blockers` or
`requestedJudgment`); a `next`-only packet is rejected; synthesizing evidence
already reviewed is final reasoning, not remaining work. Same gates as
SC-EVAL-021. The scalar retry was gone, but retrieval, workflow, response-count
and economics gates failed: the model opened the four sources, then
`finance-schedule.md` and `security-addendum.md` for an unrequested all-gate
determination, persisted an intermediate packet whose remaining work was "complete
the answer", and completed in the next response. No rejected tool call, and the
final obligation and completed run now committed through the same storage
transaction. (An adversarial checkpoint before this run had found that terminal
`stateful_run_update` wrote the final obligation and the run transition in two
SQLite transactions; commit `f42997030f` made it one guarded transaction, and a
forced obligation-identity failure proves the run update rolls back.) The answer
and result were correct (outage history, North Ridge, Birch's 24-hour loss,
Cedar's 120-hour claim and ordering, verified-history versus vendor-claim, no
final approval); run
`run-3e2303305a5733db09daa1d7642895b73f64e3e543e90971d64a6a5e185ddde7` completed
at revision 2 with 5,340 characters. Five responses, two evidence batches, one
intermediate obligation, one completion; 109,325 full and 31,501 uncached,
34.96% and 103.66% above SC-EVAL-020 and 39.10% and 119.72% above ordinary.
**This procurement continuity prompt is frozen as a development case**: more
prompt-specific tuning would overfit, and evaluation moves to diverse projects,
judging intermediate updates by usefulness rather than presence rules.

## Longitudinal evaluation program (from 2026-09-22)

The ladder after SC-EVAL-022, designed to prevent selection and accounting bias:

1. Curate six diverse projects and publish all six breadth results; five
   longitudinal projects are chosen for domain and workload coverage before any
   comparative result, and the sixth is a reserved replication, not a pool for
   winners.
2. Run matched continuous ordinary and Stateful threads from empty state over 20
   sequential questions on each of the five projects; cross-thread transfer is a
   separate experiment.
3. Record per-turn usage, actual compactions, source regions and repeat reads,
   retries, wall time, state revisions, injected root size, maintenance cost,
   freshness and cumulative cost from question one.
4. Include compaction boundaries, a thread restart, source revisions that
   invalidate prior conclusions, cross-source deductions, contradictions and
   questions whose supported answer is unknown.
5. Freeze semantic obligations and a blinded evidence rubric before execution;
   literal term matching stays diagnostic; visible answers and durable state are
   scored separately.

The one-rollout-per-question series comparator would double-count cumulative
usage in a continued thread, so a turn-aware evaluator was a prerequisite.
`clients/stateful-codex/eval/compare-longitudinal-rollouts.mjs` now consumes one
continuous rollout per arm, attributes response usage by turn ID, reconciles it
with recorded turn totals, and reports per-turn and cumulative cost, compaction
checkpoints, latency, read operations, rejected tool results, exact
`evidence_read` regions, completion attempts and the model-visible project and
root size, revision, omissions and freshness per turn. It does not infer quality,
complete source access or deep-state size from prose; case-keyed observation
files supply blinded scores, an audited source ledger and state counts, and
missing evidence invalidates the comparison. Multi-project aggregation reports
project win counts with projects, not questions, as the clustered units. The
method is in
[`LONGITUDINAL_PROTOCOL.md`](./clients/stateful-codex/eval/LONGITUDINAL_PROTOCOL.md);
`eval/manifests/longitudinal-template.json` is a non-registered shape example.
Synthetic tests passed, and a historical five-turn rollout with a canonical
compaction correctly surfaced four incomplete or superseded turns instead of
treating them as valid. Published Codex or Luna scores stay contextual unless
model, benchmark version, harness, budget, retries and scoring are comparable.

## SC-EVAL-023: six-project breadth screen (2026-09-22)

Cohort pre-registered before any arm ran, chosen by workload coverage: AGI
Thesis (technical-thesis and evidence synthesis), Latent-Space-Reasoning
(long-running empirical research), Open Exploration (publication research and
buyer evidence), Iqidis (production TypeScript application analysis),
new-computation-model (mathematical and theoretical-computer-science research),
and memory-benchmark-harness (reserved replication, agent-memory evaluation
software). The first five are the longitudinal cohort if the screen is valid;
the sixth is never substituted for an unfavorable result; procurement is
excluded as the frozen development case. Source selections and one question
per project are frozen in `eval/manifests/breadth-projects.json`. Each arm gets
an isolated copy of the `rg --files`-visible corpus without caches, build
outputs, credentials, prior `.blackboard` or `.codex` state; the AGI corpus is
bounded to the root thesis controls plus `publication` and `reviews` (its
multi-gigabyte raw experiment store is outside the screen). The six pre-run
corpus hashes and file counts are in `breadth-snapshot-hashes.json`; preparation
verifies byte identity between arms and the runner rehashes before and after
every turn. Both arms: branch debug binary, cached login with API keys removed,
Luna at high effort, read-only permissions, one continuous thread per project;
Stateful in explicit Autonomous mode. A run is valid only if it completes,
preserves the corpus, exposes project state, commits its durable result, and
yields the blinded quality, source-audit and state observations.

**Pre-cohort isolation correction.** The first AGI pair was an infrastructure
dry run: Stateful's stderr showed an attempted read of the host memory registry,
because removing API-key variables had kept the login but not disabled Codex's
memory subsystem. Ordinary used 1,158,270 total and 132,734 uncached tokens;
Stateful 1,394,703 and 137,743, with 18 against 10 read-bearing operations.
These stay an operational diagnostic and are excluded, since prior
project-specific memory could replace the work being measured. The next
Latent-Space-Reasoning ordinary arm was interrupted once the path was confirmed,
before any outcome was accepted. The runner now requires an explicit
disabled-memory policy (`memories.use_memories=false` and
`memories.generate_memories=false`) with the login kept, and the cohort
restarted from fresh snapshot paths; every final result was collected after the
correction and none was selected for inclusion.

**Outcome: unfavorable and directional only.** Both blind graders preferred
ordinary on five projects and disagreed on AGI Thesis. All twelve arms had zero
compactions, so the screen tested neither compaction survival nor mature state.
The grading packet placed an auxiliary `S` object (the model-submitted Stateful
completion narrative, not the persisted result or project intelligence) beside
the blinded answers, which could reveal the Stateful arm. The grades stay
unchanged but support no causal quality claim. A valid rerun needs answer-only
public packets with a disjoint private mapping, hash-pinned SQLite artifacts
after every Stateful turn, and separate grading of submitted answer, persisted
result, obligations and project intelligence.

## SC-EVAL-024 and SC-EVAL-025: source revision and compaction diagnostics (2026-09-23)

Mechanism diagnostics, not protocol-valid comparisons: the independent quality,
source-audit and state observations were never supplied. Both arms, failed
infrastructure attempts and the two compaction regimes are kept, and neither
regime substitutes for the other.

**SC-EVAL-024 (20,000-token total-context limit).** Clean cohort under
`%LOCALAPPDATA%/Temp/stateful-source-canary-run-final-8421d388` (snapshot
`stateful-source-canary-snapshot-final-8421d388`). Ordinary thread
`01a0cd05-8c21-7990-93a1-5e6727e82259` and Stateful thread
`01a0cd05-8a7b-7603-8adc-f7055ac91c10` answered all four cases correctly.
Stateful detected the changed policy, replaced the threshold of 10 with 6,
changed permitted to not permitted at the unchanged count of 8, and kept the old
conclusion and fingerprint as superseded history; four terminal run and state
artifacts were captured on the one thread. **Economics failed decisively:**
ordinary 229,285 total, 41,637 uncached, 15 responses, one compaction, 179,756
ms; Stateful 833,145 total, 229,241 uncached, 39 responses, 16 compactions,
1,007,707 ms. Reconciliation splits Stateful into 449,138 productive tokens over
23 responses and 384,007 compaction tokens over 16 responses; the extra
compaction explains about 60.3% of the 603,860-token gap. The fixed Stateful
prefix began near the artificial limit, so post-compaction requests kept hitting
it: valid extreme-stress evidence, not proof that the roughly 2.5 KiB root causes
that cost. Three correctness limits: runs started on resumed threads claimed an
Autonomous continuation against the previous run's last turn before the new
question arrived; the first project fragment after the hidden file replacement
still labelled old promoted evidence current until model-driven refresh repaired
it; and the v1 launch finding cited `policy.md` lines 3-5 although the threshold
is on line 6 (the answer was right because the model read the whole file). Old
entries were current-query-inaccessible and their source bytes were not kept
after overwrite; the historical answer was helped because v2 restated the old
threshold. Commit `c65993a196` binds Autonomous continuation to a turn that
started while the same run was active (the resumed-exec test rejects a
continuation instruction in a new prompt); commit `aae58b6512` makes run states
pin their exact rollout path so graders verify the session ID.

**SC-EVAL-025 (20,000-token `body_after_prefix` limit).** Same sources,
questions, model, effort, login, memory isolation and intervention. Artifacts
under `%LOCALAPPDATA%/Temp/stateful-source-growth-run-897af904` and
`stateful-source-growth-snapshot-897af904`. Ordinary thread
`01a0cd2a-7895-7bf2-bfaf-5378018974ee`: 1,002,847 total, 83,039 uncached, 39
responses, 557,834 ms. Stateful thread `01a0cd2a-766c-7b33-9e9a-9453429cd40b`:
507,855 total, 62,415 uncached, 17 responses, 209,877 ms, i.e. 49.36%, 24.84%,
56.41% and 62.38% lower. Both answered all four cases correctly; each Stateful
question created its own completed run, every artifact reports zero
continuations, and both run states pin their dated rollout. **Neither arm
compacted**, so this is an economics and lifecycle improvement, not
equal-pressure compaction continuity; that needs a separately frozen lower
growth threshold, and production-default economics remain a separate lane.
Automated raw-read counts from these Windows rollouts are lower bounds (looped
`type`, `find` and `findstr` commands are not expanded per file), so no
read-saving claim is made.

## SC-EVAL-026: explicit historical-state retrieval (2026-09-23)

Matched mechanism diagnostic, not protocol-valid: blinded quality, source audits
and state observations were not collected before arm identity was known, and the
case named `post-compaction-recall` saw zero compactions in both arms, so it does
not show post-compaction recall. The frozen v2 source removed every mention of
the old threshold. Artifacts under `%LOCALAPPDATA%/Temp/stateful-history-run-20260923b`;
ordinary thread `01a0cd7a-c1cb-7a42-8e28-b40463edce51` and Stateful thread
`01a0cd75-5ad7-7f83-90cf-a2aec12ade18` completed all four turns first time with
correct v1 and v2 decisions. Stateful kept the old 10-defect policy and permitted
decision as superseded revisions, the current 6-defect policy and not-permitted
decision active, and the unchanged count of 8. The reconcile turn's rollout
contains `blackboard_query({entryScope: "historical", ...})`, which returned the
superseded v1 entries, the correct old fingerprint
`79647a43c5c02d1c26d56b5ccbac4287f70f7afb62e6697e86941603d65d01ed` and threshold
10 with stale historical verification; the model then separated that history
from the current source. This closes the earlier defect where history could be
inferred from v2 restating v1, but does not prove counterfactual dependence,
because same-thread conversation history was also present.

| Measure | Ordinary | Stateful | Stateful change |
| --- | ---: | ---: | ---: |
| Full lifetime tokens | 134,154 | 992,033 | +857,879 (+639.47%) |
| Uncached input + output | 26,890 | 84,257 | +57,367 (+213.34%) |
| Model responses | 9 | 27 | +18 |
| Recorded read operations | 7 | 13 | +6 |
| Rejected tool results | 0 | 2 | +2 |
| Canonical compactions | 0 | 0 | 0 |
| Turn duration | 132,442 ms | 317,816 ms | +185,374 ms |

The excess is not compaction: a malformed v1 persistence call, a correct
stale-source rejection followed by refresh, a v2 relationship failure needing
another mutation, and repeated persistence rounds. The last turn is
directionally better on marginal uncached usage (5,260 against 9,541), but the
four-turn lifetime is a large regression. Three integrity defects: the reconcile
completion copied the fingerprint wrongly as the invented hybrid
`79647ad1f8e25747262bade3cba32bbd2b85f1db12c9c2cf00d607f71e36561d` (stored
links keep the correct value); Stateful answer links use `/C:/...`, which is not
a valid Windows path although prose and ranges are right; and the runner kept
only a hash of each turn's corpus, so its grading packet would have shown v2
while grading a v1 answer. Commit `462b5a25c1` keeps one content-addressed corpus
artifact per distinct turn revision and makes a separate blinded packet per
question for future runs. SC-EVAL-026 stays ungraded rather than reconstructing
observations after seeing the arms.

## External custom-harness benchmark ladder (2026-09-23)

Primary-source review only; no submission, maintainer contact or external run
at that point. The unit is an agent-model pair, so model-only scores from an
organizer scaffold cannot measure Stateful Codex; the program runs the same
built binary and `gpt-5.6-luna` with Stateful off and on.

1. **Terminal-Bench 2.1**: the strongest immediate local comparison. Its 89-task
   dataset and Harbor runner accept custom agents and its table includes Codex
   CLI; GPT-5.3-Codex scores 79.1% with Codex CLI and 68.5% with Terminus 2,
   showing the harness matters. Official community submissions were closed;
   public Harbor uploads are shareable but not leaderboard rows. The full
   protocol is five trials per task, 445 per arm.
2. **SWE-bench Verified and Multilingual**: the open publication route (Stateful
   generates patches; the official harness grades; `swebench submit package`,
   `publish`, `register` and `verify` produce a public artifact repository and
   registration pull request; metadata records agent and model separately). The
   default `swebench infer` uses mini-SWE-agent and must not replace our agent.
3. **SWE-bench Pro V2**: a locked local protocol and adapter reference released
   on 2026-09-22 with 642 tasks, not comparable with v1 results; it ships a
   pinned Codex adapter and fresh-sandbox regrading; no open custom-agent
   leaderboard path verified.
4. **SWE-Marathon v1.1**: the most relevant long-horizon supplement; 20 tasks
   through Harbor; its public scripts name Codex with `gpt-5.6-luna` at high
   effort, which is a declared configuration, not a measured score; no open
   submission process verified.
5. **DeepSWE v1.1**: runs custom harnesses locally, but its leaderboard
   standardizes on mini-SWE-agent, so its Luna result is not a
   Stateful-versus-Codex baseline.

Plan then: one version-pinned Harbor installed-agent adapter (not one script per
benchmark) that installs a hashed bundle, isolates state per task and trial,
emits schema-valid ATIF trajectories, keeps all-attempt token and latency costs,
and captures the final patch even on failure; a three-to-five-task smoke before
any frozen full run; official claims only from the complete benchmark.

## SC-EVAL-027: Terminal-Bench `fix-git` technical control

Status: one ordinary control and three Stateful replications completed on
2026-09-23. This is a successful harness and evidence-export pilot, not an
official submission, complete Terminal-Bench score, or statistically meaningful
quality claim.

Every run used task checksum
`d3220d70bc668ec6f4034fab51e62873dff724a61f824d764fd201d6f5e7a88a`,
image `alexgshaw/fix-git:20260403`, `openai/gpt-5.6-luna` at max effort,
cached Codex login with API-key environment variables removed, Harbor commit
`15da91c18580a25489f5bdf2ee71029f3ff3bb2e`, and bundle digest
`a321910f2cae0ccfed8e3b412f4f3ef7a7ec22f62f683444655be6c76c6841ad`.
The bundle identifies source commit `ef3a1ff83886563df02cc567c9ff1fd9cb580d10`.
The control received no Stateful flag; the treatment received only
`--stateful autonomous` in addition to the shared configuration.

Ordinary trial `fix-git__gzoXJJL` found detached commit `c499730`, merged it,
and passed the layout-file test. Its conflict-resolution edit nevertheless
removed the required prior-experience paragraph from `_includes/about.md`, so
the exact-file verifier failed and reward was 0.0. Its eight represented model
calls used 129,629 input tokens, 114,688 cached input tokens, 4,302 output
tokens, 117.848 seconds of adapter execution, and $0.01044436 reported cost.

The first matched Stateful trial, `fix-git__UcYWBSM`, preserved the paragraph,
passed both tests, and received reward 1.0. It used 299,076 input tokens,
260,096 cached input tokens, 8,633 output tokens, 183.076 seconds, and
$0.02335752: 2.24 times the control's reported cost. Two artifact-export
replications also passed, yielding three Stateful passes in three attempts. The
final shareable trial, `fix-git__kbi3spG`, used 575,883 input tokens, 509,952
cached input tokens, 10,611 output tokens, 235.632 seconds, and $0.03611844,
or 3.46 times the preserved control. This task therefore supplies a useful
correctness signal and a clear small-task overhead warning at the same time.

The trajectories explain more than the scalar reward. Both systems recovered
the same Git object and encountered the same about-page conflict. Ordinary
Codex stated that it would preserve the shared paragraph but its patch deleted
it, then stopped after conflict-marker and whitespace checks. Stateful Codex
persisted an early semantic obligation to inspect and preserve the candidate,
refreshed the context map after the merge, opened exact source ranges, recorded
the verified content and build limitation in project intelligence, and carried
those facts through its final obligation and result. That extra verification
correlates with the successful outcome, but this cold-start sample cannot show
that mature persistent memory caused it.

The final trial exports a 4,914-byte committed patch, initial and final commit
IDs, schema-valid ATIF and native trajectories, both verifier cases, three
blackboard entries, two evidence links, two relations, 83 context-map entries,
two obligations, and the completed run. All original state-file hashes matched
before inspection, every SQLite database passed `PRAGMA integrity_check`, and
the portable ten-file state manifest also verifies. The first exporter assumed
the task repository was `/app` and produced empty patches for the control and
first treatment; `fc620e7dd1` now records the actual Git top-level. The control
was deliberately not rerun merely to repair packaging; its transcript and
verifier evidence remain intact and the limitation is explicit.

The local allowlisted packet excludes login material, raw encrypted session
files, lock files, and mutable SQLite shared-memory files. It is stored outside
Git at
`clients/stateful-codex/eval/artifacts/terminal-bench-2-1-fix-git-20260923-public.tar.gz`
with SHA-256
`35f3c218a7bf559583b9583b7b179575c6a65c5b5d206287d5cc914e6142a293`.
Its 40-file manifest and portable-state manifest pass, and a credential-pattern
scan found no matches. The machine-readable tracked result is
`clients/stateful-codex/eval/results/terminal-bench-2-1-fix-git-20260923.json`.
The packet has not been uploaded or presented as an official leaderboard entry.

## SC-EVAL-028: Terminal-Bench four-task Stateful smoke

Status: passed on 2026-09-23. One Stateful trial each of
`cobol-modernization`, `vulnerable-secret`, `db-wal-recovery`, and
`multi-source-data-merger` completed with reward 1.0, yielding 4/4, zero
exceptions, and zero retries in 19 minutes 59 seconds. The run used the same
portable bundle, cached Codex login, Luna max configuration, and pinned Harbor
commit as SC-EVAL-027. All four exact task images passed bundle preflight before
model inference.

The job consumed 2,068,846 input tokens, of which 1,884,160 were cached,
48,487 output tokens, and $0.1328048 reported cost. The trials respectively
used 27, 10, 18, and eight represented model calls. Every verifier assertion
passed: 3/3 for COBOL modernization, 3/3 for vulnerable-secret, 7/7 for WAL
recovery, and 3/3 for the multi-source merger.

Artifact validation found no silent runtime failure. Every captured state-file
hash matched its manifest and all 32 SQLite databases passed
`PRAGMA integrity_check`. Each run was durably `completed` at revision 2.
COBOL modernization retained three blackboard entries, five context-map
entries, eight evidence links, and one relationship. WAL recovery retained two
blackboard entries, four context-map entries, two evidence links, and one
relationship. The other two tasks retained semantic obligations and completed
runs but no reusable project-intelligence entries; their success therefore does
not demonstrate a memory mechanism.

These non-Git `/app` tasks declared no Harbor artifact paths, so their exported
`final.patch` files are empty and the deleted containers' final workspace files
are not independently preserved. The verifier outputs and trajectories prove
the scored outcomes, but this smoke is not yet a complete public evidence
packet. The longitudinal runner captures per-turn corpus snapshots and durable
state and is the correct next vehicle for inherited-state validation.

The public Codex 0.144.1 Luna-max submission reports 75.73% over 445 trials.
Its GitHub record exposes aggregate metrics and opaque trial IDs, but neither
the original source job nor the leaderboard-owned clone returned task rows to
an authenticated non-maintainer Harbor account. Exact public Codex results for
these four tasks therefore remain unavailable. Comparing this selected 4/4
directly with the full-suite 75.73% would confound task mix, sample size, and
build version and is not permitted.

The tracked result is
`clients/stateful-codex/eval/results/terminal-bench-2-1-stateful-smoke4-20260923.json`.

SC-EVAL-029 was retired here; see its own record after this entry.

The next external gate follows the canonical Terminal-Bench 2.1 protocol: all
89 official tasks, five fresh independent trials per task, 445 trials total.
Every trial receives a new task container and isolated Stateful database; no
project intelligence crosses trial boundaries. Trials may execute concurrently
because concurrency changes scheduling rather than task semantics. The pinned
job configuration is
`clients/stateful-codex/eval/manifests/terminal-bench-2-1-stateful-full.json`.
The public Codex result remains a descriptive comparator because its released
Codex binary differs from this branch build.

The first full launch expanded all 445 slots but stopped every slot during
agent installation before model execution. The global `harbor` command had
resolved PyPI Harbor 0.16.1, whose Codex base class lacks the pinned adapter's
`ensure_system_dependencies` contract. The earlier four-task smoke had run the
compatible pinned API, but the adapter merely wrote the intended Harbor commit
to artifacts and did not verify the executing package. The failed job remains
at `%LOCALAPPDATA%/Temp/stateful-harbor-full-20260923/sc-tb21-stateful-full445-ef3a1ff`.
It contains 445 uniform setup exceptions, zero valid trials, no agent
executions, and no verifier executions.

The adapter now rejects an unpinned, mismatched, or dirty Harbor source runtime
once during preflight. The canonical run uses a clean checkout of commit
`15da91c18580a25489f5bdf2ee71029f3ff3bb2e` and `uv sync --frozen`; a direct
check rejects global Harbor 0.16.1 and accepts the pinned Harbor 0.23.0 checkout.
An install-only `fix-git` check through that executable completed in 25 seconds
with zero exceptions before the scored relaunch.

### Full-run checkpoint and failure ledger

The pinned relaunch was deliberately checkpointed after 19 result records when
the eight active workers prevented a second Windows process from observing the
job. Ten records are valid scored trials and must remain in the final result:
eight rewards of 1.0 (`kv-store-grpc`, `openssl-selfsigned-cert`,
`log-summary-date-ranges`, `write-compressor`, `torch-tensor-parallelism`,
`model-extraction-relu-logits`, `regex-log`, and
`schemelike-metacircular-eval`) and two rewards of 0.0 (`pypi-server` and
`dna-assembly`). The checkpoint job is retained at
`%LOCALAPPDATA%/Temp/stateful-harbor-full-20260923/sc-tb21-stateful-full445-pinned-ef3a1ff`.

The two scored failures are product evidence, not infrastructure exclusions:

- `pypi-server` built `vectorops-0.1.0`, served it successfully, and installed
  it from the requested local index while the agent was active. The model first
  noticed that a `nohup` server was cleaned up with its shell, then moved the
  server into a persistent command session. That session still belonged to the
  agent lifecycle and ended before Harbor invoked the verifier. The verifier's
  independent `pip install` therefore received connection refusals. The lesson
  is that self-validation is insufficient when the delivered service must
  survive agent teardown; the final environment boundary must be tested.
- `dna-assembly` produced structurally valid primers and matched every required
  annealing site, but its eGFP pair failed the verifier's melting-temperature
  calculation. The official values were 67.716453 and 62.305244 degrees C, a
  5.411209-degree difference against a maximum of 5.0. The 0.411209-degree miss
  shows that scientific tasks require the exact specified or verifier-equivalent
  calculation rather than a nearby local approximation.

One checkpoint record, `qemu-alpine-ssh`, is infrastructure-invalid. The agent
never ran because `apt-get install ca-certificates ripgrep` fetched a stale
Debian Bullseye security package URL and received HTTP 404. It contributes no
score and requires a fresh independent replacement. The other eight records
were cancelled by the checkpoint while their tasks were active and likewise
contribute no score: `torch-pipeline-parallelism`, `circuit-fibsqrt`,
`path-tracing`, `pytorch-model-recovery`,
`llm-inference-batching-scheduler`, `caffe-cifar-10`, `mteb-leaderboard`, and
`regex-chess`.

The resume work exposed three orchestration mistakes before consuming any valid
trial slot. First, Harbor 0.23 uses repeatable singular
`--exclude-task-name`, not the plural spelling. Second, CLI dataset filters
cannot attach to a dataset supplied only through `--config`; they belong inside
the dataset object. Third, Harbor matches the fully qualified
`terminal-bench/<task>` name, so bare names excluded nothing. Dry runs caught
all three errors. Short seven-, six-, and four-worker launches were then
cancelled while diagnosing the Windows observer-process conflict. They produced
only cancellations plus repeated pre-agent `qemu-alpine-ssh` mirror failures,
no valid verifier result, and no reusable score.

The remaining protocol is split into bounded jobs rather than one opaque
435-trial process. Task names are sorted deterministically from the pinned
445-trial lock. Round 1 schedules one attempt for the 79 tasks without a valid
checkpoint result in batches of 30, 30, and 19. Rounds 2 through 5 each schedule
one fresh attempt for all 89 tasks in batches of 30, 30, and 29. This yields 15
jobs and exactly 435 remaining trial slots; together with the ten preserved
valid checkpoint results, it yields the canonical 445. Each job uses eight-way
concurrency, fresh containers and isolated Stateful databases, zero automatic
retries, and an inspection gate before the next job. Valid rewards of either
0 or 1 are immutable. Only a trial with no agent/verifier result because of an
infrastructure exception or explicit cancellation is replaced, and every such
replacement remains documented.

### Round 1, batches 1 and 2

Status: 60 additional valid trials completed on 2026-09-23. Together with the
ten preserved checkpoint trials, the run now has 70 valid trials over 70
distinct tasks: 56 rewards of 1.0 and 14 rewards of 0.0, or 80.00% for this
single-attempt breadth screen. This is not the Terminal-Bench headline score.
The official accuracy is the number of successful trials divided by all 445
fresh trials. Best-of-five task coverage is separately reported as `pass@5`.
For reference, the published Codex 0.144.1 Luna-max submission reports 337 of
445 successful trials (75.73% +/- 1.32 percentage points) and `pass@5` 0.8876,
equivalent to 79 of 89 tasks solved at least once. Its different Codex build
makes it a descriptive comparator, not a matched control.

Primary scoring and comparator sources are the official
[`metrics.py`](https://github.com/harbor-framework/terminal-bench-2-1/blob/main/leaderboard/src/leaderboard/core/metrics.py),
[Codex Luna-max submission](https://github.com/harbor-framework/terminal-bench-2-1/blob/main/leaderboard/submissions/2026-07-11-openai-gpt-5-6-luna-max-codex.json),
and [Terminal-Bench 2.1 task chart](https://github.com/harbor-framework/terminal-bench-docs/blob/main/components/terminal-bench-2-1-charts.tsx).

Round-one batch 1 is retained at
`%LOCALAPPDATA%/Temp/stateful-harbor-full-20260923/sc-tb21-stateful-r1b1-30-pinned-ef3a1ff`.
It completed 30 valid trials in 55 minutes 57 seconds with 25 passes and five
failures. It used 78,334,480 input tokens, 75,023,744 cached input tokens,
589,365 output tokens, and $2.86986008 of reported cost. The five failures are:

- `chess-best-move`: `/app/move.txt` contained the valid mating move `g2g4`
  but omitted the second required mating move `e2e4`. This is an
  exhaustiveness failure, not an infrastructure failure.
- `configure-git-webserver`: the agent built and locally exercised the Git
  deployment path, but tested Git through the local filesystem as the existing
  `ubuntu` account rather than through the requested
  `user@server:/git/server` transport. It never installed or started an SSH
  server, and it kept its Python HTTP process alive with `exec sleep infinity`
  inside an agent-owned command session. That session ended before the
  verifier, so the official post-agent SSH clone/push/curl workflow received
  HTTP 000. The prompt does not literally say "survive agent teardown," but it
  asks for servers the user can access after configuration; post-agent
  availability is therefore part of the requested outcome. The official
  reference independently confirms the intended implementation by starting
  both SSH and nginx as services. This is a valid delivery-lifecycle failure,
  not a hidden-condition or infrastructure exclusion.
- `dna-insert`: the primers were structurally valid and annealed at the correct
  sites, but the official melting temperatures were 66.274364 and 59.742459
  degrees C. Their 6.531905-degree difference exceeded the required maximum of
  5 degrees. This is a scientific-calculation near miss.
- `extract-moves-from-video`: the agent exhausted the 1,800-second budget and
  never created `/app/solution.txt`. The verifier ran and failed both output
  checks, so the timeout remains a scored zero.
- `filter-js-from-html`: six executable XSS cases survived, and five of twelve
  clean inputs were changed after the verifier's normalization. This is a
  substantive security/correctness failure rather than a formatting-only miss.

Batch 1 was not easy-task dominated. Its declared mix was nine hard, 19 medium,
and two easy tasks; Stateful passed seven hard, 16 medium, and both easy tasks.
Among the 24 successful tasks with nonzero author expert-time estimates, the
median was 60 minutes and the mean 224.58 minutes. The five failures had a
45-minute median and 51-minute mean. The official Terminal-Bench 2.1 task chart
covers only a subset, but the covered failures include tasks with representative
agent pass rates of 47.1% (`configure-git-webserver`), 17.1%
(`extract-moves-from-video`), and 4.3% (`filter-js-from-html`).

Round-one batch 2 is retained at
`%LOCALAPPDATA%/Temp/stateful-harbor-full-20260923/sc-tb21-stateful-r1b2-30-pinned-ef3a1ff`.
It completed 30 valid trials in 49 minutes 49 seconds with 23 passes and seven
failures. It used 87,629,742 input tokens, 84,237,056 cached input tokens,
775,966 output tokens, and $3.29443752 of reported cost. Its failures are:

- `gcode-to-text`: the agent reconstructed and visually inspected the embossed
  geometry, but wrote `flag{gcode_iz_ch4LLenc1Ng}` instead of the verifier's
  `flag{gc0d3_iz_ch4LLenGiNg}`. It then exact-read and persisted its own wrong
  generated file. This separates artifact verification from semantic
  correctness: evidence that the requested bytes were written is not evidence
  that the decoded bytes are right.
- `gpt2-codegolf`: the 900-second budget expired after the agent produced a
  compiling sub-5,000-byte implementation and a correct GPT-2 token ID for its
  smoke prompt, but its generation remained repetitive and wrong. The verifier
  compiled and ran the program; the required warranty substring was absent.
  This is an incomplete numerical/model-layout implementation.
- `largest-eigenval`: the 900-second budget expired just after the agent found
  and repaired a column-permutation validation bug. All 18 eigenpair/dominance
  assertions passed, as did speed at sizes 2, 3, 4, 7, 8, 9, and 10, but the
  candidate lost the timing comparisons at sizes 5 and 6. This is a valid
  performance near miss rather than infrastructure noise.
- `make-doom-for-mips`: the agent installed the cross-toolchain and compiled 81
  Doom objects, but did not finish a compatible freestanding runtime and ELF
  before the 900-second limit. `vm.js` did not produce `/tmp/frame.bmp`, and all
  three verifier cases failed. This is a major incomplete implementation.
- `mteb-retrieve`: the agent loaded the exact model revision through the
  required MTEB version but chose raw embeddings because the prompt does not
  name an MTEB retrieval task. That ranked HumanEval fifth; the reference uses
  SciFact query/passage prompts and expects `MTEB: Massive Text Embedding
  Benchmark`. The official zero is valid for protocol accounting, but the
  failure is specification-sensitive because the hidden SciFact choice is not
  stated explicitly in the task.
- `overfull-hbox`: compilation and input-integrity checks passed, but two
  overfull warnings remained, including 20.01048 pt and 0.46426 pt. The agent
  exhausted the 750-second budget while iterating allowed synonym replacements.
  This is a valid incomplete near miss.
- `protein-assembly`: the output was one valid DNA line within the size and GC
  limits, but the translated fusion did not contain the verifier-recognized
  donor sequence in the required FLAG-donor-DHFR-acceptor-SNAP order. The agent
  manually expanded crystallographic placeholder residues and persisted its
  inferred component selection without proving exact FASTA identity. This is a
  substantive biological-selection/sequence failure.

Batch 2 contained 11 hard, 17 medium, and two easy tasks. Stateful passed eight
hard, 14 medium, and one easy task. The 23 successes had a 60-minute median and
112.39-minute mean declared expert estimate; the seven failures had a 60-minute
median and 447.86-minute mean, driven especially by the 2,400-minute
`gpt2-codegolf` estimate and 480-minute Doom estimate. The failures therefore
skew harder rather than easier by author effort. Public representative-agent
rates reinforce the mix: Stateful passed `install-windows-3.11` at 21.4%,
`mteb-leaderboard` at 44.3%, and `mcmc-sampling-stan` at 72.9%; it failed
`gpt2-codegolf` at 14.3%, `make-doom-for-mips` at 4.3%,
`protein-assembly` at 20.0%, `mteb-retrieve` at 45.7%, and `overfull-hbox` at
51.4%. These rates are task-mix context, not task-level Codex baselines.

The Stateful artifacts are intact. Batch 1's 372 and batch 2's 432 exported
state-manifest files all match their recorded SHA-256 values. All 240 SQLite
databases in each batch pass `PRAGMA integrity_check`. Completed trials retain
terminal revision-2 runs; the four batch-2 timeouts retain revision-1 running
records because they never reached the terminal mutation, which is faithful to
the interrupted execution rather than a hidden successful completion. Batch 2
also demonstrates bounded persistence at very different corpus sizes: most
trials created zero to hundreds of context-map routes, while the successful
`install-windows-3.11` task retained 20,000 routes and 23,180 hierarchy nodes.

Resource sampling during batch 2 produced 89 observations. Host CPU averaged
37.12%, reached 97%, and had a nearest-rank 95th percentile of 90%; free memory
never fell below 17,243 MiB, and eight task containers were active at once.
Memory was not the constraint, but CPU saturation and Docker/I/O contention
make a higher concurrency level an unjustified protocol risk. Subsequent jobs
therefore remain at eight workers and do not overlap.

Across the ten-trial checkpoint and these two batches, cumulative reported usage
is 183,477,835 input tokens, 175,474,816 cached input tokens, 1,693,991 output
tokens, and $7.14288932. The final round-one manifest contains the exact 19
remaining task names derived by subtracting the 70 valid distinct tasks from
the pinned 89-task lock:
`clients/stateful-codex/eval/manifests/terminal-bench-2-1-stateful-r1b3-19.json`.
It retains one attempt, eight independent workers, fresh per-trial state, and
zero automatic retries.

The pre-batch dry run resolved exactly those 19 trials. A direct setup probe
then reproduced the earlier `qemu-alpine-ssh` infrastructure failure without a
model call: the Bullseye image already contained a nonempty CA certificate
bundle, but Harbor's unconditional `ca-certificates` install tried to upgrade
it to a security-mirror version whose package URL returned HTTP 404. Installing
the actually missing `ripgrep` package with the existing CA bundle succeeded.
The adapter now installs normal command dependencies first and installs
`ca-certificates` only when `/etc/ssl/certs/ca-certificates.crt` is missing or
empty. This does not change the task, model, bundle, or Stateful behavior; it
removes an unnecessary package upgrade that prevented the agent from starting.
An exact-image Harbor install-only trial then completed in 20 seconds with zero
exceptions and no model or verifier execution. The scored batch may therefore
include the task without knowingly repeating the pre-agent failure.

### Round 1, batch 3

Round-one batch 3 is retained at
`%LOCALAPPDATA%/Temp/stateful-harbor-full-20260923/sc-tb21-stateful-r1b3-19-pinned-ef3a1ff`.
It ran for 1 hour 6 minutes 9 seconds and produced 19 reward-bearing records:
13 apparent passes and six apparent failures. Two of those failures are invalid
verifier-infrastructure records, leaving 17 valid trials with 13 rewards of 1.0
and four rewards of 0.0. The 13 valid passes are
`pytorch-model-recovery`, `query-optimize`, `regex-chess`, `reshard-c4-data`,
`rstan-to-pystan`, `sam-cell-seg`, `sparql-university`, `sqlite-db-truncate`,
`sqlite-with-gcov`, `torch-pipeline-parallelism`, `tune-mjcf`,
`vulnerable-secret`, and `winning-avg-corewars`.

The four valid failures are:

- `train-fasttext`: the agent exhausted its execution budget after building
  fastText and running a long sequence of feature, loss, and quantization
  experiments. Its last measured trigram candidate reached 0.6188 on the
  supplied test data, just below the requested 0.62 threshold, but the decisive
  failure is more basic: it never checkpointed any candidate to the required
  `/app/model.bin`. The verifier therefore failed both accuracy and size checks
  because the deliverable was absent. This is a search-budget and deadline
  management failure. A best-known valid artifact should have been installed
  before spending the remaining budget on optional experiments.
- `raman-fitting`: the agent parsed the decimal-comma data and performed many
  numerical fits, but treated the first column as the fitting coordinate. The
  task data require the reciprocal conversion `1e7 / x` before selecting the
  G and 2D Raman bands. Consequently it fit raw-coordinate features near
  19,197 and 33,251 instead of peaks near 1,580 and 2,670. The produced G
  parameters were `x0=19197.453631`, `gamma=402.843617`,
  `amplitude=73538.824490`, and `offset=12162.500740`, versus expected values
  1580.3, 9.06, 8382.69, and 5561.03; the 2D values were similarly wrong.
  This is a scientific unit/domain interpretation failure, not optimizer
  precision noise.
- `video-processing`: the script existed, ran, used only allowed imports, and
  passed three of five verifier assertions. On the example it reported takeoff
  54 within the accepted 50--54 range but landing 61 against 62--64. On the
  private video it reported takeoff 216 against 219--223; the assertion stopped
  before the landing value was evaluated. This is a genuine but close
  generalization/calibration miss: one frame early on the visible landing and
  three frames early on private takeoff.
- `sanitize-git-repo`: all planted credentials were removed, placeholders were
  consistent, and no unrelated file was edited. The sole failing condition was
  exact content preservation for the contaminated JSON file: patch application
  added a final newline to a file that originally had none. The agent explicitly
  detected and reported that difference but accepted it as harmless, while the
  task required avoiding any non-secret modification. This is a valid
  exact-byte correctness failure and a useful example of self-observation not
  being converted into corrective action.

The first `qemu-alpine-ssh` and `qemu-startup` records are not scored zeros. In
both trials the agent completed, but the official verifier's own setup attempted
to install `curl` from stale Debian Bullseye security-mirror package URLs.
Downloads for `libnghttp2-14`, `libcurl4`, and `curl` returned HTTP 404. The
verifier then had no `curl`, could not install `uv`, had no
`/root/.local/bin/env`, and had no `uvx`; no substantive task assertion ran.
These verifier-prerequisite failures remain documented and consume no valid
trial slot. The adapter's earlier CA-package fix did work: both agents ran, so
this was a separate downstream failure.

A no-model probe established that both unmodified task images already contained
commented Debian snapshot sources pinned to 2025-10-20 and that enabling only
those image-authored sources made the verifier prerequisites installable. The
replacement job therefore bind-mounted an explicit source list containing those
same three snapshot repositories read-only at `/etc/apt/sources.list`. It did
not change either task prompt, image, agent, model, verifier, or answer. Because
this is an explicit host-side infrastructure repair rather than the untouched
published environment, the results are labeled **infrastructure-repaired** and
must not be represented as an untouched official submission.

The pinned replacement job is retained at
`%LOCALAPPDATA%/Temp/stateful-harbor-full-20260923/sc-tb21-stateful-r1b3-qemu-repaired-2-pinned-ef3a1ff`.
It used the committed replacement manifest at
`clients/stateful-codex/eval/manifests/terminal-bench-2-1-stateful-r1b3-qemu-replacement-2.json`,
ran both official verifiers substantively, and produced one pass and one
failure. `qemu-startup` completed in 431 seconds and passed the official kernel
and telnet assertion. `qemu-alpine-ssh` reached the 900-second agent limit; the
verifier then ran normally but received `Connection reset by peer` from port
2222 instead of an authenticated Alpine shell. That record is a valid task
failure despite the accompanying `AgentTimeoutError`, because the requested
post-agent service was tested and failed. The replacement job used 4,003,335
input tokens, 3,802,368 cached input tokens, 37,453 output tokens, and
$0.16118436 of reported cost. All 32 files named by the two exported state
manifests match their recorded SHA-256 values, and all 16 copied SQLite
databases pass `PRAGMA integrity_check`.

The valid batch mix was six hard and 11 medium tasks. Stateful passed four hard
and nine medium tasks and failed two of each. Successful tasks had a declared
expert-time median of 60 minutes and mean of 274.23 minutes; failures had a
30-minute median and 116.25-minute mean. This batch therefore provides no
evidence that the high pass rate came from an easy subset. In particular, the
hard passes include `regex-chess` (1,440 expert minutes), `sparql-university`
(800), `sam-cell-seg` (600), and `torch-pipeline-parallelism` (240).

All 237 files named by the batch's exported state manifests match their
recorded SHA-256 values, and all 152 exported SQLite databases pass
`PRAGMA integrity_check`. Completed trials have terminal revision-2 run
records; `train-fasttext` correctly remains a revision-1 running record because
the timeout interrupted it before terminal state mutation. Persistence scaled
from small records to 19,804 context-map routes and 19,809 hierarchy nodes for
the successful `reshard-c4-data` trial, and to 2,389 routes and 2,452 nodes for
the successful `sqlite-with-gcov` trial.

The whole 19-record execution used 51,258,105 input tokens, 49,057,024 cached
input tokens, 519,731 output tokens, and $2.04503388. Those execution totals
include the two invalid qemu trials because the agents had already run. The 17
valid trials alone used 48,441,740 input tokens, 46,402,816 cached input tokens,
483,204 output tokens, and $1.91568592. Across every executed checkpoint and
round-one batch record, including the two invalid qemu attempts, cumulative
usage is 234,735,940 input tokens, 224,531,840 cached input tokens, 2,213,722
output tokens, and $9.18792320.

Round 1 is complete as a single-attempt breadth screen. The canonical ledger
contains one valid fresh result for every one of the 89 distinct tasks: 70
passes and 19 failures, or **78.65%**. By declared difficulty, Stateful passed
3 of 4 easy tasks (75.00%), 45 of 55 medium tasks (81.82%), and 22 of 30 hard
tasks (73.33%). The result is not easy-task dominated, and seven of the nine
hard tasks in batch 1, eight of the eleven hard tasks in batch 2, and four of
the six hard valid tasks in batch 3 passed.

Across the checkpoint, all three breadth batches, both invalid original qemu
records, and the two infrastructure-repaired replacements, total execution was
238,739,275 input tokens, 228,334,208 cached input tokens, 2,251,175 output
tokens, and $9.34910756 of reported cost. Those are execution-accounting totals,
not an official 445-trial score. The 78.65% breadth rate is directional
pass-at-one evidence only; Terminal-Bench's published five-attempt protocol
still requires 445 fresh independent trials and reports both trial accuracy and
task-level pass-at-five.

## SC-EVAL-029: concurrent project-family smoke (retired 2026-09-23)

SC-EVAL-029, the proposed product-specific concurrent project-family smoke, was
retired before it produced a result. One procurement turn completed correctly
but hit an over-narrow state-evidence assertion; one source-change turn
completed before the orchestration process interrupted its second turn; and
the first licensing turn was interrupted. Those partial records remain outside
Git as negative harness evidence and are not repaired, scored, or described as
Terminal-Bench. Terminal-Bench 2.1 defines 89 independent tasks rather than
cross-task longitudinal families, so no relationship between its tasks will be
inferred for evaluation.

**Disposition (2026-10-07 cleanup).** Its only dedicated input,
`clients/stateful-codex/eval/manifests/concurrent-family-smoke.json` (231 lines,
SHA-256 `d0795c14cc3e62796b3f68cdf460f5dac5d3ffd0db5e4e1b9e521764fd9ec386`),
was removed from the tree. No script, test, package command or document
selected it. It named "SC-EVAL-029 concurrent family inheritance smoke":
`gpt-5.6-luna` at high effort, Autonomous mode, memories disabled, a
20,000-token `body_after_prefix` compaction limit, and three families
(`procurement-smoke` with cases `viability`, `authority-and-cost` and
`continuity-basis`; `licensing-smoke` with `economics`, `territory` and
`termination-risk`; `source-change-smoke` over the `source-change-canary` fixture with `learn-v1`,
`revise-v2` carrying the `executed-policy-v2-smoke` intervention, and
`reconcile`). The exact file is
recoverable as
`46d9071051445583c00b21e0163d373372d1a966:clients/stateful-codex/eval/manifests/concurrent-family-smoke.json`.
The shared procurement, licensing and source-change fixtures and the generic
`--manifest` runner remain, and the external partial records are unchanged.

## SC-EVAL-030: feedback-conditioned persistent-state pilot

### Question and protocol

This pilot asks a product question that the official Terminal-Bench protocol
does not ask: when an agent revisits the same project and can retain structured
knowledge of its prior work and authoritative verifier outcomes, does its work
become cheaper or more successful?

Each attempt received a fresh task container. Only the task's own Stateful
project database persisted, so no task could inherit facts from another task.
Attempts two through five also received the cumulative official verifier
history from earlier attempts. The model was `openai/gpt-5.6-luna` at maximum
effort through the cached ChatGPT login. The dataset, Harbor runtime, and agent
bundle were pinned. This is therefore a feedback-conditioned longitudinal
product experiment, not Terminal-Bench pass-at-five and not an official score.

The pilot was intentionally frozen after 50 valid attempts. Eight of the 15
selected task families had completed all five attempts; the other seven had
between one and three completed attempts. Continuing the remaining 25 repeats
would add less information than moving the same evaluation budget to a new
scientific-research domain. The complete machine-readable frozen ledger is
`clients/stateful-codex/eval/results/terminal-bench-2-1-feedback-pilot-50-20260923.json`
(SHA-256 `765046a7c0edd22f023962ba07069e6eb098ebd3ad21bbf76763b148c1d02a9c`).

### Frozen result

Across all 50 completed attempts, 40 passed and ten failed, for an 80.0% trial
pass rate. Reported execution cost was $5.12689804. The run recorded two missed
project-state mutations and nine infrastructure retries; both are included in
the public ledger rather than silently discarded.

The clean longitudinal comparison is the eight-family cohort with all five
attempts complete:

| Task | Rewards, attempts 1-5 | Cold cost | Final cost | Final vs. cold |
| --- | --- | ---: | ---: | ---: |
| `vulnerable-secret` | `11111` | $0.0192 | $0.0189 | -1.9% |
| `winning-avg-corewars` | `11111` | $0.1141 | $0.0293 | -74.3% |
| `regex-chess` | `10101` | $0.3512 | $0.3075 | -12.4% |
| `sam-cell-seg` | `11011` | $0.1360 | $0.1059 | -22.2% |
| `torch-tensor-parallelism` | `11111` | $0.0463 | $0.0329 | -28.9% |
| `configure-git-webserver` | `01111` | $0.1254 | $0.1235 | -1.5% |
| `video-processing` | `00000` | $0.1422 | $0.1604 | +12.8% |
| `sanitize-git-repo` | `01111` | $0.1149 | $0.0403 | -64.9% |

For those eight families:

- cold attempts passed 5/8; fifth attempts passed 7/8;
- two of the three cold failures were repaired by the fifth attempt;
- cold attempts cost $1.04927264 in aggregate and fifth attempts cost
  $0.81863588, a 22.0% reduction;
- the mean across all 32 warm attempts was $0.10381426 versus a $0.13115908
  cold-attempt mean, a 20.8% reduction; and
- among the five families that passed both cold and final attempts, final cost
  was 25.8% lower while preserving a passing result.

### What the pilot does and does not establish

The directional result supports the product thesis: persistent project state
can help repair failed work and can reduce the cost of later work on the same
project. It is especially encouraging because this was the first execution of
the feedback-aware protocol against an early system rather than a tuned repeat
study.

The result is not causal proof that project intelligence alone produced the
reduction. Prompt caching, model sampling, explicit verifier feedback, and the
task distribution also affect cost and success. There was no repeated ordinary
Codex control arm. Later attempts were not monotonically better:
`regex-chess` regressed twice and recovered twice, `sam-cell-seg` regressed once,
and `video-processing` failed all five attempts while becoming more expensive.
The two persistence misses also show that the state update path is not yet
perfectly reliable. Those are product findings, not records to remove.

Seven interrupted attempts created during the decision to pivot were excluded
before scoring because their Harbor processes had ended while task containers
were still running. Their mounted project-state directories were archived, the
exact pre-attempt baselines were restored and hash-verified, and the stopped
containers and task networks were removed. No partial verifier result or
partial state mutation entered the 50-attempt ledger.

### Next gate

The next external evaluation is BixBench. It provides a materially different
scientific-research workload and therefore more information about whether the
same project-intelligence mechanisms generalize beyond software execution. The
first gate is a custom-harness compatibility and scoring smoke; a larger run is
allowed only after task isolation, artifact capture, and evaluator integrity
are demonstrated.

## SC-EVAL-031: BixBench custom-harness gate

Status: pre-registered on 2026-09-23 before any BixBench model call; compatible
local gates completed on 2026-09-24.

This evaluation tests whether the Stateful Codex harness can perform auditable
scientific-data work under BixBench v1.5. It does not test longitudinal memory:
every scored question receives a fresh workspace and fresh Stateful databases.
Any later same-capsule persistent-state study is a separately labelled product
experiment and cannot be merged into this result.

The protocol pins the official BixBench repository, dataset revision, capsule
archive hashes, and FutureHouse scientific-environment source revision. The
published environment image is ARM64-only. The completed local gates on this
AMD64 host used a recorded debug-base-derived compatible image rather than a
completed rebuild of the unchanged pinned Dockerfile; the repair image added
the pinned Excel reader to that same base. Both image IDs and complete package
inventories were retained. The agent sees only the single capsule `Data` directory;
reference notebooks, answer keys, distractors, Codex memories, live web search,
and known benchmark-source hosts are excluded. Cached ChatGPT authentication is
used with API-key environment variables removed.

The frozen execution sequence is:

1. One Stateful smoke on `bix-18-q1`, Luna at maximum reasoning effort,
   concurrency one, and a one-hour agent timeout.
2. If the smoke is operationally valid, the five preregistered Bix-18 questions
   run question-isolated with concurrency two.
3. If that expansion retains valid artifacts, the preregistered breadth gate
   runs one question from each of ten distinct capsules with concurrency two.

There is one attempt per task. A detected provider/authentication outage, a
failure before the Codex invocation boundary, or a positively identified
container-engine disconnect is invalid and may be repeated once in the same
arm. Agent timeout, an ordinary nonzero agent exit, a missing/malformed final
answer, or prohibited benchmark-source access is a recorded failure. Tasks are
never substituted after outcomes are known.

The smoke is operationally valid only if it retains a parseable structured
answer, a structurally valid submitted notebook, an offline network-disabled
replay of that notebook, intact protocol audit, complete usage/artifact hashes,
both integrity-valid Stateful databases, and a terminal `completed` run. Local
answer correctness is reported independently; an incorrect but operationally
valid smoke triggers diagnosis before expansion rather than being relabelled as
infrastructure failure.
The adapter reports these as separate `operationalValidity` and local-verifier
fields; aggregate correctness never masks a broken notebook or durable run.

Primary reported measures are local metadata-verifier correctness, notebook
reproducibility, durable run completion, full and uncached tokens, wall time,
and persisted hierarchy/context-map/blackboard/relationship/obligation counts.
String/range metadata checks are diagnostic and are not called official
BixBench scores because current official open-answer postprocessing uses an LLM
judge. Official-shaped records are retained for later scoring under a separately
pinned judge protocol. Published BixBench aggregates may provide context but
are not treated as matched controls. An ordinary Codex arm is not required for
this operational smoke; one may be added only as an explicitly labelled
technical control if a failure cannot otherwise be localized.

### Compatible local results

These results are not official BixBench scores. They use the pinned tasks and
capsule hashes, a debug-base-derived AMD64 compatible image, cached Codex login,
Luna at maximum reasoning effort, fresh state per question, and local metadata
verification. No ordinary-Codex arm was run. The compact machine-readable
record is
`clients/stateful-codex/eval/results/bixbench-v1-5-compatible-gates-20260924.json`.

The first smoke passed its local verifier, notebook replay, protocol audit, and
durable completion checks. The five-question single-capsule gate then produced
four local passes and five operationally valid, replayable notebooks. Its only
local miss was `0.2901422004` against a `25-30` range: the calculation is
29.01422004 percent, so this remains a local-verifier miss but is diagnosed as
a percent-scale response mismatch rather than a different analysis.

The frozen ten-capsule breadth gate was materially weaker:

| Measure | Result |
| --- | ---: |
| Local metadata-verifier correct | 3 / 10 |
| Operationally valid | 7 / 10 |
| Correct and operationally valid | 2 / 10 |
| Reproducible notebooks under protocol v1 | 7 / 10 |
| Completed durable Stateful runs | 9 / 10 |
| Input tokens | 13,901,862 |
| Cached input tokens | 12,906,240 |
| Uncached input tokens | 995,622 |
| Output tokens | 231,374 |
| Wall time at concurrency two | 4,137.134 seconds |

The three locally correct answers were `bix-18-q1`, `bix-19-q1`, and
`bix-51-q1`. The first two were fully valid. `bix-51-q1` installed `openpyxl`
inside the agent container, so its correct notebook failed clean replay in the
original image. The protocol-v2 repair image pins `openpyxl==3.1.5`; the post-hoc
control reproduced the same answer (`0.3952158855`) from pristine inputs with
network-disabled replay and no runtime installation.

The original `bix-39-q2` process ended with Docker exit 125 and
`error waiting for container: unexpected EOF` after Codex had started. Review
of container stderr makes this an infrastructure-invalid run, not an answer
failure. Its isolated protocol-v2 replacement completed and replayed cleanly
but answered `3` against metadata `2.5`. The notebook treated the exome
workbooks as already exon-filtered; the reference workflow additionally removes
intronic, intergenic, and UTR sequence-ontology rows. The replacement therefore
confirms a real analysis-interpretation miss after the infrastructure defect is
removed.

The other misses are not one homogeneous failure class:

- `bix-30-q1` omitted the reference workflow's log2 transform and exclusions
  of P_3 and C_18, producing 36% rather than 28%.
- `bix-29-q1` used a different severity field, cohort, and interaction model,
  producing 1.25699 rather than the reference model's approximately 1.633.
- `bix-1-q1` used a condition-only DESeq2 model and a different enrichment
  universe; the reference removes two samples, models sex plus condition, and
  uses the supplied Gencode universe.
- `bix-27-q2` clustered all 222 expression columns while the reference first
  reduces to 178 matched subjects and reports 173 stable assignments.
- `bix-52-q1` reported W/chromosome-1 density; the inverse is approximately
  0.658 and falls in the reference range, while the question does not name the
  numerator explicitly.
- `bix-53-q3` used all six samples and the prompt's raw-p threshold, while the
  reference uses a four-sample analysis and an adjusted-p threshold despite the
  question saying `p<0.05`. Its submitted notebook also depended on live gene
  mapping and enrichment services, so offline replay failed.

Several tasks therefore depend on workflow choices not fully stated in the
question, including sample exclusions, cohort alignment, numerator direction,
covariates, enrichment universe, and even raw versus adjusted p-value. That
does not convert the local misses into passes. It does mean this small local
metadata score should be read as a diagnostic of reference-workflow recovery,
scientific assumption handling, and artifact reproducibility rather than a
clean estimate of general scientific reasoning quality.

Protocol v1 also replayed notebooks from the post-agent workspace and did not
reject runtime package installation. Protocol v2 now replays from untouched
capsule inputs plus only the submitted notebook, rejects runtime Pip/Conda/R
package installation, pins the missing Excel reader in the explicitly
compatible image, and separates known container-engine disconnects from agent
failures. The two-task repair passed every operational check under this stronger
protocol. The negative scientific answer remained negative. Protocol v3 adds a
required replayed `BIXBENCH_ANSWER=<answer>` marker matching the submitted
structured answer and treats Codex JSONL usage as cumulative thread snapshots
rather than summing them across turns. The published v1/v2 results predate
those two controls and remain labelled accordingly.

## SC-EVAL-032: Pramana ten-question matched stateful/ordinary A/B

Status: completed on 2026-09-24. All 20 turns and the blind grading (two rounds) are finished.

This was a qualitative probe requested by Devansh, not a protocol-valid longitudinal result. It used one project, one grader, and n=10.

**Scope (added 2026-09-27):** this is the *one continuous thread* regime, where ordinary Codex's own conversation already acts as memory. Across **separate sessions** on the same project, hands-on testing found the opposite: Stateful was 20–54% cheaper after the first session, with comparable answers (issue #35; see "Hands-on product testing, issues #24-#42" below). The thesis holds across sessions and is currently inverted within a long thread.

**Setup.** The corpus was the Pramana chip repository snapshot at commit `9c16fb73dc`, placed in isolated copies with per-turn corpus hashes. Each arm ran in one continuous thread (baseline `01a0d326-52f0-70c0-b61b-32244cf8ab36`, stateful `01a0d32b-e40e-72c2-b7f1-a6f1a4a04808`). Both arms used the default model at high effort, read-only, with memories disabled and a dedicated `CODEX_HOME`.

The ten questions were the project's real open design decisions: the timing path, UART RX soundness, pinguard design, IMEM depth, bug hunting, certificate attack, the response-store SVA, the injection theorem, ABI integration, and strategy. They ran strictly sequentially and interleaved (q1 ordinary, q1 stateful, q2 …), alternating which arm went first. Artifacts are in `clients/stateful-codex/eval/results/pramana-ab-2026-09-24/`.

**Economics (cumulative per arm):**

| | Ordinary | Stateful |
|---|---:|---:|
| Input tokens | 12,139,665 | 24,157,501 (2.0×) |
| Uncached input | 502,801 | 1,380,157 (2.7×) |
| Output tokens (reasoning) | 103,118 (56,371) | 137,394 (81,513) |
| Summed turn wall time | 2,643 s | 3,591 s |
| Canonical compactions | 2 | 4 |
| Tool-output characters returned | 1.12 M | 2.56 M (2.3×) |

**Trajectory.** Ordinary per-question input fell from about 1.5 M to about 0.4 M by q9–q10, and its model requests per question fell from 26 to 2 on q9. It answered later questions from conversation memory. Stateful per-question input did not fall: it stayed at 1–5 M, with q5 at 5.2 M against 1.6 M for ordinary. Its requests stayed at 12–35 per question.

**Mechanism.** Both arms stayed in their single thread, so resume and continuation worked. The gap has three observable sources:

1. **Re-reading instead of reuse.** Stateful pulled 2.3× more tool output. It kept re-verifying source regions rather than relying on understanding it had already established in the thread or the blackboard.
2. **Repeated project injection.** Thirty `<stateful_project>` developer messages totalled about 443 K characters over the thread. They were re-sent on every turn and after every compaction.
3. **Durable state did not adequately replace conversational conclusions after compaction.** Each compacted replacement contained one fresh Stateful project packet, so the root itself was rehydrated rather than duplicated. However, the project state held only 30 sparse entries after ten deep questions, while completed-run results and final obligations were not projected into later model context. The remaining conclusions depended on the provider's opaque compaction output, and the subsequent re-reading behavior shows that the installed state plus summary did not provide adequate working continuity. The observed empty `retained_context.verified_answers` is not evidence for this claim: that host-owned field records verified replies to `request_user_input` for Guardian authorization, not general project conclusions, and was expected to be empty in these autonomous read-only turns.

**Quality (final: blind Droid grading on a read-only corpus copy, key unblinded afterwards).** Ordinary won 6 of 10 queries (q01, q02, q04, q06 and q10 with high confidence; q09 medium). Stateful won 4 (q05, q07 and q08 high; q03 medium). There were no ties. The summed 1–5 rubric scores were level: 208 ordinary, 210 stateful. Stateful scored higher on grounding (43 vs 39): its citations were exact where ordinary's line numbers were off by one. It also avoided overclaiming on q07 and q08.

A first grading pass on a writable copy was contaminated by a formatter hook. It was preserved separately and regraded; every query had the same winner in both passes.

**Cost (API-equivalent list-price estimate: $4/M uncached input, $0.40/M cached, $20/M output).**

| Arm | Estimate | Wall time |
|---|---:|---:|
| Ordinary | $9.02 | 46.0 min |
| Stateful | $18.16 | 61.6 min |

- From q01–05 to q06–10, ordinary's cost fell 48% ($5.93 → $3.09). Stateful's fell only 22% ($10.19 → $7.98), and its uncached input did not fall at all.
- The stateful/ordinary cost ratio therefore rose from 1.72× to 2.58× over the session.

**How state was actually used.** Reuse was passive. Earlier findings reached later turns only through the injected project summary, which grew from 2.9 KB to 22 KB. No explicit blackboard query returned an entry recorded in an earlier turn.

In q06, a finding that was present in the stateful arm's injected summary (a real reset-synchronizer mismatch between the formal UART RX harness and the RTL) went unused. The ordinary arm carried the same finding forward from its own q02 answer and built the stronger attack with it. A continuous ordinary thread obtained much of the reuse for free.

**Interpretation, bounded to this probe.** On a fast-changing repository with open-ended design questions, the state layer added cost and latency without a demonstrated quality gain. The root blackboard is not implicated as the primary cost; its injected size is small relative to the tool-output gap. This result must **not** be answered by thinning the rich root, per the product intent. The actionable problems are the trust and compaction ones:

- verified, provenance-bound findings are not trusted in place of re-reads;
- durable state plus the compacted replacement does not yet preserve enough prior conclusions to prevent rebuild reading.

SC-EVAL-024 had already exposed a compaction-cost pathology under an artificial limit; this probe shows a related one under default limits.

## Mechanism canaries and implementation checkpoints (2026-09-24 to 2026-09-27)

Status: transferred on 2026-10-07 from the retired restart handoff and clean-build plan (recoverable at
`46d9071051445583c00b21e0163d373372d1a966`). These are small mechanism gates and component checkpoints run after
SC-EVAL-032 on the published line (heads `19222f07f331`, then `89e79f57f1`). None is a broad benchmark or a cost
claim. Unless stated, the canaries used Luna, cached ChatGPT login with API-key variables removed, isolated project
copies and disabled host memories. Every named commit except the rejected prototype `a6cdb157de` is an ancestor of
`stateful/main`.

The product problem these gates serve is the coupled loop behind issues #15, #16 and #17: precise semantic capture,
continuity across turns, threads and compaction, active retrieval, exact routing for missing detail, point-of-use
freshness, and therefore less rereading. SC-EVAL-032 showed the loop was not working: the stateful/ordinary cost ratio
worsened from 1.72x in q01-q05 to 2.58x in q06-q10, q06 had a decisive finding in state but did not use it, and q10
lacked a decisive fact in state that the old file-only context map could not route to.

### Exact-region routing and the frozen q10 evidence

Commits `3fd48bdf16` (routes survive scan limits), `9a27507d90` (fingerprint-bound evidence routes), `e0c66eb5f7`
(an edit that shifts lines between query and read fails closed), `4242a7a9df` (regions split at 64 lines and 4 KiB;
a route never claims a range whose matched text was truncated; stable cell identity), `f9b3e37602` (file node, file
entry, regions and retirements publish in one SQLite transaction; a forced mid-publication failure leaves the previous
generation visible), `e90eaed41d` (FTS ranks a bounded candidate page before hierarchy validation; per-source and
top-level-directory caps; literal multi-token queries), `d5b35490c6` (app-server proof of exact region knowledge reuse)
and `f1e7002acd` (240-byte UTF-8-safe previews centred on the matching window). Validation: project-intelligence 37/37,
Stateful extension 24/24, the targeted app-server route/read/record/requery test, `just fix` and `just fmt`. The
rejected bare-line-range prototype stays only on `checkpoint/stateful-region-routing-unsafe-20260925` at `a6cdb157de`.

The two q10 `context_map_query` inputs were recovered read-only from the protected rollout:
`STRATEGY_MAP earliest missing link final artifact end-to-end proof certificate integration` and
`PLAN.md Production integration merge winning src main proof migration next action`. The frozen corpus had 1,537
indexed files and 25,096 regions; the source scan alone took 12.362 seconds; the SQLite database was 107,503,616 bytes;
full debug indexing varied from about 169 to 248 seconds under build and system contention (no indexing improvement is
claimed). Before the FTS pre-limit the two queries took about 50.107 and 27.998 seconds; after it, max-10 probes took
about 111.6 and 71.3 ms and later max-20 probes about 145-180 ms. Recall was mixed: query 1 did not put the decisive
passages in its top 10 (with max 20, `PLAN.md:116-168` ranked 15 and `SCORECARD.md:48-76` ranked 19; in raw FTS they
ranked 50 and 65), while query 2 ranked them 2 and 6. Promoting those two cells artificially would be
benchmark-specific overfitting. Conclusion: routing and latency improved and the real second query reaches both facts;
q10 is not "solved".

### Behavioural reuse canaries (2026-09-26)

Licensing fixture, two read-only turns in one thread, a project-state artifact after each turn.

- **Cued root reuse.** Turn 1 (controlling royalty as of 2026-03-15) made five exact evidence reads, concluded 6% of
  net sales effective 2026-03-01, and reached intelligence revision 45 with two current root entries and four evidence
  routes. Turn 2, told to start from verified project intelligence, queried state, made zero source reads, gave the same
  answer and left revision 45 unchanged. Responses 5 to 3, tool calls 4 to 2, source reads 5 to 0; input 104,781 to
  87,787; uncached input 27,981 to 7,659 (72.6%); output 2,283 to 753; model-turn time 52.8 to 20.8 seconds.
  Limitations: the prompt cued state-first behaviour, the fact was root-promoted, the source was unchanged and the
  fixture is small.
- **Uncued deeper-state reuse.** Turn 1 persisted one source-verified conclusion at `executed-amendment-1.md` file
  scope (the records audit is quarterly and binding under an executed amendment); the root stayed empty. Turn 2 asked
  the same question without mentioning memory, queried state, made zero source reads, answered correctly and left
  intelligence revision 43 and entry revision 1 unchanged. Responses 7 to 3, tool calls 6 to 2, source reads 1 to 0;
  input 135,455 to 75,342; uncached input 26,399 to 6,478 (75.5%); output 1,422 to 592; time 41.4 to 18.0 seconds.
  This closes the small root and deeper-state reuse gate only; it says nothing about compaction, changed sources, large
  corpora or an advantage over ordinary Codex.
- Two evaluator defects were found and fixed, not product failures: an evidence assertion broke on a phrase spanning a
  line break (`six\npercent`; whitespace normalised in `66f9831b31`), and the summariser carried only the revision-0
  packet into the next turn (deltas `0 -> 42 -> 43` are now folded in `4a5558fbda`). The companion rebuild could not
  download the prebuilt V8 archive; the existing companion executed every canary tool call.

### Compaction continuity canary (2026-09-26)

A five-file synthetic UART certificate corpus modelled on the Pramana reset-boundary failure, Luna at medium effort,
automatic compaction forced at a low token limit. The q02-style turn persisted five current source-verified root
findings at intelligence revision 35, including the decisive contrast (production resets both synchronizer stages low;
the formal model initialises both high and has no production reset branch), and compacted twice; the post-compaction
packet held all five findings and routes. The uncued q06-style prompt ("What would a skeptical certificate reviewer
attack first in RX-EXACT? ...") started at revision 35 with 16 hierarchy nodes, 10 context-map entries and all five
entries, made zero reads or queries, chose the reset-to-first-frame mismatch as the strongest attack, kept the caveat
that the sources alone do not prove an unconditional extra push, wrote no duplicate state, and compacted twice more.
Responses 8 to 4; tool calls 5 to 1; read-bearing calls 1 to 0; input 167,724 to 87,480; uncached input 43,564 to
17,848; output 11,207 to 4,837; runner time 218.5 to 97.4 seconds. Limits: five files, root-promoted findings,
unchanged source, a continuous thread, aggressive forced compaction and no ordinary arm. Two precursor attempts with the
same finding were rejected by an over-specific bag-of-words assertion (`different`, `unsound`, `do not agree`); the
final gate asserts one current entry co-locating production, formal, reset, low and high with the production lines
cited. Exact-synonym assertions are an eval fragility, not product evidence.

### Changed-authority canary (2026-09-26)

Base policy threshold 10 (executed), draft amendment threshold 6, assessment count 8. Turn 1 correctly said launch was
permitted (the draft was not binding), persisted four entries and ended at revision 25. Between turns only the
amendment changed (executed effective 2026-03-01, replacing the threshold with 6); the user did not announce it. Before
the first response of turn 2 the point-of-use audit advanced 25 to 26 and marked the old root decision and route
stale; the model read the changed source, concluded launch was not permitted (8 exceeds 6), superseded the old
decision and completed at revision 35. Safe but inefficient: turn 1 used 157,121 input, 40,129 uncached input, 7,664
output, 8 responses, 5 tool calls, 2 compactions and 161.0 seconds; turn 2 used 320,041 input, 52,265 uncached input,
14,495 output, 14 responses, 9 tool calls (2 blackboard queries, 2 context-map queries, 1 read-bearing call), 4
compactions and 288.6 seconds. One completion was rejected because supersession moved the old entry from revision 1 to
2 while the model still sent 1; it recovered on retry. Cleanup debt: the earlier draft-status entry stayed active with
a stale fingerprint, and the conditional base-authority fact stayed active because its bytes did not change. The
summariser first misreported turn 2 as starting at revision 25 (a pre-prompt compaction usage record was mistaken for
the first response); it now folds `world_state` root revisions and reports 25 at turn start, 26 at first response, 35
at completion.

The trajectory decomposition found two tool-contract costs (the first query isolated `amendment.md` as stale but gave
no copyable refresh locator, so the model reread all four sources; the obsolete revision 1 was resubmitted after
supersession). Fixes: `f2fbe0a390` adds `refreshInput` to stale hits; `74bc2e949b` returns a copyable
`historicalFinding` at the new revision after supersede or retire; `a78f394b8b` enumerates changed-source dependents
(file seed expands to current and retired region routes, transactional, revision-pinned pagination, invalid seeds
rejected). Suites: Stateful extension 24/24, project-intelligence 37/37, targeted app-server integrations, scoped
Clippy (the pre-existing `RootBlackboardStatus` large-enum warning remains). These give a six-call repair path in
place of the frozen nine-call path; that is a displacement theory, not a measured result. Independent Astra, Claude
Code and Droid reviews challenged the slice and agreed that semantic dependency provenance is a separate layer.

### Telemetry, snapshot and trust repairs

- **Evidence-read accounting (issue #18, closed):** `649a89e116` and `7f94840695` pair tool results with calls and
  count completed reads by source fingerprint and exact range, longitudinally. A read-only replay of the protected
  SC-EVAL-032 rollout found 178 completed reads: 143 new exact identities, 22 prior-turn repeats and 13 within-turn
  repeats, plus 2 failed and 7 unresolved attempts; 6 attempts had no concrete identity. Earlier syntactic reread counts
  must be regenerated before reuse. This repairs telemetry; it does not show any repeat was unnecessary. Client suite
  41/41.
- **Root projection snapshot (issue #20, closed):** `c4067d433b` builds the root projection in one read transaction (37/37).
  `c6266e5559` does the same for ordinary blackboard search and context-map reads (two WAL regressions; 40/40). Neither
  claims a mixed projection was ever observed.
- **Generic upsert:** `f0c9366f50` rejects a caller-chosen `sourceVerified` grade without a host receipt; `9d0b2b113e`
  proves it through public JSON-RPC.
- **User confirmation (issue #23):** `44bf6dcc58` removes `userConfirmed` from model schemas, `6aa45e7f92` adds
  `blackboard/confirm` (entry ID and expected revision only), `59ea293d75` adds the browser action (client 42/42; a
  real-browser check proved the payload, badge and action removal). The accepted limit is the trust boundary in
  `STATEFUL_CODEX_CURRENT_STATE.md`.
- **Exact evidence in the browser (issue #22):** `b47e85b470` reconstructs the guarded route in `evidence/read` and
  rejects a mismatched range.
- **Hashing (issue #19 stays open):** `fc671dedb9` reuses one live source check per audit when file and region routes
  share a source (26 extension tests).

### Premises, refresh and routing checkpoints

- **Revision-pinned semantic premises:** `b37d91d360`, `cae829c436`, `00e61132de`. An entry revision may name at most
  16 exact revisions of active `sourceVerified` or `userConfirmed` entries it relied on; premises never count as direct
  evidence or raise verification; freshness walks the premise graph, so a changed amendment makes an unchanged-source
  conclusion stale and discoverable. Model record and update tools audit premise evidence against live bytes; the
  generic API reports stored state and does not re-read sources. Suites: project-intelligence 38, extension 27,
  app-server-protocol 310 with one skip. Whether Luna uses the path is unmeasured.
- **Interrupted refresh:** `dc51e7b320`: an injected `PermissionDenied` read yields an incomplete inventory, no missing
  reconciliation, and the last complete generation stays queryable (41/41). This protects only within the
  scanner's error contract; it does not make a genuinely deleted file distinguishable from every host-specific
  filesystem anomaly.
- **Index cost profile:** with 496 regions, initial publication was about 277 ms at 16 files, 450 ms at 124 and 828 ms
  at 496; unchanged publication about 110, 155 and 332 ms; at 160 files and 4,960 regions, scan, initial and unchanged
  publication were about 1.26, 3.43 and 1.33 seconds. Roughly linear, so the 169-248 second Pramana refresh cannot be
  assigned to the per-region SQL loop; filesystem, OneDrive or antivirus effects, database growth, contention or older
  code may dominate. No indexer rewrite was made. `99fb9ce290` adds `regionsIndexed`, `scanDurationMs` and
  `publicationDurationMs` to refresh results (`contextMap/refresh` and the tool; project-intelligence 41/41, extension
  27/27, protocol 310/310 with one skip, two public app-server integrations). Issue #19 stays open until a real
  refresh records them.
- **Thread-view continuity:** `f2af68d17b` proves a completed outcome and its learning reach a second thread on the
  same project while a private transcript marker does not. Live canary, `gpt-5.6-sol` at high: thread
  `01a0dec1-0f8a-70b0-a470-97c02d9fdfee` read `DEPLOYMENT.md:L1-L3` once and recorded the conjunctive gate (checksum
  `C7-42` and a passing rollback rehearsal are both required); thread `01a0dec2-29d8-7382-bd14-d2c7a69740a9` reused it
  with no evidence-read, context-map, shell or code-mode filesystem call (its four tool calls were blackboard and run
  state). Turn 1 against turn 2: 149,106 against 130,142 total tokens; 28,786 against 15,070 uncached input plus output
  (47.64% lower); six against five requests; 6,630 against 4,988 bytes of tool output; about 34.3 and 34.8 seconds.
  The second prompt explicitly asked for a continuity finding, which caused a record/query/promote sequence. Control
  thread `01a0dec6-c7cc-7603-86d5-61988c48dbdf` without that instruction used one `stateful_run_update` call, two
  requests, 50,457 total tokens, 11,417 uncached input plus output, 2,554 bytes and 17.1 seconds: 66.16% and 60.34%
  below the cold turn. Limits: a tiny fixture; the first refresh indexed four diagnostic stdout and stderr files written
  inside the project; `gpt-6-luna` and `gpt-6-sol` were unavailable through the ChatGPT endpoint; the CLI reported
  version `0.0.0` and predated the newest commits because a rebuild was blocked before linking when the `v8` 150.4.0 archive download and
  the local Python fallback both failed.
- **Route diversity (issue #17):** `fb3b60a55a`. With 160 matching files under `reviews/`, the decisive 161st match
  under `docs/` was dropped by the old `limit * 16` early stop; a bounded FTS window plus one exhaustion-probe row now
  returns `docs/guide.md` and an exact `truncated` flag (`mayHaveMore` in the tool, exposed by `contextMap/query`)
  instead of guessing from `data.len() == limit`. The exhausted ten-result request reports `truncated: false`; a
  three-result request reports `truncated: true` (project-intelligence 41/41, extension 27/27, protocol 310/310 with
  one skip, two affected app-server integrations). Ranking quality and cost are not established.
- **File-to-region knowledge:** `b85c1dbe64` counts active findings across a file and its direct regions with
  `COUNT(DISTINCT entry.id)`, so a file-cited finding shows on a child region without a second read (42/42).
- **Unchecked audit transitions:** `5dc0fd7b14` stops normalising `uncheckedThisTurn` to `current` in the semantic
  fingerprint, so a user-confirmed finding whose live audit becomes unchecked is invalidated in the next request
  (extension 27/27).
- **Refresh health:** `f177cce404`, `197c48ba8c`, `8884f86384` persist full-refresh health (inventory versus region
  coverage, counts, skipped and missing paths, truncation, timings) through restart, expose it in v2, World State and
  the browser, and retry incomplete projects at startup (PI 42, extension 28, protocol 310 with one skip, browser 44, four affected
  app-server integrations). `3578c5e5a0` fences competing full refreshes by a durable generation checked inside each
  write transaction (43/43). Targeted single-file refresh stays outside that generation policy with only its per-file
  atomic transaction; broader live-edit coordination remains open under issue #16.
- **Completion learning:** `dc2dd91aeb` rejects completion with non-empty `finalObligation.learning` unless a current
  root alias or exact historical revision is selected; the rejected run stays running and no final obligation is
  written (extension 28/28; both completion-guard integrations, the Autonomous continuation integration and the
  unselected-provenance integration passed). The host cannot check semantic equivalence.

### Attribution, trajectory and measurement (2026-09-26 to 2026-09-27)

- **Attribution (issue #24):** `1f55cc0d62` adds a content-free per-turn Stateful contribution record, the
  `statefulAttribution/completed` notification and an invocation-level `stateful_attribution` object in `codex exec
  --json` (extension 29, protocol 310 with one skip). The real app-server steering trajectory reconciled four Stateful
  calls (one obligation write, two steering writes, one run update) and one selected material finding; a headless
  regression merged two turn summaries into one terminal aggregate. `duration_ms` is tracked turn time, summed across
  autonomous continuations in the headless aggregate. First live reconciliation: a collaborative Luna run on the
  licensing fixture took 34.801 seconds with 134,782 input, 97,536 cached and 1,352 output tokens; one completed turn,
  five World State samples, 65 root entries across samples, ten current evidence routes, ten physical sources, 4,837
  source bytes hashed, three successful Stateful calls (one evidence read, one obligation write, one run update) and two
  selected material findings. The raw rollout independently contained exactly three code-mode calls, to `evidence_read`,
  `obligation_update` and `stateful_run_update`, with aliases `E4` and `E7` selected at completion; the answer applied the 6%
  royalty effective 2026-03-01. It also showed two duplicate ignored-config warnings. Counter definitions: evidence-route
  counts are blackboard provenance routes, while `root_unique_sources_observed` counts physical files deduplicated during
  a recomputed audit; root entries are counted per World State sample, not as unique knowledge; material reuse is
  credited only when exact current aliases or historical revisions are selected and the terminal completion succeeds
  (attempted or rejected selections do not count).
- **Headless trajectory:** `a3f27fb130`, `1cdf984ef8`, `a288e3eb54` add duration, completed model responses (failed
  provider-request attempts are not observable to the client and are not counted), compactions, tool calls by kind and
  output bytes, with `turn.progress` and `stateful.attribution` JSONL snapshots (`codex-exec` 122/122).
- **Correction, 2026-09-27: the first live trajectory claim was invalid.** The licensing run recorded after
  `19222f07f3` had correct attribution and answer but zero responses, tool calls, output bytes and compactions: the
  counters were fed only by raw response-item notifications that ordinary headless clients do not receive, and the
  compaction event was consumed before the accumulator. The claim that issue #24 was partly closed is withdrawn. The
  repair moves accounting behind the app-server boundary, publishes `turn/trajectory/updated` (the terminal notification is guaranteed; intermediate updates may coalesce or
  drop under backpressure), records failed and interrupted invocations as a status (still not a count of failed
  provider requests), and reports resume usage relative to the current invocation; the TypeScript Jest
  run stayed blocked by a Windows `file://C:\\...` module-resolution failure.
- **Migration and backfill (issues #29, #31; #30 to recheck):** LF/CRLF-equivalent checksums, background insert-only
  backfill with filesystem fallback, owner-token fencing, and a one-time legacy reset in the migration then numbered
  `0056` (now `0059`). Astra and Sol reviews found the overwrite, skip-on-failure, lease-fencing and initialisation
  races; Claude Code and Droid reviews could not run (HTTPS forced through an unavailable `127.0.0.1:9` proxy) and
  are not claimed. `codex-state` plus `codex-rollout` 331/331.
- **Durable turn measurements:** records keyed by run, project, thread and turn, merged in either arrival order, with
  graceful-shutdown draining; `statefulMeasurement/list` and `statefulMeasurement/summary` (runtime 5/5, including exact aggregation over a
  truncated newest-two-record window; four schema fixture checks; a public app-server test read one record back
  through list and an exact one-record summary). Astra and Sol found the binding-loss, arrival-race, failed-status, notification-loss,
  destructive-replay, mutable-ordering and shutdown-cancellation defects that drove the redesign. A combined broad
  app-server run stalled at 45 percent and is not claimed.
- **Startup warnings:** an exec-only one-for-one deduper removes the duplicate ignored-config warning.
- **Measured-work panel:** the browser shows the bounded summary; a later review found the refresh race and the
  false-zero trajectory rendering, both fixed (the initial panel passed four workspace-view assertions in-process; after the fixes refresh-policy 2/2,
  workspace-view 5/5). Node's test worker could not
  spawn in that sandbox (`EPERM`); the assertions passed in-process.

Pause validation on 2026-09-27: refresh-policy 2/2, workspace-view 5/5, `codex-stateful-runtime` 5/5, `codex-state`
plus `codex-rollout` 331/331, `codex-app-server-protocol` 310/310 with one skip, `codex-exec` 127/127, three focused
app-server integrations, `just fmt`. That work was committed as `89e79f57f1` once `.git` became writable. It widened
issue #38: the TUI did not compile because exhaustive `ServerNotification` matches lacked `StatefulAttributionCompleted`
(added in `1f55cc0`) and `TurnTrajectoryUpdated`; the last commit whose TUI built was `926f2ac`. Both variants are
handled in `stateful/main`; no TUI build was rerun for this record.

Operational notes from the same period: on 2026-09-26 a long-running Codex process returned `401 Unauthorized:
Incorrect API key provided` because it predated a fresh `codex login`; no API key or override existed at any scope; a
newly started `codex-cli 0.157.1` reported `Logged in using ChatGPT` and a read-only `codex exec --json` request
returned `AUTH_OK`. A Windows scheduled task (`StatefulCodex-Hourly-Droid-Review`, script
`C:\Users\devan\.codex\automations\stateful-droid-hourly\run-review.ps1`) ran a four-stage Droid and Codex review of
a detached worktree of the committed head, never overlapping and bounded to three hours, with Codex progress events
kept separate from the final critique so raw event streams were not fed back as peer analysis; its first full cycle (`%LOCALAPPDATA%\StatefulCodex\hourly-droid-reviews\20260926-110328`)
drove the confirmation threat-model audit and the hashing fix, after which Factory returned HTTP 402 (usage windows
exhausted, Droid `0.228.0`).

Still open at that pause, and not since recorded as closed: a normal-home live smoke of startup recovery, non-zero
trajectory counters, exact turn-local usage and agreement between the raw rollout and both measurement methods; real
index-cost attribution (issue #19); authoritative monetary cost (the client sees no reliable unit or currency).

## Hands-on product testing, issues #24-#42 (2026-09-25 to 2026-09-27)

Status: transferred on 2026-10-07 from the retired product-testing handoff. Hands-on use of the CLI, headless and web
surfaces; A/B arms ran `-m gpt-5.6-luna -c model_reasoning_effort="high"`. Tokens are the CLI's "tokens used",
uncached input plus output, which excludes Stateful's large cached input (cached input costs about 10% of uncached).
Raw notes, scripts and logs are outside the repository: `C:/Users/devan/sc_v2/` (campaign scripts `campaign*.sh` and
`long*_run.sh`, timelines, answers, analysis scripts such as `cite_check.py`, `template_check.py`, `we_len.py`,
`topic_tokens.py`, `build_packet.py` and `judge_pairs.sh`, and `sc_v2/review/v3_sc_notes.md`) and the PTY TUI driver
`C:/Users/devan/sc_tui/drive.py`. Test builds then were the headless `codex-exec.exe` at `19222f0` and the CLI/TUI
plus `codex-code-mode-host.exe` at `926f2ac`, run against an isolated store passed with `-c sqlite_home=...`.

Cross-session series (issue #35): a fresh headless session per question on the same project; the ordinary arm starts
cold each time.

| Series | Stateful versus ordinary |
| --- | --- |
| Datadog analyst, 15 questions | -5% overall; -10% on Q6-15; Q15 (4-filing synthesis) -49% with identical numbers |
| ServiceNow analyst, 12 questions | -30% tokens, but +102% wall time from SQLite pool contention under concurrency (#37) |
| Python coding, 4 tasks (tests pass in both arms) | task 1 +37% (first contact); tasks 2-4 -33% tokens and about -35% time |
| JavaScript site, 4 tasks | -27% tokens, -27% time; no first-contact penalty |
| Research repos, 3 projects x 6 questions | -35% (series A), -54% (series B, more accumulated state); cheaper in 29 of 30 pairs; answers sometimes shorter |
| Hardware RTL repo, 5 questions | -15% tokens, faster on all 5 |
| First broad A/B, 4 projects (#32) | +5% overall, dominated by first-contact cost and thin learning capture |

Reading: project memory paid off across sessions (20-54% fewer tokens after the first session, comparable answers);
within one continuous thread (SC-EVAL-032: 2.0x input, 2.7x uncached, $18.16 against $9.02, blind quality 6-4 to
ordinary, about 443k characters of re-injected packets) it was overhead. Each is n=1 per project.

Long run v3 (2026-09-27, 150 independent topics) is **not memory evidence**: Stateful made about 3 memory-tool calls in
844. It is kept for two process observations: the obligation was updated once at the start and then not for 31 minutes
and 34 topics until compaction 1, then every 4-7 minutes; and right after compaction 1 re-injected the agent's own
starting strategy (ignored for topics 21-39), behaviour switched to match it. The rolling obligation window also drops
early learnings.

Issues filed on `dl1683/stateful-codex`: #24 headless opacity (progress, cost, contribution record); #25 evolve beyond
Codex limits (logins, providers, computer use, transparency); #26 version skew (an older binary silently loses memory);
#27 thread crossing surfaces (partly resolved: the Continue picker lists TUI threads); #28 start-screen clutter and
unstable re-rendering; #29-#31 migration line endings, stock-Codex migration skew and slow first-run backfill; #32 first
A/B across 4 projects (+5% tokens, thin capture, a cited-but-wrong number); #33 and #36 headless transparency and UX
(tool actions not streamed, counters read 0, PDFs opaque to evidence); #34 headless resume unsupported; #37 concurrent
sessions starve on the SQLite pool; #35 the cross-session evidence above; #38 the interactive CLI did not compile from
`1f55cc0`; #39 TUI and web report (prompt required at launch, placeholder branding, hierarchy freeze, excellent mid-run
steering and obligation panel; every workspace page ended in a busy render loop of about 1.15 cores and 900 MB per tab
even on a 220-file project, and DevTools evaluation over 45 s failed); #40 staleness detection works but line citations
were off by 2-25 lines in 4 of 4 checked cases, where base Codex's were exact; #41 Socratic mode's first reply is a
refusal and "resume implementation" is impossible headless; #42 a long single run took a scripted, templated shortcut
(blinded judge preferred base 19/20). Fork issue #1: background `codex exec` hangs without closed stdin.

Test-design lesson: Devansh's core question is whether nuanced facts and instructions given mid-session survive and are
used many compactions later. Backlogs of independent units cannot answer it. The suggested test (not run then): one
long session on cumulative work, nuanced facts injected at known times, several forced compactions, then check whether
later decisions honour them and where they were written, measuring obligation cadence throughout.

## External benchmark program (from 2026-09-28)

Status: program defined on 2026-09-28; the portfolio is being planned with an independent reviewer, and results are
recorded below as they land. Planning records live outside the repository in the dogfood campaign notes.

### Rules

- **Stateful Codex only.** Ordinary Codex results already exist publicly, so no Stateful-off arm is run. Every
  comparison against a published score is descriptive and names the different build; it is never presented as a
  matched control or a causal effect of Stateful.
- **Agentic benchmarks with a real comparator.** A benchmark qualifies only if it has an official harness able to run
  a custom agent, or a published evaluation of Codex itself.
- **Partial and unofficial runs are allowed.** A runnable subset is reported with its exact task list, labelled
  unofficial or partial, and compared against published results on the same tasks where available. Vetted results are
  offered to the benchmark maintainers.
- **Judging.** Where an official LLM judge requires an API credential that is not used here, the judge is
  `gpt-5.6-luna` via ChatGPT login, in a fresh isolated session per judging unit, with the official grader prompt and
  schema unchanged. Such scores are labelled "Luna judge, not the official judge; unofficial proxy score". A judge is
  never rerun because of an unfavourable result.
- **Publish everything.** For every run, all per-task outputs (answers, patches, notebooks), full recorded
  trajectories, tool logs, and every judge input and output are published so anyone can re-grade with the official
  judge. Only authentication material is redacted. No post-result filtering.
- **No benchmark-specific tuning.** Stateful Codex contains no benchmark-specific branches, prompts, answer tables or
  cue rules; harness changes are limited to integrity boundaries (pinned environments, network isolation) that are
  disclosed.
- **Memory is the headline track.** Priority goes to benchmarks where durable, cross-session, evidence-backed memory
  can show a difference.

### Results so far

- **Terminal-Bench 2.1 regression check** (one attempt x 89 tasks, maximum effort, bundle at the Phase A freeze
  candidate): 68/89 (76.40%), against the earlier Stateful breadth screen of 70/89 (78.65%) and the published ordinary
  Codex result of 75.73% (different build). The two-task difference is inside single-attempt noise; session logs show
  no Stateful-attributable failure in the nine tasks that flipped from pass to fail.
- **BixBench gate re-grade** (retrospective; debug-base environment; Luna judge through the unmodified official
  grader): smoke 1/1, single-capsule five 5/5, ten-capsule breadth 3/10, repair re-runs 1/2. Unofficial proxy scores.

- **Memory-benchmark track status**: DolphinBench and the other external memory benchmarks are queued behind the
  read-only project-knowledge gate (SC-PKRO-001), whose first stage is implemented and under repair. The matched
  internal study below (SC-EVAL-033) was run first to understand behaviour before external scoring.

### Planned portfolio (agreed 2026-09-28)

The memory track leads. Its benchmarks were chosen because they test knowledge that must survive separate sessions,
carry provenance, be revised or superseded, and cover the agent's own prior actions and failures:

1. **Read-only project knowledge (product gate, SC-PKRO-001).** Memory benchmarks require completed memory to be
   frozen at test time; a prompt instruction is not enough. Stateful Codex gains a general, process-wide mode in which
   project knowledge can be read but not changed (useful for reviewers, CI replay and shared knowledge snapshots). No
   memory benchmark is scored before it passes its acceptance tests.
2. **DolphinBench** (mem0ai/dolphinbench): about 500K tokens of dated history per persona, delivered as one fresh
   conversation per history message (3,400 / 5,011 / 5,128 conversations), then 200 action-graded tests per persona,
   each in a fresh conversation against frozen memory. Run at the published reasoning setting (high). First a
   100-message calibration, then one full persona, then all three. Published reference rows (Hermes harness, same
   model): built-in memory 394/600, Mem0 424/600. Every comparison is labelled a cross-harness descriptive comparison:
   the published systems differ in both agent harness and memory implementation, so no causal memory effect is
   claimed from it. No raw copy of the history is placed in the project; the agent keeps only what its own memory
   recorded.
3. **AgentMemoryBench** pilot (continual learning, replay, transfer and repair after wrong feedback).
4. **SC-MEM-REUSE-001, the causal study.** Twelve public task chains, each target run in four Stateful conditions:
   cold project; placebo history (matched volume of unrelated prior work); relevant persistent history (prior work,
   verifier failure and correction in separate sessions); explicit replay (fresh project given the exact prior
   transcripts). Relevant versus placebo isolates useful memory from "more state"; relevant versus replay measures
   whether persistent memory keeps quality while cutting input. This, not any leaderboard delta, is the experiment
   that can substantiate a memory advantage.
5. Then MINTEval (revision and interference), MemoryAgentBench (conflict resolution slice), and AMA-Bench as a
   regression suite.

Horizon (orinlabs) is deferred: its no-network task policy needs an nftables feature
(`CONFIG_NFT_FIB_INET`) that the local WSL2 kernel lacks. It resumes on a dedicated Linux VM, not by weakening the
task policy. After the memory track: BixBench v1.5 on an unmodified pinned environment, DeepResearch Bench II and I,
SWE-Marathon's locally runnable tasks, and SWE-bench Verified and Multilingual pilots.

Every memory run uses one isolated project and store per persona or episode, never resumes a conversation, freezes
and hashes state before evaluation, gives each test a fresh copy of that state and a fresh runtime database, and
publishes the world state shown to every session, per-response token usage, trajectories and integrity hashes.

## SC-EVAL-033: matched behaviour study at the Phase A freeze (Campaign II, 2026-09-28/29)

Status: internal study, not a public benchmark. Every comparison uses the same freeze binary with and without
`--stateful`, the same model and reasoning setting (Luna, high), isolated project copies and state stores, host memories
disabled, and preregistered hidden checks written before the arms ran. LLM-judged scores use a fresh blinded Luna
session per memo with shuffled labels and two independent passes. Single runs of the same arm can move by 5-10 rubric
points, so only results replicated three times are stated as findings; the rest are labelled.

Arms: *ordinary continuous* (one thread, `exec resume`), *Stateful continuous* (one thread), *Stateful fresh-thread*
(a new thread every session, continuity only from project state — the intended workflow), and where noted *ordinary
fresh-thread + NOTES.md* (the agent is told every session to keep and read a notes file).

### Results replicated three times

| Study | Ordinary continuous | Stateful continuous | Stateful fresh-thread | Ordinary fresh + NOTES.md |
|---|---:|---:|---:|---:|
| M&A data room, 5 sessions, 14 text documents, 48-criterion blinded rubric (mean of 6 scores) | 13.5 | 14.3 | **18.2** | 16.8 |
| Uncached + output tokens per run (range) | 236-275k | 428-444k | 402-465k | 435-624k |
| Trivial asks after one state-building question (10 asks; answers correct) | 10/10 | 10/10 | — | — |
| Model responses / uncached tokens on those asks | 1x / 1x | ~2.2x / ~2x | — | — |

- On the data room the fresh-thread Stateful workflow was consistently best (range 16-22) and far less variable than an
  ordinary continuous thread (10-17). When the ordinary thread lost, it lost early decisive, cross-document findings to
  compaction. One continuous Stateful thread was not reliably better than ordinary Codex. Against a disciplined
  hand-kept notes file the edge is small (+1.4, within noise) but Stateful was ~20% cheaper and needed no per-session
  process prompting.
- Every trivial ask paid a fixed Stateful cost (an extra model round trip and the completion ritual) without consulting
  memory.

### Results replicated twice or once (labelled)

- **Code study** (5 sessions on `pallets/click`, final design review graded against a 32-fact rubric written before any
  run; two runs): ordinary 18/16 then 9/7, Stateful continuous 12/12 then 7/8, Stateful fresh 8/9 then 13/12 — the
  ranking flipped between runs: no reliable difference; ordinary Codex was cheaper in both. A post-hoc late
  "re-verify and rewrite" session made the ordinary thread compact twice and fall to 4/3 while Stateful held.
- **Six-session tool build** with a standing rule and a reversal (three variants incl. forced compaction): all arms
  passed every hidden check; Stateful cost 10-34% more uncached tokens.
- **Lifetime cost of re-asked facts** (12 fresh sessions, 6 facts asked then re-asked): on text files Stateful was
  35-40% more expensive in both halves (ordinary Codex answers with three cheap targeted reads); on the original
  Office files Stateful answered repeats from memory with no reads and was 52% cheaper on repeats (break-even after
  about two re-asks per fact) while ordinary Codex re-extracted the files each time.
- **Standing user instructions** given once: carried into the next two fresh sessions in 4/4 runs, but stored as a
  durable instruction entry in only 1 of 4 runs; they otherwise survive through the bounded recent-results window,
  which dropped a style rule three sessions later in one data-room run.
- **Integrity**: two projects sharing one store showed zero leakage; a planted vendor falsehood was stored as an
  attributed claim with a `contradicts` relation, an injected AI-directed instruction was not stored, and a user
  correction was dated and linked (but stored with agent provenance).
- **Crash recovery**: a run killed at 150 s had persisted nothing (knowledge is written near the end), the killed run
  stayed `running`, and ordinary `resume` recovered more.

### Surface findings (same freeze)

- **Web UI** delivers the charter's transparency and steering: live obligation packets, durable steering applied in
  ~30 s, a durable completion basis with evidence fingerprints, exact evidence, and user confirmation that later
  headless sessions honour. Gaps: Begin execution and Resume change status without starting a turn; thread listing
  fails during a state-database backfill and the error is hidden; the hierarchy is flattened by a CSP-blocked inline
  style; approvals appear at the bottom of the page without a reason; no busy state on Open workspace.
- **TUI** shows almost none of the Stateful layer: obligation updates are written but never rendered, only the first
  message of a session becomes a Stateful run, steering is not recorded durably, a Socratic run cannot be moved to
  execution, `--stateful` requires a prompt at launch, and `resume --stateful` is unusable.
- **Headless**: no per-response timeout (one run hung for over an hour); an isolated `CODEX_HOME` without the Windows
  sandbox state silently degrades `workspace-write` to read-only or hangs.

### What this establishes

Project state measurably improves multi-session, multi-document synthesis when work is split across fresh threads, and
it reduces cost where re-reading is expensive. It does not yet improve short or single-thread work, it costs more on
cheap lookups, standing instructions are not captured deterministically, and the TUI does not expose it. Detailed
findings, repro paths and the ranked improvement backlog are kept in the campaign notes outside the repository.

## SC-EVAL-034: 25-question model eval, GPT-6.1 Sol, Stateful vs ordinary (2026-09-30)

**Setup.** 25 questions in five batches, run in order in one working directory per arm, same machine, same day: b1 and b2 objective mathematics; b3 grounding in chip repositories; b4 open insight; b5 deep reasoning. Ordinary arm: codex-cli 0.159.2. Stateful arm: the fork at 4ea34e7d10 with the workspace version stamped to 0.159.2 (debug build with the code-mode host), one project store across all five batches. Costs are list-price equivalents computed from session usage records at $2 per 1M uncached input, $0.10 per 1M cached input and $10 per 1M output (ChatGPT login; nothing was billed).

| Batch | Stateful cost | Ordinary cost | Δ cost | Stateful time | Ordinary time | Input tokens (S / O) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| b1 objective | $0.085 | $0.045 | +89% | 127 s | 119 s | 107k / 62k |
| b2 objective | $0.055 | $0.036 | +55% | 129 s | 80 s | 109k / 61k |
| b3 repository grounding | $0.363 | $0.482 | −25% | 447 s | 435 s | 1.09M / 1.45M |
| b4 open insight | $0.384 | $0.492 | −22% | 553 s | 670 s | 1.04M / 1.29M |
| b5 deep reasoning | $0.535 | $0.586 | −9% | 1,086 s | 1,365 s | 1.44M / 1.10M |
| **Total** | **$1.422** | **$1.641** | **−13.4%** | **2,342 s** | **2,669 s** | |

**Result.** Same objective score (10/10). Stateful paid its setup overhead on the small one-shot batches and saved once batches worked in the same repositories. On the hardest repository-derivation question it gave the most complete answer of the GPT arms (43-clock abort recovery, with 39 as the overlap minimum). This is the first model on this eval where the Stateful arm was cheaper than ordinary Codex: GPT-5.6 Sol $5.78 vs $5.59 (+3%) and GPT-6 Astra $8.81 vs $7.60 (+16%).

**Limits.** One run per arm, so no variance estimate; only b1 and b2 are objectively scored; a single store accumulated across batches (the intended use, but it also means later batches are not independent). Raw data: `model_eval/sc61_compare.json`, `run_p_61sol/`, `run_s_61sol/` (local). An earlier Stateful 6.1 run on an incomplete binary (missing code-mode host) was invalid and excluded.

### Same-day negative findings (Campaign II continued, 2026-09-30)

- **Stale memory served as current.** After a collaborator committed a change to a project's key status (git status clean), a fresh thread was asked "quick one: current status and next action?". Correct first answers: ordinary Codex 7.5/10, Stateful 0/10 across two experiments and five real projects. Stateful answered from stored findings without opening a file. When challenged, it recovered 4/4 when the changed file was one it had findings about, but only 1/6 when the change was elsewhere (it re-verified only its own sources).
- **Compaction loses what the agent told the user.** In one continuous thread with forced compaction, "what were the three numbers you gave me earlier?" was answered with the same numbers by ordinary Codex 3/4 and by Stateful 0/4 (3 different, 1 partial), with no uncertainty acknowledged in any thread. Stateful compacted more often (11 vs 6) because of its larger fixed prefix.
- **Standing instructions are saved but not reliably applied.** "Remember for every future session" was honoured in a later fresh thread 2/4 by each arm. All 4 Stateful stores did capture it as a user-priority instruction entry, so the gap is in applying stored instructions, not capturing them. Ordinary Codex persisted it by writing it into the project's own handoff files. (An earlier version of this line said 0/4 stores contained it; that was a measurement error, corrected here.)
- **Web UI:** live responses from concurrent sessions on one server leak into each other's pages; "Open workspace" ignores the first click; continuing without a desired outcome silently does nothing.

## SC-EVAL-035: October 2–3 hands-on campaign (Campaign II continued, 2026-10-02/03)

Status: internal hands-on study, not a public benchmark. Claude used the product as a person would (CLI, TUI and web UI)
on real repositories and public corpora. Every comparison runs the same binary with and without `--stateful`
(`collaborative` unless stated), with isolated homes and project copies, model gpt-5.6-luna at high reasoning unless
stated, and auto-compaction at 80k tokens unless stated. **Every test is n=1 per arm.** One identical first-session prompt cost between 0.59M and 1.35M input tokens
across arms and reruns, and the ordinary arm's totals moved 6-15% between identical reruns, so ratios within about
±0.15 are noise. Builds, in order: `cont_0e160ee0f6` (first continuity release), `cont_9a3c2109d8` (lean memory protocol),
`cont_27aa1988f8` (standing rules captured verbatim and placed first; receipts; cheap path), `cont_c466898af7`
(integrated candidate: supersession, `memory_read`, stderr receipts) and `cont_e9ee21021e` (patch).

### Across fresh threads: the 20-session horizon series (humanize, 20 new-thread sessions per arm)

| Build | Cumulative input S/O | Uncached S/O | Excluding ordinary re-asks | Compactions S/O | `Next:` rule S/O | Re-asks S/O |
|---|---:|---:|---:|---:|---:|---:|
| `cont_9a3c2109d8` (horizon1) | 0.89x (16.16M / 18.25M) | 0.80x | 0.97x | 12 / 13 | 7/20 / 1/20 | 0 / 2 |
| `cont_27aa1988f8` (horizon2) | 0.73x (13.53M / 18.62M) | 0.57x | 0.83x | 3 / 15 | 20/20 / 1/20 | 0 / 2 |
| `cont_c466898af7` (horizon3) | 0.82x (13.15M / 15.95M) | 0.68x | 0.89x | 2 / 14 | 20/20 / 1/20 | 0 / 2 |

- On horizon1 Stateful's per-session cost rose (ratio slope +0.045 per session, SE 0.021; the last six sessions cost
  1.07x) because standing rules decayed out of the packet after session 5. Capturing rules verbatim and first fixed this:
  0 full-suite runs in 20 sessions on horizon2 and horizon3, against full suites in 16/20 ordinary sessions; the ratio
  slope became flat (+0.008, SE 0.030; then -0.005, SE 0.018).
- The ordinary arm needed two re-asks in every horizon because it could not recall a brainstorm (session 4) or a review
  (session 10) that existed only in conversation. On horizon3 the crossover gate (ahead by session 4 and through
  session 20) passes as measured but fails when those re-asks are excluded (input crosses only at session 10).
  Priced at assumed GPT-5-family list ratios ($1.25/M uncached, $0.125/M cached, $10/M output; Luna pricing
  unconfirmed), horizon3 cost $4.71 vs $6.00 (0.78x).
- On gpt-6.1-sol (sessions 1-8, `cont_e9ee21021e`), Stateful was 0.94x input and 0.86x uncached, but 1.10x / 0.92x
  without the ordinary arm's one re-ask. The ordinary 6.1 arm wrote its own `AGENTS.md` with the rules and kept all of
  them, so rule persistence is not a differentiator there; conversation-only recall still is.

### Across fresh threads: other use cases (all n=1)

| Test | Build | Input / uncached S/O | What memory changed |
|---|---|---:|---|
| click 7-day week (hand4 / hand7 rerun) | `0e160ee0f6` / `9a3c2109d8` | 1.01x / 0.85x, then 1.03x / 0.94x | `Next:` 12/12 vs 2/13; ordinary re-explain turn needed on day 2; weekly summary 556k vs 244k from 23 serial history reads |
| re-asking a known "main issue" (refind1) | `9a3c2109d8` | 0.36x / 0.25x on the 4 repeats | same answer every time vs 4 different "main issues" in 5 asks |
| FOMC analyst week (know1) | `9a3c2109d8` | 0.91x / 0.84x (1.30x input on sessions 2-5) | prior conclusions recalled 3/3 vs 0/3; stale conclusions corrected 4/4 vs 0 |
| license review (legal1) | `9a3c2109d8` | 0.87x / 0.95x (1.52x input on sessions 2-5) | current on a swapped license vs stale; neither caught the decisive "uses" -> "contains" change |
| multi-day debugging (debug1) | `9a3c2109d8` | 1.25x / 0.82x | ruled-out list kept vs a false "ruled out"; root-cause fix vs workaround; regression tests vacuous in both arms |
| patent drafting (patent1) | `9a3c2109d8` | 1.14x / 0.96x | drafting rules 3/3 vs 0/3, but the ordinary arm's final claims were stronger |
| codebase tutor (learn1 / learn2 replay) | `9a3c2109d8` / `27aa1988f8` | 1.25x / 1.23x, then 1.21x / 1.14x | day-5 loss of the learner's preferences fixed by verbatim rule capture |
| data analysis with a revision (data1) | `27aa1988f8` | 0.85x / 1.03x | findings and preferences carried; Stateful made 3 framing errors, the ordinary arm none |
| long-form essay (write1) | `e9ee21021e` | 0.93x / 0.80x | kept British spelling and did not adopt a co-founder's US preference; the ordinary arm switched and misattributed it |
| literature synthesis, 25 arXiv papers (research1) | `e9ee21021e` | 1.21x / 1.16x | all three user decisions recalled with reasons vs ignored |

### Within one run: no benefit yet

| Test | Build | Cost S/O | Quality |
|---|---|---|---|
| long1: one 17-minute click task | `9a3c2109d8` | 1.36x input (3.71M / 2.72M) | blind Codex review: ordinary better, high confidence (probes 71/77 vs 73/77) |
| long2: 7-hour research goal | `9a3c2109d8` | 1.12x input, 1.23x uncached, 10 / 10 compactions | blind Droid and Codex reviews: ordinary better (moderate; 65%) |
| within1: 12-turn single thread, 50k compaction limit | `e9ee21021e` | 1.27x priced units, 1.43x input, 23 / 18 compactions | blind Codex review: Stateful better (~80%); never-restated facts recalled 2/3 vs 0/3; both lost the "next step", Stateful lost a learned test route after compaction |

A measured forensic pass over these rollouts found the within-run excess is extra requests, not larger context: in
long2, 66% of the excess was per-turn memory writes and 28% the fixed prefix; after compaction only 4 of 155 packet
items were used before being re-derived, while about 90% of re-read files were the agent's own working set, which
memory does not hold. within1 has a confound: `exec resume` drops the sandbox flag, so resume turns ran under
workspace-write in both arms.

### Surfaces

- TUI on `cont_e9ee21021e`: memory is visible for the first time (receipts while working; `/memory` list, correct and
  forget in about 1 s with no model turn, persisted across launches). One correction restored a dropped rule and
  `Next:` then held 14/14 turns over 3 launches.
- Web on the integrated candidate: a Project memory panel with one-click Forget works end to end; the workspace is still
  a 17-section page.

### Limits and open defects

- Single runs throughout; the horizon gate result depends on the ordinary arm's re-asks; blind reviews are single
  reviewers per test except long2.
- Open: a two-rule message lost its second rule behind a truncated receipt; scoped "ground rules for this X"
  were captured in one test and missed in two on the same build; background about the user is not captured, which
  leads to invented preferences; decisions are forced into rule form and lose their verb; memory reads silently drop
  ruled-out entries at size caps; `§` and dashes shown as U+FFFD (disputed: a later store check found correct UTF-8 bytes and traced two cases to a cp1252 console and PowerShell decoding; garbling reported in the injected context is unverified); "source is not indexed" errors for files that
  exist; within-run cost and compactions remain above ordinary Codex.

Per-test notes, tables, transcripts and review files are kept outside the repository in the campaign folder
(`sc_dogfood/campaign2/<test>/NOTES.md`, harness folders `hz1_harness`-`hz3_harness`, and the consolidated
`campaign2/FINDINGS.md`).
