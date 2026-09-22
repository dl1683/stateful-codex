# Stateful Codex evaluation record

This record distinguishes implemented behavior from demonstrated product
advantage. `STATEFUL_CODEX_PRODUCT_INTENT.md` defines the outcome being tested;
component tests and a runnable interface are necessary, but are not evidence
that Stateful Codex is better than ordinary Codex.

## Evaluation method

`clients/stateful-codex/eval/compare-rollouts.mjs` compares two rollout JSONL
files. It refuses to treat a pair as comparable unless originator, source,
model, reasoning effort, approval and sandbox policy, permission profile,
working directory, workspace roots, and normalized user prompt all match. It
reports full lifetime usage separately from uncached usage, checks predeclared
answer terms, and counts read-bearing tool calls plus Stateful retrieval and
persistence calls.

Read-bearing tool calls are a rollout-level proxy. They are not an exact count
of operating-system reads, and a single call may perform multiple operations.
Token totals are the final cumulative totals recorded by Codex, not estimates
from text length.

## Benchmark SC-EVAL-001: trivial exact directory inventory

Date: 2026-09-21

Question: does accumulated project intelligence reduce work or token cost on a
small task whose answer is cheap to verify directly?

The ordinary and Stateful runs used:

- TUI originator and CLI source;
- `gpt-5.6-luna` with `xhigh` reasoning effort;
- `danger-full-access` sandbox policy;
- the exact same selected directory;
- the exact same prompt; and
- cached Codex ChatGPT login, with no API key required.

Prompt:

```text
Count-regular-files-in-this-selected-directory-not-recursively-report-exact-total-and-group-every-filename-by-extension-do-not-edit-verify-from-filesystem
```

Rollouts:

- ordinary Codex: `01a0c60b-3489-7d72-96ae-54c1eb632a0a`
- Stateful Codex: `01a0c608-945f-7d42-a6d5-e889d4d6f19c`

The expected result was registered as the exact count plus all eight filenames.
Both runs returned the correct total, extension grouping, and filenames, and
both stated that no files were edited.

| Measure | Ordinary | Stateful | Stateful change |
| --- | ---: | ---: | ---: |
| Full input tokens | 68,968 | 84,145 | +15,177 |
| Cached input tokens | 47,360 | 52,736 | +5,376 |
| Uncached input tokens | 21,608 | 31,409 | +9,801 |
| Output tokens | 932 | 1,332 | +400 |
| Full lifetime tokens | 69,900 | 85,477 | +15,577 (+22.28%) |
| Uncached input + output | 22,540 | 32,741 | +10,201 (+45.26%) |
| Model responses | 3 | 3 | 0 |
| Read-bearing tool calls | 2 | 1 | -1 (-50%) |

Stateful also made one blackboard query, one obligation update, and one run
update. It did not need a context-map query for this task.

### Interpretation

This is a negative token-efficiency result. Stateful Codex reduced the number
of read-bearing tool calls, but the saved read was much cheaper than the added
project context, tools, reasoning, and semantic persistence. The benchmark is
below the expected break-even point for durable project intelligence: direct
verification of eight filenames is already trivial.

One paired run is not enough to attribute the entire token difference to a
specific context fragment or to estimate a stable percentage. It is enough to
reject any claim that Stateful is already cheaper for every task. The product
should preserve semantic continuity without pretending that memory is free.

### Remediation rerun SC-EVAL-001R

After binding model-facing Stateful tools to the selected thread's active run,
the exact benchmark was rerun through the rebuilt native CLI. The new Stateful
rollout is `01a0c64c-e23b-7333-beec-32603b4d8eb0`. The comparator confirmed
full parity with the ordinary rollout across entry point, model, reasoning,
permissions, selected directory, workspace roots, and prompt. Both answers
again contained the exact total and every expected filename.

The remediation achieved its reliability goal: `steering_query`,
`obligation_update`, and `stateful_run_update` no longer accepted or received a
model-authored `runId`. The live run completed its obligation and run update
without the malformed-ID failure seen in the previous diagnostic run. It used
one read-bearing call and made no blackboard or context-map query for this
cheap-to-verify task.

| Measure | Ordinary | Remediated Stateful | Stateful change |
| --- | ---: | ---: | ---: |
| Full input tokens | 68,968 | 128,712 | +59,744 |
| Cached input tokens | 47,360 | 99,328 | +51,968 |
| Uncached input tokens | 21,608 | 29,384 | +7,776 |
| Output tokens | 932 | 1,539 | +607 |
| Full lifetime tokens | 69,900 | 130,251 | +60,351 (+86.34%) |
| Uncached input + output | 22,540 | 30,923 | +8,383 (+37.19%) |
| Model responses | 3 | 5 | +2 |
| Read-bearing tool calls | 2 | 1 | -1 (-50%) |

This remains a negative efficiency result. Uncached input plus output improved
from 32,741 in the first Stateful run to 30,923 in the remediated run, but the
model used separate responses for steering, source verification, obligation
publication, run completion, and the final answer. The resulting cached-context
replays made full lifetime usage worse. A single rerun cannot separate prompt
effects from model sampling variance; it does show that removing opaque run IDs
fixed the observed reliability failure without establishing a token advantage.

## Live interface validation

The evaluation surface has also been exercised through both supported product
paths:

- Native CLI: the branch CLI built successfully; five Stateful TUI tests and
  snapshots passed; and a fresh real cached-login run completed as
  `01a0c64c-e23b-7333-beec-32603b4d8eb0` without API keys or source edits. Its
  selected-directory answer was exact, and its active-run tools required no
  model-authored run ID.
- Browser client: a rendered client was exercised against the real local
  gateway, branch app-server, branch CLI, and cached login. The run covered
  creating, continuing, and forking threads; exact evidence inspection;
  Socratic execution gating; pause/resume; and reload recovery. A fresh run on
  the remediated package created thread
  `01a0c646-d8f7-7571-b398-e77f423b429d`, surfaced the known Windows host-shell
  failure and approval recovery, then displayed the correct semantic obligation
  and completed result. Its screenshots are under
  `%LOCALAPPDATA%/Temp/stateful-client-smoke-1790032608507`.

The browser-client suite passes 5/5. Building its code-mode companion locally
on Windows used the matching `v150.4.0` archive and generated binding from the
published Codex `rusty-v8-v150.4.0` release because the upstream crate's default
sandbox archive URL returns 404. This was a local artifact override, not a
dependency or source change.

These checks demonstrate operability, not comparative product advantage.

### 2026-09-22 noninteractive CLI and live client regression

Testing exposed that root-level `--stateful` selection was silently dropped by
`codex exec`; the first attempted run therefore was not valid Stateful evidence.
After sharing native startup between TUI and exec and propagating the selection,
rollout `01a0c83a-c55b-75d0-89fa-8ab8b9cbebd5` ran through cached ChatGPT login
with both API-key environment variables removed. It received a selected project
and durable run, used `steering_query`, bounded `evidence_read` calls, one semantic
obligation update, and a terminal run update. It made no shell/file-read call and
no source edit. The answer correctly selected Cedar and cited exact project
files.

The final usage record was 116,310 input tokens, 83,200 cached input tokens,
2,920 output tokens, and 119,230 lifetime tokens. Uncached input plus output was
36,030. Because the prompt and entry point differ from the registered ordinary
baseline, this run is not reported as a comparative win. It demonstrates that
the repaired exec path is genuinely Stateful and that exact-range verification
works in a real run.

The local browser gateway then completed two real client-path runs with cached
ChatGPT login:

- Thread `01a0c841-34c6-7d31-a76a-68da3373edaf` automatically received the
  mature root blackboard and exact-source aliases, explicitly skipped deeper
  search, verified the controlling line ranges, published one obligation, and
  completed with the correct result. Its final usage was 124,627 lifetime tokens
  with 90,880 cached input tokens and 33,747 uncached input plus output.
- Thread `01a0c851-0974-74f2-92a6-db2ba4c9f3e6` began with a submitted user
  steering instruction. During the live turn the instruction became `applied`,
  its resulting strategy revision became 1, and the durable strategy,
  obligation, and final result all incorporated the requested distinction
  between viability and a final award.

The live exercise also proved and fixed a terminal-state defect. A completed run
accepted a new steering instruction in `submitted` state even though it could
never be applied. The runtime now rejects terminal-run steering, and the client
does not offer steering, mode, maintenance, or free-form instruction controls on
a terminal outcome; it preserves the record and offers `Start another outcome`.
The focused runtime test passes, the browser-client suite passes 6/6, and the
rebuilt live gateway rejects the same request with `cannot submit steering to a
terminal run (Completed)`.

No fresh screenshot-based run was possible in this session because the in-app
browser automation capability was unavailable. HTTP delivery, real gateway RPC,
real model execution, durable state reads, and renderer tests were exercised.
Rebuilding the code-mode companion was independently blocked by the pinned V8
archive download on Windows; the live gateway used the existing companion from
the prior validated build. This limits the scope of the fresh client claim but
does not affect the successful branch CLI rebuild or the real gateway/model
results above.

