# Stateful Codex clean-build plan

Status: implementation contract for `feature/stateful-codex`, based on
`origin/main` at `48f897e4ce94152818ce0563b3a8aac3a70ce9b8`.

Read `STATEFUL_CODEX_PRODUCT_INTENT.md` first. Product behavior in that document
outranks an implementation shortcut in this plan.

## Decisions

1. `Project` is the durable user-selected scope. Current Codex already persists
   project IDs, ordered filesystem roots, thread membership, and v2 project APIs.
   Stateful Codex extends that identity; it does not create a competing workspace
   identity or infer a project from a prompt, thread, or recent activity.
2. Project intelligence is keyed by project ID. Threads are views. Starting,
   resuming, and forking a thread must attach the same project intelligence when
   the user selects the same project.
3. Blackboard and context map are separate record types and query paths. They may
   share a physical database when that keeps transactions and operations simple,
   but they must not collapse into one semantic object. A blackboard record
   contains understanding. A context-map entry contains source location,
   revision, and a retrieval description. Neither substitutes for exact source.
4. Model-visible project state enters through typed World State contributions.
   It is incrementally diffed, persisted with rollout state, bounded, and stable
   across compaction. Do not append an opaque prompt packet each turn.
5. The root blackboard is always loaded for a selected project. Deeper state is
   retrieved by query and relevance, followed by context-map lookup and exact
   source verification when needed.
6. Autonomous, Collaborative, and Socratic are explicit Stateful workflow modes.
   They are not inferred from text. They configure lower-level Codex facilities
   but retain their own durable identity and behavior.
7. Obligation updates are typed semantic records and server notifications. The
   UI does not reconstruct progress, evidence, readiness, or steering state by
   parsing assistant prose or tool-call text.
8. The app-server v2 API is the product boundary. New API is experimental until
   the end-to-end contract stabilizes. No v1 surface is added.
9. The web client consumes the same API as other clients. It contains no private
   truth and must distinguish observed, inferred, stale, failed, and verified
   state.
10. The frozen prototype is not an implementation source or compatibility
    target. Its code, schemas, APIs, terminology, and UI data are not
    transplanted. They may be consulted only as non-authoritative evidence about
    failed approaches, useful scenarios, or product questions; any resulting
    design is re-derived from the product intent and current main.

## Clean-rebuild boundary

The sources of authority, in order, are:

1. `STATEFUL_CODEX_PRODUCT_INTENT.md` for the user problem and intended product;
2. current Codex behavior and abstractions on the clean `origin/main` base;
3. repository engineering rules and verified live behavior; and
4. this plan as a revisable implementation contract.

The old charter, handoff, master review, `workspace-state` crate,
`stateful-workspace` extension, workspace APIs, migrations, client, and demo data
remain on the frozen prototype branch only. They do not establish requirements
for this branch. There is no promise to read or migrate prototype databases.

Prior work may contribute a test scenario, observed failure, benchmark method,
or interaction idea only when the new implementation records why it serves the
product intent and validates it against real current APIs. Similarity by itself
is not reuse justification.

This rule applies especially to the blackboard. Previous blackboard schemas and
projection machinery may reveal questions worth testing, but they do not define
the new entry model, promotion policy, maintenance process, or UI. Those must be
introduced in minimal vertical slices that prove reduced rereading, continuity,
source routing, and useful discovery.

## Live implementation checkpoint

As of 2026-09-21, the clean branch contains the complete first product slice
through the native CLI and local browser client:

- canonical project identity follows new, resumed, and forked thread views and
  enters model context through a bounded typed World State contribution;
- `codex-project-intelligence` persists the filesystem-shaped hierarchy,
  semantically separate context map, structured blackboard, evidence links,
  relationships, promotion state, revisions, freshness, and exact-source reads;
- the Stateful extension exposes bounded retrieval and mutation tools, semantic
  obligation publication, steering consumption, workflow-mode policy, and an
  autonomous idle supervisor with lease-based continuation recovery;
- experimental app-server v2 APIs own project intelligence, runs, obligations,
  steering, evidence, mode changes, pause/resume/cancel, and revisioned events;
- `codex --stateful <mode> <goal>` uses the selected working directory, cached
  ChatGPT login, durable project intelligence, and native obligation/run cards;
  and
- `clients/stateful-codex` provides explicit project, thread, and mode selection
  plus hierarchy, findings, source routing, exact evidence, semantic progress,
  steering, controls, recovery, and final-result views against the real API.

Focused native validation passes: the branch CLI builds, five Stateful TUI
tests and snapshots pass, and two real cached-login CLI runs completed against
`clients/stateful-codex/public` without source edits. The runs reused accumulated
project intelligence across threads and produced durable semantic obligations;
their thread IDs are `01a0c5d8-d37a-71c3-9904-8561c286afe8` and
`01a0c5e2-1a12-7603-85fa-08f138dcc93c`.

