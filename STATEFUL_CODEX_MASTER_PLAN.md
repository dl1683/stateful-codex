# Stateful Codex master build plan

Agreed with Devansh on 2026-10-07. This is the canonical build plan. It merges two sources:
- the October 2–3 councils and hands-on campaign (evidence in SC-EVAL-035 and `campaign2/FINDINGS.md`);
- the reference design, reviewed 2026-10-06 and archived unchanged as [`STATEFUL_CODEX_MEMORY_DESIGN_2026_10_06.md`](./STATEFUL_CODEX_MEMORY_DESIGN_2026_10_06.md).

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

## Canonical build order

This is the one current order. It records the latest binding decisions, in sequence:
- step-back 11i (`47c902e4d`);
- council 12a, the robustness preflight (`0fdd58ec0`);
- the coordinator's binding decisions in `sc_dogfood/integrate/briefs/step3_truthful_inputs.md`, from "Order change per step-back 11i" onward.

Earlier orders, the reasons they changed and the evidence behind each change are archived verbatim in [`STATEFUL_CODEX_PLAN_HISTORY.md`](./STATEFUL_CODEX_PLAN_HISTORY.md). They are not authoritative.

**Process for every step** (step-backs 11e and 11g):
- Freeze before implementation: one capability, its disclosure contract, its supported surfaces, an acceptance journey and its fallback.
- One implementation review plus one confirmation. A further fundamental failure means the pre-agreed cut.
- A changed contract starts a new step; it never converts a failed acceptance into a pass.
- Each step gets its own hands-on acceptance check.

**Order:**
1. **3b: the final message carries the deliverable.** Status: landed and closed (`64e6176ed`, follow-on `9eddc1af4`); no R1-R4 revert; R1 is 0/317 after adjudication. OPEN: q36a3 critical-error rate 3.7% to 9.3%, checked in the step-7 release rerun.
   - Freeze: `sc_dogfood/integrate/briefs/step3b_freeze.md`. One review plus one confirmation, then push.
   - Then remeasure on the open-model ladder: rerun the affected ladder tasks (`dsg/ladder`) on the pushed 3b head, counting real usefulness and false positives.
   - The predeclared revert criteria R1-R4 are in `campaign2/FINDINGS.md` ("[PREDECLARED 2026-10-11", as amended there).
   - The length or status-only heuristic is a warning only, never blocking.