## Open release evidence

The Stage 8 release claim remains open until representative paired scenarios
measure the outcomes the product exists to improve:

1. knowledge precision and recall after multiple turns and compaction;
2. reduced repeated source reading and lifetime cost in a mature workspace;
3. recovery of a decisive detail and a non-obvious cross-source connection;
4. steering acknowledgement and application latency;
5. unattended autonomous completion and restart recovery; and
6. final-claim correctness, freshness, and traceability to exact evidence.

Each scenario must predeclare its expected facts or decisions, use matched
ordinary/Stateful conditions where comparison is meaningful, preserve negative
results, and avoid turning component success into a product-level claim.

## Benchmark SC-EVAL-002: decisive procurement connection

Corpus: `clients/stateful-codex/eval/fixtures/procurement`.

The Stateful condition first receives one maturation run whose explicit job is
to map the corpus and persist materially reusable, evidence-linked project
understanding. The measured question is then asked in a fresh Stateful thread.
The ordinary condition receives the same measured question in a fresh ordinary
CLI thread with the same model, reasoning effort, permissions, working
directory, workspace roots, and prompt. The maturation run is reported
separately and is not hidden from lifetime-cost interpretation.

Measured prompt:

```text
Determine-which-vendor-is-viable-under-all-binding-criteria-identify-the-decisive-cross-source-details-that-rule-out-each-alternative-cite-exact-files-and-do-not-edit
```

Predeclared expected decision and decisive details:

- Cedar is the only viable vendor.
- Alder fails the EEA gate because `security-addendum.md` permits temporary
  plaintext access by a support engineer in Virginia, United States, despite
  the proposal's EU-hosting claim.
- Birch fails continuity because its 72-hour queue is shorter than the verified
  96-hour outage in `operations-log.md`.
- Cedar provides 120 hours of offline acceptance, EEA-only plaintext support,
  and an authoritative $285,000 year-one total, so it passes all three gates.
- The answer cites `binding-criteria.md`, `security-addendum.md`,
  `operations-log.md`, and `finance-schedule.md`, and reports that no files were
  edited.

The evaluation will record answer correctness, source traceability, read-bearing
calls, Stateful retrieval and persistence calls, full and uncached token usage,
and whether the fresh Stateful thread uses accumulated state before reopening
exact source material. A failure to persist useful knowledge, a stale or
unsupported claim, or a cost regression remains a negative result.

### Maturation run

The Stateful workspace was matured in an autonomous native CLI run before the
measured question. Rollout `01a0c657-cded-7882-891d-e4ae2911ea33` read all eight
files and persisted the binding gates, authority order, authoritative costs,
verified outage requirement, vendor outcomes, the Cedar-only gate matrix, the
contradiction with the preliminary ranking, open questions, and cross-source
relationships. It correctly preserved the distinction between "only viable
candidate" and "finally awarded."

The maturation run also exposed reliability and cost problems. The model had to
recover from an omitted confidence field, a source-fingerprint typo, an
idempotency collision after a partially successful batch, and an invalid
relationship kind. Recovery preserved the final state, but the run consumed
994,169 lifetime tokens: 971,091 input tokens, of which 864,000 were cached, and
23,078 output tokens. That one-time cost is part of the product's lifetime cost;
it cannot be excluded merely because the measured question ran later.

### Matched measured runs

The ordinary rollout is `01a0c660-a7eb-7092-b5b4-11e1213b779f`; the fresh
Stateful rollout is `01a0c662-8f72-7333-809d-9a1267b5164f`. Both used the native
TUI/CLI, cached ChatGPT login, `gpt-5.6-luna` with `xhigh` reasoning,
`danger-full-access`, the exact same selected directory and workspace root, and
the exact pre-registered prompt. The comparator accepted every parity field.

Both answers passed every predeclared content check. Both selected Cedar, found
the Virginia plaintext-access conflict and 72-versus-96-hour continuity
failure, reported Cedar's 120-hour queue and $285,000 authoritative total,
cited the controlling exact files, distinguished viability from final award,
and reported no edits.

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

The rollout-level comparator reports two ordinary read-bearing calls and one
Stateful read-bearing call because each call may contain parallel operations.
Inspection of the call contents shows that each condition opened all eight raw
source files. The proxy must therefore not be presented as a 50% source-read
reduction in this benchmark.

Stateful did use accumulated state first: it queried steering, then used the
context map, then reopened the corpus. It published a concise semantic
obligation and completed the durable run through tools bound to the selected
thread; no model-authored run ID was present. That demonstrates continuity,
correct decisive-detail recovery, source traceability, and semantic progress.
It does not demonstrate the intended mature-workspace access pattern.

### Interpretation

SC-EVAL-002 fails the reduced-rereading and token-efficiency gates. The mature
root state already contained the decision and decisive cross-source facts, yet
the measured run performed eight broad context-map searches and reopened every
file to produce exact citations. Stateful added four model responses and more
than doubled both full and uncached usage. Including maturation makes the
lifetime economics substantially worse.

The next remediation should target the two observed causes rather than weaken
verification:

1. make source verification selective by directing the model from verified
   root claims to the smallest controlling source set, using the context map as
   a locator rather than querying once per known filename; and
2. reduce persistence round trips during maturation so a coherent group of
   validated findings can be committed and projected without repeatedly
   replaying a large context between individual semantic writes.

No product-level efficiency advantage is established yet. SC-EVAL-002 does,
however, establish that the current system can carry a non-obvious decision and
its provenance across threads and return the right evidence-grounded result.

### Remediation reruns SC-EVAL-002R

Two implementation changes followed the negative result:

- current source routes are resolved into the always-loaded root, and a bounded
  batch-record tool can persist up to 16 coherent findings in one model call
  with per-item success or failure; and
- root rendering now prioritizes semantic content and uses compact `E#`, `R#`,
  and `S#` aliases instead of repeating opaque IDs, absolute paths, provenance
  call IDs, and full relationship endpoint IDs.

The batch path is covered through the real app-server/extension integration: one
model call persisted two independently idempotent records, returned two
successes and zero failures, and a later query retrieved the result. It has not
yet been measured in a new corpus-maturation run, so no maturation-cost reduction
is claimed.

The route-only intermediate CLI rollout is
`01a0c7db-bb7f-7a50-884f-753920172109`. It consumed 147,944 lifetime tokens and
37,352 uncached input plus output. It skipped the context-map query but still
opened all eight files. That result exposed why merely adding routes was
insufficient: the old 24 KiB root spent most of its budget on repeated machine
identifiers and paths, truncated the decisive gate-matrix content, and omitted
later promoted findings.

The compact-root CLI rollout is `01a0c7e3-6569-71e3-a526-3430f1b82fd7`.
Its injected Stateful project section was 6,456 bytes and contained the full
gate matrix, every current exact-source route, and every promoted finding
without entry truncation. API-key environment variables were removed before
launch; the native branch CLI used the cached Codex ChatGPT login.

| Measure | Ordinary | Original Stateful | Compact-root Stateful |
| --- | ---: | ---: | ---: |
| Full lifetime tokens | 72,891 | 237,459 | 149,360 |
| Uncached input + output | 18,363 | 43,411 | 36,720 |
| Model responses | 3 | 7 | 5 |
| Raw source files reopened | 8 | 8 | 8 |
| Context-map queries | 0 | 1 | 1 |

Relative to the original Stateful measurement, compact root rendering reduced
full lifetime usage by 88,099 tokens (37.10%) and uncached input plus output by
6,691 tokens (15.41%). Relative to ordinary Codex it remains a regression:
104.91% more lifetime tokens and 99.97% more uncached input plus output. Both
conditions opened all eight raw files. The compact rerun again passed every
predeclared answer term and completed its semantic obligation and active run
without a model-authored run ID.

This rerun narrows the diagnosis. Missing or truncated project intelligence is
no longer the reason for broad verification: the model saw the complete current
decision and routes, then deliberately rechecked the full eight-file corpus for
an exact, alternative-by-alternative cited answer. A task that explicitly asks
for every alternative and exact citations has a high legitimate verification
floor. Future reduced-rereading evaluation should separately test questions
whose answer depends on a small subset of a much larger mature corpus. This
benchmark still fails the lifetime-cost gate and must remain negative evidence.

## Benchmark SC-EVAL-003: selective controlling evidence

Status: pre-registered and executed on 2026-09-22.

Corpus: the mature `clients/stateful-codex/eval/fixtures/procurement` project
used by SC-EVAL-002. This benchmark asks a narrow question whose controlling
source set is smaller than the eight-file corpus.

Prompt:

```text
Determine whether Alder satisfies the binding EEA data-residency gate. Identify the controlling evidence that overrides any proposal claim, cite exact project files, distinguish this gate decision from overall vendor viability, and do not edit files.
```

Predeclared expected result:

- Alder fails the binding EEA residency gate.
- `binding-criteria.md` makes storage, processing, debugging, emergency support,
  and temporary plaintext access part of the gate.
- `security-addendum.md` is executed and supersedes inconsistent proposal
  language; it permits a Virginia support engineer to decrypt and view plaintext
  records during emergency support.
- The failed mandatory gate makes Alder non-viable under the conjunctive criteria,
  but the answer does not need to reevaluate Birch or Cedar.
- The answer cites exact project files and reports no edits.

The ordinary and Stateful conditions must use the same native `codex exec` entry
point, model, reasoning effort, working directory, permissions, workspace roots,
and exact prompt with cached ChatGPT login and no API-key environment variables.
The result will report answer correctness, files and line ranges opened, Stateful
retrieval/persistence calls, full lifetime tokens, cached tokens, and uncached
input plus output. Reading unrelated vendor, operations, or finance files counts
against the selective-routing claim. Lower source-read breadth without lower
token cost is useful mechanism evidence but not an efficiency win.

### Execution and result

The matched ordinary rollout is `01a0c860-5a90-7ff1-9542-f673bf38faf3`.
The first Stateful rollout is `01a0c861-6e0f-7850-b0a9-1beb71ec7140`.
After that run exposed two avoidable orchestration turns, the run-state context
was changed to identify complete pending steering directly and to ask the model
to publish an already-decided obligation and terminal run update sequentially
in one code-mode call. The resulting optimized rollout is
`01a0c875-8a49-7783-9569-bc9ab50102b9` at commit `4154ebcbc9`.

All three runs used native `codex exec`, `gpt-5.6-luna` at `xhigh`, the same
prompt, working directory, workspace root, `never` approval,
`danger-full-access`, the cached ChatGPT login, and no API-key environment
variables. The comparator accepted every parity field. All three answers
reached the correct controlling result and reported no edits.

| Measure | Ordinary | First Stateful | Optimized Stateful |
| --- | ---: | ---: | ---: |
| Full measured-turn tokens | 102,914 | 140,567 | 83,056 |
| Cached input tokens | 81,920 | 106,496 | 50,688 |
| Uncached input + output | 20,994 | 34,071 | 32,368 |
| Model responses | 4 | 5 | 3 |
| Read-bearing outer tool calls | 3 | 1 | 1 |
| Distinct raw project files opened | 6 | 3 | 4 |
| Steering-query calls | 0 | 1 | 0 |
| Stateful persistence model turns | 0 | 2 | 1 |

Ordinary Codex opened `binding-criteria.md`, `security-addendum.md`,
`vendor-alder.md`, `operations-log.md`, `finance-schedule.md`, and
`committee-notes.md`. The first Stateful run opened only the three decisive
residency sources: the binding criteria, executed addendum, and Alder proposal.
The optimized rerun opened those three plus the finance schedule. It still
avoided the operations log, committee notes, and the other two vendor files.
Thus the optimized condition reduced raw-file breadth from six to four (33.3%)
and outer read-bearing calls from three to one, while the first Stateful run
demonstrated the stricter three-file minimum.

The optimized rollout did not call `steering_query`. Its first model tool turn
used one `evidence_read` batch for four exact line ranges. Its second and final
tool turn issued `obligation_update` and then `stateful_run_update` in one
code-mode call. The durable run completed with a semantic packet and an
evidence-grounded result. This removed two model responses relative to the
first Stateful run and reduced its full measured-turn usage by 57,511 tokens
(40.91%) and uncached input plus output by 1,703 tokens (5.00%).

Relative to ordinary Codex, the optimized run used 19,858 fewer full tokens
(19.30%) and one fewer model response, but 11,374 more uncached input plus
output (54.18%). It also reopened one predeclared non-controlling finance file.
The measured turn therefore demonstrates selective source routing and lower
full-token usage, but not a clean uncached-token or lifetime-cost advantage.
The already-incurred maturation cost is not hidden in this comparison, and a
single stochastic rerun cannot establish a stable performance distribution.
The next efficiency work should reduce fixed injected/tool-schema overhead and
repeat matched runs without sacrificing the exact-source behavior shown here.

### Same-binary ordinary replication SC-EVAL-003R

Status: pre-registered before execution on 2026-09-22.

The original ordinary run predates commit `4154ebcbc9`, while the optimized
Stateful run uses the binary built from that commit. The runtime change is
Stateful-specific, but one ordinary replication will remove the binary-version
confound without spending on a broad run matrix. It will use that same built
binary, exact prompt, model and reasoning effort, working directory, workspace
root, permissions, cached ChatGPT login, and cleared API-key environment.

The replication will be compared with optimized Stateful rollout
`01a0c875-8a49-7783-9569-bc9ab50102b9`. The report will preserve correctness,
raw files opened, model responses, full and uncached usage, and cache behavior.
One replicated pair remains directional evidence rather than a stable
performance distribution.

Execution completed in ordinary rollout
`01a0c880-4ea5-7911-9b19-065635cb608d`. Comparator parity with the optimized
Stateful rollout passed for every field. The ordinary answer reached the right
gate and viability conclusions, but it did not explicitly report that no files
were edited; a post-run clean-worktree check confirmed no edits. The Stateful
answer included the explicit no-edit statement.

| Measure | Same-binary ordinary | Optimized Stateful |
| --- | ---: | ---: |
| Full measured-turn tokens | 101,020 | 83,056 |
| Cached input tokens | 80,896 | 50,688 |
| Uncached input + output | 20,124 | 32,368 |
| Model responses | 4 | 3 |
| Read-bearing outer tool calls | 3 | 1 |
| Distinct raw project files opened | 6 | 4 |

The ordinary replication again opened the three controlling residency files
plus the operations log, finance schedule, and preliminary committee notes.
The optimized Stateful run remained narrower by two files and two read-bearing
outer calls. It used 17,964 fewer full tokens (17.78%) but 12,244 more uncached
input plus output (60.84%). This reproduces the selective-routing and full-token
advantages while also reproducing the uncached-cost regression. The result
strengthens the mechanism finding; it still does not establish lower lifetime
cost after workspace maturation or a stable performance distribution.

## Project-state quality instrumentation

`clients/stateful-codex/eval/evaluate-project-state.mjs` now reads a live
project through the real gateway and scores a versioned manifest. It separates:

- semantic concept recall;
- recall of concepts backed by current source-verified evidence;
- the fraction of returned entries that are active, source-verified,
  evidence-linked, and current; and
- prohibited affirmative conclusions.

It refuses a completeness result when the bounded 50-entry response is
truncated. The default procurement manifest covers 11 controlling facts,
decisions, contradictions, and open questions. At project-intelligence revision
43, the live mature project returned 14 of 14 currently supported entries,
matched and currently supported all 11 probes, and contained no prohibited
affirmative conclusion. The client suite, including the scorer behavior, passes
7/7.

This is a regression baseline, not an independent estimate of semantic
precision or recall: the procurement corpus and existing state had already been
inspected before the manifest was committed. A held-out corpus must commit its
expected probes before maturation and model evaluation before supporting a
general project-memory quality claim.

## Benchmark SC-EVAL-004: held-out licensing memory

Status: executed on 2026-09-22. The manifest and procedure below were committed
before the maturation run; neither was changed after observing the result.

Corpus: `clients/stateful-codex/eval/fixtures/licensing`, a ten-file licensing
review created after the procurement evaluator. The expected semantic manifest
is committed separately at
`clients/stateful-codex/eval/manifests/licensing-state.json`; it is outside the
project root and unavailable to the evaluated agent.

The corpus contains a proposal, master agreement, two executed amendments, an
executed territory side letter, approved economics, verified risk evidence,
preliminary minutes, a closing checklist, and a binding review policy. The
decisive detail is that executed amendment 2 removes data-security,
confidentiality, and intellectual-property indemnity exposure from the master
agreement's $2,000,000 general cap. The transaction nevertheless is not ready
to close because Canadian consent, counsel confirmation, and final board
approval remain pending.

The evaluation has three phases:

1. A native Stateful maturation run must inspect the project, record reusable
   evidence-linked understanding, relationships, contradictions, and open
   questions, and report its full cost.
2. The committed manifest must score the resulting live blackboard before any
   probe is changed. It checks ten concepts, current evidence support, and five
   prohibited affirmative conclusions.
3. Fresh ordinary and Stateful CLI threads receive the same narrow question:

```text
Is liability exposure capped at $2,000,000 for a data-security breach under the current licensing documents? Identify the controlling instrument and exact project files, distinguish that liability conclusion from whether the transaction is ready to close, and do not edit files.
```

The expected answer is no: amendment 2 controls over the master agreement and
proposal, and its carve-out makes data-security exposure uncapped by Section
7.3. That conclusion does not make the transaction ready to close; the closing
checklist's pending conditions remain independently controlling. The measured
phase will report correctness, exact source breadth, read-bearing calls, full
and uncached tokens, and model responses. Maturation cost remains explicit in
the lifetime interpretation.

### Maturation result