A fresh post-remediation CLI run, `01a0c64c-e23b-7333-beec-32603b4d8eb0`,
also completed through cached ChatGPT login with the exact selected-directory
answer. Stateful model tools are now bound to the selected thread's active run;
the model no longer receives or supplies opaque run IDs, and this run completed
without the malformed-ID retry observed during diagnosis. Focused extension and
app-server tests cover the same active-run contract.

Focused browser validation also passes against the real gateway, app-server,
branch CLI, and cached ChatGPT login. A rendered Edge/CDP run demonstrated
continue, fork, the Socratic execution gate, exact evidence inspection,
autonomous pause/resume, and reload recovery. Its final machine-readable result
and screenshots are under
`%LOCALAPPDATA%/Temp/stateful-client-live-1790028346984`. The live exercise found
and fixed three client defects: an invalid context-search limit, stale
thread-scoped session keys that reopened the wrong run, and an unsupported fresh
history call surfacing as a user-visible error. The client test suite passes
5/5 after those fixes and the rollout-comparison coverage. A fresh rendered run
against the rebuilt CLI and code-mode companion created thread
`01a0c646-d8f7-7571-b398-e77f423b429d`, visibly recovered from the known Windows
host-shell failure through its approval UI, and displayed the correct obligation
and completed result. Current screenshots are under
`%LOCALAPPDATA%/Temp/stateful-client-smoke-1790032608507`.

These results establish a usable end-to-end product slice, not the Stage 8
release claim. A rollout comparison tool and the first matched ordinary versus
Stateful CLI benchmark now exist; `STATEFUL_CODEX_EVALUATION.md` records the
method and result. Both runs were correct and Stateful reduced read-bearing tool
calls from two to one, but it increased full lifetime tokens by 22.28% and
uncached input plus output by 45.26% on the trivial inventory task. That negative
result is preserved rather than presented as a product win. The post-remediation
rerun fixed active-run tool reliability but remained negative: full lifetime
tokens were 86.34% above the ordinary baseline and uncached input plus output
were 37.19% above it. Representative
evaluation of knowledge precision/recall, mature-workspace rereading, decisive
detail discovery, steering latency, autonomous recovery, and evidence
correctness remains open. The repository-wide Rust suite also remains an
explicit approval-gated validation step. On this Windows installation, some
model-issued shell commands fail before execution because the configured
Windows Store `pwsh.exe` returns access denied; Stateful recovery and non-shell
tools continue, but that host-shell problem must not be reported as successful
command execution.

The pre-registered decisive-detail benchmark SC-EVAL-002 has now also completed
through matched native CLI runs. Both ordinary and fresh-thread Stateful Codex
returned the correct, exact-source-grounded Cedar decision, and Stateful reused
project memory before verification. It nevertheless reopened the same eight raw
files as ordinary Codex and increased full lifetime tokens from 72,891 to
237,459 (+225.77%) and uncached input plus output from 18,363 to 43,411
(+136.40%). The preceding maturation run consumed another 994,169 tokens. This
is positive evidence for continuity, decisive-detail preservation, and
traceability, but a failed Stage 8 gate for reduced rereading and lifetime cost.
The next implementation slice must make exact verification selective and reduce
semantic-persistence round trips; the branch must not be called product-complete
until a rerun demonstrates those outcomes or the remaining limitation is made
an explicit release boundary.

That remediation slice is now implemented and measured. Root entries carry
current exact-source routes; coherent findings can be written through a bounded
16-record batch call with per-item results; and compact semantic root rendering
uses short aliases rather than spending the model-visible budget on repeated
opaque identifiers. The compact root exposed the complete gate matrix and every
route in 6,456 bytes instead of filling the 24 KiB cap and truncating decisive
content. Focused extension and app-server integration tests pass.

The matched native CLI rerun `01a0c7e3-6569-71e3-a526-3430f1b82fd7` used the
cached Codex ChatGPT login with API-key environment variables removed. It
reduced the prior Stateful result from 237,459 to 149,360 lifetime tokens and
from 43,411 to 36,720 uncached input plus output, while preserving the correct
source-grounded answer. It still reopened all eight files and remains roughly
twice the ordinary run's token cost. The rendering and round-trip work is a
measured improvement, not closure of the Stage 8 efficiency gate. The batch
path has integration evidence but still needs a fresh maturation-cost benchmark;
and selective rereading should be evaluated on a larger corpus where the
question's decisive source set is materially smaller than the corpus.

### 2026-09-22 CLI and client validation checkpoint

The noninteractive `codex exec` path originally accepted the root `--stateful`
option but did not propagate it into exec startup. The apparent Stateful run was
therefore ordinary Codex behavior. The branch now shares one project/run startup
implementation between TUI and exec, propagates root and exec-local Stateful
selection, attaches the chosen project before `thread/start`, and creates the
durable run before the first model request. Focused app-server-client, exec, TUI,
and CLI tests passed before the required lint/format pass.

