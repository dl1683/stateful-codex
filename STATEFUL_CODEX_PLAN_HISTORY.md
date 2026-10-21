> Archived on 2026-10-12 from the historical STATEFUL_CODEX_MASTER_PLAN.md and STATEFUL_CODEX_CURRENT_STATE.md during the entropy pass; not authoritative. Current inventory and work boundaries are in the [roadmap](./STATEFUL_CODEX_ROADMAP.md), the current snapshot in [current state](./STATEFUL_CODEX_CURRENT_STATE.md), and governing decisions in [decisions](./STATEFUL_CODEX_DECISIONS.md).

> **Archived historical record, not current operating instructions.**
> Use [current state](./STATEFUL_CODEX_CURRENT_STATE.md) for status and holds.
> Historical external evidence is not distributed in this repository.
> Paths and personal identifiers are redacted; original bytes and any
> historical digest claims refer to the version preserved in Git history.

# Stateful Codex plan history

Every part below is moved verbatim; line numbers refer to the source file at `e2249ad43`.

# Part 1. Master plan: superseded master priorities (STATEFUL_CODEX_MASTER_PLAN.md lines 49-58)

Superseded by the dated orders in part 2. The within1 reference of 3.70M units and 18 compactions was replaced on 2026-10-11 by SC-EVAL-040's plain arm (1.94M units, 10 compactions).

## Master priorities
1. **One integrated build.** Merge the items 1–3 foundation, item 4 (reduced), item 5 (file-level needs-check), item 6 (partial) and item 7's safe parts. Use one migration map. Rebuild from a clean tree.
2. **Memory within a run.** The capsule carries the next step and the last working test command. Re-send less text after a compaction. Pass Gate B, then the within1 replay: cost ≤ plain (3.70M units) and ≤ 18 compactions.
3. **A lower first-session premium** (today 1.7–1.9x): a thinner opening packet, and no memory work on trivial requests.
4. **Capture what plain Codex cannot keep:** decisions with reasons, ruled-out items (first-class: the evidence plus a capsule locator, never silently flipped; within2 showed both arms reporting a rejected tool as accepted), the working hypothesis, and user-adopted colleague formats as attributed, scoped source.
5. **Safety only where it protects memory:** no resurrection of forgotten items, no invented rules, project isolation.
6. **Prove it:**
   - repeat the 20-session and cross-domain hands-on tests;
   - then the external benchmarks;
   - then the reference design's statistical gates before any public claim.

# Part 2. Master plan: dated orders and decisions, 2026-10-08 to 2026-10-12 (STATEFUL_CODEX_MASTER_PLAN.md lines 74-382)

## Market study update (2026-10-08; `sc_dogfood/insights/MARKET_STUDY.md`, 500 classified benchmark failures)
- Coding is on par with each model's published numbers (Luna, Gemini 3.7, DeepSeek), so the harness is not overfit to OpenAI on scores.
- Largest weakness: premature completion on long, all-or-nothing work (TB4.0 8.6% vs 58% best; Harvey 0.86 per-criterion but 5.3% all-pass).
- Memory leads on memory-native tasks (MemoryArena) but Luna under-captures and loses event time (LoCoMo 84 vs 93-95 for other models).

**Ranked improvements (these refine priorities 2-4):**
1. Evidence-bound completion (checkpoint E). Admitted doubts block `Completed`, and an independent check runs when most of the budget remains.
2. Coverage ledger for document work, verified before completion.
3. Capture done by the host (not left to the model) for exact facts and event time, plus abstaining when memory does not support an answer.
4. A portable tool layer: provider-neutral web search, tolerant argument parsing, a relaxed provenance rule, a model capability registry.
5. Deliverable-first pacing, and trimming bookkeeping on single-session tasks.

## Build order after E1c (step-back review, 2026-10-09; `sc_dogfood/council/council_20261009_stepback.md`)
Memory usefulness goes ahead of general agent reliability. Each slice is followed by a hands-on user check before the next one starts.
1. **E1c:** finish its bounded repair (cap of 3) without widening the exemption.
2. **C4–C6 capture-to-use slice:**
   - capture two rules, a conversation-only decision with its full reason, a rejected hypothesis, and the event time;
   - then, from a cold start in a new session, verify exact recall, correct application, attribution and Forget.