Native Stateful CLI thread `01a0c88f-9c66-70b1-8210-fd14b2068f3d`
refreshed the context map, found all ten files with zero skipped or missing,
read each file once, and did not edit the fixture. It persisted 18 records, 13
of them root-promoted, with 24 relationships. The records captured the
authority hierarchy, executed terms, stale proposal and preliminary-board
contradictions, closing blockers, numbers, cross-source implications, and open
questions. The run cost was:

| Measure | Result |
| --- | ---: |
| Model responses | 15 |
| Input tokens | 630,185 |
| Cached input tokens | 554,496 |
| Output tokens | 18,988 |
| Full tokens | 649,173 |
| Uncached input plus output | 94,677 |

This is a substantial one-time maturation cost. It must be amortized across
future work before Stateful can claim a lifetime economic advantage.

The run also exposed two recoverable ergonomics failures. Its first batched
source read requested `maxBytes: 20000`, above the declared 12,288-byte limit,
and had to be retried. One relationship batch used a malformed endpoint ID;
the successful relationships remained durable and the missing links were
retried with the correct IDs. These failures count against maturation
efficiency even though the final state was coherent.

### Held-out state score

The first post-maturation invocation of the committed manifest returned:

- 18 total entries;
- 17 currently source-supported entries, or 94.44% supported-entry precision;
- 6 of 10 exact lexical concept probes matched and source-supported;
- zero prohibited affirmative conclusions; and
- overall `passed: false`.

The one unsupported entry was a corpus-coverage inventory stating that all ten
files had been read. It had no source evidence and should not have been
persisted under the product's reuse-value rule.

Manual inspection shows that all ten intended concepts are present in current,
source-verified state. Four exact probes missed because the intentionally
simple scorer requires every literal term group to occur in one entry. Examples
include `6%` versus `six percent`, `supersede inconsistent` versus `supersedes
inconsistent`, and `written Canadian regulatory consent` versus the contiguous
phrase `written consent`; the royalty probe also spans the executed-term and
approved-forecast records. This means the 6/10 result does not establish 60%
semantic recall. It does establish that the current lexical scorer is not a
semantic recall measure, and the unchanged failing result is retained rather
than tuning the held-out manifest after observation.

### Fresh CLI comparison

Ordinary thread `01a0c898-cab9-7ad1-ac00-aedc316858aa` and Stateful thread
`01a0c898-c9e2-7670-8f23-57a0f3c1b7f1` used the same locally built CLI,
`gpt-5.6-luna` at `xhigh`, cached ChatGPT authentication with API-key variables
removed, identical permissions, identical workspace roots, and the exact
pre-registered prompt. Both answers were correct, identified executed Amendment
2 as controlling, cited the master license and closing checklist, separated the
uncapped data-security conclusion from close readiness, and made no edits.

| Measure | Ordinary | Stateful | Stateful delta |
| --- | ---: | ---: | ---: |
| Full tokens | 173,902 | 146,500 | -27,402 (-15.76%) |
| Uncached input plus output | 45,390 | 35,908 | -9,482 (-20.89%) |
| Model responses | 6 | 5 | -1 |
| Read-bearing outer calls | 4 | 3 | -1 |
| Project files read | 10 | 4 | -6 |

Ordinary Codex listed and reread the entire ten-file corpus. Stateful began
from the root blackboard and verified four decisive files: the master license,
executed Amendment 2, the closing checklist, and the binding review policy. Its
first parallel verification wrapper accidentally forwarded a local `key` field
to `evidence_read`; it corrected the wrapper and completed the four focused
reads. The comparison therefore demonstrates reduced rereading and both full-
token and uncached-token savings despite that avoidable retry.

The generic rollout scorer reported both answer expectations as failed only
because the post-run check requested the literal phrase `not ready to close`;
both answers instead said that the liability conclusion does not make the
transaction ready and that pending items prevent closing. The source-grounded
meaning is correct. This is another lexical-evaluator limitation, not an answer
failure.

Including maturation, the first Stateful question is not economically cheaper:
its cumulative full-token cost is 795,673 and its cumulative uncached cost is
130,585. The measured per-question saving is real, but this single follow-up
does not amortize the 649,173-token maturation investment. More consequential
questions on the same mature project are required to establish a break-even
point and lifetime advantage.

### Interactive CLI/TUI smoke test

The interactive Stateful CLI was also exercised in a Windows PTY with
`RUST_LOG=trace`, an explicit log directory, the licensing directory, the
existing Stateful project, Collaborative mode, cached ChatGPT login, and the
API key removed. Thread `01a0c8a5-1503-7243-a8bd-f62ad30f2e4f` launched the TUI,
rendered the selected directory and model, accepted the prompt, streamed and
rendered `TUI_STATEFUL_OK`, reported token usage, produced a resumable thread,
and shut down cleanly on Ctrl-C.

The required `just codex` source path compiled through `codex-cli` but could not
replace `target/debug/codex.exe` because the live Stateful gateway's app-server
process held that executable open on Windows. The already current local binary
was then used for the PTY smoke test without stopping the gateway. This proves
the interactive path operates; it does not claim a clean relink while the live
gateway owns the output binary.

The gateway was subsequently stopped so the focused crate checks could relink
the binary. `just test -p codex-cli` ran 448 tests: 447 passed and the unrelated
`sandbox_fetches_and_enforces_cloud_managed_permission_profile` test timed out
twice while launching a nested Windows sandbox. The Stateful root-to-exec CLI
selection test passed. A targeted `just test -p codex-tui stateful_ui` then ran
the four Stateful TUI tests; all four passed, with nextest reporting one existing
leaky-handle annotation. These cover selected mode/project startup, native run
attachment, semantic obligation rendering, and durable completion rendering.

The full `codex-tui` crate suite was also attempted and is not green on this
checkout. It exposed broad non-Stateful Windows failures: active-theme snapshot
drift, multiple stack overflows in session and pagination tests, and several
timeout/leak failures. The generated `.snap.new` and `.pending-snap` artifacts
were removed without accepting snapshot changes. After testing, the gateway was
restarted against the relinked binary; `/health` returned
`{"ready":true,"authMode":"chatgpt"}`, `codex login status` returned
`Logged in using ChatGPT`, and the Git worktree was clean.

## Benchmark SC-EVAL-005: maturation amortization series

Status: pre-registered before execution on 2026-09-22.

This benchmark reuses the mature held-out licensing project from SC-EVAL-004.
It asks three new consequential questions that depend on different subsets of
the ten-file corpus: current economics, Canadian territory, and termination
notice plus modeled insurance exposure. The exact prompts, semantic term
groups, prohibited conclusions, and one-time maturation usage are committed in
`clients/stateful-codex/eval/manifests/licensing-series.json` before any run.

Each question will be run once through fresh ordinary and Stateful native CLI
threads using the same built binary, model, reasoning effort, selected
directory, workspace roots, permissions, cached ChatGPT login, and cleared API
key environment. Both answers must pass the pre-registered semantic checks and
the rollout pair must pass the existing parity checks before its cost is
included.

The series report will preserve per-question full and uncached token usage,
model responses, and read-bearing calls. It will report follow-up wins across
the three pairs, cumulative ordinary follow-up cost, cumulative Stateful
follow-up cost, and Stateful lifetime cost after adding SC-EVAL-004's 649,173
full and 94,677 uncached maturation tokens. A projected break-even count is
reported only when the observed average per-question saving is positive. Three
pairs can show whether the first saving repeats across distinct questions; it
cannot establish a universal workload distribution.

### Execution and result

All six fresh runs used the same branch binary, `gpt-5.6-luna` at `xhigh`, the
same licensing directory and workspace root, `never` approval,
`danger-full-access`, cached ChatGPT authentication, and cleared
`OPENAI_API_KEY` and `CODEX_API_KEY`. The rollout comparator accepted every
parity field. The paired thread IDs are:

| Question | Ordinary | Stateful |
| --- | --- | --- |
| Economics | `01a0c8c1-aa6a-71f2-ac7a-3b2a0b450f89` | `01a0c8c2-d0f8-75c1-a29f-b47ea7049ac1` |
| Territory | `01a0c8c4-9760-7cd1-9a74-a38270081e86` | `01a0c8c5-d66c-7881-9e72-944812d73eda` |
| Termination and risk | `01a0c8c7-52fa-74c2-aca1-0bfc71831455` | `01a0c8ca-438a-77e1-be0a-1606fcc85e3f` |

Each answer reached the correct substantive result, cited exact project files,
distinguished executed terms from proposals or planning assumptions, and made
no edits. Ordinary Codex opened all ten project files for every question.
Stateful Codex verified four files for economics, three for territory, and four
for termination and risk. It began from the mature root blackboard and did not
need a deeper-blackboard or context-map query.