A fresh cached-login exec run, `01a0c83a-c55b-75d0-89fa-8ab8b9cbebd5`, was
launched with `OPENAI_API_KEY` and `CODEX_API_KEY` removed. Its rollout contains
the selected Stateful project and run, used bounded `evidence_read` line ranges
instead of shell/file reads, queried steering, published a semantic obligation,
completed the run, returned the correct Cedar decision, and made no source edits.
It consumed 119,230 lifetime tokens, including 83,200 cached input tokens; its
uncached input plus output was 36,030. This is a live mechanism check, not a
matched benchmark or an efficiency claim.

The real browser gateway was also exercised on a separate loopback port with the
fresh branch CLI and cached ChatGPT login. Thread
`01a0c841-34c6-7d31-a76a-68da3373edaf` received the mature root blackboard,
explicitly judged deeper search unnecessary, and verified only exact routed line
ranges before completing. A second client-path run,
`01a0c851-0974-74f2-92a6-db2ba4c9f3e6`, proved live steering: the instruction
advanced from submitted to applied, the run strategy revision advanced from 0
to 1, and the requested viability-versus-final-award distinction appeared in the
durable strategy, obligation, and result.

That exercise found one trust defect: the API accepted steering after a run had
completed, leaving an instruction that could never be applied. The runtime now
rejects new steering on terminal runs, and the web client replaces terminal
steering, mode, maintenance, and instruction controls with an explicit path to a
new outcome while retaining the completed record. The focused runtime test and
browser-client suite (6/6) pass; a rebuilt live gateway now rejects the formerly
accepted request with the terminal-run reason.

The in-app browser automation tool was unavailable in the validating session, so
this checkpoint does not add a fresh rendered screenshot. The HTTP/RPC gateway,
real model turns, persisted state, and pure rendering tests were exercised. A
fresh `codex-code-mode-host` build was also blocked before compiling project code
because the pinned V8 crate could not download its Windows prebuilt archive and
Python was unavailable; the live gateway used the existing matching companion
binary from the preceding validated client build. These are explicit validation
limits, not product successes.

### 2026-09-22 selective-routing and rebuilt-surface checkpoint

SC-EVAL-003 now provides matched live evidence on the native cached-login CLI.
After the run-state context was made self-sufficient for complete steering and
final obligation/run persistence was grouped into one code-mode turn, optimized
Stateful rollout `01a0c875-8a49-7783-9569-bc9ab50102b9` used three model
responses and one exact-evidence batch. It opened four of eight project files,
versus six files and three read-bearing outer calls in ordinary Codex. A
same-binary ordinary replication, `01a0c880-4ea5-7911-9b19-065635cb608d`,
reproduced the six-file pattern.

Against that replication, Stateful used 83,056 full measured-turn tokens versus
101,020 (17.78% fewer), but 32,368 uncached input plus output versus 20,124
(60.84% more). This proves selective routing and a measured-turn total-token
advantage in this scenario, not lower uncached or lifetime cost. Maturation cost
and a stable multi-run distribution remain outside the demonstrated claim.

The optimized branch CLI was rebuilt and run with `OPENAI_API_KEY` and
`CODEX_API_KEY` removed; the cached ChatGPT login completed the durable run and
made no project edits. The rebuilt browser gateway on port 4174 reported
`authMode: chatgpt` and read the exact CLI-created completed run, final result,
obligation, hierarchy, blackboard, and paginated activity through real v2 RPCs.
The client suite passes 6/6. The in-app browser harness was again unavailable,
so there is no new screenshot-level layout claim. An attempted interactive TUI
smoke was blocked before Codex launched because the PTY wrapper resolves the
Windows Store `pwsh.exe`, which returns access denied; native `codex exec` is
validated, while a fresh interactive-terminal observation is not.

The branch is therefore feature-complete for the first product slice but not
release-complete under Stage 8. Remaining gates are representative precision and
recall measurement, maturation-inclusive lifetime economics, a stable matched
run distribution, a fresh rendered browser/TUI pass when the host surfaces are
available, and the repository-wide Rust suite after explicit approval.

### 2026-09-22 held-out memory and amortization checkpoint

SC-EVAL-004 matured a previously held-out ten-file licensing project, preserved
all ten intended concepts in source-verified state under manual semantic review,
and then answered a fresh liability question by reopening four files instead of
ten. The first literal state scorer remained at 6/10 because valid concepts used
different morphology or spanned entries; its unchanged failure and one genuinely
unsupported corpus-inventory entry are preserved in the evaluation record.

SC-EVAL-005 then ran three additional pre-registered matched CLI pairs against
that mature project. Stateful reopened 11 project files across the three
questions versus ordinary Codex's 30, used four read-bearing outer calls versus
13, and used ten model responses versus 16. It won full tokens in all three
pairs, reducing the aggregate from 428,784 to 291,780 (31.95%). It lost uncached
input plus output in all three, increasing the aggregate from 73,456 to 90,052
(22.59%). Including the 649,173-token maturation run leaves Stateful 512,169
full tokens and 111,273 uncached tokens behind ordinary for this series. The
observed full-token slope projects break-even around question 15; the uncached
slope has no break-even.

