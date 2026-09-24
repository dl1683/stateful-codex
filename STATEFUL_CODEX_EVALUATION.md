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

## Benchmark SC-EVAL-012: final semantic coverage replication

Status: executed on 2026-09-22 after pre-registration; mechanism passed, final
prose passed, and the durable-result gate failed.

SC-EVAL-010's Stateful termination-risk answer omitted the executed
uncapped-liability carve-out even though the root blackboard contained it, the
model verified the controlling amendment, and the final semantic obligation
recorded uncapped exposure. The omission occurred at finalization: both the
persisted result and final prose dropped a material conclusion already present
in the system's structured understanding.

Commit `4fc80c460c` strengthens the domain-neutral completion contract. A run
cannot complete without a final semantic obligation. Completion returns a
bounded checklist drawn from that packet's learnings, implications,
uncertainties, and blockers, plus an explicit instruction to reconcile the
persisted result and final prose before replying. The checklist is capped at 16
items and 640 bytes per item. It does not infer legal concepts, rewrite user
goals, or add a second finalization turn.

The replication will rebuild the native CLI from `4fc80c460c` and submit the
exact SC-EVAL-010 termination-risk prompt in one fresh Collaborative thread
bound to mature project `01a0c98a-e567-7890-9711-3260f8fd1a0d`. It will use the
same byte-identical corpus, cached ChatGPT login, cleared API keys,
`gpt-5.6-luna` at `xhigh`, roots, and permissions.

The mechanism passes only if the completion tool output exposes the material
checklist and terminal completion remains the final Stateful mutation. The
product behavior passes only if both the persisted result and final prose state
that the executed amendment removes or carves data-security/confidentiality
exposure out of the master agreement's general liability cap, while preserving
the 60-day notice, $5.5 million planning scenario, $3 million identified cyber
limit, $2.5 million gross difference, unresolved coverage, planning-versus-legal
distinction, exact evidence, and read-only boundary. The corpus must remain
byte-identical. A lexical scorer is not sufficient; the rollout, persisted
state, and final answer will be reviewed directly. One same-corpus replication
tests the observed coverage failure only and does not close Stage 8.

### Execution result

The rebuilt cached-login CLI completed thread
`01a0c9d6-ae8c-7623-bd76-e3aed052708d` against the registered mature project,
corpus, prompt, model, effort, roots, and permissions. Its rollout is
`%USERPROFILE%/.codex/sessions/2026/09/22/rollout-2026-09-22T11-58-05-01a0c9d6-ae8c-7623-bd76-e3aed052708d.jsonl`.
Post-run SHA-256 comparison found all ten corpus files byte-identical to the
committed fixture.

The mechanism gate passed. The model wrote a final semantic obligation and then
made completion the final Stateful mutation. The completion output exposed 13
bounded checklist items with zero omitted items, including the 60-day notice,
the planning and insurance figures, uncertainty, and the binding policy's
uncapped-exposure requirement.

The final prose also passed the registered semantic gate. It expressly stated
that executed amendment 2 carves data-security exposure out of the master
agreement's general $2 million liability cap, while preserving the 60-day
notice, $5.5 million planning scenario, $3 million listed cyber limit, $2.5
million gross difference, unresolved coverage, planning-versus-contractual
distinction, exact file citations, and read-only boundary.

The durable result failed. Its narrative said only that the binding policy
requires uncapped or carved-out exposure to be identified. It did not state the
material project conclusion already present in the root blackboard: executed
amendment 2 actually removes the data-security, confidentiality, and IP
indemnity exposure from the master cap. The final obligation also preserved the
policy and coverage uncertainty but not that exact root conclusion. The
post-completion checklist could improve the later assistant prose, but it could
not alter the already-terminal result supplied as the completion argument.

The run used four model responses. It recorded 93,223 input tokens, including
65,792 cached input tokens, and 3,984 output tokens. This is a correctness
replication rather than an economics comparison. SC-EVAL-012 therefore closes
neither final semantic integrity nor Stage 8; it localizes the remaining defect
to the boundary between selected durable project knowledge and terminal result
assembly.

## Benchmark SC-EVAL-013: durable material-finding completion replication

Status: passed on 2026-09-22 after pre-registration, with one rejected opaque-
reference attempt retained as an operational efficiency finding.

Commit `0f7684a3f4` replaces the post-terminal reminder with a structured,
domain-neutral completion basis. Root entries now expose compact stable `K`
references in addition to ephemeral display aliases. A completed run must
explicitly submit the root references material to the requested outcome. Before
the run becomes terminal, the completion tool resolves those references against
the current project root, carries forward their verification and freshness,
adds exact source paths and line ranges when available, and appends them ahead
of the bounded final-obligation learning, implications, uncertainties, and
blockers in the durable result. Empty selection remains possible only as an
explicit assertion that no root finding is material; unknown, duplicate, or
oversized selections fail before completion.

The replication will rebuild the native CLI from `0f7684a3f4` and submit the
exact SC-EVAL-010 termination-risk prompt in one fresh Collaborative thread
bound to mature project `01a0c98a-e567-7890-9711-3260f8fd1a0d`. It will use the
same byte-identical ten-file corpus, cached ChatGPT login, cleared API keys,
`gpt-5.6-luna` at `xhigh`, roots, permissions, and read-only request.

The mechanism passes only if:

1. the root World State exposes stable `K` references;
2. the completion call selects the reference for the source-verified critical
   finding that executed amendment 2 removes data-security, confidentiality,
   and IP-indemnity exposure from the master cap;
3. the completion output returns that exact finding as a `rootFinding` with its
   current verification state and ranged source routes;
4. completion remains the final Stateful mutation; and
5. the ten corpus files remain byte-identical.

The product behavior passes only if both the persisted run result and final
assistant prose explicitly state the executed carve-out while preserving the
60-day notice, $5.5 million planning scenario, $3 million listed cyber limit,
$2.5 million gross difference, unresolved coverage, planning-versus-contractual
distinction, exact evidence, and read-only boundary. The result must contain a
bounded durable completion basis rather than a dump of the root blackboard.

The report will retain model responses, outer calls, per-response cached and
uncached usage, selected root references, final checklist size, persisted
result, final prose, mutation order, exact evidence access, and corpus hashes.
A lexical scorer is not sufficient. One same-corpus replication can close only
the observed finalization defect; it cannot establish representative
precision/recall, cache behavior after expiry or root changes, lifetime
economics, or Stage 8 release readiness.

### Execution result

The rebuilt cached-login CLI completed thread
`01a0c9f8-5fbb-7a23-ab17-dfaa26a73cfa`; its rollout is
`%USERPROFILE%/.codex/sessions/2026/09/22/rollout-2026-09-22T12-34-53-01a0c9f8-5fbb-7a23-ab17-dfaa26a73cfa.jsonl`.
It used the exact registered prompt, mature project, byte-identical corpus,
model, effort, roots, permissions, and cleared API-key environment. A real
gateway/API read after the turn confirmed run
`run-4d7df998518f144b9fe69e173051aa2912e76f41d6dd6a2521cbc4e016f7f908`
at terminal revision 2.

All registered correctness gates passed. The initial World State exposed stable
`K` references. The successful completion selected seven materially relevant
root findings, including the source-verified critical carve-out record. The
tool returned those findings first in the 16-item completion checklist with
current verification and exact ranged routes; four lower-priority final-packet
items were reported as omitted from the bounded checklist and remained in the
structured obligation. Completion was the final Stateful mutation. SHA-256
comparison found all ten corpus files unchanged.