3. **Within-run continuity (priority 2):** the capsule's remaining review and Gate B. Keep the next action, the working test route and the values already given to the user through compaction. Target the within1 replay at no more than plain cost and 18 compactions.
4. **File-level freshness next to recall:** mark remembered status as unknown or needs-check unless it was qualified (the SC-EVAL-034 stale-status failure).
5. **Narrow cross-app slice:**
   - the shared operation contract plus a minimal CLI and MCP adapter;
   - the journey Codex → Droid → Codex, with attributed findings and Forget.
   - K3 is split. Only what this journey needs comes now; neutral web search, provider metadata and bridge polish are deferred.
6. **Deferred until after the first user session:** E2 (coverage ledger), P1 (pacing) and optional integration polish. Necessary safety and compatibility gates stay.

Alternatives that stay live and are tested narrowly:
- exact-source and topic recall against summary-heavy memory;
- model-assisted capture against host-sealed sources;
- full root plus capsule against a separately authorised thinner-root experiment.

One final integrated-build acceptance gate remains.

### Step-back 2026-10-10 (Codex; sc_dogfood/council/council_20261010_stepback.md)

- The order stands. C4-C6 no longer gates all memory progress: after repair 3, either it passes or the pre-agreed cut lands, followed by one hands-on check, then we advance.
- **Successor candidate for Forget: a turn-boundary contract.**
  - Forget first answers "pending" and later "effective".
  - "Effective" is issued only after the affected in-flight turns have settled or been discarded. The transition is serialized across processes.
  - The next root or capsule is rebuilt from the new retirement epoch.
  - No SQLite guard is held across model or tool waits.
  - It ships only as a bounded follow-on, never as a fourth repair.
- **Dependable delivery beats voluntary tool use.** Surface adopted rules and a bounded, attributed decision digest at session start. Measure correct application and recovery effort, not query counts.
- **Rejected-hypothesis capture is still open.** Capture goes through model proposal plus explicit adoption. Event time is kept separate from adoption time.
- **Evaluation journeys** use conversation-only facts, deny the agent access to harness logs, and separate recovery from files from recovery from memory.

### C4-C6 outcome and reorder (2026-10-10)

Confirmation 3 returned STOP/CUT (sc_dogfood/integrate/reviews/c456_confirm3_review.md).
- **Cut:** the automatic tool-result publication pipeline, automatic model-tool disclosure of user memory and conversation history, and Code Mode exposure of memory reads.
- **Kept:** rules capture, receipts, the 240 B Apply bound, Forget, the TUI, and the thread-start root.
- **Usefulness loss:** the model can no longer read past conversation except through thread-start continuity.

New order:
1. E1d, bounded: one design review, at most one repair.
2. **Turn-boundary Forget contract plus restored model reads** of conversation history and user memory, with a session-start digest of adopted rules and decisions.
3. Within-run continuity.
4. File-level freshness.
5. The narrow cross-app slice.
6. Freeze, then acceptance.

### Long-running redesign (council 2026-10-10c, Codex and Droid converged; sc_dogfood/council/council_20261010c_longrun_FINAL.md)

The within-run continuity step now means the following:
- **Host-side capture** replaces routine writes made by the main model.
- **A deterministic working-set capsule** is installed on every compaction path. It starts at 512 tokens and holds the next action, the test route, the files in play, open and rejected hypotheses, and values already delivered.
- **A stable request:** append-only amendments.
- **An operation journal** (add, revise, supersede, merge, retire) whose every mutation advances the epoch.
- **Two-speed maintenance:**
  - instant hygiene, applied automatically;
  - a batched maintainer that runs in shadow mode first. It may auto-change only agent-origin derived entries; user rules and decisions need adoption. Batching is offline only, via optional Gemini or DeepSeek adapters, since there is no OpenAI Batch API.

The order is unchanged: E1d, then Forget plus reads, then continuity slice A (capture, journal, writer-turn removal), then slice B (capsule, prefix), then batched maintenance, then freshness, then cross-app, then freeze. Every slice passes the frozen long-thread gate against plain Codex at no more than 3.70M units and 18 compactions.

### Order refinement (Codex step-back 2026-10-10d; sc_dogfood/council/council_20261010d_stepback.md)