A fresh interactive TUI run also completed through cached ChatGPT login and the
four focused Stateful TUI tests pass. The remaining release work is therefore
not basic CLI/TUI wiring or selective source routing. It is reducing fixed
uncached context/tool overhead, demonstrating maturation-inclusive advantage on
an appropriate workload, obtaining a fresh rendered browser pass when that
surface is available, and running the approval-gated repository-wide Rust suite.

One bounded attempt to reduce the uncached overhead moved seven exceptional
Stateful tool schemas behind Codex's deferred-tool boundary. A same-binary
territory replication still reduced full tokens by 47.12%, model responses by
three, and file reads from ten to three, but increased uncached input plus output
by 35.95%. The initial request shrank by only 380 tokens; a zero-cache second
response accounted for 27,021 uncached input tokens before the third response
cached 26,368. Because the small schema saving did not justify making core
memory tools less discoverable, the change was reverted and preserved as
negative evidence in `STATEFUL_CODEX_EVALUATION.md`. Do not respond to this
result by thinning the rich root blackboard or explicit run contract that
enabled selective retrieval.

### 2026-09-22 fresh rendered browser checkpoint

A fresh Edge render against the live gateway on port 4174 and the current
branch CLI completed a new Collaborative browser run using cached ChatGPT
authentication. Thread `01a0c904-e60c-7391-9845-a9211c9e3233` and run
`run-33c385b12d21a4b44b90a9fcdc1587c24a46d8c107cce2bd52bf6b415f685f8a`
answered a predeclared licensing question from the mature project state. The
result correctly identified the executed 6% royalty and Amendment 1 as the
controlling instrument, published a concise learned/implication obligation,
advanced the strategy revision, completed without a continuation, and made no
fixture edits. The evidence control opened the exact 296-byte
`executed-amendment-1.md` source containing the executed date, replacement
clause, and six-percent term.

The rendered inspection found a real information-design defect: accumulated
findings occupied a narrow third column for the entire page, leaving the main
work area blank below the result. At intermediate widths, the controls,
steering, and long findings list were compressed into three narrow columns.
The workspace now keeps run controls and steering in the operational rail, then
places findings in a full-width responsive grid below the live work. The same
18-finding workspace dropped from 5,042 to 2,615 rendered pixels at 1440 px
without hiding state; it uses three finding columns on wide screens, two at
intermediate widths, and one on mobile. Desktop, 768 px, and 480 px renders and
the exact-evidence state were inspected. Screenshots are under
`%LOCALAPPDATA%/Temp/stateful-client-render-1790078328677`.

The reviewed workspace snapshot was updated and the browser-client suite passes
8/8. This closes the fresh rendered-browser gate for the current executable. It
does not close the maturation-inclusive lifetime-cost or repository-wide Rust
suite gates.

The first maturation-efficiency slice is also implemented. A bounded
`blackboard_record_batch` call can now commit up to 24 findings and 48
relationships together. Relationships use the records' idempotency keys as
local references, so the model does not have to copy generated entry IDs into
later turns. Existing incremental record and relation tools remain intact. A
focused app-server integration test proves that one model call persists two
findings plus their relationship; the extension suite passes 5/5 and the
focused app-server test passes 1/1.

SC-EVAL-006 then repeated the original maturation workload in a fresh project
over a byte-identical copy of the ten-file corpus. The model naturally used one
linked batch for all 17 findings and 27 relationships with zero failures. Versus
SC-EVAL-004, model responses fell from 15 to 10, custom tool calls from 14 to 9,
full tokens from 649,173 to 344,870 (-46.88%), and uncached input plus output
from 94,677 to 75,814 (-19.92%). All 17 findings are current, source-verified,
and evidence-linked; manual review found all ten predeclared concepts and no
forbidden conclusion. The frozen literal scorer matched only 3/10 because its
single-entry string rules miss equivalent wording and linked multi-entry
concepts; that result remains unchanged and is not being tuned post hoc.

This closes the first measured maturation-round-trip remediation, not Stage 8.
Combining the improved maturation cost with the already measured three-question
series leaves a directional 207,866 full-token and 92,410 uncached-token deficit
to ordinary Codex. Full-token break-even moves from roughly question 15 to
question 8 under that observed follow-up slope; uncached usage still has no
break-even. A matched end-to-end series and a stable workload distribution
remain required before claiming lifetime economic advantage.

The next bounded efficiency slice is committed in `88280d1f1d`. A model-issued
`context_map_refresh` now returns a response-capped inventory of current source
routes, allowing a fresh small project to proceed directly to exact evidence.
Targeted `context_map_query` remains available when the inventory is truncated
or does not identify the needed source, and the app-server protocol is
unchanged. Storage and extension suites pass 26/26 and 5/5; a focused
app-server integration test proves that a model refresh over two files receives
both routes in its next request. SC-EVAL-007 pre-registers a fresh-project
replication before measuring whether this removes the redundant listing and
context-query steps observed in SC-EVAL-006.