| Measure | Ordinary | Stateful follow-ups | Delta |
| --- | ---: | ---: | ---: |
| Full tokens | 428,784 | 291,780 | -137,004 (-31.95%) |
| Uncached input plus output | 73,456 | 90,052 | +16,596 (+22.59%) |
| Model responses | 16 | 10 | -6 |
| Read-bearing outer calls | 13 | 4 | -9 |
| Raw project files opened across pairs | 30 | 11 | -19 |
| Per-question full-token wins | — | 3 of 3 | — |
| Per-question uncached-token wins | — | 0 of 3 | — |

The full-token improvement is stable across these three distinct questions:
Stateful saved 25.49%, 40.14%, and 29.73% respectively. The uncached result is
also stable in the wrong direction: it cost 16.62%, 19.24%, and 30.48% more.
The mechanism therefore reduces rereading, model turns, and cached/full token
work, while its fixed uncached context and tool overhead remains too high.

The committed lexical scorer returned `passed: false`. Its raw failure is not
being repaired after observation. The Stateful economics answer described the
proposal as “explicitly subordinate” and said it should control neither answer,
rather than using the registered `does not control`/`supersed`/`non-binding`/
`stale` strings. The ordinary territory answer said the licensee “may not sell”
and quoted “excluding Canada,” rather than one registered exclusion phrase.
Both risk answers described the data-security claims as outside or removed from
the cap rather than using the registered `uncapped`/`cap does not apply`/
`carve-out` wording. These are scorer misses, not substantive errors; the raw
failure remains evidence that a literal manifest is not a semantic judge.

Adding the recorded maturation run yields 940,953 full and 184,729 uncached
Stateful tokens for this three-question series, versus 428,784 full and 73,456
uncached ordinary tokens. Stateful therefore remains behind by 512,169 full and
111,273 uncached tokens. At the observed average full-token saving, maturation
would break even around question 15. No uncached break-even can be projected
because all three follow-ups regressed. This closes the stable selective-routing
question for this fixture, but it fails the maturation-inclusive lifetime and
uncached-cost release gates.

## Benchmark SC-EVAL-005R: deferred-tool remediation replication

Status: pre-registered before execution on 2026-09-22.

Commit `8aa897c673` moves seven exceptional-path Stateful tools behind Codex's
existing deferred-tool discovery boundary. Exact evidence reading, semantic
obligation publication, terminal run updates, and active steering reconciliation
remain immediately visible. Deeper blackboard and context-map queries, context
refresh, blackboard mutation, relationship mutation, and historical steering
query remain registered and callable but no longer contribute their full schemas
to every initial request.

The territory question from SC-EVAL-005 will be rerun once in fresh ordinary and
Stateful threads using the same rebuilt binary and the original committed prompt
and answer manifest. Territory is the median uncached regression in the original
series and needs only the executed side letter, closing checklist, and binding
policy, making it a useful fixed-overhead probe. The comparison will preserve
answer correctness, parity, project-file breadth, full tokens, uncached input
plus output, model responses, and read-bearing calls. One remediation pair can
show whether the intended payload changed; it cannot replace the three-pair
distribution or establish maturation-inclusive break-even.

### Execution, diagnosis, and decision

The same-binary ordinary thread is
`01a0c8e5-1458-7a11-ad92-ca037549a2a1`; the Stateful thread is
`01a0c8e7-623f-7182-89e3-0efe34244e6b`. Both passed every parity field and the
three direct answer checks. Ordinary reopened all ten files. Stateful verified
only `executed-side-letter.md`, `closing-checklist.md`, and
`binding-review-policy.md`, completed the semantic obligation and run, and made
no source edits.

| Measure | Ordinary | Stateful | Stateful delta |
| --- | ---: | ---: | ---: |
| Full tokens | 156,315 | 82,664 | -73,651 (-47.12%) |
| Uncached input plus output | 40,091 | 54,504 | +14,413 (+35.95%) |
| Model responses | 6 | 3 | -3 |
| Read-bearing outer calls | 4 | 1 | -3 |
| Project files opened | 10 | 3 | -7 |

The intended schema reduction was real but small. The optimized Stateful first
request contained 26,062 input tokens, versus 26,442 in the original territory
run: 380 fewer tokens. Provider cache behavior dominated the result. The three
optimized Stateful responses received 1,792, 0, and 26,368 cached input tokens
respectively. The second request alone contributed 27,021 uncached input tokens;
the third then cached almost the entire prefix. This is not evidence that the
root blackboard was reread or that selective routing failed.

The mature root project fragment is 11,150 characters and carries the 13
decisive source-routed findings and their relationships. The 1,311-character run
fragment carries the explicit user-selected mode, goal, constraints, current
steering state, and completion contract. Removing either would trade away the
product behavior being measured. Moving the exceptional tools behind discovery
saved only about 1.4% of the first input while making core memory-authoring and
routing capabilities less immediately discoverable.

The deferred-tool implementation is therefore rejected and reverted. The
result remains useful: full-token and rereading advantages strengthened, while
uncached cost remains dominated by volatile prefix-cache misses rather than a
large removable tool-schema block. Future efficiency work must preserve the
rich root and explicit run contract, measure distributions rather than a single
cache outcome, and target stable prefix construction or maturation efficiency
only when the host/provider boundary makes that actionable.

## Fresh rendered browser validation

Status: passed on 2026-09-22 against the live port-4174 gateway, current branch
CLI, and cached ChatGPT login.

The browser created Collaborative thread
`01a0c904-e60c-7391-9845-a9211c9e3233` for the mature licensing project with
this predeclared, read-only task: determine the currently executed royalty rate
and controlling instrument, verify exact evidence, publish one concise semantic
obligation, complete the run, and do not edit project files. Durable run
`run-33c385b12d21a4b44b90a9fcdc1587c24a46d8c107cce2bd52bf6b415f685f8a`
completed with zero continuations. It reported 6% of net sales from 2026-03-01
and identified executed Amendment 1's express replacement of master Section 4.2
as controlling. The rendered obligation separately stated the learning and its
implication, and the strategy explained why the executed amendment rather than
the stale 8% proposal controlled.

The UI's evidence action returned the exact 296-byte
`executed-amendment-1.md` revision. The visible source included execution by
both parties on 2026-02-05, the Section 4.2 replacement, and the six-percent
royalty. No licensing fixture path changed.

The first full-page render exposed a layout failure rather than a data failure:
18 findings remained in a narrow right rail, producing a 5,042-pixel page with
large blank regions under the hierarchy and result. The corrected layout keeps
operational controls in the right rail and renders findings as a separate
full-width responsive grid. The same desktop page is now 2,615 pixels tall and
retains all findings, badges, evidence links, obligations, strategy, result,
controls, and source routing. Renders at 1440 px, 768 px, and 480 px were
visually inspected; the finding grid resolves to three, two, and one columns
respectively. The exact-evidence state was also inspected. Evidence images are
under `%LOCALAPPDATA%/Temp/stateful-client-render-1790078328677`.

The workspace HTML snapshot changed with the layout and the complete client
suite passes 8/8. This establishes a fresh rendered browser pass for the current
executable. It is not evidence of maturation-inclusive token advantage and does
not substitute for the approval-gated repository-wide Rust suite.

## Benchmark SC-EVAL-006: linked-batch maturation replication

Status: executed on 2026-09-22. The procedure below was committed before the
run and was not changed after observing the result.

The SC-EVAL-004 maturation trace required separate turns for a 16-record batch,
a relationship batch using copied opaque entry IDs, a second two-record batch,
another relationship batch, and a retry after one copied endpoint ID was
malformed. Commit `093cfc3211` raises the bounded finding batch to 24 and lets
the same call persist up to 48 relationships by referencing the records'
idempotency keys. The existing single-record and entry-ID relation tools remain
available for incremental updates.

This replication will copy the unchanged ten-file licensing corpus to a fresh
temporary directory so project identity and durable state start empty, rebuild
the branch CLI, remove API-key environment variables, and run the exact
SC-EVAL-004 maturation prompt with `gpt-5.6-luna` at `xhigh` through cached
ChatGPT authentication. It will report model responses, tool-call sequence,
blackboard records and relationships, full and uncached token usage, malformed
or retried persistence calls, and source edits.

This is a same-corpus mechanism replication, not a new held-out semantic test.
Its purpose is to determine whether the model naturally uses the linked batch
and whether that removes persistence turns and opaque-ID failures while
preserving the existing state quality. The result will be recorded even if the
model ignores the new path or cost regresses. No claim about general maturation
economics will be made from one replication.

### Execution and result

Native Stateful CLI thread `01a0c91e-af11-7aa0-a7f9-cd77a9ccb9a2` used a
fresh project rooted at
`%LOCALAPPDATA%/Temp/stateful-maturation-1790080610019`, `gpt-5.6-luna` at
`xhigh`, cached ChatGPT authentication, and no API-key environment variables.
The temporary corpus contains the same ten filenames and is byte-for-byte
identical to the committed licensing fixture. A post-run hash comparison also
confirmed that no source file changed.