Order from here:
1. E1d (bounded), plus the follow-up to the C4-C6 cut (web confirm-then-refresh fix; honest pending Forget receipt as the interim).
2. Frozen long-thread baseline, labelled restricted-read; repeated after reads return.
3. **Minimal shared journal and epoch foundation:** revision and retirement on the existing store, with instant tombstone and index hygiene.
4. Turn-boundary Forget lifecycle on that foundation, covering agent findings, derived outputs, rules and conversations. Then restore direct reads incrementally, with Code Mode still excluded, and add the adopted rule and decision digest.
5. The rest of slice A: host observations, then removal of routine writer turns where capture is non-inferior. Explicit model proposals for insights and rejected hypotheses stay.
6. Slice B: the capsule on every compaction and resume path, then prefix trimming.
7. File-level freshness.
8. The narrow cross-app slice.
9. Guarded semantic maintenance: shadow mode first. It now comes after freshness and cross-app.
10. Freeze, then acceptance.

Fallbacks:
- If turn-boundary Forget stalls, keep restricted disclosure with honest pending semantics.
- If E1d exceeds its repair bound, return "answered, unverified".

### Ask and Work split (Codex step-back 2026-10-10e; sc_dogfood/council/council_20261010e_stepback.md)

**Ask** is a run-less, project-attached thread:
- root, rules, decisions and capture;
- no run or acceptance tools and no obligation guidance;
- ends through ordinary turn completion.

It becomes the default for plain `--stateful` if compatibility permits; otherwise it ships as explicit Ask.

**Runs** exist only for explicitly started sustained Work. Truthful Blocked stays for Work. No more terminal exemptions or final-message classifiers.

**One bounded Ask acceptance packet, then advance:**
- the Paris and leap-year prompts;
- a project-memory question;
- a source-reading question;
- one with an adjacent open Work run.

Each runs in real TUI, exec and Web, including reconnect and interrupt. The explicit-Autonomous-question answered rate and the Code Mode flailing are recorded as separate findings; neither gates memory progress.

**Memory order after Ask:**
1. Finish the restricted-read baseline, plus a small Ask comparison.
2. Minimal journal and epoch foundation. It must work without a run id, and it carries the first-class answered status debt.
3. Forget plus restored reads plus the digest.
4. Prove cold-session recall.
5. The rest of slice A. If it expands or shows little benefit, the capsule (slice B) moves ahead of the remaining capture work.
6. Freshness. Honest "unknown / needs-check" status wording ships now.
7. Cross-app.

### Baseline result moves the capsule forward (SC-EVAL-040, 2026-10-10)

The restricted-read within1 baseline (n=1) showed plain Codex ahead on cost and quality:
- 1.94M vs 2.27M units;
- 10 vs 13 compactions;
- Stateful kept N3 and N4, against plain Codex's N3 and N6, plus N2, N4 and N5 recovered.

The causes map directly onto slice B:
- the prefix overhead;
- the compaction packet dropping N1 and N2;
- the thread-start continuity cut that removed the "Next step" sentence at 480 of 740 bytes.

Order change: after Ask, land an immediate small fix so continuity truncation never drops the trailing next step. Then:
1. journal foundation;
2. Forget plus restored reads;
3. **slice B (capsule and prefix trimming) before the rest of slice A**;
4. the rest of slice A;
5. freshness;
6. cross-app.

Re-run the frozen baseline after each.

### Capsule first (Codex step-back 2026-10-11a; sc_dogfood/council/council_20261011a_stepback.md)

Order from here:
1. Next-step truncation fix.
2. **Working-set capsule.** It needs only a small consistency contract:
   - project and thread identity without a run id;
   - source-event references and a watermark;
   - an eligibility recheck and invalidation;
   - checked install.

   It starts at 512 tokens / 2,048 bytes. Task-relevant entries persist until superseded, never in a recency window. It needs a changed-content witness for untracked files, and covers every compaction and resume path.
3. Measured prefix and schema trimming: replace duplicated prose, omit unusable tools. Root subsetting stays a separate authorized arm.
4. Minimal journal and epoch foundation.
5. Forget plus restored reads plus the digest.
6. The rest of slice A.
7. Freshness.
8. Cross-app.

**Gates:**
- Staged same-build paired replays: truncation-only, then the capsule, then trimming.
- Reference: this baseline's plain arm, 1.94M units and 10 compactions. This replaces 3.70M/18.

**Triggers for the alternatives:**
- epoch-first, if the capsule cannot be invalidated safely;
- reads-first, if the capsule lacks evidence;
- a capsule plus exact-event retrieval hybrid, if 512 tokens is too small.

### Capsule: measure before finishing infrastructure (Codex step-back 2026-10-11b)

Sequence:
1. Finish stage (a)'s bounded review; keep it inactive and don't expand it.
2. Make stage (b)'s generic reducer runnable offline.
3. **Offline content test** on the recorded within1 rollouts:
   - render the capsule at every real compaction and resume boundary, at both 2,048 B and 512 tokens;
   - score N1, N2, N5, N6 and N7 at their probes while keeping N3 and N4;
   - classify each failure as missing source, extraction miss, packing omission, or failure to use.