SC-EVAL-007 now confirms the mechanism on a rebuilt cached-login CLI. Refresh
returned all ten current routes, and the model performed no context-map query,
shell listing, or shell content pass before exact verification. Relative to
SC-EVAL-006, model responses fell from 10 to 8, outer tool calls from 9 to 7,
full tokens from 344,870 to 279,247 (-19.03%), and uncached input plus output
from 75,814 to 69,071 (-8.89%). The final state retained 17 of 17 current,
source-verified findings, all ten concepts under manual review, and no forbidden
claim. The byte-identical fixture remained unchanged.

The next maturation-efficiency work is now narrower and evidence-driven: clamp
oversized evidence requests at the bounded tool boundary instead of spending a
model retry, then remove copied context-entry fingerprints from same-project
blackboard evidence writes so one malformed identifier cannot split the linked
batch. The former is a small defensive tool improvement; the latter should use
current context-map routes rather than weaken source verification. Neither
should thin the root blackboard or hide memory tools.

Those two defensive slices are now implemented. Oversized positive evidence
reads are transparently clamped to the existing 12 KiB response boundary, and
blackboard evidence accepts current source routes rather than model-copied
fingerprints. The resolver persists the authoritative current entry ID and
fingerprint itself. A single-source finding without an explicit node now lands
on the corresponding file blackboard; multi-source knowledge remains at the
project level, while root promotion remains independent. Existing exact-ID
references remain available for anchored regions or duplicate paths.

The extension suite passes 5/5, and focused app-server tests prove both the live
oversized request observed in SC-EVAL-007 and route-resolved, source-verified,
file-level batch persistence. SC-EVAL-008 is pre-registered before rebuilding
and measuring whether the model eliminates both remaining retries in practice.

SC-EVAL-008 confirmed both mechanisms on a fresh rebuilt cached-login CLI. An
oversized 30,000-byte evidence request was clamped without a retry, and the
successful linked write resolved 30 relative-path evidence references without
model-authored entry IDs, fingerprints, or node IDs. Current fingerprints and
filesystem-shaped placement were correct. All 13 final findings were current,
source-verified, and evidence-linked; manual review recovered all ten intended
concepts with no forbidden conclusion.

The run did not improve total cost over SC-EVAL-007. It used 11 model responses,
10 outer tool calls, 417,784 full tokens, and 75,256 uncached input plus output.
The first linked batch failed because blackboard evidence could not persist the
exact line ranges the model carried forward from `evidence_read`; the retry
removed those ranges. The model also completed the run before adding one final
valid question, so a second completion update was rejected even though the
finding itself persisted. These failures define the next narrow work: preserve
exact line locators through storage, APIs, tools, and evidence navigation, then
harden finalization ordering without making terminal runs generally mutable.

Exact line-range provenance is now implemented in `db88bf1784`. The bounded
locator survives model writes, durable revisions, blackboard queries, root
source aliases, v2 APIs, and browser evidence navigation. The model-facing
integration test proves that `lineRange: {start: 2, end: 3}` is returned by the
next query and rendered as `S1:L2-L3` in the next request's World State. The
browser API returns only those exact lines, and the UI displays and reuses the
locator. Focused storage, protocol, extension, app-server, and client suites
pass.

SC-EVAL-009 was pre-registered to measure the completed provenance path and a
stricter terminal-ordering contract. The contract will keep completed runs
immutable: instead of reopening a terminal record, it tells the model that all
knowledge, relationship, obligation, and verification writes precede the final
completion call. The live replication measured whether this removed both
SC-EVAL-008 retries without adding a second finalization phase.

SC-EVAL-009 subsequently passed both mechanism gates on a fresh byte-identical
ten-file project. One linked batch accepted 13 findings, 19 relationships, and
all 30 exact evidence ranges with zero failures or retries. All findings were
active, current, source-verified, and evidence-linked. The final code-mode call
wrote the last semantic obligation and then completion sequentially; completion
was the final Stateful mutation and no terminal reopening was required. The
run used 7 model responses, 6 outer calls, 250,849 full tokens, and 68,065
uncached input-plus-output tokens, reductions of 36.36%, 40.00%, 39.96%, and
9.56% respectively from SC-EVAL-008. The frozen literal scorer reached 7/10,
manual review found all ten concepts, no forbidden conclusion appeared, and
the corpus remained unchanged.

This closes exact-provenance persistence and terminal finalization as bounded
implementation slices. A fresh Edge render against the live gateway now
passes at 1440 px and 480 px with no horizontal overflow, all 13 durable
understandings visible, all 30 ranged-evidence controls present, and an exact
lines 5–11 read rendered correctly. That pass found and closed a UI filter that
had hidden strategy and decision records; the browser-client suite passes 8/8.
The post-completion CLI also logged one harmless `UnknownProcessId` cleanup race
after exit-success; it should remain visible for later operational hardening but
does not justify interrupting the higher-value release gates.