The persisted result now expressly records that the master agreement's $2
million aggregate cap does not govern data-security, confidentiality, and
IP-indemnity obligations under executed amendment 2. It includes the 60-day
notice, superseded 30-day baseline, $5.5 million planning scenario, $3 million
listed cyber limit, $2.5 million gross difference, unresolved coverage,
planning-versus-contractual distinction, no-edit boundary, and exact source
ranges. The final assistant prose independently preserves the same substantive
conclusions and exact file citations. This closes the specific durable-result
failure from SC-EVAL-012.

The run used five model responses and four outer custom-tool calls: one
non-project memory lookup, one five-file exact-evidence batch, one combined
final-obligation/failed-completion call, and one successful completion retry.
It consumed 118,455 input tokens, including 89,088 cached input tokens, and
3,767 output tokens: 122,222 full tokens and 33,134 uncached input plus output.

The failed attempt is material negative evidence about the mechanism's
ergonomics. The model submitted two hexadecimal references that were not in the
rendered root. Completion rejected the first unknown reference before changing
run state, after which the model copied seven valid references and succeeded.
Fail-closed behavior is correct, but opaque reference copying caused an
avoidable response and approximately 3,022 uncached input-plus-output tokens in
the retry response. The next narrow slice should retain explicit model
selection while replacing opaque references with compact display aliases bound
to an explicit root revision. That gives the tool an optimistic-concurrency
check without asking the model to reproduce hashes.

SC-EVAL-013 proves durable semantic carry-through for the observed failure. It
does not establish representative final-answer recall or lifetime economics,
and the opaque-reference retry should be removed before the broader matched
distribution.

## Benchmark SC-EVAL-014: revision-bound completion alias replication

Status: passed on 2026-09-22 after pre-registration.

Commit `4efb44bcab` removes the opaque `K` handles exposed by SC-EVAL-013.
Completion now uses the compact `E` aliases already present in the root
blackboard plus the project-intelligence revision shown beside them. The tool
resolves aliases only against that exact revision and rejects a changed root,
malformed alias, duplicate selection, or out-of-range alias before terminal
mutation. New knowledge created during the current run remains covered by the
automatically appended final semantic packet; if root ordering changes, the
model must review the new revision rather than silently binding an old alias to
a different finding.

The replication will rebuild the native CLI from `4efb44bcab` and submit the
exact termination-risk prompt again in one fresh Collaborative thread bound to
mature project `01a0c98a-e567-7890-9711-3260f8fd1a0d`. It will use the same
byte-identical ten-file corpus, cached ChatGPT login, cleared API keys,
`gpt-5.6-luna` at `xhigh`, roots, permissions, and read-only request.

The mechanism passes only if the initial root exposes its revision and `E`
aliases without opaque completion handles; the completion call supplies that
exact `rootRevision`, selects the alias containing the executed liability
carve-out, succeeds without an alias/reference retry, returns the selected
current source-verified finding with ranged routes, and remains the final
Stateful mutation. The corpus must remain byte-identical.

The product behavior passes only if both the persisted run result and final
assistant prose preserve the executed carve-out, 60-day notice, $5.5 million
planning scenario, $3 million listed cyber limit, $2.5 million gross
difference, unresolved coverage, planning-versus-contractual distinction,
exact evidence, and read-only boundary. The report will retain every model
response and outer call, the selected revision and aliases, persisted API
result, final prose, mutation order, per-response cached and uncached usage,
and corpus hashes.

This replication tests whether the new addressing contract removes the exact
copy failure observed in SC-EVAL-013 without weakening durable result coverage.
It does not establish behavior after a concurrent root revision, representative
precision/recall, lifetime economics, or Stage 8 release readiness.

### Execution result

The rebuilt cached-login CLI completed thread
`01a0ca0b-28a8-7dd2-91b7-730d7c1c54f2`; its rollout is
`%USERPROFILE%/.codex/sessions/2026/09/22/rollout-2026-09-22T12-55-24-01a0ca0b-28a8-7dd2-91b7-730d7c1c54f2.jsonl`.
It used the registered prompt, mature project, byte-identical ten-file corpus,
model, effort, roots, permissions, and cleared API-key environment. The cached
ChatGPT login supplied authentication.

All registered mechanism gates passed. The initial root exposed revision 54 and
compact `E` aliases without `K` handles. Completion supplied `rootRevision: 54`
and selected `E1`, `E6`, `E8`, `E9`, `E12`, and `E13`; it succeeded on the first
attempt, returned all six current source-verified root findings with exact
ranged routes, and remained the final Stateful mutation. The rollout contains
no opaque `K` reference. SHA-256 comparison found all ten corpus files
byte-identical after the run.

The product-behavior gates also passed. Both final prose and the persisted
result preserve the 60-day notice, superseded 30-day baseline, $5.5 million
planning scenario, $3 million listed cyber limit, $2.5 million gross difference,
the executed amendment's data-security carve-out, pending coverage and counsel
review, the planning-versus-contractual distinction, exact evidence, and the
read-only boundary. A real gateway/API read confirmed run
`run-2b309747a13c75404f697c262114b81fa63fc847e1de1432334ef244a817170e`
at terminal revision 2 with a 5,016-character durable result. The completion
checklist contained 15 items and omitted none.

The run used four model responses and three outer custom-tool calls: one
four-file exact-evidence batch, one final semantic obligation update, and one
successful completion. It consumed 94,549 input tokens, including 66,816 cached
input tokens, and 2,496 output tokens: 97,045 full tokens and 30,229 uncached
input plus output. Relative to SC-EVAL-013, that is 25,177 fewer full tokens
(20.60%), 2,905 fewer uncached input-plus-output tokens (8.77%), one fewer model
response, one fewer outer call, and no completion retry. It also avoided a raw
read of `binding-review-policy.md` because the verified root carried the needed
relationship and route.

SC-EVAL-014 closes the model-facing completion-addressing defect without losing
durable semantic coverage. It remains one same-corpus replication. A broader
pre-registered matched distribution, representative precision and recall,
maturation-inclusive economics, concurrent-root behavior, and the approval-
gated repository-wide Rust suite remain open Stage 8 evidence.

## Benchmark SC-EVAL-015: two-project release distribution

Status: stopped after the first maturation run exposed a completion-contract
retry on 2026-09-22; no matched outcome case was run.

The frozen manifest is
`clients/stateful-codex/eval/manifests/release-distribution.json`. It contains
six outcome questions across the independently matured procurement and
licensing projects: binding vendor viability, authority and cost, the verified
continuity basis, current licensing economics, Canadian territory, and
termination/data-security risk. Each case fixes its prompt, expected semantic
concepts, prohibited conclusions, project identity, and requirement that the
Stateful completion result plus returned completion basis preserve the same
material coverage as the visible answer.

Each fixture will be copied to a fresh temporary directory and hashed before
execution. One Collaborative Stateful maturation run per project will use the
exact manifest prompt, then each case will run as a fresh ordinary thread and a
fresh Stateful thread attached to that project's durable intelligence. Every
pair must use the same rebuilt branch binary, cached ChatGPT login with
`OPENAI_API_KEY` and `CODEX_API_KEY` removed, model, reasoning effort, prompt,
working directory, roots, approval policy, sandbox policy, and permission
profile. Codex's separate user-memory feature will be disabled in both
conditions because it already contains notes about these fixtures and would
leak expected answers into the benchmark; Stateful project intelligence remains
enabled only for the Stateful condition. Pair order will alternate which
condition runs first. The two actual
maturation rollouts are mandatory inputs to the aggregate scorer; a missing
project maturation is an error rather than a zero-cost default.

The correctness gate requires all twelve visible answers to contain every
registered concept and no prohibited conclusion. Each of the six Stateful runs
must also complete once without a completion retry, expose a successful durable
completion record in the rollout, and pass the same semantic checks across its
submitted result and returned completion basis. A live API read must separately
confirm that terminal revision and persisted result; the rollout reconstruction
is not treated as authoritative storage evidence. Both fixture copies must
remain byte-identical.