4. **Frozen continuation probes** with and without the capsule.
5. **Scoped experimental paired within1 replay**, with unsupported paths named. This is a provisional measurement.
6. **Production completion only after that:** stage (c), every native boundary, crash and fork safety.

Branches on the result:
- capture works but cost fails: trimming comes first;
- the cap is too small: try a larger capsule or a retrieval hybrid;
- the evidence is absent: Forget plus restored reads comes first.

### Capsule redesign after the held-out failure (council 2026-10-11c, Codex and Droid converged; sc_dogfood/council/council_20261011c_capsule_FINAL.md)

Lexical semantic extraction is dropped: it scored 5/5 tuned but 2/5 held-out, carried a superseded rule as active, and listed a rejected tool as working.

The capsule becomes:
- **Floor:** a structural layer of next step, locators, change witnesses, and verified routes.
- **Primary:** source-validated extractive semantic slots, selected inside the existing local compaction request (zero extra requests), validated against the request input with full provenance.
- **Unsupported paths:** remote-v2 semantics are unsupported until capability-tested; token-budget and resume carry prior validated records only.
- **Directives:** come from store supersession. Conflicting unlinked messages show the newer one; the two are never both shown as active.

Gates, in order:
1. offline comparison on the regression sets;
2. a fresh sealed holdout;
3. frozen continuations;
4. a scoped paired within1 run on local compaction in both arms, with at least 3 predeclared pairs before claiming.

Required: zero wrong-active rules and zero rejected-working routes.

### Piggyback cut; Forget plus restored reads next (three-way 2026-10-11; step-back 11d)

Offline three-way result (campaign2/capsule_3way/NOTES.md), across 53 boundaries on the regression sets:
- Piggyback carried no exact recall beyond structural-only: it produced 2 trivial records.
- It cost +33% units and +89% compaction latency.
- The hand upper bound shows capacity that is not being used: N3, H1, H2 and RULE-NEW.

The in-band trailer is CUT: it failed 3 leak reviews and brought no gain.

Next:
1. **Forget plus restored exact reads** (on the minimal epoch seam), then the digest. The structural floor stays, held for the gate.
2. Later, and bounded: a separate structured extraction call at compaction boundaries, about 10 per run, metered, with history untouched.
   - It gets one offline comparison with the same validator, packer and scorer.
   - It advances only on a predeclared exact-recall gain with zero safety violations.
   - It must recover the within-run excess plus its own overhead.
   - Resume never triggers extraction.

### Restored reads with a next-turn Forget contract (2026-10-11)

The full Forget lifecycle design (sc_dogfood/integrate/reviews/forget_reads_design.md) is about 13 stages and 8.5k lines. It is kept on file as the successor.

First slice, chosen for usefulness first: entry reads, bounded conversation history, and the digest, admitted at the turn's epoch.

The contract: "Forget takes effect from the next turn. Within the current turn, content already read, held tool results and hook copies included, may still reach the model."
- Delivering forgotten content in any later turn is blocking.
- The in-turn route is a named limitation.

Gate: the frozen restricted-read within1 baseline, re-run with reads restored, paired against plain.

### Forget contract amendment for restored reads (2026-10-11)

E1 review 4 reproduced agent-authored durable text carrying copies of user memory: run strategy, next items, findings. The repair cap is reached.

Contract for this slice: Forget removes the entry from memory reads, the session-start root and the digest.
- Derivatives are not removed: text the agent wrote into its own run notes, strategy or findings before the Forget, and earlier messages in existing threads.
- This is stated on the receipts.

Literal-overlap withholding for derivatives moves to the successor Forget lifecycle.

### Process and order reset (Codex step-back 2026-10-11e)

**Process.** Every slice freezes these BEFORE implementation:
- one capability;
- its disclosure contract;
- its supported surfaces;
- an acceptance journey;
- its fallback.

It also gets a producer-to-consumer matrix (entry reads, history, root, digest, derivatives × supersession, attribution, paging, concurrent Forget, store unavailable). Discovered failures become shared executable regressions, and the implementer runs them before review.

The review budget is one implementation review plus one confirmation; a further fundamental failure means the pre-agreed cut. A changed contract starts a new slice and never converts a failed acceptance into a pass.