SC-EVAL-010 then completed the pre-registered current maturation-plus-follow-up
series. Across three matched questions, Stateful reduced full follow-up tokens
from 308,300 to 248,456 (19.41%), model responses from 15 to 11, and
read-bearing calls from 11 to 5. It replaced broad ten-file reading in every
ordinary run with exact verification of four, three, and six files. Those are
real mature-workspace gains. However, Stateful uncached follow-up cost increased
from 65,356 to 83,592 (27.90%) and lost on all three pairs. Including the actual
250,849-token maturation run yields 499,305 Stateful lifetime tokens versus
308,300 ordinary tokens and 151,657 uncached tokens versus 65,356. Estimated
full-token break-even is approximately 13 similar questions; uncached cost has
no observed break-even. Manual review also found that one Stateful final answer
omitted the executed uncapped-liability carve-out despite its presence in
durable state and the semantic obligation.

The branch is therefore functionally complete for the intended first product
slice but remains short of the Stage 8 release claim. Remaining work is to
reduce the fixed uncached context cost without sacrificing the rich always-
loaded root, preserve selected decisive findings through final-answer assembly,
replicate gains on a broader pre-registered workload, and run the
approval-gated repository-wide Rust suite. The corpus remained byte-identical
through all six SC-EVAL-010 runs.

The first uncached-cost remediation is now implemented in `c0f120f009` and
measured by pre-registered SC-EVAL-011. Stateful prompt-cache affinity follows
the project explicitly selected by the user, including live selection changes,
while thread/session metadata remains distinct and internal review/fork
overrides retain precedence. Two independent app-server threads expose the same
project cache key in focused integration coverage. In the real cached-login CLI
replication, the second fresh thread reused 12,032 first-response input tokens
instead of zero and reduced first-response uncached input from 20,938 to 8,239
(60.65%). Whole-turn uncached input plus output fell from 27,155 to 14,822
(45.42%) with identical four-source verification, three model responses, two
outer calls, complete answer coverage, and a byte-identical corpus.

This removes the diagnosed adjacent-thread cache-prefix defect without cutting
the root blackboard. It is not yet the Stage 8 economics claim: provider cache
expiry, root changes, broader workloads, and the previously observed
final-answer omission remain unproven. The next high-value slice is preserving
selected decisive root findings in final-answer obligations, followed by a
fresh matched distribution and the approval-gated repository-wide Rust suite.

### 2026-09-22 durable completion-basis checkpoint

SC-EVAL-012 passed its completion-checklist mechanism gate and its final-prose
gate but failed its durable-result gate. The assistant's final answer recovered
the executed liability carve-out from the completion checklist and root state;
the terminal run result had already been stored without that conclusion and
could not be repaired afterward. This distinguishes visible answer quality from
durable semantic integrity and keeps Stage 8 open.

Commit `0f7684a3f4` makes material root selection part of completion rather than
a post-completion reminder. Stable compact `K` references let the model identify
the exact root findings relevant to the requested outcome without copying
opaque entry IDs. Completion resolves the selected current records and their
evidence routes, then appends those findings before the bounded semantic packet
in the result prior to the terminal mutation. The design is domain-neutral,
keeps the broad root intact, adds no model turn, and rejects unknown, duplicate,
or oversized selections before completion.

The extension suite passes 6/6. Focused app-server integration coverage proves
that a selected root finding is visible in initial World State, returned in the
completion checklist, and present in the stored terminal result; the focused
Autonomous continuation test also passes with the stricter final-obligation and
explicit-selection contract. SC-EVAL-013 is pre-registered in
`STATEFUL_CODEX_EVALUATION.md` to replicate the exact termination-risk failure
against a rebuilt cached-login CLI. Broader matched distribution and the
approval-gated repository-wide Rust suite remain subsequent release gates.

SC-EVAL-013 subsequently passed the durable-result gate on a rebuilt live CLI.
The selected source-verified carve-out, exact source ranges, notice period,
planning figures, coverage uncertainty, and final semantic packet all survived
in the terminal API result and final prose; the corpus remained byte-identical.
This closes the specific SC-EVAL-012 correctness failure.

The run also exposed that compact opaque hashes are still poor model-facing
handles. Its first completion attempt invented two `K` references; validation
rejected the call before mutation, and a retry with seven valid references
succeeded. The safety boundary worked, but the retry added a fifth response and
roughly 3,022 uncached input-plus-output tokens. Before the broader workload,
replace these hashes with the already-rendered `E` aliases plus an explicit root
revision. Completion can then reject an alias only when the root changed or the
selection is malformed, while ordinary follow-up runs avoid copying opaque
identifiers.