The retrieval gate requires Stateful to use fewer aggregate read-bearing calls
and fewer unique raw project files than ordinary Codex without weakening exact
source citations. The economics gate requires lower aggregate full follow-up
tokens and will report uncached follow-up usage, both measured maturation runs,
total lifetime delta, win counts, and projected break-even without hiding a
regression. No uncached or maturation-inclusive advantage will be claimed
unless the measured totals actually establish it.

This distribution is deliberately broader than a same-question replication but
is still two small synthetic knowledge corpora. It can close the observed
cross-case durable-coverage and short-distribution gates. It cannot alone prove
large-corpus scaling, code-editing workloads, multi-day cache behavior,
concurrent root revisions, or general production readiness.

### Execution result

The fresh procurement maturation completed thread
`01a0ca24-35fc-7513-9ec0-6d422e1cae46` and persisted 13 source-verified
findings plus 14 relationships across all eight files. It correctly captured
the conjunctive gates, authority order, Alder and Birch failures, Cedar's sole
apparent eligibility, chronology, costs, and unresolved final-award question.
The source copy was byte-identical before execution, and the final answer
reported no source edits.

The completion path needed three attempts. First, the model selected all 13
root aliases even though the tool cap is eight. After correctly reducing the
selection, it copied project-intelligence revision 45 into both `rootRevision`
and the unrelated `expectedRevision`; the run correctly rejected that call
because its revision was 1. The third call used `expectedRevision: 1`,
`rootRevision: 45`, and eight aliases and completed successfully at run revision
2. The run consumed 453,078 full tokens and 68,054 uncached input plus output.

This is a product-contract failure rather than evidence about the matched
distribution. The rendered root said to select every material alias despite the
eight-item bound, and the two nearby revision numbers were not explicitly
distinguished at the call site. No ordinary or follow-up Stateful cases were
run. SC-EVAL-015 is retained as negative evidence and is not used for release
economics.

Commit `6f0f03c981` makes the contract self-consistent. The run World State now
renders the exact current `expectedRevision` and says not to substitute project
intelligence revision. The root footer and tool schema say to select at most
eight highest-priority directly material aliases and preserve additional
conclusions in the final semantic obligation. Focused extension tests pass 6/6,
the scoped lint and repository formatting completed, and the branch CLI was
rebuilt. The unchanged code-mode companion rebuild remains blocked by the known
external Windows V8 archive download; the existing matching binary is retained.

## Benchmark SC-EVAL-016: disambiguated two-project distribution

Status: stopped after both maturation runs exposed a separate evidence-schema
contradiction on 2026-09-22; no matched outcome case was run.

SC-EVAL-016 repeats the exact frozen SC-EVAL-015 manifest and protocol from
fresh byte-identical procurement and licensing directories using the rebuilt
CLI from `6f0f03c981`. The same two maturation prompts, six matched questions,
condition-order alternation, disabled ordinary memory, authentication, model,
effort, roots, permissions, correctness gates, live persisted-result checks,
and economics accounting apply without modification.

The added operational gate is explicit: each maturation and Stateful follow-up
must select no more than eight aliases, must keep run `expectedRevision`
distinct from project `rootRevision`, and must complete on its first terminal
call. Any retry is retained as a failure and its cost remains in the rollout.
Fresh directories and project identities prevent the successful SC-EVAL-015
maturation state from entering this replication.

### Execution result

The repaired completion contract passed in both fresh projects. Procurement
thread `01a0ca30-f52a-71b2-b6ca-3e52e7c1d9dc` selected exactly eight aliases,
kept run revision 1 distinct from project revision 41, and completed on its
first terminal call. It persisted 14 source-verified findings and nine
relationships. The run used six model responses, five outer calls, 159,212 full
tokens, and 65,004 uncached input plus output. Relative to SC-EVAL-015's failed
preflight, full usage fell 64.86% and the two completion retries disappeared.

Licensing thread `01a0ca36-26b2-7ef3-9117-b51c4b9af46b` also completed on its
first terminal call with the correct run/root revisions and eight aliases. It
persisted 12 source-verified findings and 13 relationships, used seven model
responses and six outer calls, and consumed 207,649 full tokens plus 58,657
uncached input and output. Both fresh corpora remained byte-identical.

The licensing run exposed a different pre-completion retry. Its first atomic
batch supplied both `contextMapEntryId` and `relativePath` for each evidence
item. Runtime correctly rejected the entire batch because exactly one route
identity is valid, but the published JSON schema used `anyOf`, which allowed an
item containing both alternatives. The second batch used only context-map IDs
and succeeded. Continuing the matched cases would mix a known avoidable
maturation cost into the release distribution, so execution stopped before any
ordinary or follow-up run.

Commit `22290f0b98` replaces the evidence schema's `anyOf` with `oneOf` and
states the mutual exclusion on the array and both identity fields. Focused
extension tests pass 6/6, the scoped lint and repository formatting completed,
and the branch CLI was rebuilt. The evaluator was also corrected in
`d9a3a970c1` to recognize the nested completion output produced when the final
obligation and terminal update are correctly grouped in one code-mode call; its
11-test suite passes and reconstructs both live completion records.

## Benchmark SC-EVAL-017: exclusive-evidence two-project distribution

Status: completed on 2026-09-22; mechanism and semantic-correctness review
passed, retrieval improved, and maturation-inclusive economics failed.

SC-EVAL-017 repeats the exact frozen six-case manifest and all SC-EVAL-016
conditions from two new byte-identical project directories using the rebuilt
CLI from `22290f0b98`. Neither prior project's durable intelligence will be
reused. In addition to the existing completion gate, each maturation must
persist its coherent evidence-linked batch without retrying an invalid evidence
identity. Any failure or retry remains in the rollout and stops execution before
the matched cases. If both maturations pass, all six ordinary/Stateful pairs
will run under the already frozen order and gates.

### Execution result

Both fresh maturation preflights passed the repaired contracts without an
invalid evidence identity or terminal-call retry. Procurement thread
`01a0ca3e-3bf3-7ad1-b99e-90b2b78ef2d9` persisted eight findings and nine
relationships in run
`run-6d5c68a0f1e9ecd3788f58983a232299470658112e0124c3aaa869e60f38e695`;
licensing thread `01a0ca41-cee0-7402-b78f-8a925d3b80bd` persisted fourteen
findings and ten relationships in run
`run-86ae359bc9dc850cdde43ab432a1080bbfdbf82d964dfd586eae359e22c67a1e`.
Each completed on its first terminal call. Together they consumed 447,702 full
tokens, 114,134 uncached input-plus-output tokens, and fifteen model responses.
The licensing maturation also discovered a cross-source closing gap: the
executed side letter requires both Canadian regulatory consent and licensor
written acknowledgement, while the closing checklist explicitly tracks only
the consent.

All twelve frozen matched runs completed. Every Stateful follow-up completed on
its first terminal call, and no Stateful rollout contains a failed script or
terminal mutation. A fresh real-gateway read independently confirmed all six
Stateful runs as `completed` at revision 2 with nonempty persisted results.
Post-run SHA-256 comparison found no difference between either temporary corpus
and its source fixture: all eight procurement files and all ten licensing files
remained byte-identical.