**Delivery.**
- Stop growing the unshipped stack. Freeze the 45-commit candidate, check its exact HEAD locally, resolve auth, gate it in the cloud, and push.
- While blocked: offline evaluation and the E2 pre-implementation freeze only, with no new production code on main.
- The metric is shipped, hands-on-qualified journeys.

**Order.**
1. E2 (bounded exact conversation history).
2. Immediate measurement: frozen continuation probes, then the same-build paired within1 run, measuring prefix trimming alongside.
3. The digest, only if evidence is available but not retrieved or applied.
4. A narrow cross-app journey (Codex → Droid → Codex with an attributed decision and rejection packet, minimal export and import) right after the first qualified local recall journey.
5. Honest "unknown / needs-check" freshness wording now. Broader freshness only when it blocks a journey.

**Wording.** Correct the "new thread gives clean context" hint, since derivatives survive.

### Hands-on fixes and order (council 2026-10-11g, Codex and Droid converged; sc_dogfood/council/council_20261011g_handson_FINAL.md)

Each step is a separate freeze, with one review plus one confirmation and its own hands-on acceptance check:
1. Gate the pushed head f097a8008. Repeat the passing controls on it (cold decision recall, current-rule application, Forget disclosure, the continuity banner).
2. Origin plumbing. Legacy seals without a proven origin never gain user authority.
3. Truthful memory presentation:
   - "verified when recorded; current code not checked" freshness labels;
   - proposals shown as "Awaiting review, not applied", with Apply only when applicable.
4. Ask decision proposals: a settled choice in Ask is proposed from the same-turn sealed source.
5. Explicit replacement: the model picks a `replaces {entryId, revision}` target, the host validates it, and Apply is atomic. No lexical inference.
6. Close the slash-command submission escapes.
7. E2, under its unchanged freeze, then immediately freeze E2-answers.
8. Continuation probes, then the paired within1 run: 3 predeclared pairs, local compaction, cost decomposition.
9. The narrow Codex → Droid → Codex journey with attributed decisions, rejections and replacement or retirement state.

### Origin cut; ship-first order (step-backs 2026-10-11h and 2026-10-11i)

Origin plumbing was CUT after 3 reviews: composer bypasses kept appearing, and provenance per span would be needed. The C4-C6 host-text-authority risk is instead closed for FUTURE rules by step 3: rule activation becomes explicit-Apply-only. Legacy auto-admitted rules get a read-time "saved automatically (not reviewed)" label. E2 attribution stays open.

Order:
1. **3b:** the final message must carry the deliverable. Land it first and re-measure on the ladder tasks.
2. **Lean 3a:** explicit activation, an Apply route, truthful receipts and the history framing; the F1 whole-message fallback is allowed.
3. **The capture usefulness trial**, which selects the next step: Ask proposals or replacement.
4. **3c and the slash fixes** land when ready.
5. **Bounded within1** and the **narrow cross-app journey**, once decisions, retirement and replacement behave.
6. **E2** after attribution has an accepted solution.

### Robustness preflight from the open-model ladder (council 2026-10-12a, Codex and Droid converged)

After 3b, and before lean 3a, comes a short, separately frozen preflight:
1. extension-boundary tool-argument normalization;
2. completion-loop classification and recovery;
3. codex-exec bounded hydration;
4. a provider role adapter or fail-fast gate (Mistral);
5. serving admission control and a concurrency ramp;
6. per-request prefix and schema cost measurement, with trimming only on paired evidence.

Each item reruns its exact failing ladder sessions. Rationale: for deploying an open model in this harness on GCP, reliability beats additional memory capture right now.

# Part 3. Current state: source pin, integration chronology and paused-branch inventory

## 3.1 Section 1 before the 2026-10-12 pass (STATEFUL_CODEX_CURRENT_STATE.md lines 30-47)

## 1. Which code this describes

The fork began as `feature/stateful-codex` on upstream `origin/main` at
`48f897e4ce94152818ce0563b3a8aac3a70ce9b8`. The published branch is
`stateful/main` at `5eacf03387` (2026-09-30), the
upstream sync that carries the fork at `4ea34e7d10`. Everything in sections 3
to 6 is read from that source. This page is a source description, not a fresh
build or test run: the 2026-10-07 cleanup audit ran only `npm test` in
`clients/stateful-codex` (46 reported tests, of which two are discovered
fixture modules) and the BixBench adapter unit tests (17 pass, fixtures and
mocks only). No Rust suite, live model run or browser session was run for this
page.

