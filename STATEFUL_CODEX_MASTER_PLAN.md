# Stateful Codex master build plan

Agreed with Devansh on 2026-10-07. This is the canonical build plan. It merges two sources:
- the October 2–3 councils and hands-on campaign (evidence in SC-EVAL-035 and `campaign2/FINDINGS.md`);
- the reference design `STATEFUL_CODEX_MEMORY_PLAN.md`, reviewed 2026-10-06.

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
4. **Capture what plain Codex cannot keep:** decisions with reasons, ruled-out items, the working hypothesis.
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
