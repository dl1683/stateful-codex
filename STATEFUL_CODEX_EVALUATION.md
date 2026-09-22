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