The October memory work is **not** in this code. The frozen builds measured in
SC-EVAL-035 (`cont_0e160ee0f6`, `cont_9a3c2109d8`, `cont_27aa1988f8`,
`cont_c466898af7`, `cont_e9ee21021e`) came from the unmerged branches below.
Do not attribute their behavior to `stateful/main`.


## 3.2 Section 2, integration records Step 1 to E1c (STATEFUL_CODEX_CURRENT_STATE.md lines 48-146)

## 2. Unmerged work on the `stateful` remote

### Integration on `main` (in progress, 2026-10-07)

The paused branches are being integrated directly on `stateful/main`, one reviewed
checkpoint at a time (plan: `sc_dogfood/integrate/INTEGRATION_PLAN.md`; log:
`sc_dogfood/integrate/EXEC_LOG.md`). Landed so far:

- **Step 1, foundation and direct control** (`052741b9c`..`6839ad4f5`): the
  shared continuity base `52694362a1` merged once; transactional capture writer
  and journal; explicit unscoped `/memory add|correct|forget` with `ID@REV`
  targets, expected-project admission and client result fences; bounded review
  and exact reads. Verdict: landable with named limitations after the repair cap.
  **Cut** from this slice, acceptance rows OPEN: automatic word-for-word rule
  capture from messages, quotation-backed user decisions, investigations and
  scoped rules (historical scoped entries are held back and readable), model
  editing or replacement of user-provenance memory, and historical scope titles
  in review. A narrower automatic-capture contract is scheduled around Step 3.
  Linux: 0 new failures against the 83eee86b6 baseline.

Step 2's final cut additionally refuses model lifecycle operations when any recorded
revision of the target has non-Agent provenance, including Agent entries produced by
older binaries from imported memory. Explicit user correction and Forget remain available.

| Acceptance row | Status |
| --- | --- |
| Model lifecycle of entries with any non-Agent revision history | **OPEN** — cut from the retained model mutation, promotion, retirement, supersession and committed replay surface. |

- **Cleanup K1** (`fd89dfca5`..`f5f9fea96`): eval script usage/port fixes, test-only
  TUI version stamp, platform shell in a Stateful test, boxed dispatcher futures.
  The backfill-warning JSON fix was reverted (it lost the warning under default
  log filters).
- **Step 2, migration map and store classification** (`716e3ae5e`..`e334727cb`):
  every store open checks its applied migration history in one read snapshot and
  refuses unknown or incompatible histories (paused item-4/item-5 `0018`, runtime
  `0005`+) with a precise error, leaving the store untouched; common-prefix and
  continuity stores upgrade normally. No offline converter (no data-bearing
  incompatible store exists). Also: legacy context is bounded before it is read,
  predecessor fan-in is bounded, and the model can change or retire only memory
  whose every revision is assistant-authored. Verdict: landable with named
  limitations. Linux: 0 new failures.

- **Capture C2, source/identity/storage foundation** (`7ae0a43b5`..`a7e0bb127`):
  sealed user-turn sources with byte ranges and digests, durable source locators
  and bounded source search, typed temporal context (unknown stays unknown),
  canonical identity/aliases and one retirement fence shared by every writer and
  automatic read, project-fenced source groups (forward PI migrations
  `0020`-`0023`, native `0060`). No new user-authority producer yet (that is
  C3/C4). Cut: root predecessor-content publication, legacy alias replay on
  upgraded stores, historical-findings run completion, existing-entry reuse for
  new source groups. Contract: `sc_dogfood/integrate/capture/CONTRACT.md`
  (capture-c1-v3). Linux: 0 new failures.

- **Capture C3, model proposals and exact source** (`3fe03dfa6`..`42370b992`):
  the model can record a bounded, non-applied proposal citing byte ranges of the
  same sealed user turn (never assistant text, summaries or compaction output);
  proposals carry verbatim detail and temporal spans, are stored atomically
  without user authority, and are labelled as proposals. Cut for now (no
  post-hook publication fence exists, so a result held by a hook could outlive a
  Forget): automatic exact-source/source-search recall and automatic proposal
  recall. Linux: 0 new failures.

- **Cleanup K2, portable tool schemas** (`738e1c401`..`68638f6e6`): Stateful tool
  schemas stay within a portable JSON Schema subset (checked by a test);
  rejections name the field and show a valid call within the response budget;
  call IDs are recorded verbatim, and empty, control-character or over-512-byte
  IDs are refused up front with a clear reason. Known gap (OPEN): providers whose
  bridge puts very long data in call IDs (LiteLLM with Gemini thought signatures)
  are refused rather than supported, and there is no host-enforced cap on a model
  that never produces a valid completion.

