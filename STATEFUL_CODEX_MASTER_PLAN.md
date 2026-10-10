# Stateful Codex master build plan

Agreed with Devansh on 2026-10-07. This is the canonical build plan. It merges two sources:
- the October 2–3 councils and hands-on campaign (evidence in SC-EVAL-035 and `campaign2/FINDINGS.md`);
- the reference design [`STATEFUL_CODEX_MEMORY_PLAN.md`](./STATEFUL_CODEX_MEMORY_PLAN.md), reviewed 2026-10-06.

The reference design stays as a detailed design reference, not as the execution plan. It was written without the October 2–3 evidence.

## Principles
- Usefulness first. Memory must pay back over sessions and help within a run. Judge it by using the product as a person would.
- Small changes, under 800 lines each.
- Reviews use a fixed list of what can block a change:
  - data loss;
  - forgotten items coming back;
  - false "settled" or "current" claims;
  - stale state applied after a switch;
  - regressions.
  
  Everything else goes to a backlog with a named limitation. There is no fourth repair round on the same design.
- Every result is recorded in the shared docs on the day it arrives.

## Kept from the council plan
- User rules captured word for word and placed first.
- Receipts.
- `/memory` in the TUI and web.
- Topic-first recall.
- On-demand indexing of a named file.
- The within-run fix: a small, host-built working-state capsule at each compaction that replaces duplicated Stateful text. Codex's native compaction item and the verbatim user messages stay.

## Taken from the reference design
- **Lifetime cost accounting.** Count compactions, retries, capture, maintenance and user effort, not only session tokens.
- **Two kinds of freshness:** "the cited bytes are unchanged" is not "the conclusions are current". This is the basis for the item-5 replacement.
- **Conversation-only decisions** stored with exact attribution.
- **Gate B, the frozen-compaction-point replay with planted facts,** as the standard within-run check.
- **Warnings kept:** no unproven savings claims; do not replace native compaction.

## Deferred from the reference design
- **Its Stage 1 durability and authority machinery:**
  - a crash-safe delivery outbox;
  - client submission keys;
  - instruction-diff admission;
  - tamper resistance;
  - cross-project fork rules;
  - exactly-once side-effect handling.

  These come back only when memory is demonstrably useful.
- **The 10-chain statistical release gates.** They are reserved for public claims and do not gate development.

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

## Open decision
Should the full project root stay loaded after an in-thread compaction? The product intent requires it, for unprompted connections. The within-run data shows it mostly unused there and costly. This is not decided yet.

## Benchmarks
These follow the selection rules in the evaluation ledger's "External benchmark program":
- agentic benchmarks only;
- each has an official harness that runs a custom agent, or a published Codex comparator;
- no Stateful-off arm;
- partial runs are labelled;
- everything is published;
- never Harvey LAB.

The full candidate list and status are kept in the evaluation ledger.

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