The unchanged literal scorer reports the distribution as failed. It passes all
three licensing pairs and the visible procurement continuity pair, but rejects
semantically correct procurement wording such as "the only vendor shown to pass
every binding deployment criterion" because the frozen alternatives require
"only viable" or "sole viable." It similarly rejects complete statements about
all three sub-cap totals and preliminary, non-final committee notes because the
registered phrases are narrower. The durable continuity result says Cedar
"meets the continuity gate" with 120 hours and 24 hours of headroom rather than
using the scorer's exact "satisfies" alternative. These misses are retained
unchanged as evidence about evaluator brittleness; the manifest was not tuned
after seeing the answers. Independent semantic review finds all twelve visible
answers and all six Stateful durable results correct, complete for the prompts,
properly caveated, and source-grounded.

Retrieval improved materially. Ordinary Codex used 20 read-bearing outer calls;
Stateful used seven, a 65% reduction. Manual call-input inspection shows ordinary
Codex reopened all project files in every case, totaling 54 per-question unique
raw-file reads, while Stateful opened 29, a 46.30% reduction. Model responses
fell from 26 to 21 (19.23%). Stateful follow-ups used 77,806 uncached
input-plus-output tokens versus 97,693 for ordinary Codex, a 20.36% reduction,
and won five of six pairs on that measure.

The full-token and lifetime gates failed. Stateful follow-ups used 430,830 full
tokens versus 381,853 for ordinary Codex, a 12.83% increase, and won only one of
six pairs. Including the two required maturations raises Stateful lifetime use
to 878,532 full tokens and 191,940 uncached tokens, respectively 496,679
(130.07%) and 94,247 (96.47%) above the ordinary series. The observed uncached
slope projects break-even only around question 35; the observed full-token slope
has no break-even. Procurement follow-ups account for the full-token regression,
while the licensing cases show one substantial full-token win and two near-ties.

SC-EVAL-017 therefore establishes the intended continuity, durable semantic
integrity, decisive-detail preservation, selective routing, exact-source
verification, and read-only behavior across this small distribution. It does
not establish lower full lifetime token use or release readiness. The next
optimization must explain and reduce procurement's repeated cached-context and
completion overhead without deleting the rich root knowledge that enabled the
correct cross-source answers. A future preregistered replication also needs a
semantic evaluator or human rubric fixed before execution; post-hoc expansion
of this frozen lexical scorer would invalidate the present result.

## Benchmark SC-EVAL-018: single-call completion and outcome-bounded verification

Status: pre-registered before implementation on 2026-09-22.

Response-by-response inspection of SC-EVAL-017 localizes the procurement
regression. The authority case spent four model responses because it published
its final obligation and completion in separate inference rounds. The continuity
case spent five responses because it first verified the four decisive continuity
sources, then opened two adjacent residency and price sources, and only afterward
grouped final persistence. The completion call also returned 5,842-7,151
characters of checklist output, but the avoidable 20,000-plus-token model rounds
are the dominant cost.

The implementation under test will make the final semantic obligation part of
the terminal `stateful_run_update` input. Intermediate `obligation_update` calls
remain available when learning or strategy materially changes during longer
work, but one terminal tool operation must express the final packet, result, root
revision, and material root aliases. The selected-project guidance will also say
that exact verification is bounded by the requested outcome: current verified
root knowledge may supply adjacent context, but the model should not reopen
sources merely to prove gates that the question does not ask it to decide.

The replication will use the unchanged authority-and-cost and continuity-basis
prompts from
`clients/stateful-codex/eval/manifests/release-distribution.json`. Each runs in a
fresh ordinary thread and a fresh Stateful thread attached to the mature
SC-EVAL-017 procurement project. Both conditions use the same rebuilt branch
binary, cached ChatGPT login with both API-key variables removed, disabled
ordinary Codex memory, model, effort, roots, permissions, and alternating order.
The already measured maturation cost remains reported but is not repeated
because this experiment isolates follow-up completion and verification behavior.

The mechanism gate requires each Stateful run to use one exact-evidence batch,
one terminal update containing its final obligation, no separate final
`obligation_update`, one successful completion attempt, at most three model
responses, a completed revision-2 API result, and no source edit. The semantic
gate is a human rubric frozen here: authority must preserve all three approved
totals, the controlling order, the conjunctive/non-waivable rule, and the
preliminary-not-final distinction; continuity must preserve the verified
88/91/94/96-hour history, North Ridge, Birch's 72-hour/24-hour consequence,
Cedar's 120-hour claim and ordering preservation, the verified-history versus
vendor-claim distinction, and no final-award implication.

The efficiency gate requires fewer full and uncached tokens than the
corresponding SC-EVAL-017 Stateful runs, whose aggregate was 189,003 full and
32,587 uncached input-plus-output tokens. The result will also be compared with
the ordinary SC-EVAL-017 aggregate of 114,445 full and 34,573 uncached tokens,
but this two-case diagnostic does not replace a fresh full release distribution
or establish maturation-inclusive release economics.

### Execution result

The single-call completion mechanism passed in both Stateful runs. Authority
thread `01a0ca7a-4d3d-71f3-9123-6fa1be61bbe9` used one five-file evidence batch
and one terminal `stateful_run_update` carrying `finalObligation`; continuity
thread `01a0ca7b-b2a1-72e3-b15d-3edf978b1a45` used two evidence batches and the
same single terminal operation. Neither used a separate final
`obligation_update`, both completed on their first attempt, and live API reads
confirmed revision-2 completed results of 7,923 and 5,466 characters. No
Stateful rollout contained a failure marker, and all eight source files remained
byte-identical.

The authority run met the three-response target. Its first response received no
cached input, unlike the preceding project-affine runs, so it consumed 64,248
full and 28,920 uncached tokens. The continuity run still opened finance and
security after its first decisive four-file continuity batch because the model
chose to establish all three viability gates. It therefore used four responses,
80,045 full tokens, and 10,413 uncached tokens. The exact-outcome guidance did
not eliminate that adjacent verification round, so the one-batch/three-response
mechanism gate failed for continuity.

Together the two Stateful runs used 144,293 full tokens, 44,710 fewer than
SC-EVAL-017's 189,003, a 23.66% improvement. Responses fell from nine to seven.
The zero-cache authority start raised aggregate uncached usage to 39,333, 20.70%
above the previous 32,587 despite continuity's improvement. The same-binary
ordinary replications used 136,346 full and 28,058 uncached tokens; Stateful was
5.83% higher on full tokens and 40.18% higher on uncached tokens in this live
pair. The ordinary authority run also emitted a non-fatal missing collaboration-
thread host log after producing its correct answer; that environmental event is
retained and not attributed to Stateful.

Both visible answers and durable results preserve the registered substantive
facts without an unsupported award. A narrower final-assembly defect remains:
both submitted terminal narratives explicitly said the committee recorded no
final approval or that Cedar's status was viability rather than final approval,
while the later visible prose reduced that to provisional/current-evidence
wording. The completion checklist did not prevent this small but consequential
caveat loss. SC-EVAL-018 therefore passes the single-call completion change and
full-token improvement target, but fails its complete mechanism gate and its
uncached target. It is not a release-economics result.

## Benchmark SC-EVAL-019: submitted-result final-answer fidelity

Status: pre-registered before implementation on 2026-09-22.

The completion tool will return the model's own concise submitted terminal
narrative as a named `submittedResult`, in addition to the bounded checklist.
Its instruction will require the visible final answer to preserve that narrative
without dropping or weakening any conclusion, caveat, uncertainty, or blocker;
formatting and exact-source links may improve, but semantic compression may not
remove material content. This does not change the durable result, root
blackboard, obligation packet, or evidence policy.

The exact SC-EVAL-018 authority and continuity prompts will each run once in a
fresh Stateful thread on the same mature procurement project and rebuilt branch
binary. Authentication, disabled ordinary memory, model, effort, working
directory, roots, permissions, and no-edit boundary remain unchanged. No
ordinary replication is needed because the change affects only post-completion
Stateful answer assembly.