- **Checkpoint E, evidence-bound completion** (`fb148cb38`..`0a83b1f58`): a run-owned
  acceptance ledger (criteria linked to request text, required/optional,
  dependencies, revisions), host-observed check evidence and one completion gate
  for every writer of Completed; admitted open issues end the run Blocked with a
  partial result. Autonomous runs can complete unattended against a host-admitted
  frozen checker. **Not shippable alone:** after the final cut a trivial lookup
  cannot complete and checks settle only in fully pinned workspaces; checkpoint
  E1b restores a host-decided read-only exemption first. Checker adequacy is not
  independently judged.

- **Checkpoint E1b, real-repository check qualification** (`056912335`..`179be19c4`):
  checks settle in real Git repositories through bounded content identity (tracked
  and untracked non-ignored files, HEAD and index; never size or mtime). Non-Git
  roots and files inside embedded repositories fail closed. The read-only exemption
  it was meant to restore was cut after review.

- **Checkpoint E1c, no-tool completion exemption: CUT** (`0df6aedbd`..`668602bb8`):
  three repair rounds closed each reviewed witness, but the final review found an
  auxiliary user-shell command and an interrupt that could still coexist with an
  exempt completion, so the code was restored to E1b's behaviour (migration 0012
  stays as applied history, unused).
  **Known limitation until E1d lands:** an Autonomous run that only answers a
  question still ends Blocked, because every completion owes acceptance criteria
  and an admitted check. Successor E1d ("host-ended Autonomous Q&A": the host
  finalizes a text-only, no-tool turn inside its task under the task's cancellation
  token, with user-shell submission refused or queued while an eligible run's turn
  is active) is scheduled after the C4-C6 capture slice.


## 3.3 Section 2, paused-branch inventory as of 2026-10-03 (STATEFUL_CODEX_CURRENT_STATE.md lines 230-254)

The table below describes the branch tips as paused; rows are consumed as their
checkpoints land.

All of these branches start from `stateful/main` (merge base `5eacf03387`).
None is merged. Master plan priority 1 is to integrate them into one build
with one migration map and rebuild from a clean tree. Status lines come from the
builder logs (`sc_dogfood/fix/*_LOG.md`) at the 2026-10-03 pause.

| Branch | Tip | Status |
| --- | --- | --- |
| `fix/continuity` | `d10057bac0` | **Consumed by Step 1 (curated, cuts above).** Continuity line (base `3da78a47c4`): trimmed packet, exact delivery floor, standing rules, deferred tools, lean protocol and window budget, each reviewed clean; frozen builds `cont_0e160ee0f6`, `cont_9a3c2109d8` and `cont_c466898af7` came from it. Also carries items 1-3 (`/memory`, `codex memory`, investigations), which hit the repair cap and stay open; on-demand indexing stopped at the cap with a zero-entry once-cell defect; two post-cap commits (`9b09858e40`, `d10057bac0`) are unreviewed fixes. |
| `h1done/foundation` | `9de1ef0052` | **Consumed by Step 1 (curated, cuts above).** `fix/continuity` plus one unreviewed `wip` commit extracting the items 1-3 foundation (action-ID fence, root projection module, explicit-only investigation join/end, host background capture off). When saved it left 4 app-server Stateful tests failing; the rubric review was not run. |
| `h1done/item4` | `7be002b288` | Item 4 reduced contract (base `52694362a1`): 2 of 4 slices committed, unreviewed (canonical identity index; capture judged by canonical identity inside the commit). Parser and recall slices must be redone. Its migration `0018` may clash with other branches. |
| `h1done/item5file` | `4418ddc737` | File-level "needs check" replacement for item 5 (base `52694362a1`): project-intelligence jobs and `paths_changed_between` committed and tested but unreviewed, plus a `wip` worker commit. Read adapters, the remaining tests and the review are left. The rejected regex-confirmation pivot stays on `h1done/item5` (`0b458025a2`). |
| `h1done/item6` | `d265021d3e` | Execution-waste item (base `52694362a1`): the final scoped Codex check rated it safe to land as partial. Open: thin 2 KiB opening packet, topic card, per-response attribution, recipe executor identity, and the other named limits in its log. |
| `h1done/item7` | `d0c7ea9c8a` | Visibility item: the Rust head reviewed before the reduced contract (`12a3dc65ad`) was rejected (2 P1, 7 P2). The reduced contract (recap cut, `memory_add` revert, reconnect fencing) is committed as unreviewed `wip`; one TUI reconnect snapshot test was failing at the pause. |
| `h1done/insession` | `f7aebbc01` | In-session memory (base `52694362a1`): 25 reviewed commits. Step 1 (smaller tool schemas) is landable. Step 2 (render mode) is capped with 3 open findings. Steps 3 (window journal) and 5 (run admission) are STOP/CUT. Step 4 (task capsule) has its second repair committed as unreviewed `wip` with one review round left. |
| `fix/web` | `dfff18633c` | Web client line: gateway origin check, long-project glance, memory panel, item 7 web (approved at `28a971c4c0`). The tip ("turn the return card off") is an unreviewed pure cut; `npm test` 195/195. |
| `h1done/web-items123` | `067272525f` | Web side of items 1-3 (About you, Add form, paging, revision-keyed drafts), from `fix/web` at `e753c36676`; repair round 1 applied; `npm test` 198/198. Kept off `fix/web`; the integrator merges it. |