The model inspected all ten files, verified exact evidence from all ten through
one parallel `evidence_read` call, and persisted 17 findings plus 27
relationships. All 17 findings and all 27 relationships were accepted in one
`blackboard_record_batch` call with zero failures. The relationship endpoints
used local idempotency keys; there was no malformed opaque entry ID and no
persistence retry. The unsupported corpus-inventory record from SC-EVAL-004
was not recreated.

| Measure | SC-EVAL-004 | SC-EVAL-006 | Delta |
| --- | ---: | ---: | ---: |
| Model responses | 15 | 10 | -5 (-33.33%) |
| Custom tool calls | 14 | 9 | -5 (-35.71%) |
| Full tokens | 649,173 | 344,870 | -304,303 (-46.88%) |
| Uncached input plus output | 94,677 | 75,814 | -18,863 (-19.92%) |
| Blackboard findings | 18 | 17 | -1 |
| Relationships | 24 | 27 | +3 |
| Malformed or retried persistence calls | 1 | 0 | -1 |

The live frozen-manifest scorer returned 17 of 17 active, current,
source-verified, evidence-linked findings, zero forbidden conclusions, and
3 of 10 literal concept probes. Manual semantic review found all ten intended
concepts: the authority hierarchy, current royalty, base cap and executed
carve-outs, termination, territory, risk/insurance gap, preliminary-board
contradiction, closing blockers, and open insurance question. The lower literal
score is retained rather than tuned after observation. Its misses again require
all registered strings in one finding and do not recognize representations such
as `6%` for `six percent`, `convenience termination` for `termination for
convenience`, or one concept intentionally split across linked executed-term and
economics findings. The result therefore establishes perfect supported-entry
precision and complete manual concept coverage, not a machine-scored semantic
recall pass.

The linked batch changed real model behavior and materially reduced maturation
cost without thinning the root blackboard or hiding core memory tools. It does
not establish general maturation economics: this is one same-corpus mechanism
replication, and the run still used a full-corpus shell pass before exact-source
verification, a context refresh plus two context queries, and three final
blackboard checks grouped into one tool turn.

Using the already measured SC-EVAL-005 follow-up aggregate only as a directional
lifetime projection, the improved maturation plus three Stateful follow-ups
would cost 636,650 full tokens versus 428,784 ordinary tokens, a remaining
207,866-token deficit. The observed average full-token follow-up saving projects
full-token break-even around question eight instead of question fifteen. The
same series would still cost 165,866 uncached tokens versus 73,456 ordinary, so
there is still no observed uncached break-even. These cross-run calculations are
not a matched end-to-end rerun and must not be presented as a release claim.

## Benchmark SC-EVAL-007: refresh-route maturation replication

Status: executed on 2026-09-22. The procedure below was committed before the
run and was not changed after observing the result.

Commit `88280d1f1d` changes the model-facing `context_map_refresh` result to
include a deterministic, response-bounded inventory of current source routes.
The tool description directs the model to use those routes directly and call
`context_map_query` only when the inventory is truncated or insufficient. The
app-server protocol remains unchanged, and projects with more routes than the
bounded response retain targeted query as the fallback.

This replication will copy the unchanged ten-file licensing corpus to another
fresh temporary directory, rebuild the branch CLI, remove API-key environment
variables, and run the exact SC-EVAL-004 maturation prompt with
`gpt-5.6-luna` at `xhigh` through cached ChatGPT authentication. It will compare
against SC-EVAL-006 and report whether the model uses refresh-returned routes,
context-map queries, shell listing or reads, exact evidence reads, model
responses, custom tool calls, full and uncached token usage, state quality,
persistence failures, and source edits.

The intended mechanism result is removal of the separate file-list and
context-map-query steps without weakening exact verification or durable state.
The result will be retained if the model ignores the routes or cost regresses.
Like SC-EVAL-006, this is a same-corpus mechanism replication rather than a new
held-out semantic or general-economics claim.

### Execution and result

Native Stateful CLI thread `01a0c93b-f307-7e12-856b-d242cacb5c1b` used fresh
project `01a0c93b-f2ef-7c00-bf49-710fdcd05d14` rooted at
`%LOCALAPPDATA%/Temp/stateful-refresh-routes-1790082503050`. The rebuilt branch
CLI ran `gpt-5.6-luna` at `xhigh` through cached ChatGPT authentication with
API-key environment variables removed. The temporary ten-file corpus was
byte-identical before the run, and a post-run directory diff confirmed that it
remained unchanged.

The first and only discovery call was `context_map_refresh`. It returned all ten
current routes with `routesTruncated: false`. The model explicitly used that
inventory to launch exact verification. It issued no `context_map_query`, shell
file listing, or shell content read. This removed the two context queries and
broad shell pass observed in SC-EVAL-006.

| Measure | SC-EVAL-006 | SC-EVAL-007 | Delta |
| --- | ---: | ---: | ---: |
| Model responses | 10 | 8 | -2 (-20.00%) |
| Outer custom tool calls | 9 | 7 | -2 (-22.22%) |
| Full tokens | 344,870 | 279,247 | -65,623 (-19.03%) |
| Uncached input plus output | 75,814 | 69,071 | -6,743 (-8.89%) |
| Context-map queries | 2 | 0 | -2 |
| Shell listing or content passes | 2 | 0 | -2 |
| Final blackboard findings | 17 | 17 | 0 |
| Final relationships | 27 | 29 | +2 |

The live frozen-manifest scorer again returned 17 of 17 active, current,
source-verified, evidence-linked findings, zero forbidden conclusions, and 3 of
10 literal probes. Manual review found all ten predeclared concepts. This
preserves the SC-EVAL-006 quality interpretation: perfect supported-entry
precision and complete manual concept coverage, but not a machine-scored
semantic-recall pass.

Two avoidable retries remain. The first parallel evidence call requested
`maxBytes: 100000`, above the declared 12,288-byte tool limit, and was repeated
with the legal limit. The linked batch then accepted 16 findings and 26
relationships but rejected one finding whose copied source fingerprint was
malformed; a single-record retry plus its three relationships produced the
final 17 findings and 29 relationships. An unrelated memory-maintenance patch
also failed after durable state was complete. These failures did not change the
fixture or final state, but their turns and tokens count against the result.

Using the already measured SC-EVAL-005 follow-up aggregate only as a directional
projection, the new maturation plus three Stateful follow-ups would cost
571,027 full tokens versus 428,784 ordinary tokens, a remaining 142,243-token
deficit. The observed follow-up slope projects full-token break-even around
question seven. Uncached usage would remain 159,123 versus 73,456, an
85,667-token deficit with no observed break-even. A matched end-to-end series
is still required before making a lifetime-economics claim.

## Benchmark SC-EVAL-008: resolved-evidence maturation replication

Status: executed on 2026-09-22. The procedure below was committed before the
run and was not changed after observing the result.

SC-EVAL-007 exposed two remaining model-correctable retries. Commit
`783bc5d4af` clamps a positive oversized `evidence_read.maxBytes` request to the
12,288-byte model-context boundary and reports the applied limit and clamp
state; zero remains invalid. Commit `c2708822c0` replaces model-authored source
fingerprints in blackboard writes with current route references. The tool
resolves and verifies the authoritative context-map entry and fingerprint. When
`nodeId` is omitted, evidence from exactly one source also places the finding on
that file node; cross-source findings remain project-wide and root promotion is
unchanged.

This replication will use another fresh byte-identical copy of the same
ten-file licensing corpus, rebuilt branch CLI, cached ChatGPT login, cleared
API-key variables, exact SC-EVAL-004 prompt, and `gpt-5.6-luna` at `xhigh`. It
will compare with SC-EVAL-007 and report refresh-route use, evidence-read
clamping or retries, blackboard batch success, source-route versus opaque-value
evidence inputs, file-level placement, model responses, outer tool calls, full
and uncached tokens, final state quality, and source edits.

The intended mechanism result is one refresh, one exact-evidence turn, one
obligation turn, one complete linked write, and one completion turn, with no
retry for byte limits or evidence identity. The result will be retained if the
model ignores the new contract or cost regresses. This remains a same-corpus
mechanism replication, not a new held-out semantic or general-economics claim.

### Execution and result

Native Stateful CLI thread `01a0c955-4d4c-7f81-a704-bcb568cc3407` used fresh
project `01a0c955-4d3d-7c83-94e8-eefa910dca54` rooted at
`%LOCALAPPDATA%/Temp/stateful-resolved-evidence-1790084192908`. The rebuilt
branch CLI ran `gpt-5.6-luna` at `xhigh` through cached ChatGPT authentication
with API-key environment variables removed. The ten-file corpus was
byte-identical to the committed fixture before the run, and a post-run
directory diff confirmed that it remained unchanged.

The route resolver and evidence clamp both worked in live model behavior. The
model requested `maxBytes: 30000`; the tool applied the 12,288-byte boundary
without an error or retry. In the successful linked batch, the model supplied
30 relative-path evidence references and no context-map entry IDs, source
fingerprints, or node IDs. The resolver persisted current authoritative
fingerprints, placed single-source findings on their file nodes, and retained
cross-source conclusions at the project node. Root promotion remained
independent.