The mechanism gate requires one successful terminal call carrying
`finalObligation`, a returned `submittedResult`, no separate final obligation
call, a completed revision-2 API result, and a byte-identical corpus. The
semantic gate requires the visible answer to preserve every material conclusion
in `submittedResult`, including all three authoritative totals and non-waiver
order for authority, the complete 96/72/24/120-hour distinction for continuity,
verified history versus vendor claims, and the explicit statement that viability
is not a recorded final approval. Any new unsupported conclusion or weakened
caveat fails the run. Token use and response count will be reported, but this is
a correctness replication rather than an economics claim.

### Execution result

SC-EVAL-019 passed every registered mechanism and semantic gate. Authority
thread `01a0ca86-4356-7403-82de-3701e6c14f87` used one evidence batch and one
terminal call; continuity thread `01a0ca87-6f61-7da1-a1ba-f5c2a1d25499` used
two evidence batches and one terminal call. Both completion outputs contained
the named `submittedResult`, neither used a separate final obligation, both
completed on the first attempt, and neither contained a failure marker. Live
API reads confirmed runs
`run-84a3c6207976ad707a7b82bb0d4276c5efcb1fd56dbf9954228f1b53c624a3a0`
and
`run-8d73434252388fb04cf8a2d356eaa36fbaf89e3572ab541817b2aa9e5416697f`
as completed at revision 2. Their persisted results contain 6,742 and 5,474
characters, and the corpus remained byte-identical.

The visible authority answer preserved all three totals, the controlling order,
the conjunctive/non-waivable rule, Alder and Birch's distinct failures, Cedar's
current evidence status, and the explicit statement that no final approval is
recorded. The visible continuity answer preserved the verified 88/91/94/96-hour
history, North Ridge, Birch's 72-hour queue and 24-hour loss, Cedar's 120-hour
claim and ordering preservation, the verified-history/vendor-claim distinction,
and the explicit statement that Cedar's viability is not a recorded final
approval. Neither added an unsupported award or certainty claim.

The runs used seven responses, 145,312 full tokens, and 24,224 uncached
input-plus-output tokens. Relative to SC-EVAL-018, full usage rose 1,019 tokens
(0.71%) while uncached usage fell 15,109 tokens (38.42%) because both initial
requests reused the project cache. Against SC-EVAL-018's same-binary ordinary
pair, Stateful used 6.58% more full tokens but 13.66% fewer uncached tokens. The
final-answer fidelity defect is closed. The remaining full-token difference is
largely the continuity run's second evidence/inference round, not completion
persistence or final-answer repair.

## Benchmark SC-EVAL-020: bounded-outcome continuity replication

Status: pre-registered before implementation on 2026-09-22.

The selected-project guidance will make outcome scope explicit: do not turn a
question about one criterion or decision dimension into an overall project
determination. Verify and conclude the requested dimension; use current
source-verified root knowledge only as labelled adjacent context, and state when
broader viability or approval is outside the evidence review rather than opening
unrequested sources to re-prove it. This remains domain-neutral and does not
restrict the model when the user's requested outcome actually requires all
dimensions.

The unchanged continuity-basis prompt will run in one fresh Stateful thread on
the same mature procurement project and rebuilt branch binary, with the same
authentication, disabled ordinary memory, model, effort, directory, roots,
permissions, and no-edit boundary. It will be compared with SC-EVAL-019
continuity thread `01a0ca87-6f61-7da1-a1ba-f5c2a1d25499` and the same-binary
ordinary continuity thread `01a0ca7c-e83d-7322-8333-f02a40622fdb`.

The mechanism gate requires one evidence batch limited to
`binding-criteria.md`, `operations-log.md`, `vendor-birch.md`, and
`vendor-cedar.md`; one terminal call with `finalObligation` and
`submittedResult`; at most three model responses; one completion attempt; a
completed revision-2 API result; and an unchanged corpus. The semantic gate
requires all SC-EVAL-019 continuity facts and caveats while stating that the
answer decides the continuity dimension, not overall approval. Opening finance,
security, committee, or Alder sources fails the bounded-outcome gate even if the
answer remains correct. Full and uncached usage must be below SC-EVAL-019's
84,033 and 13,377 tokens; comparison with the ordinary 78,593 full and 14,337
uncached tokens remains diagnostic rather than a release-distribution claim.

### Execution result

The bounded-outcome behavior passed. Thread
`01a0ca8e-d9f8-7ca3-9ec6-ac8b351debd7` opened exactly the four registered
continuity sources in one evidence batch, did not open finance, security,
committee, or Alder material, and concluded only the offline-acceptance
dimension. Its visible answer preserved the complete verified-history and vendor-
claim distinction, Birch's 24-hour operational loss, Cedar's 24-hour stated
margin, and the explicit boundary that this was not final approval across all
gates. It returned `submittedResult`, completed once, contained no failure
marker, and left the corpus byte-identical. A live API read confirmed run
`run-fe5316cfad34b5ecdfce74f6d000a721f93b79cdd1962b68e864ec9e93acb1f9`
completed at revision 2 with a 4,442-character result.

The complete mechanism and economics gates did not pass. After the decisive
evidence batch, the model used a separate `obligation_update` containing all of
its final learning and implications but no `next`, blocker, or requested user
judgment, then immediately sent the terminal call with substantially the same
packet. That added a fourth response. The run used 81,003 full and 15,467
uncached tokens: 3.61% fewer full tokens but 15.62% more uncached tokens than
SC-EVAL-019 continuity, and 3.07% more full plus 7.88% more uncached than the
same-binary ordinary run. SC-EVAL-020 closes the adjacent-source behavior defect
but exposes a redundant semantic-persistence round.

## Benchmark SC-EVAL-021: substantive intermediate-obligation gate

Status: pre-registered before implementation on 2026-09-22.

An intermediate `obligation_update` will require evidence of substantive work
remaining: at least one nonempty `next`, `blockers`, or `requestedJudgment`
field. Its schema and tool description will tell the model that answer drafting,
formatting, and terminal persistence are not substantive next work. When the
learning packet is final and only the answer remains, the model must place that
packet directly in `stateful_run_update.finalObligation`. Runtime validation
will reject an empty-future intermediate packet before persistence. This keeps
meaningful real-time transparency for longer investigations while eliminating a
duplicate update at the end of a short task.

The exact SC-EVAL-020 continuity prompt will run once in a fresh Stateful thread
on the same mature project and rebuilt branch binary under the unchanged model,
effort, authentication, memory, directory, roots, permissions, and no-edit
conditions. The mechanism gate requires one four-file evidence batch, no
intermediate obligation call or rejected retry, one terminal call, three model
responses, one completion attempt, a revision-2 API result, and an unchanged
corpus. The SC-EVAL-020 semantic rubric remains unchanged. Full usage must be
below both SC-EVAL-020's 81,003 tokens and the same-binary ordinary run's 78,593;
uncached usage must be below SC-EVAL-020's 15,467 and will be compared with the
ordinary 14,337 without assuming stable provider cache behavior.

### Execution result

SC-EVAL-021 preserved the correct retrieval and answer boundary but failed the
registered workflow and efficiency gates. Thread
`01a0ca9f-9c48-7c42-bf59-c760f4af492f` opened exactly the four registered
continuity sources in one evidence batch and no others. Its visible answer and
durable result preserve the verified 88/91/94/96-hour history, North Ridge,
Birch's 72-hour queue and 24-hour intake loss, Cedar's 120-hour claim and
timestamp/order statement, the verified-history versus vendor-claim
distinction, and the boundary that this is not final procurement approval. A
live API read confirmed run
`run-f4453bd9e75fae9a5a0df210bc0376a8793c523e1ec215710bc9730e5a5a93d5`
completed at revision 2 with a 4,478-character result. All eight corpus files
remain byte-identical to the fixture.