2. **The robustness preflight** (council 12a). It is short and separately frozen, and comes after 3b and before lean 3a.
   - Items 1-5 are frozen in `sc_dogfood/integrate/briefs/robustness_preflight_freeze.md`. P1 (item 3) landed at `b8dcee46f` with named limitations; the remaining packets have not landed. Production packets P1-P5 cover items 1-4, with P4 conditional on the Step rerun. P3 (the idle-continuation stop, R2c) is deferred to its own freeze; the active item-2 acceptance is the R2a+R2b-only criterion. Item 5 is diagnostics only.
   - Item 1: tool-argument normalization at the extension boundary only.
   - Item 2: completion-loop classification and recovery.
   - Item 3: codex-exec bounded hydration.
   - Item 4: a provider role adapter or a fail-fast gate (Mistral), never a silent demotion of developer authority.
   - Item 5: per-request prefix and schema cost measurement, with trimming only on paired evidence.
   - Each item reruns its exact failing ladder sessions and gets a Codex error hunt.
   - Serving admission control and the concurrency ramp (item 5 of the council's list) went to the ladder operator, not the integrator (`robustness_preflight_inputs.md`).
3. **Lean 3a: explicit activation, an accessible Apply route and truthful status.** Freeze: `sc_dogfood/integrate/briefs/step3a_freeze.md`.
   - Stage 1a: the behaviour-neutral preparation in the project-intelligence store. It lands first, with its own quick review.
   - Stage 1b: containment, authorization, the accessible Apply route and truthful pending status, together.
   - Stage 2: Dismiss, visible eviction and the 50-proposal cap, plus the legacy "saved automatically (not reviewed)" label and the model-visible marker. Only stage 2 completes 3a.
   - F1 has been taken: lean 3a uses the frozen whole-message Apply fallback.
   - Legacy review and its Undo are cut. They are a named follow-on: "Apply to confirm for legacy rules".
4. **The capture usefulness trial**, immediately after stage 2.
   - If natural-rule proposal coverage is under 80%, enable the broad host proposal trigger.
   - 3c (freshness wording, its own slice) ships alongside only if it is ready.
5. **The trial decides the next step.**
   - Ask proposals, if settled decisions vanish.
   - Explicit replacement, if corrections leave competing rules.
   - The slash-command fixes go in whenever they are ready.
6. **Cross-app**, once the selected decisions, retirement and replacement behave correctly.
   - The plan's decided form is the narrow Codex → Droid → Codex journey, with attributed decisions, rejections and replacement or retirement state (council 11g step 9; 11i step 5).
   - 11i places the bounded within1 run at the same point. The 11g form is continuation probes, then a paired within1 run with 3 predeclared pairs, local compaction and a cost decomposition. The reference is SC-EVAL-040's plain arm: 1.94M units and 10 compactions.
   - Within1 is no longer a strict dependency of E2.
7. **E2 (bounded exact conversation history) stays paused** until attribution has its own accepted solution.

**Parked: the capsule working-set scaffold** (within-run continuity). Parked 2026-10-12 (entropy pass); the code deletion lands in the entropy code slice.
- Tag: `parked/capsule-scaffold-2026-10-12`, on `e2249ad43`.
- To revive, restore from the tag:
  - `codex-rs/project-intelligence/src/working_set_storage.rs`;
  - `codex-rs/project-intelligence/src/working_set_storage/publication.rs`;
  - their tests, `working_set_storage_tests.rs` and `working_set_storage/publication_tests.rs`;
  - the shared file-sample types and constants, which the entropy code slice moves into the extension.
- Migrations `0024_working_set_capsule.sql` and `0025_capsule_source_observation.sql` stay in the tree.
- Why it would be revived: the structural floor results recorded in SC-EVAL-041. "The floor holds both hard gates on within3: zero wrong-active rules and zero rejected-working routes over 104 capsules." Its SC-EVAL-041 rows: "Structural floor, tuned set (grammar iterated on it)", `c96429fa1`, 512 tok, 5/5 plain and 5/5 Stateful; "Structural floor, within3 (now a regression set)", 2/5 and 2/5.

## Open decision
Should the full project root stay loaded after an in-thread compaction? The product intent requires it, for unprompted connections. The within-run data shows it mostly unused there and costly. This is not decided yet.

## Benchmarks
These follow the selection rules in the "External benchmark program" section of the archived ledger, [`STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md`](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md):
- agentic benchmarks only;
- each has an official harness that runs a custom agent, or a published Codex comparator;
- no Stateful-off arm;
- partial runs are labelled;
- everything is published;
- never Harvey LAB.

The full candidate list and its status up to 2026-10-08 are in that archived section and SC-EVAL-036/037; later results are in [`STATEFUL_CODEX_EVALUATION.md`](./STATEFUL_CODEX_EVALUATION.md).

### Acceptance gate decision (council 2026-10-12b, Codex and Droid converged; sc_dogfood/council/council_20261012b_gate_FINAL.md)

Evidence: the three-way showed Qwen clean exits falling 98 to 3 after the acceptance-ledger gate. Named bugs: plain questions end Blocked; non-git workspaces cannot verify; schema and revision repair loops.

Decision: verification becomes opt-in for explicitly started Work with declared criteria. Ordinary Autonomous answers end Answered (not verified); unfinished work ends partial. Completed stays evidence-bound.

**Resume point (2026-10-12, paused for the night).** Slice 0 is complete. Slice 1 step F1 landed at `f256e307e`; activation is pending. The cleanup landed (`4d6706d6a` to `175b77ee1`). **Slice 2 (git-independent workspace identity): freeze NOT STARTED.** Council 12b order, with status: (0) done; (1) answer safety, activation pending; (2) git-independent workspace identity, freeze not started; (3) simplified Work verification; (4) refreeze P2 against the new outcome contract; (5) the release check: the matched 216-session rerun with floors of 98/108 Qwen and 54/108 Mistral, zero trivial Blocked, outcomes reported separately, hashes captured, plus a Luna non-git journey; (6) lean 3a's rollout waits for (5). Open observations: the q36a3 critical-error rate (3.7% to 9.3%), checked in (5); the gate-bypass confound (load and time), not isolated.

Status: slice 0 complete (P1 landed at `b8dcee46f`; the causal probe is done). Slice 1 step F1 landed at `f256e307e`; activation is pending. The cleanup landed (`4d6706d6a` to `01aeba725`). Slice 2 is next.

Order:
1. P1 (landed);
2. answer safety (F1 landed; activation pending);
3. git-independent identity;
4. simplified Work verification;
5. P2 refrozen;
6. the matched 216-session release check (floors 98 and 54) plus a Luna non-git journey;
7. lean 3a.