Other remote branches (`dogfood/*`, `fix/phase1`, `bench/horizon`,
`harness/scbench`, `checkpoint/stateful-region-routing-unsafe-20260925`) are
historical or evaluation-only. The checkpoint branch holds the rejected
bare-line-range routing prototype at `a6cdb157de`; it must not be merged.


## 3.4 Passages replaced in place on 2026-10-12

Line numbers refer to STATEFUL_CODEX_CURRENT_STATE.md at `e2249ad43`.

Original lines 3-3 (Header date):

````markdown
Updated 2026-10-07. This page says what the published code does, what has and
````

Original lines 13-16 (Reading order, item 3):

````markdown
3. [`STATEFUL_CODEX_MASTER_PLAN.md`](./STATEFUL_CODEX_MASTER_PLAN.md): the
   canonical build plan, agreed 2026-10-07. Its reference design is
   [`STATEFUL_CODEX_MEMORY_PLAN.md`](./STATEFUL_CODEX_MEMORY_PLAN.md). Where an
   older plan conflicts with the master plan, the master plan wins.
````

Original lines 263-266 (Section 3 table rows: startup, project intelligence, runtime, agent integration):

````markdown
| Startup and identity | `codex-rs/app-server-client/src/stateful.rs` binds an explicit project and root and starts the run; exec and the TUI reuse it. Root CLI flags (`--stateful <mode>`, `--stateful-project`) propagate into `codex exec`. Autonomous runs default to 24 continuations and 14,400 seconds (`DEFAULT_STATEFUL_MAX_CONTINUATIONS`, `DEFAULT_STATEFUL_MAX_ELAPSED_SECONDS`), not unlimited activity. |
| Project intelligence | `codex-rs/project-intelligence`: hierarchy, context map, blackboard, evidence links, relations and premises (migrations `0001`-`0014`); `indexer.rs` scans, publishes and tracks refresh generation and health. |
| Runs, steering, measurement | `codex-rs/stateful-runtime`: runs, obligations, steering, autonomous leases and per-turn measurements (migrations `0001`-`0004`). |
| Agent integration | `codex-rs/ext/stateful`: context, tools, tool policy, turn and thread lifecycle. `tools/mod.rs` registers 13 tools: `blackboard_query`, `blackboard_record`, `blackboard_record_batch`, `blackboard_update_batch`, `blackboard_relate`, `context_map_query`, `context_map_refresh`, `evidence_read`, `obligation_update`, `stateful_run_update`, `stateful_run_read`, `steering_query`, `steering_reconcile`. `world_state.rs`, `run_world_state.rs` and `outcome_world_state.rs` own the incremental, bounded model context. |
````

Original lines 403-406 (Section 5, known TUI limits bullet (tail)):

````markdown
  unusable. The published source does route obligation notifications to a TUI
  history cell (`stateful_ui.rs`, covered by a snapshot test), but no live TUI
  check has been recorded since. The memory-visible TUI measured in SC-EVAL-035
  is on the unmerged branches.
````

Original lines 436-437 (Section 7, evidence heading):

````markdown
**Latest evidence** (SC-EVAL-035, 2026-10-02/03, unmerged candidate builds,
every test n=1 per arm):
````

Original lines 463-464 (Section 7, within-run open gate):

````markdown
- Within-run cost: the master plan's Gate B replay and the within1 replay
  (cost at or below plain, 3.70M units; at most 18 compactions).
````