The model attempted an intermediate packet whose sole field was scalar `next`.
The conditional schema had repeated only `minItems` inside its `anyOf` branch;
the code-mode tool surface consequently did not preserve the base array shape,
and deserialization rejected the string before persistence. The retry supplied
the expected list and persisted an obligation containing only a plan to
synthesize and compare evidence that had already been reviewed. It then
completed in the following response. The run therefore used five responses,
one rejected intermediate call, one recorded intermediate call, and one
successful completion. It consumed 102,833 full tokens and 27,057 uncached
input-plus-output tokens, respectively 26.95% and 74.93% above SC-EVAL-020 and
30.84% and 88.72% above the same-binary ordinary run.

This result shows that the first gate was structurally too weak and its
conditional schema was unnecessarily fragile. A forward-looking sentence alone
is not a meaningful semantic update, and synthesizing already-reviewed evidence
into the answer is not additional project work. The failed attempt and retry
remain in the rollout; SC-EVAL-021 does not pass.

## Benchmark SC-EVAL-022: meaningful intermediate-obligation remediation

Status: pre-registered before implementation on 2026-09-22.

The intermediate tool will retain the ordinary array schema for every packet
field instead of layering partial property definitions through `anyOf`.
Runtime validation will require both (a) a forward signal in `next`, `blockers`,
or `requestedJudgment` and (b) meaningful semantic content in `rationale`,
`learning`, `implication`, `strategy`, `changed`, `uncertainty`, `blockers`, or
`requestedJudgment`. A packet containing only `next` will be rejected before
persistence. Model guidance will state that summarizing, synthesizing, or
comparing evidence already reviewed for the current answer is final reasoning,
not substantive remaining work. Short tasks should complete directly; longer
investigations retain intermediate transparency when evidence, strategy,
uncertainty, blockers, or user judgment materially change while more work
remains.

The exact continuity prompt will run once more in a fresh Stateful thread on
the same mature project and rebuilt binary under the unchanged model, effort,
authentication, memory, directory, roots, permissions, and no-edit conditions.
The mechanism gate requires one four-file evidence batch, no intermediate
obligation call or rejected retry, one terminal call, three model responses,
one completion attempt, a revision-2 API result, and an unchanged corpus. The
semantic rubric remains unchanged. Full usage must be below SC-EVAL-020's
81,003 and the ordinary run's 78,593 tokens; uncached usage must be below
SC-EVAL-020's 15,467 and will be compared with ordinary's 14,337.

### Execution result

SC-EVAL-022 removed the malformed scalar retry but failed the registered
retrieval, workflow, response-count, and economics gates. Thread
`01a0cab8-0dff-7c32-afd4-927ea33683b8` first opened the four registered
continuity sources, then opened `finance-schedule.md` and
`security-addendum.md` to make an unrequested all-gate viability determination.
It persisted an intermediate packet whose stated remaining work was to complete
the answer, then completed in the following response. There was no rejected
tool call, and the final obligation and completed run now committed through the
same storage transaction.

The visible answer and durable result are substantively correct and preserve
the verified outage history, North Ridge, the 24-hour Birch loss, Cedar's
claimed 120-hour capacity and ordering preservation, the verified-history
versus vendor-claim distinction, and the no-final-approval caveat. A live API
read confirmed run
`run-3e2303305a5733db09daa1d7642895b73f64e3e543e90971d64a6a5e185ddde7`
completed at revision 2 with a 5,340-character result. The eight source files
remain byte-identical to the fixture.

The run used five model responses, two evidence batches, one intermediate
obligation, one successful completion, 109,325 full tokens, and 31,501 uncached
input-plus-output tokens. That is 34.96% more full and 103.66% more uncached
than SC-EVAL-020, and 39.10% more full plus 119.72% more uncached than the
same-binary ordinary run. SC-EVAL-022 does not pass.

This procurement continuity prompt has now served as a repeated development
case. Further prompt-specific policy tuning would risk overfitting while still
being unable to validate semantic novelty structurally. The case is frozen
here. The next evaluation work moves to diverse projects and measures whether
intermediate updates are useful as an outcome, rather than adding another
presence-rule proxy.

## Longitudinal evaluation program

Status: design and tooling phase begins after SC-EVAL-022; no comparative
project result is pre-registered yet.

The next experiment follows the user-directed ladder while preventing
selection and accounting bias:

1. Curate six diverse projects and publish all six breadth results. Five
   longitudinal projects will be selected by domain and workload coverage
   before comparative results are observed; the sixth is a reserved replication
   project, not a pool from which only winners are chosen.
2. Run matched continuous ordinary and Stateful threads from empty state over
   20 sequential questions on each of the five projects. Both arms receive the
   same ordered work and native history; cross-thread transfer is a separate
   experiment.
3. Record per-turn rather than cumulative-session usage, actual compaction
   events, source regions and repeat reads, retries, wall time, state revisions,
   injected root size, maintenance cost, state freshness, and cumulative cost
   from question one.
4. Include tasks that genuinely cross compaction boundaries, a thread restart,
   source revisions that invalidate prior conclusions, cross-source deductions,
   contradictions, and questions whose supported answer is unknown.
5. Freeze semantic obligations and a blinded evidence rubric before execution.
   Literal term matching remains diagnostic only. Visible answers and durable
   state are scored separately for correctness, decisive-detail coverage,
   unsupported claims, uncertainty calibration, contradiction handling, and
   stale-state repair.

The existing one-rollout-per-question series comparator cannot measure this
design because cumulative usage would be double-counted in continued threads.
A turn-aware evaluator is therefore a prerequisite, not post-hoc analysis.
Published external Codex or Luna scores remain contextual unless model,
benchmark version, harness, budget, retry policy, and scoring are demonstrably
comparable.

### Turn-aware evaluator checkpoint

The prerequisite evaluator is now implemented at
`clients/stateful-codex/eval/compare-longitudinal-rollouts.mjs`. It consumes one
continuous rollout per arm, attributes response usage through persisted turn
IDs, reconciles the response sum with the recorded turn total, and reports
per-turn plus cumulative cost without re-summing cumulative thread usage. It
also records actual compaction checkpoints, latency, read operations, rejected
tool results, exact `evidence_read` regions, completion attempts, and the
model-visible Stateful project/root projection size, revision, omissions, and
freshness at each turn.

The evaluator deliberately does not infer semantic quality, complete source
access, or deep-state size from rollout prose. Frozen case-keyed observation
files provide blinded rubric scores, an audited source ledger, and post-turn
state counts. Missing required evidence invalidates the comparison. Literal
term checks remain explicitly diagnostic. Multi-project aggregation reports
project win counts and labels projects—not questions—as the clustered units.

The complete frozen method and observation shape are in
`clients/stateful-codex/eval/LONGITUDINAL_PROTOCOL.md`; a non-registered shape
example is in `eval/manifests/longitudinal-template.json`. The synthetic focused
tests pass, and the parser was also exercised against an existing five-turn
rollout containing a canonical compaction. That live historical file correctly
surfaced four incomplete/superseded turns rather than silently treating them as
valid observations. No six-project result has been run or claimed yet.

## Benchmark SC-EVAL-023: six-project breadth screen

Status: pre-registered before any model arm was run on 2026-09-22.

The cohort is selected by workload coverage rather than expected outcome:

- AGI Thesis: technical-thesis and evidence synthesis;
- Latent-Space-Reasoning: long-running empirical research;
- Open Exploration: publication research and buyer evidence;
- Iqidis: production TypeScript application analysis;
- new-computation-model: mathematical and theoretical-computer-science research;
- memory-benchmark-harness: reserved replication in agent-memory evaluation
  software.