| Measure | SC-EVAL-007 | SC-EVAL-008 | Delta |
| --- | ---: | ---: | ---: |
| Model responses | 8 | 11 | +3 (+37.50%) |
| Outer custom tool calls | 7 | 10 | +3 (+42.86%) |
| Full tokens | 279,247 | 417,784 | +138,537 (+49.61%) |
| Uncached input plus output | 69,071 | 75,256 | +6,185 (+8.95%) |
| Final blackboard findings | 17 | 13 | -4 |
| Final relationships | 29 | 11 | -18 |

This cost regression is retained. It does not come from failure of the two
mechanisms under test. The first linked batch was rejected because the model
naturally copied exact `lineRange` locators from `evidence_read`, while
blackboard evidence accepted source identity but could not persist line ranges.
The retry removed the ranges and successfully committed 12 findings and 11
relationships with zero item failures. A later single-record write added a
thirteenth source-verified question. The model had already marked the run
completed before that last write, so its second completion update failed with
the selected thread having no active Stateful run. No durable finding was lost,
but the stored terminal result may omit the final question.

All 13 final findings are active, current, source-verified, and evidence-linked.
Manual semantic review found all ten predeclared concepts and no forbidden
conclusion. The unchanged literal scorer matched 4 of 10 probes and returned
`passed: false`; its morphology and same-entry limitations remain visible rather
than being tuned after the result. The live hierarchy showed the policy
instruction and effective-date question on the policy file, the territory fact
on the side-letter file, and genuinely cross-source conclusions at project
scope.

The run also performed two global-memory reads required by the surrounding
Codex environment. They are not Stateful project-memory reads, but their turns
and tokens remain included. Relative to the original SC-EVAL-004 maturation,
SC-EVAL-008 still used 231,389 fewer full tokens (-35.64%) and 19,421 fewer
uncached input plus output tokens (-20.51%), but SC-EVAL-007 is the better
current result.

The next bounded slice is therefore exact line-range persistence, followed by
terminal-finalization hardening. Exact locators are part of trustworthy
provenance and should survive from source verification into durable memory;
they must not be discarded merely to avoid a retry. Completion must likewise
remain terminal without allowing a model to strand a valid final finding
outside the run's summarized result.

## Benchmark SC-EVAL-009: exact-provenance finalization replication

Status: executed on 2026-09-22. The procedure below was committed before the
run and was not changed after observing the result.

Commit `db88bf1784` preserves optional exact line ranges from model-authored
blackboard evidence through validation, SQLite revisions, query results, root
World State aliases, app-server v2, and browser evidence reads. It retains one
source link per finding and one bounded inclusive range per source. Existing
whole-source links remain valid. Focused storage, protocol, extension,
app-server, and browser-client tests pass.

SC-EVAL-008 also showed that a model can mark a run completed, persist a final
valid finding, and then attempt another completion update. Terminal runs must
not become generally mutable to accommodate that ordering error. Before this
replication, the active-run context and terminal tool contract will state that
all blackboard, relationship, obligation, and verification work must finish
before `completed`, and that completion is the final Stateful mutation.

The replication will use a fresh byte-identical copy of the same ten-file
licensing corpus, rebuilt branch CLI, cached ChatGPT login, cleared API-key
variables, exact SC-EVAL-004 prompt, and `gpt-5.6-luna` at `xhigh`. It will
report whether exact line ranges survive the linked batch, whether the batch
requires a retry, whether completion is the final Stateful mutation, model
responses, outer calls, full and uncached tokens, state quality, hierarchy
placement, and source edits.

The intended mechanism result is one successful linked write containing exact
line ranges and one final completion, with no evidence-shape retry and no
post-completion mutation attempt. The result will be retained if the model
still completes early or cost regresses. This is another same-corpus mechanism
replication, not a general economics claim.

### Execution and result

Native Stateful CLI thread `01a0c98a-e574-7912-ac68-9f55011304db` used fresh
project `01a0c98a-e567-7890-9711-3260f8fd1a0d` rooted at
`%LOCALAPPDATA%/Temp/stateful-exact-provenance-1790087681185`. The rebuilt
branch CLI ran `gpt-5.6-luna` at `xhigh` through cached ChatGPT authentication
with `OPENAI_API_KEY` and `CODEX_API_KEY` cleared. Filename and SHA-256
comparison established that the ten-file corpus was byte-identical to the
committed fixture before the run. The same comparison after the run found all
ten files unchanged.

The run used six outer code-mode calls: one context-map refresh combined with
the required global-memory lookup, one parallel exact-evidence read, one
semantic obligation update, one linked blackboard batch, one focused
blackboard-verification call, and one final call that wrote the verification
obligation before completing the run. Refresh returned all ten routes with no
truncation. The model issued no context-map query and no shell listing or
content read of the corpus. Its one parallel `evidence_read` call requested
`maxBytes: 50000`; the tool clamped each request to 12,288 bytes without an
error or retry and returned all ten complete sources exactly once.

The first and only linked write accepted all 13 findings and all 19
relationships with zero failures. The model supplied 30 relative-path evidence
links with bounded line ranges. Live API inspection after persistence found all
30 ranges intact, all 13 findings active and current, and all 13 findings
source-verified and evidence-linked. No evidence-shape retry, single-record
repair, malformed endpoint, or post-completion mutation occurred.

The terminal-ordering contract also changed live behavior as intended. The
last outer call first persisted the final semantic obligation and then invoked
`stateful_run_update(status: "completed")` sequentially. Completion succeeded
at run revision 2 and was the final Stateful mutation. The terminal result
therefore includes the same 13 findings, 19 relationships, explicit
uncertainties, and verification outcome that existed when the run became
inactive.

| Measure | SC-EVAL-008 | SC-EVAL-009 | Delta |
| --- | ---: | ---: | ---: |
| Model responses | 11 | 7 | -4 (-36.36%) |
| Outer custom tool calls | 10 | 6 | -4 (-40.00%) |
| Full tokens | 417,784 | 250,849 | -166,935 (-39.96%) |
| Uncached input plus output | 75,256 | 68,065 | -7,191 (-9.56%) |
| Final blackboard findings | 13 | 13 | 0 |
| Final relationships | 11 | 19 | +8 (+72.73%) |
| Evidence links with exact ranges | 0 | 30 | +30 |
| Failed or retried persistence calls | 2 | 0 | -2 |

Hierarchy placement remained intentionally generic. The two single-source risk
findings landed on the risk-assessment file node and the single-source territory
finding landed on the executed-side-letter file node. The ten findings that
synthesize multiple sources remained at project scope. No record was unplaced,
and root promotion remained independent of hierarchy placement.

### Rendered exact-evidence validation

A direct Edge 153 render against the live port-4174 gateway reopened the
completed SC-EVAL-009 project and exercised a persisted ranged-evidence control.
The first pass exposed a transparency mismatch: project status reported 13
understandings, while the findings surface silently filtered out the durable
`strategy` and `decision` kinds and displayed only 11 cards. The UI now renders
every blackboard kind under `Project understanding & open signals`; the reviewed
workspace fixture includes strategy and decision cards, and the browser-client
suite passes 8/8.

The second live render displayed all 13 understanding cards, including one
strategy and one decision, plus all 30 exact-range evidence controls. Clicking
`Open evidence · lines 5–11` opened only `risk-assessment.md` lines 5–11 and
displayed `408/455 bytes · lines 5–11`; the text began with the identifiable
diagnostic-record fact and ended with the unresolved coverage/exclusions
question. No UI or RPC error appeared. At both 1440 by 900 and 480 by 900, the
page had no horizontal overflow and retained the completed run, semantic
obligation, strategy, result, hierarchy, controls, exact evidence, and all
project-understanding cards. The inspected captures are
`%LOCALAPPDATA%/Temp/stateful-sc009-final-desktop.png` and
`%LOCALAPPDATA%/Temp/stateful-sc009-final-mobile.png`.

The unchanged frozen manifest found 13 of 13 currently supported entries,
zero forbidden conclusions, and 7 of 10 literal concept probes. Manual semantic
review found all ten predeclared concepts. The three literal misses remain
visible: the authority record says `supersede inconsistent` rather than the
manifest's `supersedes inconsistent`; the royalty concept is distributed across
the operative 6% finding and the linked stale-8% contradiction and uses numeric
notation; and the base-cap finding omits the literal `Section 7.3` label while
preserving the amount, aggregate-cap rule, executed carve-outs, and exact source
range. This establishes complete manual concept coverage and perfect
supported-entry precision, not a machine-scored semantic-recall pass.

After the successful `turn.completed` event and exit code 0, the CLI emitted a
single `UnknownProcessId` cleanup log for an already-finished command process.
It occurred after durable completion and did not change the rollout, project
state, corpus, or exit result, but it remains recorded as an operational cleanup
signal rather than being silently discarded.