## System boundaries

### Existing Codex primitives to reuse

- `codex-thread-store`: canonical projects, roots, thread membership, and thread
  history.
- `codex-state`: host state only where shared state-runtime ownership is required.
- `codex-extension-api`: lifecycle, native tools, typed World State, and extension
  event delivery.
- `codex-core`: inference and execution harness. Stateful logic should not live
  here unless a generic harness capability is genuinely missing.
- `codex-app-server-protocol` v2: client contract and generated SDK types.
- `codex-app-server`: host composition, project selection, notifications, and
  request processors.

### New subsystems

`codex-project-intelligence` owns project-scoped knowledge and its persistence:

- hierarchical nodes mirroring project/directory/file/optional anchored region;
- typed blackboard entries and relationships;
- context-map entries and source revisions;
- provenance and verification state;
- queries, bounded projections, revision checks, and maintenance operations.

`codex-stateful-extension` owns inference-time behavior:

- selected-project attachment;
- root-blackboard World State contribution;
- deeper-state and context-map query tools;
- evidence capture proposals;
- obligation and steering lifecycle integration;
- autonomous continuation policy and Socratic transition policy.

The app server owns product orchestration:

- validates and attaches project selection to new/resumed/forked threads;
- exposes project-intelligence, run, obligation, steering, and readiness APIs;
- emits ordered revisioned notifications;
- installs the extension with shared project-intelligence services.

The client owns presentation and explicit choices, never semantic inference.

## Canonical data model

All durable mutable semantic records have a stable ID, project ID, revision,
created time, updated time, and provenance. Derived filesystem-topology records
also have stable identity and revisions; their configured root, relative path,
region anchor, and source fingerprint establish their origin. Updates use
compare-and-swap or an idempotency key.

### Hierarchy node

- `id`
- `project_id`
- `parent_id` (null only for the project root)
- `kind`: `project | directory | file | region`
- normalized project-relative path
- optional generic region anchor
- current source revision/fingerprint
- lifecycle: `active | missing | replaced`

The hierarchy must enforce one project node, one directory node for each
configured filesystem root, acyclic parentage, containment under those roots,
and uniqueness of active path/anchor identity.

### Blackboard entry

- node ID and semantic kind: instruction, fact, claim, number, decision,
  strategy, question, contradiction, failure, rejected approach, signal, note;
- concise content plus optional structured value/unit;
- confidence and verification state;
- importance and root-promotion state;
- evidence links and related-entry links;
- supersession/tombstone metadata.

The root projection is a curated view, not a second source of truth. Promotion
must preserve provenance and cannot silently convert a hypothesis into a fact.

### Context-map entry

- node ID and source revision;
- bounded description of contents;
- locator/anchor and byte or line bounds when available;
- symbols/headings/keywords useful for routing;
- extraction status and last verification time.

Source movement or content changes mark affected entries stale. Stale entries may
route discovery but cannot support a consequential verified claim.

### Run, obligation, and steering records

A run belongs to a project and references one or more thread views. It stores the
user goal, explicit workflow mode, status, current strategy revision, and result.

An obligation update contains structured fields for examined material, rationale,
learning, implication, strategy, changed assumptions, next work, uncertainty,
blockers, and requested user judgment. Rendering targets roughly 500 useful words
but storage remains structured.

A steering instruction records the user's exact input, affected run/obligations,
acknowledgement, application status, resulting strategy revision, and any stated
reason it could not be applied.

## Context contract

Every model-visible Stateful fragment must:

- implement the typed contextual-fragment path through World State;
- have a stable content classification and markers;
- declare and test a hard byte/token cap below 10K tokens;
- avoid unstable ordering and timestamps that cause cache misses;
- include project identity and knowledge revision;
- distinguish verified evidence, derived understanding, hypotheses, and stale
  material;
- render only a semantic diff when the prior snapshot is known;
- survive compaction without rewriting prior history.

Initial cap targets, to be evaluated rather than treated as permanent product
limits:

- project identity and retrieval instructions: 1 KiB;
- always-loaded root blackboard: 24 KiB and below 8K tokens;
- one deeper-state tool response: 16 KiB;
- one context-map response: 16 KiB;
- one obligation update: 8 KiB.

Truncation is deterministic, importance-aware, and disclosed. The model must be
able to query what was omitted.

## Workflow modes

### Autonomous

- Continues useful in-scope work after a turn becomes idle.
- Does not request approval for ordinary authorized execution.
- Stops only for completion, user pause/cancel, exhausted explicit budget, a
  genuine authorization boundary, or a blocker requiring user/external state.
- Writes meaningful obligation updates and bounded heartbeat/lease state so a
  crash or restart can recover ownership safely.

### Collaborative

- Executes normally while emitting semantic updates at meaningful changes.
- Accepts steering at any time and visibly reconciles it into the strategy.
- Does not turn transparency into an approval checkpoint for routine work.

### Socratic