The first five are the preselected longitudinal cohort if the breadth screen is
operationally valid. The sixth is reported as a reserved replication rather
than substituted for an unfavorable result. Procurement is excluded because it
is the frozen development case. The exact source selections and one breadth
question per project are frozen in `eval/manifests/breadth-projects.json`.

Each arm receives an isolated copy of the same `rg --files`-visible current
corpus, with ignored caches, build outputs, credentials, prior `.blackboard`,
and prior `.codex` state excluded. The AGI corpus is intentionally bounded to
the root thesis controls plus `publication` and `reviews`; its multi-gigabyte
raw experiment store is outside this screen. The six pre-run corpus hashes and
file counts are frozen in `breadth-snapshot-hashes.json`. The preparation tool
verifies byte identity between arms, and the runner rehashes the corpus before
and after every turn.

Both arms use the branch debug binary, cached ChatGPT login with API-key
environment variables removed, Luna at high reasoning effort, read-only source
permissions, and one continuous thread per project. Stateful uses explicit
Autonomous mode. A run is operationally valid only if it completes, preserves
the corpus, exposes project state, commits its durable result, and yields the
required blinded quality, source-audit, and state observations. No result is
recorded yet.

### Pre-cohort isolation correction

The first AGI pair was an infrastructure dry run, not an SC-EVAL-023 result.
The arms completed against identical unchanged corpora, but the Stateful stderr
showed an attempted read of the host memory registry. Removing API-key
environment variables had preserved cached ChatGPT login as intended, but had
not disabled Codex's separate memory subsystem. The ordinary arm used
1,158,270 total tokens and 132,734 uncached-input-plus-output tokens; the
Stateful arm used 1,394,703 and 137,743 respectively, with 18 versus 10
read-bearing operations. Those values remain an operational diagnostic only.
They are excluded from the registered cohort because project-specific prior
memory could replace work that either experimental condition was meant to do.

The following Latent-Space-Reasoning ordinary arm was interrupted as soon as
the contamination path was confirmed, before any outcome was accepted. The
runner now requires an explicit disabled-memory policy and passes both
`memories.use_memories=false` and `memories.generate_memories=false` while
retaining the cached ChatGPT login. The registered cohort will restart from
fresh snapshot paths so the completed dry-run Stateful project identity and
state cannot carry forward. Every final result will therefore be collected
after this correction; no favorable or unfavorable completed result was
selected for inclusion.

## Benchmarks SC-EVAL-024 and SC-EVAL-025: source revision and compaction diagnostics

Status: mechanism diagnostics completed on 2026-09-23. Neither cohort is a
protocol-valid comparative result because the required independent quality,
source-audit, and state observations have not yet been supplied. Both arms,
failed infrastructure attempts, and the two distinct compaction regimes remain
preserved; one regime is not substituted for the other.

SC-EVAL-024 used a 20,000-token total-context limit. Its final clean cohort is
under `%LOCALAPPDATA%/Temp/stateful-source-canary-run-final-8421d388`, with the
matched snapshot under `stateful-source-canary-snapshot-final-8421d388`.
Ordinary thread `01a0cd05-8c21-7990-93a1-5e6727e82259` and Stateful thread
`01a0cd05-8a7b-7603-8adc-f7055ac91c10` both answered all four diagnostic cases
correctly. Stateful detected the changed policy, replaced the current threshold
of 10 with 6, changed the decision from permitted to not permitted at the
unchanged measured count of 8, and retained the old conclusion and source
fingerprint as superseded history. Four separate terminal run/state artifacts
were captured on the one continued thread.

The total-context stress economics failed decisively. Ordinary Codex used
229,285 total tokens, 41,637 uncached input plus output tokens, 15 model
responses, one canonical compaction, and 179,756 ms. Stateful used 833,145 total
tokens, 229,241 uncached input plus output tokens, 39 model responses, 16
canonical compactions, and 1,007,707 ms. Canonical reconciliation separates
Stateful's usage into 449,138 productive-inference tokens across 23 responses
and 384,007 compaction tokens across 16 responses; the additional compaction
cost explains about 60.3% of the 603,860-token gap. The fixed Stateful prefix
began near the artificial limit, so post-compaction requests repeatedly reached
the same total-context threshold. This is valid extreme-stress evidence, not a
claim that the approximately 2.5 KiB root blackboard intrinsically causes that
cost.

That cohort exposed and preserved three correctness limitations. First, runs
started on resumed threads claimed an Autonomous continuation against the
previous run's last turn before the new explicit question arrived. Second, the
initial project fragment after the hidden file replacement still labelled the
old promoted evidence current; model-driven context-map refresh discovered and
repaired it only after inference began. Third, the v1 launch finding cited
`policy.md` lines 3-5 although its decisive threshold appeared on line 6. The
answer was correct because the model had read the full file, but that persisted
locator was not semantically sufficient. Historical entries also remain
current-query-inaccessible, and their original source bytes are not retained
after overwrite; the successful historical answer was helped by v2 restating
the old threshold.

Commit `c65993a196` binds Autonomous continuation eligibility to a turn that
actually started while the same run was active. The resumed-exec integration
test rejects an automatic-continuation instruction in the new explicit prompt.
Commit `aae58b6512` makes new longitudinal run states pin their exact rollout
path, and graders verify its session ID instead of guessing from a shared
directory.

SC-EVAL-025 then used the same frozen sources, questions, model, effort, login,
memory isolation, and source intervention with a 20,000-token
`body_after_prefix` limit. Its clean artifacts are under
`%LOCALAPPDATA%/Temp/stateful-source-growth-run-897af904` and
`stateful-source-growth-snapshot-897af904`. Ordinary thread
`01a0cd2a-7895-7bf2-bfaf-5378018974ee` used 1,002,847 total tokens, 83,039
uncached input plus output tokens, 39 responses, and 557,834 ms. Stateful thread
`01a0cd2a-766c-7b33-9e9a-9453429cd40b` used 507,855 total tokens, 62,415
uncached input plus output tokens, 17 responses, and 209,877 ms: reductions of
49.36%, 24.84%, 56.41%, and 62.38%, respectively. Both arms answered all four
cases correctly. Every Stateful question created its own completed run on the
same thread, every state artifact reports zero continuations used, and both run
states pin their exact dated rollout.

Neither SC-EVAL-025 arm compacted. It therefore isolates a useful economics and
lifecycle improvement but does not prove equal-pressure compaction continuity.
A separately frozen lower incremental-growth threshold is needed if that
specific mechanism is retested. Production-default economics also remain a
separate lane. Automated raw-read counts from these Windows rollouts are lower
bounds because the parser does not yet expand looped `type`, `find`, and
`findstr` commands into per-file reads; no read-saving claim is made from those
counts.

## Benchmark SC-EVAL-026: explicit historical-state retrieval

Status: matched mechanism diagnostic completed on 2026-09-23. It is not a
protocol-valid comparative result because blinded quality, independent source
audits, and state observations were not collected before arm identity was
revealed. The case named `post-compaction-recall` also observed zero compactions
in both arms, so this run does not establish post-compaction recall.

The frozen v2 source removed every mention of the old threshold. The clean
artifacts are under
`%LOCALAPPDATA%/Temp/stateful-history-run-20260923b`; ordinary thread
`01a0cd7a-c1cb-7a42-8e28-b40463edce51` and Stateful thread
`01a0cd75-5ad7-7f83-90cf-a2aec12ade18` each completed all four turns on their
first attempt. Both arms returned the correct v1 and v2 decisions in every
turn. Stateful persisted the old 10-defect policy and permitted decision as
superseded revisions, kept the current 6-defect policy and not-permitted
decision active, and preserved the unchanged measured count of 8.