SC-EVAL-009 closes the two specific failures exposed by SC-EVAL-008: exact
provenance now survives maturation, and completion is terminal without
stranding a later durable write. It is still a same-corpus mechanism
replication. It does not by itself establish general precision/recall,
maturation-inclusive lifetime savings, or release readiness.

## Benchmark SC-EVAL-010: current maturation-plus-follow-up series

Status: executed on 2026-09-22 after pre-registration.

This benchmark tests lifetime behavior using the completed SC-EVAL-009
maturation and the exact three follow-up cases already frozen in
`clients/stateful-codex/eval/manifests/licensing-series.json`. The maturation
rollout is
`%USERPROFILE%/.codex/sessions/2026/09/22/rollout-2026-09-22T10-35-18-01a0c98a-e574-7912-ac68-9f55011304db.jsonl`;
its observed 250,849 full tokens, 68,065 uncached input-plus-output tokens, and
7 model responses will be read from the rollout rather than replacing the old
SC-EVAL-005 cost embedded in the frozen manifest.

Each economics, territory, and termination-risk prompt will run once through a
fresh ordinary native CLI thread and once through a fresh Collaborative
Stateful native CLI thread explicitly bound to SC-EVAL-009 project
`01a0c98a-e567-7890-9711-3260f8fd1a0d`. Every pair will use the same rebuilt
branch binary, `gpt-5.6-luna` at `xhigh`, project directory, workspace roots,
permissions, cached ChatGPT login, and cleared API-key environment. The corpus
must remain byte-identical after all six runs.

The existing parity and frozen lexical checks will remain unchanged. The report
will preserve substantive manual review separately because prior runs establish
that literal morphology and same-entry requirements can reject correct answers.
It will report per-question and aggregate full and uncached tokens, model
responses, read-bearing calls, exact files opened, follow-up wins, actual
maturation-inclusive lifetime cost, and projected break-even only when average
savings are positive.

This benchmark can show whether the current mechanism repeats selective
retrieval and whether the measured one-time maturation cost can plausibly
amortize across this fixed workload. Three same-corpus questions are not a
general workload distribution; regressions, scorer failures, and negative
lifetime results will be retained.

### Execution result

All six fresh native CLI runs completed through cached ChatGPT login with the
pre-registered binary, model, effort, permissions, roots, and prompts. The
three Stateful runs were explicitly bound to SC-EVAL-009 project
`01a0c98a-e567-7890-9711-3260f8fd1a0d`. SHA-256 comparison after the series
found all ten working-corpus files byte-identical to the committed fixture.

| Case | Ordinary full | Stateful full | Ordinary uncached | Stateful uncached | Ordinary reads | Stateful reads |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| economics | 87,337 | 66,825 | 23,849 | 26,377 | 3 | 1 |
| territory | 101,767 | 111,343 | 21,127 | 28,399 | 3 | 3 |
| termination-risk | 119,196 | 70,288 | 20,380 | 28,816 | 5 | 1 |
| **Follow-up total** | **308,300** | **248,456** | **65,356** | **83,592** | **11** | **5** |

Stateful saved 59,844 full follow-up tokens (19.41%), four model responses,
and six read-bearing calls. It won full tokens on two of three questions. It
lost uncached input plus output on every question, increasing aggregate
uncached follow-up cost by 18,236 tokens (27.90%). Ordinary Codex broadly
searched or opened all ten project files in every case. Stateful verified four
exact files for economics, three for territory, and six for termination-risk;
territory included a repeated side-letter verification. The reduced call count
therefore represents materially more selective source access, although not the
ideal one-verification path in every case.

Including the measured SC-EVAL-009 maturation changes the comparison to 499,305
Stateful full tokens versus 308,300 ordinary tokens, a 191,005-token (61.95%)
regression. Uncached lifetime cost is 151,657 versus 65,356, an 86,301-token
(132.05%) regression. If the observed average full-token follow-up saving held,
the maturation investment would break even at approximately 13 questions.
There is no projected uncached break-even because every Stateful follow-up was
more expensive on that measure.

Manual answer review is more informative than the frozen literal scorer here.
Both economics answers covered all four intended concepts despite the scorer
missing the stale-proposal wording. Both territory answers covered all three
concepts despite a Stateful morphology miss. The ordinary termination answer
covered all four concepts. The Stateful termination answer correctly reported
60 days, the $5.5 million planning scenario, the $3 million limit, the $2.5
million gross difference, unresolved coverage, and the planning-versus-legal
distinction, but its final answer omitted the executed uncapped-liability
carve-out. That omission is substantive even though the durable root state and
semantic obligation contained the fact. No answer made a forbidden claim.

SC-EVAL-010 therefore demonstrates current-thread continuity, exact evidence
routing, and a substantial reduction in broad rereading and full follow-up
work. It does not close Stage 8. The one-time maturation cost is not amortized
by this short series, uncached cost regresses consistently, one required concept
was dropped between durable understanding and the final answer, and three
same-corpus questions are not a representative workload distribution. The next
release work should first explain and reduce the fixed uncached context cost
without weakening the always-loaded root, then test final-answer coverage from
selected durable findings, and only then run a broader pre-registered workload.

## Benchmark SC-EVAL-011: project-scoped prompt-cache replication

Status: passed on 2026-09-22 after pre-registration.

SC-EVAL-010 showed that every ordinary first response reused 9,984 cached input
tokens while every Stateful first response reused zero, including three
consecutive threads over the same mature project. Per-response traces localized
most of the uncached regression to this missing cross-thread prefix reuse rather
than to repeated source reads or the approximately 4,500-token root increment.

Commit `c0f120f009` introduces a mutable host-owned prompt-cache affinity. The
explicitly selected Stateful project supplies the affinity; thread and session
identity remain in request metadata, user project selection remains authoritative,
and changing or clearing that selection updates cache routing. Internal review
and ephemeral-fork overrides retain precedence. Focused tests verify the shared
attachment, live key changes, selection synchronization, and equal outbound
cache keys across two independent app-server threads.

The live replication will rebuild the native CLI from `c0f120f009`, clear API-key
environment variables, and use cached ChatGPT login with `gpt-5.6-luna` at
`xhigh`. It will submit the exact SC-EVAL-010 economics prompt twice in two fresh
Collaborative Stateful threads explicitly bound to mature project
`01a0c98a-e567-7890-9711-3260f8fd1a0d` and the byte-identical ten-file corpus.
No blackboard mutation or source edit is requested; each run may record its
semantic obligation and terminal result.

The first run is the cache warmer. The second run is the measurement. The report
will preserve each response's input, cached input, uncached input, output, total
tokens, calls, answer coverage, exact evidence access, and corpus SHA-256 state.
The mechanism passes only if the second thread's first response reports nonzero
cached input and lower uncached input than the warmer's first response while
retaining the correct evidence-grounded answer. A 12,000-token cached prefix and
at least 25% first-response uncached reduction are recorded as useful directional
thresholds, not release claims. A miss or regression will be retained. Two
same-project runs test cache routing only; they do not establish general lifetime
economics or close Stage 8.

### Execution result

The rebuilt CLI completed two fresh Collaborative Stateful threads through
cached ChatGPT login with cleared API-key variables. The warmer was thread
`01a0c9ca-c37b-72c1-b176-0bf5a5420c11`; the measurement was thread
`01a0c9cb-dc13-78a1-823e-b014ce67e8ce`. Both used the exact registered prompt,
project, corpus, model, effort, and permissions. Each answered all four intended
concepts, verified the same four exact source regions in one code-mode call,
wrote one semantic obligation followed by terminal completion in a second call,
and made no unsupported claim.

| Measure | Warmer | Measurement | Delta |
| --- | ---: | ---: | ---: |
| First-response input | 20,938 | 20,271 | -667 |
| First-response cached input | 0 | 12,032 | +12,032 |
| First-response uncached input | 20,938 | 8,239 | -12,699 (-60.65%) |
| Turn full tokens | 69,651 | 67,302 | -2,349 (-3.37%) |
| Turn uncached input plus output | 27,155 | 14,822 | -12,333 (-45.42%) |
| Model responses | 3 | 3 | 0 |
| Outer calls | 2 | 2 | 0 |

The measurement therefore passes both registered directional thresholds: it
reused 12,032 first-response tokens and reduced first-response uncached input by
more than 25%. The result also confirms that the cache win did not come from
less verification, fewer semantic writes, answer truncation, or source edits.
Post-run SHA-256 comparison found all ten corpus files byte-identical to the
committed fixture.

SC-EVAL-011 closes the concrete cross-thread cache-affinity defect exposed by
SC-EVAL-010. It supports the product model that threads are views over one
project and shows that a rich stable root can remain available without being
fully uncached on every adjacent thread. It does not retroactively change
SC-EVAL-010 or prove durable savings after provider cache expiry, long idle
periods, root revisions, different models, or representative workloads. Those
conditions require a fresh matched distribution. Stage 8 remains open, with the
final-answer coverage omission and broader lifetime replication still ahead.