- Begins with deliberate questions and synthesis rather than execution.
- Persists answered questions, unresolved assumptions, and the proposed strategy.
- Executes only after the user explicitly transitions the run or submits an
  execution instruction consistent with the UI contract.

Mode changes are explicit, durable, revisioned, and visible to both model and UI.

## API and event contract

Resource names are singular and v2-only. Planned families:

- `projectIntelligence/status`
- `blackboard/query`, `blackboard/upsert`, `blackboard/relate`
- `contextMap/query`, `contextMap/refresh`
- `statefulRun/start`, `statefulRun/read`, `statefulRun/pause`,
  `statefulRun/resume`, `statefulRun/cancel`
- `obligation/list`
- `steering/submit`
- `evidence/read`

Lists use cursor pagination. Request optionals use nullable TypeScript fields.
Mutations accept idempotency keys or expected revisions. Notifications include
project ID, run ID where applicable, entity revision, and an ordering cursor.
Clients recover gaps by reading current state; notification text is never the
only copy of a consequential fact.

## UI contract

The first screen makes three explicit choices:

1. project directory/project;
2. create, continue, or fork thread;
3. Autonomous, Collaborative, or Socratic mode.

The active workspace shows:

- the current semantic obligation packet and its revision;
- strategy changes and why they changed;
- important findings, uncertainties, contradictions, and blockers;
- steering input with acknowledgement/application state;
- grouped supporting activity beneath each semantic update;
- blackboard hierarchy with provenance and verification badges;
- context-map navigation to exact source regions;
- run controls, autonomous status, recovery state, and explicit mode;
- evidence-grounded final result and remaining uncertainty.

The UI must never label a run ready, complete, verified, or evidence-backed from
the mere presence of rows, tool success, assistant prose, or a process exit code.

## Delivery sequence and gates

### 1. Project bridge and empty-state World State

Attach canonical project ID and roots to new/resumed/forked thread runtimes.
Install a no-op-unless-selected Stateful extension that contributes a bounded,
typed project-intelligence World State section.

Gate: integration tests prove two threads in one project resolve the same project
identity; compaction preserves the section; an unselected thread receives none;
fork/resume cannot silently lose or change the project.

### 2. Hierarchy and context-map substrate

Land the project-intelligence crate, migrations, revision model, filesystem-safe
path rules, and read APIs before model-authored knowledge.

Gate: cross-platform tests cover hierarchy invariants, stale source revisions,
bounded queries, idempotency, and concurrent updates.

### 3. Blackboard substrate and retrieval

Add typed entries, evidence, relationships, promotion, supersession, query tools,
and root projection. Do not add autonomous writes until reads and provenance are
trustworthy.

Gate: an integration scenario answers from shared state in a fresh thread, opens
the exact source for a consequential claim, and rejects stale evidence.

### 4. Semantic observation and maintenance

Capture candidate learning from completed work, validate it, commit structured
state, and run maintenance that connects sources, exposes contradictions, and
surfaces open questions. Failed extraction remains observable and retryable.

Gate: curated benchmark cases demonstrate reduced rereading and successful
recovery of decisive cross-source details without unsupported promotion.

### 5. Runs, obligations, and steering

Add durable run state, structured obligation updates, ordered notifications, and
steering reconciliation.

Gate: UI/API tests prove updates explain learning and implications rather than
tool narration; steering is acknowledged and visibly changes later strategy.

### 6. Workflow modes and autonomous supervisor

Implement mode state machines, ownership leases, continuation scheduling,
budgets, pause/cancel/recovery, and Socratic transitions.

Gate: Autonomous completes a multi-turn task unattended; Collaborative accepts
mid-run steering without a routine approval gate; Socratic does no execution
before transition; restart does not duplicate work.

### 7. First-class web UI

Build the explicit selection flow and live workspace against the real APIs.
Use snapshot/visual tests and end-to-end runs; do not ship a transcript-derived
mock as evidence of backend behavior.

Gate: fresh, resume, fork, steer, pause, recover, inspect evidence, and final
result flows work against a real app-server with no hidden manual repair.

### 8. Evaluation and release hardening

Measure lifetime tokens, repeated source reads, state precision/recall, decisive
detail discovery, steering latency/application, autonomous completion, recovery,
and final evidence correctness against normal Codex baselines.

Gate: publish claims only for demonstrated improvements. Record regressions and
negative results; passing component tests alone is not product completion.

## Change discipline

- Land each numbered stage as reviewable vertical slices under the repository's
  change-size guidance.
- Every agent-logic change includes a Core or app-server integration test.
- Every user-visible UI change includes snapshot coverage.
- Schema changes regenerate fixtures; dependency changes refresh Bazel locks;
  build-time files are declared in Bazel data.
- Do not hide fail-open behavior. A missing or corrupt intelligence store is
  visible, prevents evidence-backed readiness, and leaves ordinary Codex usable.
- Keep a clean worktree at stage boundaries and record exact validation results.