The reconcile turn provides direct mechanism evidence. Its rollout contains
`blackboard_query({entryScope: "historical", ...})`; the response returns the
superseded v1 entries, correct old source fingerprint
`79647a43c5c02d1c26d56b5ccbac4287f70f7afb62e6697e86941603d65d01ed`, and
threshold 10 with stale historical verification. The model then correctly
distinguishes that history from the current 6-defect source. This closes the
prior defect where a correct history answer could be inferred from v2 restating
v1. It does not prove counterfactual dependence on the query, because native
same-thread conversation history was also present.

The economics failed:

| Measure | Ordinary | Stateful | Stateful change |
| --- | ---: | ---: | ---: |
| Full lifetime tokens | 134,154 | 992,033 | +857,879 (+639.47%) |
| Uncached input + output | 26,890 | 84,257 | +57,367 (+213.34%) |
| Model responses | 9 | 27 | +18 |
| Recorded read operations | 7 | 13 | +6 |
| Rejected tool results | 0 | 2 | +2 |
| Canonical compactions | 0 | 0 | 0 |
| Turn duration | 132,442 ms | 317,816 ms | +185,374 ms |

The excess work is not attributable to compaction in this run. The trace shows
a malformed v1 persistence call, one correct stale-source rejection followed by
refresh, a v2 relationship failure that required another mutation, and repeated
semantic persistence rounds. The last turn is directionally better on marginal
uncached usage—5,260 Stateful versus 9,541 ordinary—but the four-turn
maturation-inclusive lifetime remains a large regression.

Three integrity defects remain visible. First, the reconcile completion copied
the correct fingerprint incorrectly as the invented hybrid
`79647ad1f8e25747262bade3cba32bbd2b85f1db12c9c2cf00d607f71e36561d`;
the authoritative stored evidence links still contain the correct fingerprint.
Second, Stateful answer links use `/C:/...` and are not valid Windows paths even
though their prose and line ranges are substantively correct. Third, the runner
retained only a hash of each turn's corpus; after intervention its grading
packet would have shown v2 while grading the v1 answer. Commit `462b5a25c1`
fixes future runs by preserving one content-addressed corpus artifact per
distinct turn revision and producing a separate blinded packet for every
question. SC-EVAL-026 remains honestly ungraded rather than reconstructing
missing observations after seeing the arms.

## External custom-harness benchmark ladder

Status: primary-source review completed on 2026-09-23; no public submission,
maintainer contact, or external benchmark run has been performed.

The relevant unit is an agent-model pair. Published model-only scores from an
organizer-controlled scaffold cannot measure Stateful Codex. The external
program therefore uses the same built Codex binary and `gpt-5.6-luna` setting
with Stateful disabled and enabled, changing only the intentional project-state
layer.

1. Terminal-Bench 2.1 is the strongest immediate local harness comparison. Its
   89-task public dataset and Harbor runner accept arbitrary custom agents, and
   its published table includes Codex CLI. GPT-5.3-Codex scores 79.1% with
   Codex CLI and 68.5% with Terminus 2, directly demonstrating that the harness
   changes the outcome. Official community submissions are currently closed;
   public Harbor uploads are shareable evidence but do not create an official
   leaderboard row. The full published protocol requires five trials for every
   task, or 445 trials per arm.
2. SWE-bench Verified and Multilingual currently provide the open publication
   route. Stateful Codex generates patches; the official harness grades them;
   `swebench submit package`, `publish`, `register`, and `verify` produce a
   public artifact repository and registration pull request. Metadata records
   the agent separately from the model. The default `swebench infer` command
   uses mini-SWE-agent and must not replace our custom agent execution.
3. SWE-bench Pro V2 is a useful locked local protocol and adapter reference,
   but it was released on 2026-09-22 with 642 tasks and must not be compared to
   older v1 results. Its repository includes a pinned Codex adapter and
   fresh-sandbox patch regrading. An open custom-agent leaderboard admission
   path has not been verified.
4. SWE-Marathon v1.1 is the highest-relevance long-horizon supplement. Its 20
   tasks run through Harbor and its public scripts name Codex with
   `gpt-5.6-luna` at high effort. A declared configuration is not a measured
   score, and no open official submission process has been verified.
5. DeepSWE v1.1 can run custom harnesses locally, but its official leaderboard
   standardizes on mini-SWE-agent. Its published Luna result is therefore not a
   Stateful-versus-Codex-harness baseline.

The next implementation is one version-pinned Harbor installed-agent adapter,
not one script per benchmark. It must install a hashed Stateful bundle, isolate
project state per task and trial, preserve state only within that trial, emit
schema-valid ATIF trajectories, retain all-attempt token and latency costs, and
capture the final patch even on failure. A three-to-five-task smoke precedes
any frozen full run. Official claims require the complete benchmark and its
canonical limits; pilot results remain engineering diagnostics.

## Benchmark SC-EVAL-027: Terminal-Bench `fix-git` technical control

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

## Benchmark SC-EVAL-028: Terminal-Bench four-task Stateful smoke

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

SC-EVAL-029, the proposed product-specific concurrent project-family smoke, was
retired before it produced a result. One procurement turn completed correctly
but hit an over-narrow state-evidence assertion; one source-change turn
completed before the orchestration process interrupted its second turn; and
the first licensing turn was interrupted. Those partial records remain outside
Git as negative harness evidence and are not repaired, scored, or described as
Terminal-Bench. Terminal-Bench 2.1 defines 89 independent tasks rather than
cross-task longitudinal families, so no relationship between its tasks will be
inferred for evaluation.

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

## Benchmark SC-EVAL-030: feedback-conditioned persistent-state pilot

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

## Benchmark SC-EVAL-031: BixBench custom-harness gate

Status: pre-registered on 2026-09-23 before any BixBench model call.

This evaluation tests whether the Stateful Codex harness can perform auditable
scientific-data work under BixBench v1.5. It does not test longitudinal memory:
every scored question receives a fresh workspace and fresh Stateful databases.
Any later same-capsule persistent-state study is a separately labelled product
experiment and cannot be merged into this result.

The protocol pins the official BixBench repository, dataset revision, capsule
archive hashes, and FutureHouse scientific-environment source revision. The
published environment image is ARM64-only, so this AMD64 host rebuilds its
unchanged pinned Dockerfile and records the resulting image ID plus the complete
Conda/Pip inventory. The agent sees only the single capsule `Data` directory;
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

There is one attempt per task. A detected provider/authentication outage or a
failure before the Codex invocation boundary is invalid and may be repeated
once in the same arm. Agent timeout, nonzero agent exit, missing/malformed final
answer, or prohibited benchmark-source access is a recorded failure. Tasks are
never substituted after outcomes are known.

The smoke is operationally valid only if it retains a parseable structured
answer, a structurally valid submitted notebook, an offline network-disabled
replay of that notebook, intact protocol audit, complete usage/artifact hashes,
both integrity-valid Stateful databases, and a terminal `completed` run. Local
answer correctness is reported independently; an incorrect but operationally
valid smoke triggers diagnosis before expansion rather than being relabelled as
infrastructure failure.

Primary reported measures are local metadata-verifier correctness, notebook
reproducibility, durable run completion, full and uncached tokens, wall time,
and persisted hierarchy/context-map/blackboard/relationship/obligation counts.
Exact/range metadata checks are diagnostic and are not called official
BixBench scores because current official open-answer postprocessing uses an LLM
judge. Official-shaped records are retained for later scoring under a separately
pinned judge protocol. Published BixBench aggregates may provide context but
are not treated as matched controls. An ordinary Codex arm is not required for
this operational smoke; one may be added only as an explicitly labelled
technical control if a failure cannot otherwise be localized.
