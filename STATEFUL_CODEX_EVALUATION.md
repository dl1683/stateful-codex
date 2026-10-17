# Stateful Codex evaluation record

> **Rebuild active (2026-10-15).** The current owner directive supersedes the
> historical hold below. S0 evidence is landed; S1a is an inert placement foundation,
> with native project ownership, migration and atomic completion still gated.
> See the rebuild entries at the end and the linked baseline/custody receipt.
> Latest 2026-10-16: repaired extension fixtures yield Windows338/338 and WSL339/339;
> the inactive closed response reader passes19/19 on each OS. Component checks
> and installed-launcher checks have separate scopes. The inactive project-bound
> lease adapter passes87/87 runtime tests on each OS; its frozen C382 helper Luna
> journey passes within that component scope. Native routing, populated
> import, old-writer exclusion and the first database milestone remain open.

> **Where things stand and what is next (2026-10-14, owner hold; LSR has priority).** Stateful Codex is on hold in a consistent state. Code head on `stateful/main` is `b59589c84` (open-model usability stages A1 and A2); everything built since 2026-10-12 is landed and was gated on the Linux CI VM and reviewed by a fresh reviewer (the narrow fix to the stage 2B repair, applied as `125f77db6`, was re-read by a fresh reviewer only after landing: landable with two named limitations, namely that Git-mode duplicate-entry accounting across overlapping roots still depends on root order, and that the unreadable-marker test reports a pass when its permission preflight fails; both are backlog): the exec image flag, workspace identity for git roots and non-git folders (stages 2A to 2C), the Argon-driven fixes (digest provenance for long call IDs, `openIssues` on blocked updates, clearer acceptance rejections), stream-retry request filtering, the run-goal error text, slice 1 answer-safety activation (plain finals end Answered or Partial), and usability A1 and A2. The scratch work clones hold only landed work. Older local branches in the repository (`h1done/*`, `cleanup/entropy`, `docs/oct-campaign`) still carry superseded or cut commits that are not on `stateful/main` (inventory in `STATEFUL_CODEX_PLAN_HISTORY.md`); nothing live depends on them. Linux bundles exist for `b59589c84`, `73f2d82da`, `1f329a28b`, `40e68b588`, `e9316e0eb`, `36dacb2bf` and `c5a8f39eb` (paths and hashes in `sc_dogfood/integrate/EXEC_LOG.md`).
> **Evidence in hand.** Ten ordinary Autonomous prompts on Gemma 4 31B end Answered in both registered checks (before and after A1 and A2: Completed 0, Answered 10, Partial 0, Blocked 0, nonzero 0); eight of ten first completion calls were refused for a missing `openIssues` before the fix and none after, but only three of ten sessions called the completion tool after the fix, so the guidance change and the field change are not separated. Luna hands-on runs show correct work and mostly Partial endings for checked tasks, a visible outcome footer, and a model that rewrites a failing checker and then says the check passes (see the hunt-corrected entries and `campaign2/FINDINGS.md`).
> **On hold with the design filed.** Open-model usability stages A3 to A9 (the UNVERIFIABLE state, the bad-directory repair path, the observation provenance fix; frozen design `sc_dogfood/integrate/briefs/open_model_usability_freeze.md`, revision 3) and Part B (host-enforced constrained repair; owner constraints: check identity is host-held, a repaired check must be at least as strict, no wrapper scripts); slice 3 opt-in Work verification (drafts v1 to v3 and two REDRAFT reviews in `sc_dogfood/integrate/briefs/` and `reviews/`; v3 is unreviewed); slices 4 and 5 and lean 3a (`step3a_freeze.md`). The release check (the matched 216-session rerun with floors and a Luna non-git journey) has not been run, so the effect of slice 1 on the open-model regression is not measured.
> **Known gaps and unverified items.** Three `codex-core` catalog-instruction tests fail on the base and before the cleanup landing (recorded, uninvestigated); `just bazel-lock-update` was not run after the extension crate gained two dev-dependencies; the Python SDK file was hand-edited and matches the generator pipeline's output by syntax tree only (the wrapper script fails on the Windows machine); the retry filter and goal-error changes touch core or shared paths and may conflict at the next upstream sync; Windows execution was not part of any gate.
> **Upstream.** Not synced: the last merge base with `openai/codex` is 2026-09-30 and the local `origin/main` reference (2026-10-08) is 401 commits ahead of it, so the 24-hour sync policy has not been met; no fetch or merge was done during the hold.

This record distinguishes implemented behavior from demonstrated product
advantage. [`STATEFUL_CODEX_PRODUCT_INTENT.md`](./STATEFUL_CODEX_PRODUCT_INTENT.md)
defines the outcome being tested; component tests and a runnable interface are
necessary, but are not evidence that Stateful Codex is better than ordinary
Codex. Every entry keeps its failures, invalid or stopped runs, and negative
economics. Entries were compacted on 2026-10-07 without changing any result;
the uncompacted ledger (including evidence moved in from retired documents) is
recoverable from Git at `910c6c045185807ed18bc136f75d7d6f99acc700`, and the
pre-cleanup documents at `46d9071051445583c00b21e0163d373372d1a966`.
On 2026-10-12 the bodies of SC-EVAL-001 to SC-EVAL-037, and the unnumbered
records between them, were moved verbatim to
[`STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md`](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md). The index below still lists every entry.

## Index

| ID | Date | Question | Outcome | Record |
| --- | --- | --- | --- | --- |
| SC-EVAL-001 | 09-21 | Trivial directory inventory | Correct; +22.28% full, +45.26% uncached (negative) | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-001R | 09-21 | Same, active-run tools | Reliability fixed; +86.34% full, +37.19% uncached (negative) | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-002 | 09-21 | Decisive procurement connection | Correct; reopened all 8 files; +225.77% full (fails rereading and cost gates) | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-002R | 09-22 | Compact root, batch writes | 149,360 full; still about 2x ordinary (negative) | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-003 / 003R | 09-22 | Selective controlling evidence | 4 of 6 files, -17.78% full, +60.84% uncached | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-004 | 09-22 | Held-out licensing memory | Lexical 6/10 (manual 10/10); follow-up -15.76% full, -20.89% uncached; maturation 649,173 | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-005 | 09-22 | Three-question amortization | -31.95% full, +22.59% uncached; lifetime behind | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-005R | 09-22 | Deferred-tool remediation | Rejected and reverted (+35.95% uncached) | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-006 | 09-22 | Linked-batch maturation | -46.88% maturation full tokens | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-007 | 09-22 | Refresh-route maturation | -19.03% full vs 006 | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-008 | 09-22 | Resolved evidence | Mechanisms worked; +49.61% full vs 007 (regression) | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-009 | 09-22 | Exact provenance, terminal ordering | Both gates passed; 250,849 full | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-010 | 09-22 | Maturation plus three follow-ups | -19.41% follow-up full, +27.90% uncached; one answer dropped a carve-out | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-011 | 09-22 | Project-scoped prompt cache | Passed; -45.42% turn uncached | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-012 | 09-22 | Final semantic coverage | Prose passed; durable-result gate failed | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-013 | 09-22 | Durable material findings | Passed, with one rejected opaque-reference retry | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-014 | 09-22 | Revision-bound aliases | Passed | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-015 | 09-22 | Two-project distribution | Stopped after maturation (completion contract) | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-016 | 09-22 | Same, disambiguated | Stopped after maturations (evidence schema) | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-017 | 09-22 | Same, exclusive evidence | Correct and selective; full and lifetime economics failed | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-018 | 09-22 | Single-call completion | Partial pass; uncached target failed | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-019 | 09-22 | Final-answer fidelity | Passed | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-020 | 09-22 | Bounded outcome | Retrieval passed; workflow and economics failed | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-021 | 09-22 | Substantive intermediate obligation | Failed | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-022 | 09-22 | Same, remediated | Failed; case frozen | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-023 | 09-22 | Six-project breadth screen | Directional only; graders preferred ordinary on 5 of 6 | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-024 | 09-23 | Source change under total-context stress | Correct; economics failed decisively | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-025 | 09-23 | Same, `body_after_prefix` growth | -49.36% total; no compaction occurred | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-026 | 09-23 | Explicit historical retrieval | Mechanism shown; +639.47% full; ungraded | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-027 | 09-23 | Terminal-Bench `fix-git` control | Stateful 3/3, ordinary 0/1; 2.24x-3.46x cost | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-028 | 09-23 | Terminal-Bench smoke and 89-task breadth | Smoke 4/4; breadth 70/89 (78.65%) | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-029 | 09-23 | Concurrent project-family smoke | Retired before any result; manifest removed 10-07 | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-030 | 09-23 | Feedback-conditioned pilot | 50 attempts; fifth attempts -22.0% cost, 7/8 vs 5/8 | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-031 | 09-24 | BixBench gates | 4/5; breadth 3/10 local, 2/10 correct and valid | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-032 | 09-24 | Pramana ten-question A/B, one thread | 2.0x input, 2.7x uncached; quality level | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-033 | 09-28/29 | Matched behaviour study | Fresh-thread best on data room; overhead on trivial asks | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-034 | 09-30 | 25-question model eval | -13.4% cost; same-day negative findings | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-035 | 10-02/03 | Hands-on campaign | Helps across fresh threads; not yet within one run | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-036 | 10-07 | Cloud benchmark program, first pass (Stateful only) | TB2.1 76.6%, SWE-bench Verified 91.6%, GAIA 81.2%, LoCoMo J 84.1% (proxy judge); no regression, no Stateful advantage shown | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-037 | 10-08 | Multi-model cloud results + cross-model harness check (Stateful only) | Luna, Gemini 3.7, DeepSeek; TB2.1 qemu exclusion, model-label fix, self-judged scores withdrawn; market-study headline | [history](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md) |
| SC-EVAL-038 | 10-09/10 | Capture-to-use hands-on journey, C4-C6 candidate builds | Rules, cold recall and Forget pass; a rejected hypothesis is never proposed; the slice landed with a cut | [entry](#sc-eval-038-capture-to-use-hands-on-journey-c4-c6-candidate-builds-2026-10-09-2026-10-10) |
| SC-EVAL-039 | 10-10 | Plain answers in Autonomous runs, E1d fallback | Not met: one of two trivial questions ended Blocked; raw outcome block shown | [entry](#sc-eval-039-plain-answers-in-autonomous-runs-e1d-fallback-hands-on-2026-10-10) |
| SC-EVAL-040 | 10-10 | Frozen within1 baseline, restricted-read | Plain ahead: 1.94M vs 2.27M units, 10 vs 13 compactions (n=1) | [entry](#sc-eval-040-frozen-long-thread-baseline-restricted-read-2026-10-10) |
| SC-EVAL-041 | 10-11 | Working-set capsule offline tests, piggyback cut | Tuned gains did not generalise; structural floor 0 wrong-active and 0 rejected-working; piggyback cut | [entry](#sc-eval-041-working-set-capsule-offline-tests-and-the-piggyback-cut-2026-10-11) |
| SC-EVAL-042 | 10-11 | Hands-on restored reads and Ask (73d97824a; f097a8008 control) | Cold recall and rule application pass after manual admission; spoken correction not saved as supersession | [entry](#sc-eval-042-hands-on-restored-reads-and-ask-build-73d97824a-with-a-control-repeat-on-f097a8008-2026-10-11) |
| SC-EVAL-043 | 10-11/12 | Open-model ladder, one harness across open models (Stateful only) | Measured on bundle 83eee86b6, before the acceptance-ledger completion gate; 11 arms, quality 4.74-9.71; harness failure classes recorded; no Stateful-off arm | [entry](#sc-eval-043-open-model-ladder-one-harness-across-open-models-2026-10-11-runs-ended-2026-10-12-0149z) |
| SC-EVAL-044 | 10-12 | Harness regression check: original 83eee86b6 vs control 7c4707230 vs 3b 64e6176ed; gate-bypass check | Qwen rc0 98 / 3 / 2; 3b closed, R1-R4 not triggered (R1 0/317 after post-outcome adjudication, first checker 39/317); gate bypass rc0 16 / 0 / 19 on 20 q36a3 keys strongly supports the gate as main cause there (load/time not isolated; usefulness not established); OPEN q36a3 critical errors 3.7% to 9.3% | [entry](#sc-eval-044-harness-regression-check-original-83eee86b6-vs-control-7c4707230-vs-3b-64e6176ed-2026-10-12) |

Unnumbered records, all archived in the [history file](./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md): live
interface validation (09-21/22), open release evidence, project-state
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

## SC-EVAL-038: capture-to-use hands-on journey, C4-C6 candidate builds (2026-10-09, 2026-10-10)

A user journey in the real TUI (`--stateful collaborative`, Luna, high effort, fresh home, n=1 per build). Both
builds are unlanded candidates of the C4-C6 capture slice: 5db119519 (repair 1, gpt-5.6-luna) and 57302f223 (repair 2,
gpt-6-luna). Repair 2 is pre-repair-3 and unpushed. Its source review was STOP/CUT on publication-fence findings that this
journey does not exercise.

| Step | 5db119519 | 57302f223 |
| --- | --- | --- |
| Two stated project rules saved word for word, applied the same turn | PASS | PASS |
| Conversation-only decision proposed with the user's exact words, then Applied | FAIL (model saved its own paraphrase) | PASS |
| Rejected hypothesis captured as ruled out | FAIL | FAIL (never proposed) |
| New session: decision, reason, who and when, cache ruled out, vendor edit refused | PASS (duplicate entries written) | PASS (no duplicates) |
| Forget a rule: no longer applies, re-paste is not restored | PASS | PASS |

Open product issues: the model never proposes a ruled-out item. After a re-paste of a forgotten rule, the model's
reply contradicts the host's "Not restored" receipt. A one-line edit in a collaborative run took 2m29s of
acceptance ceremony and left a temporary checker script in the user's repository. Evidence:
sc_dogfood/integrate/handson/c456_journey.md and c456r2_journey.md.

C4-C6 outcome (2026-10-10): landed with a cut after three repairs (final review
reviews/c456_confirm3_review.md). Rules capture, receipts, Apply of whole
messages of at most 240 bytes, Undo, Forget and the cold root remain. Automatic
disclosure of user memory and past conversation through model tools was removed.
The model now sees them only through thread-start continuity and the root, which
is a usefulness loss until the turn-boundary Forget contract restores those reads.
The SC-EVAL-038 hands-on rows were run on pre-cut builds. The cold-recall row
("cache ruled out", from conversation history) would not reproduce after the cut.

## SC-EVAL-039: plain answers in Autonomous runs, E1d fallback hands-on (2026-10-10)

Build 0dcdbe1e2, gpt-6-luna high, real TUI, `--stateful autonomous`, fresh home, n=2 trivial questions. One run
ended Blocked: the model called the completion tool, was refused for an unmet acceptance gate, and set the run
blocked. The other ended answered, but only after ten unnecessary Code Mode cells, and the raw outcome block was
visible in the answer. No calm "answered" label appeared in the TUI. The requirement "a plain question ends answered,
never Blocked" is not met. Evidence: sc_dogfood/integrate/handson/e1dfb_journey.md.

## SC-EVAL-040: frozen long-thread baseline, restricted-read (2026-10-10)

This is a baseline, not a claim. The binary was frozen and runtime-identical to stateful/main 30c8b2791.
The model was gpt-6-luna at high effort. The workload was the unchanged within1 12-turn script with a
50k compaction limit. Plain and Stateful collaborative ran on the same binary, n=1 per arm. The
condition was **restricted-read**: model tools cannot read past conversation or user memory.

Harness-log denial was detection-only. Real deny-read needs the elevated Windows sandbox, which is not
set up. An audit of all 373 tool calls found no access to homes, rollouts, logs, stores or ground truth.

| | Plain | Stateful |
| --- | --- | --- |
| Requests | 195 | 225 |
| Units | 1.94M | 2.27M (1.17x) |
| Compactions | 10 | 13 |
| Needles kept (N1-N7) | N3, N6; N2 and N4 re-derived; N5 inferred | N3, N4 |
| Deliverable | all 12 steps; the better PR description | skipped t10, continued the wrong step at t07, one help-default bug |

On this run, plain beats Stateful on cost and quality. Stateful's extra cost comes from a prefix of about
2.5k tokens, which makes requests after each compaction about 4.5k larger and so causes more compactions.
Its recall losses came from four places:
- `memory_read` reaches only the project store.
- The compaction packet drops older values.
- Thread-start continuity truncated the t05 answer before its "Next step".
- The checkout notice lists file names only.

These are the targets of the turn-boundary Forget work with restored reads, and of slices A and B. The
baseline is repeated after reads return. Evidence: sc_dogfood/campaign2/baseline_rr1/NOTES.md.

## SC-EVAL-041: working-set capsule offline tests and the piggyback cut (2026-10-11)

Offline reducer tests on recorded rollouts. Each set has n=1 trajectory per arm. o200k_base tokens stand in for Luna
tokens. A needle counts only when it is exact at its probe turn.

| Test | Reducer | Plain | Stateful |
| --- | --- | --- | --- |
| Tuned set (baseline_rr1), first run | stage (b) 8cfb6c70a | 2/5 at both caps | 3/5 at 2,048 B, 2/5 at 512 tok |
| Tuned set after 14 generic rules (the set they were tuned on) | 01a0e96d1, 512 tok | 4/5 | 5/5 |
| Held-out within3 (spec frozen before any output) | 01a0e96d1 | 2/5 at both caps | 2/5 at both caps |
| Structural floor, tuned set (grammar iterated on it) | c96429fa1, 512 tok | 5/5 | 5/5 |
| Structural floor, within3 (now a regression set) | c96429fa1 | 2/5 | 2/5 |

- The tuned gains did not generalise. On within3 the 01a0e96d1 capsule carried a superseded user rule as active, and
  listed a rejected tool as a working route.
- The floor holds both hard gates on within3: zero wrong-active rules and zero rejected-working routes over 104
  capsules. The checker fires on the old outputs, so the zeros mean something. Route lines had empty command text in
  82 of 104 capsules; this was fixed in be0135d0f.
- **Three-way** (53 compaction boundaries, gpt-6-luna high, isolated local reconstruction): the piggyback arm equals
  structural-only (tuned 4/7 and 4/7; within3 2/5 and 2/5). The model returned explicit empty records at 46 of 53
  boundaries and no trailer at 4; 2 records validated. A hand upper bound reached 5/7 and 4/5, so the content was
  available. Piggyback cost +33% units and +89% compaction latency.
- **How the three-way was run.** It was an offline LOCAL counterfactual with experimental overrides for Lite
  withholding and for provenance (`capsule_3way/harness_patch.diff`). Luna's product path had piggyback disabled.
  The native-only calls shared the piggyback chain's reconstructed inputs; they were not chained independently.
- **Rubric change.** The earlier five-needle totals accepted an unlabelled exact N1 literal. The three-way's
  seven-needle rubric requires baseline attribution, so its scores are not directly comparable with those totals.
- Piggyback stage (1) drew three consecutive STOP/CUT reviews, each finding a new in-band leak path.
- **Decision:** the stop condition was met and piggyback was cut (reverted in 717fc9d61). The program moved to
  Forget plus restored reads. Stage (a) and the floor were pushed inactive in the f097a8008 stack.

Evidence: sc_dogfood/campaign2/capsule_offline/NOTES.md, capsule_heldout/NOTES.md and NOTES_floor.md,
capsule_3way/NOTES.md; integrate/reviews/capsule_b_report.md, piggyback_s1_review.md (and review2, review3),
piggyback_s2_report.md; integrate/EXEC_LOG.md; campaign2/FINDINGS.md (2026-10-11 capsule entries).

## SC-EVAL-042: hands-on restored reads and Ask, build 73d97824a, with a control repeat on f097a8008 (2026-10-11)

This was a real TUI and exec journey: gpt-6-luna high, a fresh home and store, a 5-file Python project, n=1 per check.

Two confounds apply to the PASS rows:
- Recall and application passed **after manual memory admission and correction**. On 73d9 the long spoken decision
  was not admitted, so the tester used `/memory add`, `/memory apply` and `/memory correct`. The f097 control inserted
  the decision and the rule by CLI. Natural-speech admission was not uniformly successful.
- Session A ran without the code-mode host; later sessions had it.

| Check | 73d97824a (unpushed) | f097a8008 control (pushed; condensed exec journey) |
| --- | --- | --- |
| Cold recall of decision and current rule in a fresh thread | PASS (10 s, 0 tools) | PASS |
| Rule applied in follow-up work | PASS: zero product interventions; one test-side intervention to restore the intended sandbox setting | PASS (Ruff on src/ only) |
| Ask journey recalled next "day" | PASS (17 s), but Ask proposed nothing to memory | not run |
| Spoken correction saved as supersession | FAIL: proposed as a separate rule rather than a supersession, and not applied. A manual `/memory correct` then superseded the old rule. | not run |
| Forget | PASS for receipt wording and removal from fresh memory reads. Continuity withholding was not tested (the turn had aged out of the 10-turn window). | receipt carries the final contract; a fresh thread still gave the forgotten rule as **current**, quoting the user's earlier message from continuity (a permitted derivative, but mislabelled) |
| "History could not be read" banner | present (defect) | absent (fix holds) |

New defects on 73d97824a:
- a stale agent finding labelled `sourceVerified` and current;
- unapplied proposals listed as knowledge;
- a slash-command typo persisted in continuity.

On f097a8008, exec Work did not admit "For this project, run Ruff on src/ only before commits.", yet the model said
"I've recorded the project rule". This is an overstated receipt.

Cost on 73d97824a: 1.57M input (88% cached) and 16k output across 5 threads. The 73d9 results were not rescored.

Evidence: sc_dogfood/campaign2/handson_73d9/JOURNEY.md, campaign2/handson_f097/CONTROL.md, campaign2/FINDINGS.md
(2026-10-11 hands-on and control-repeat entries).

## SC-EVAL-043: open-model ladder, one harness across open models (2026-10-11; runs ended 2026-10-12 01:49Z)

> **Label: every result in this entry was measured on bundle 83eee86b6, before the acceptance-ledger completion
> gate.** Later bundles behave differently on open models; see SC-EVAL-044.

This was a Stateful-only run: bundle 83eee86b6, Autonomous, effort high unless noted. It used the frozen r2+r3 real
tasks: 32 tasks, 2 trials, 108 sessions per model, isolated chains. Grading was blind Claude grading, 0-10, against
rubrics frozen before any output.

Quality is session grades averaged to trial means, then equal-weight task means. Cost is GPU node lifecycle (boot,
smoke and idle tail included) x Spot list $/h x the model's share of the node. Runner CPU is excluded.

Across 186 re-graded sessions, the mean within-session grading range was 0.50. No uncertainty interval for
between-model quality differences was computed.

| Arm (model) | Serving | Tasks | Quality all / r2 / r3 | Critical % | rc!=0 of 108 | Recorded input | Actual $ | Quality pts per $ |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| ds41fl (DeepSeek V4.1 Flash, anchor) | B200 node TP8 | 29 | 9.71 / 9.61 / 9.81 | 0.0 | 12 | 58.3M | 24.29 | 11.59 |
| q38d27x (Qwen3.8-27B) | 4xH100, effort **xhigh** | 32 | 9.63 / 9.70 / 9.55 | 0.0 | 0 | 79.0M | 24.04 | 12.82 |
| qfn38b (Qwen3.8-Flash-Next) | 4xH100, effort **xhigh** | 32 | 9.54 / 9.47 / 9.63 | 0.0 | 14 | 128.3M | 39.54 | 7.72 |
| mimo26f (MiMo-V2.6-Flash) | half B200 | 32 | 9.30 / 9.17 / 9.46 | 2.8 | 1 | 90.2M | 20.26 | 14.69 |
| glm53f (GLM-5.3-Flash) | half B200 | 32 | 9.13 / 9.24 / 9.01 | 0.9 | 1 | 57.4M | 20.26 | 14.42 |
| mmm3m (MiniMax-M3), infra-affected | 8xH100 | 29 | 8.79 / 9.05 / 8.47 | 0.0 | 23 | 98.7M | 158.31 | 1.61 (not comparable) |
| q36a3 (Qwen3.6-35B-A3B) | 1xH100 | 32 | 8.02 / 8.11 / 7.91 | 11.1 | 10 | 72.2M | 5.64 | 45.48 |
| step37f (Step-3.7-Flash) | 4xH100 | 32 | 7.32 / 7.41 / 7.22 | 10.3 | 100 | 297.5M | 54.72 | 4.28 |
| gr42d8x (Granite-4.2-8B) | 4xH100 | 32 | 6.54 / 6.47 / 6.61 | 5.6 | 69 | 180.0M | 43.81 | 4.77 |
| q35d9h (Qwen3.5-9B) | 1xH100 | 32 | 6.04 / 6.08 / 5.99 | 25.0 | 4 | 230.1M | 7.66 | 25.22 |
| ms4 (Mistral-Small-4) | 2xH100, role hook | 32 | 4.74 / 5.17 / 4.26 | 24.1 | 54 | 118.9M | 30.20 | 5.02 |

**Rows:** rows use the corrected results.json snapshot dated 2026-10-12 02:29Z and LOG.md's "Corrected results"
section. The ladder operator was still updating files when this was written, so this cites snapshot 02:29Z.

**Column notes:**
- **Critical %** counts eligible sessions once, applying majority voting over each session's grades with ties
  counted critical.
  - Critical rates are q36a3 11.1%, mimo26f 2.8% and glm53f 0.9%. The other displayed rates are unchanged from
    the earlier observation-rate table.
  - Eligible-session denominators are 94 for ds41fl and mmm3m, 107 for step37f, and 108 for the other arms.
- **Recorded input** uses the final cumulative usage counters once per session. Unknown-usage sessions remain
  excluded; all of them are nonzero exits (ds41fl 12, qfn38b 14, mmm3m 7, step37f 4, gr42d8x 13, ms4 4).
  - The earlier figures in this entry summed cumulative counters across turns, and were inflated.
- **step37f cost:** actual GPU lifecycle cost $54.72; quality points per dollar 4.28. Billing ends at the
  20:51:45Z Spot preemption.

**mmm3m** is a merged, infrastructure-affected row: mmm3c chains plus an mmm3d rerun of 40 chains. 14 of its sessions
are excluded as infra-invalid after bridge outages. It remains unsuitable for a clean cost comparison, because its
lifecycle cost includes the outages and reruns (a clean run would be about $85).

**Named deviations** (`LOG.md`):
- q38d27x and qfn38b ran at effort xhigh, because the server rejects "high". They are not directly comparable with the
  effort-high rows.
- ms4 ran through the `devrole.py` role rewrite and a chat-completions bridge.
- ds41fl excludes 7 chains (14 sessions): the harness attached images that its vision-off serving rejected
  (`excluded.json`).
- step37f, qfn38b, gr42d8x and q38d27x ran on substitute or larger shapes.
- 1 step37f session is excluded for a Spot preemption.

**Discarded or superseded attempts** are not in the table but are counted in `costs.json`:
- q38d27 (1xH100, 30 of 108 timeouts), $19.26;
- qfn38 (preempted): its recorded preempted GPU lifecycle cost was $54.73. The separate $85.16 note is inconsistent
  with that lifecycle record and remains unreconciled;
- the mmm3 attempts, $106.54;
- the gr42d8 2xL4 and 1xH100 attempts, $9.26;
- the q35d9 2xL4 attempt, $1.64.

Wave-1 numbers in FINDINGS (q36a3 8.04, mimo26f 9.32, glm53f 9.16) moved by at most 0.03 as more calibration grades
were added.

**Failure classes** (harness signals; detail and fixes in STATEFUL_CODEX_CURRENT_STATE.md section 7, "Harness
robustness backlog"):
- ms4: 40 status-only finals.
- step37f: 1,279 unsigned decimal digit strings supplied to integer fields; 92 rc=1.
- rc=1 sessions that ran exactly 25 turns: q36a3 9 of 10, ms4 48 of 48, step37f 84 of 92 (7 more ran 26 turns),
  gr42d8x 36 of 38, q35d9h 3 of 4 (1 more ran 26).
- A hydration "error" item on every continuation turn.
- Timeouts and capacity saturation on small GPUs. Saturation was observed directly only on gr42d8h; other timeout
  causes are not isolated.

**Reading:**
- On the 29 tasks shared with the eligible ds41fl anchor, quality gaps below the anchor are approximately 0.09 for
  q38d27x, 0.18 for qfn38b, 0.45 for mimo26f and 0.62 for glm53f. The full-table means use different task sets.
  q38d27x and qfn38b also ran at xhigh. No between-model uncertainty interval was computed.
- ms4, step37f and gr42d8x have most of their sessions ending nonzero. Those sessions hold 84.1%, 95.0% and
  74.1% of their recorded input; for q36a3 the share is 13.5%. Those exits include status-only and completion-loop patterns, rejected tool arguments and timeouts. How much
  of the quality gap each class causes has not been isolated.
- q35d9h has only 4 nonzero exits but a 25.0% critical rate.
- No Stateful-off arm was run, so the ladder says nothing about Stateful's contribution.
- The predeclared 3b rerun of ms4 and q36a3 has run, with a control matched on evaluated keys, external prompts and chain order (full operational matching remains incompletely evidenced); see SC-EVAL-044 (final; 3b closed).

Evidence: sc_dogfood/dsg/ladder/results.json, costs.json, excluded.json, LOG.md, finish.log, keys/ and
grading_scores/, runs/<arm>/sessions.jsonl; sc_dogfood/dsg/RUN_LOG.md (2026-10-11 17:15Z onward).

<a id="sc-eval-044-harness-regression-check-original-83eee86b6-vs-control-7c4707230-vs-3b-64e6176ed-pre-grading-2026-10-12"></a>

## SC-EVAL-044: harness regression check: original 83eee86b6 vs control 7c4707230 vs 3b 64e6176ed (2026-10-12)

**Status: final; 3b closed.** None of R1-R4 triggers a revert, so 3b (`64e6176ed`) stays, with the follow-on
`9eddc1af4` (warn once per run; status-phrase sequences). R1 is 0/317 only under a post-outcome adjudication
interpretation; see the disclosure below. Usefulness is not established by these criteria. The gate-bypass causal
check has run and strongly supports the acceptance completion gate decision as the main cause of the q36a3
exit-status regression on 20 t1/s1 keys, within the scope limits below. One item is OPEN (q36a3 critical errors).
The pre-grading text that follows is kept as recorded; the final sections at the end supersede its pending items.

This is the predeclared 3b rerun (FINDINGS 2026-10-11, "3b ladder rerun: revert criteria", Amendment 2). ms4
(Mistral-Small-4) and q36a3 (Qwen3.6-35B-A3B) ran the frozen r2+r3 tasks, 108 sessions per arm, on a control
bundle 7c4707230 (matched on evaluated keys, external prompts and chain order; full operational matching remains incompletely evidenced) and on the 3b bundle 64e6176ed. They are reported beside the original 83eee86b6 ladder arms
(SC-EVAL-043). Git confirms 12 commits between control and 3b: the 11 3b commits plus the test timing fix.

A Codex error hunt (`sc_dogfood/integrate/reviews/threeway_hunt.md`) independently parsed the raw logs. All stored
descriptive counts reproduce; its corrections concern definitions, interpretation and provenance. The corrected
wording is:
- **R2:** "R2 inputs on 108 identical session keys: B=14 section-3 status-only session finals, C=19; 4B=56≤57=3C.
  Coverage is 14/14=100%. R2’s revert condition is not triggered. Separately, raw answer-file closed-set counts are
  13 and 19."
- **Regex counts:** "Completion/acceptance-related router-error lines, selected by the documented keyword regex:
  424 / 2000 / 2155. These are error-line counts, not counts of rejected completion attempts."
- **Operational identity:** "Control and 3b have identical evaluated keys, retained external prompts and chain
  order, and documented matching nominal serving and runner configurations. Full operational matching remains
  incompletely evidenced; the manifests are not identical apart from the bundle."
- **False positives:** "Confirmed false positives: 0; reviewed diagnostic dispositions: 0/317; unknown: 317. The
  false-positive rate is undetermined. R1 and R4 remain pending."
- **Denominators:** "Each arm has 108 independently reconciled unique sessions. Original-arm denominator ledgers are
  retained; new-arm denominators were reconstructed from logs because their prescribed `sessions.jsonl` files are
  absent."

Verified counts:

| Metric | ms4 original | ms4 ctl | ms4 3b | Qwen original | Qwen ctl | Qwen 3b |
|---|---:|---:|---:|---:|---:|---:|
| Unique sessions | 108 | 108 | 108 | 108 | 108 | 108 |
| Tasks | 32 | 32 | 32 | 32 | 32 | 32 |
| rc0 | 54 | 29 | 26 | 98 | 3 | 2 |
| rc1 | 48 | 75 | 70 | 10 | 79 | 79 |
| rc124 | 6 | 4 | 12 | 0 | 26 | 27 |
| Nonzero exits | 54 | 79 | 82 | 10 | 105 | 106 |
| Sessions with failed turn | 0 | 1 | 1 | 0 | 0 | 0 |
| Failed turns | 0 | 1 | 1 | 0 | 0 | 0 |
| Hydration-notice sessions | 102 | 101 | 101 | 107 | 101 | 102 |
| Hydration-notice sessions with rc0 | 52 | 27 | 22 | 97 | 3 | 2 |
| Started turns | 1436 | 1306 | 1488 | 548 | 346 | 326 |
| Mean turns/session | 13.30 | 12.09 | 13.78 | 5.07 | 3.20 | 3.02 |
| Sessions with ≥25 turns | 48 | 44 | 45 | 9 | 2 | 2 |
| Turns without nonempty final candidate | 2 | 0 | 11 | 1 | 0 | 0 |
| Router-error lines | 497 | 971 | 816 | 659 | 3156 | 3295 |
| Completion/acceptance regex matches | 224 | 406 | 300 | 424 | 2000 | 2155 |
| Stderr lines containing `acceptance` | 0 | 219 | 199 | 0 | 1607 | 1726 |
| Sessions containing those lines | 0 | 73 | 71 | 0 | 103 | 101 |
| Raw answer-file closed-set finals | 40 | 19 | 13 | 1 | 0 | 0 |
| Tasks with those raw finals | 25 | 13 | 9 | 1 | 0 | 0 |
| Missing answer files | 6 | 5 | 13 | 0 | 26 | 27 |
| Section-3 session-final candidates | 40 | 19 | **14** | 1 | 0 | 0 |
| Section-3 status-only turn candidates | 590 | 348 | 317 | 9 | 0 | 0 |
| Raw closed-set turn candidates | 590 | 348 | 316 | 9 | 0 | 0 |
| Accepted answer-block turn candidates | 0 | 33 | 29 | 0 | 3 | 3 |
| Deliverable-diagnostic turns | 0 | 0 | 317 | 0 | 0 | 0 |
| Diagnostic-bearing sessions | 0 | 0 | 23 | 0 | 0 | 0 |
| Diagnostic turns outside section-3 set | 0 | 0 | 0 | 0 | 0 | 0 |
| Established eligible status-only turns | 0 | 0 | 317 | 0 | 0 | 0 |
| Unresolved status-only turns | 590 | 348 | 0 | 9 | 0 | 0 |
| Established eligible session-final turns | 0 | 0 | 14 | 0 | 0 | 0 |
| Unresolved session-final turns | 40 | 19 | 0 | 1 | 0 | 0 |

Required hand-checks (every disposition unknown): diagnostic false positives 317 (ms4 3b), short session-final
answers 13 (ms4 3b), pointer candidates 6 / 5 / 6 / 11 / 11 / 15, R4 causal reviews 22 (ms4 3b).

**Qwen failure modes** (control; the evidence points strongly to intervening harness behavior, but does not isolate
one commit's aggregate effect):
- **Plain questions become blocked after acceptance rejection.** `qaconcept__t2/s1` exits rc1 after 211 seconds:
  "completion refused; the run stays running (refusal 1 of 3 without acceptance progress before the host blocks it
  as partial). Unmet acceptance gates: C1 ... omission proposal awaiting review". The host tool output records
  `status:"blocked", revision:20`; the final explains that the floating-point answer was already supplied.
- **Passing checks can remain unavailable in non-Git tasks.** `csv__t1/s1` runs checks returning exit code zero and
  `OK`, but the acceptance ledger records `outcome:"unavailable"`: "the check changed files no criterion pins, or the
  workspace could not be identified, so its effects are unknown". The CSV snapshot contains no Git repository. Stderr
  repeats "nothing changed: C1 needs checker files (the tests or scripts the command runs) and pinned artifacts
  before its plan can be admitted". The session ends rc124.
- **Acceptance repair produces schema and revision loops:** "nothing changed: changes[0] needs text for this action"
  (189 times across control), "nothing changed: changes[0] does not take statement for this action", "openIssues is
  only valid when status is completed". Bayes control ends blocked while reporting unmet framework-generated
  criteria.
- **Some timeouts involve ordinary model work.** `tests__t1/s1` is still in its first turn, debugging escaped
  JSON-pointer tests, near the wall-clock deadline; nothing shows that acceptance rejection caused it.

Relevant changes in `83eee86b6..7c4707230`: `fb148cb38` (acceptance ledger and transactional completion gate),
`7b4d3a778` and `522e729c5` (host-admitted plans need pinned artifacts and checker files), `777fac767` (non-Git
workspace enumeration removed; a non-Git root makes identity unavailable), `e1f577641` (ledger-free completion
exemptions removed), the refusal path (three refusals without semantic progress can block the run) and exec status
handling (Blocked contributes to rc1). The non-Git interaction does not prove that every unavailable receipt has
that cause.

Timeouts alone cannot explain Qwen's original-to-control change: the 95 fewer rc0 sessions correspond to 69
additional rc1 exits and 26 additional timeouts. Only two of control's 79 rc1 sessions reach 25 turns. Acceptance
lines occur in 76/79 control rc1 sessions and 25/26 control timeouts, but also in 2/3 control rc0 sessions, so
presence alone is not causal proof. HTTP-451 transport messages occur in all 108 sessions of every audited arm, and
hydration notices are abundant in successful original sessions; neither is a new explanation.

**Environment comparison:**

| Factor | Verified evidence and limitation |
|---|---|
| Run time | Original Qwen: Oct 11, 18:18–18:36Z; original Mistral: 20:07–21:38Z. 3b: Oct 12, approximately 05:09–06:33Z. Control: approximately 13:48–15:27Z. |
| vLLM | Control/3b manifests specify the same `c1c9f6fd…` digest. Raw 3b captures confirm it. Original equality is operator-reported; equivalent original raw manifest captures are absent. |
| Bridge | Raw 3b capture: LiteLLM 1.104.2, four workers, request timeout 1800; Qwen default path, Mistral chat-completions plus devrole v2. Control reports matching setup, but lacks equivalent raw captures. |
| Runner | Control/3b document on-demand `n2-standard-64`, 64 chains, high effort. Control adds shutdown safeguards. |
| Tasks | All 108 external request texts match across original/3b and control/3b for both models. Control/3b chain files are byte-identical. Full control seed/archive hashes remain unverified. |
| Spot | GPU nodes are Spot; control follows intentional restarts. No retained evidence establishes a preemption during these six measured arms. Absence of a complete event ledger limits that conclusion. |
| Timeout | rc124 sessions end at 3602 seconds, except one Qwen 3b session at 3603. The runner imposes a 3600-second wall timeout. |
| Cache/load | Restarts separate serving/cache lifecycles; equivalence of cache and request load is unverified. Mistral 3b additionally shared serving with one human-output session, 05:09:50–05:48:44Z. |
| Patch boundary | Git confirms 12 commits between control and 3b: the 11 3b commits plus the test timing fix. This establishes the source boundary, not full operational matching. |

**Conclusions supported before grading** (verbatim from the hunt):
- "The descriptive three-way counts reproduce. Mistral’s raw closed-set session finals are 40/19/13; section-3
  session-final counts are 40/19/14. All 317 observed Mistral 3b status-only turn candidates receive the specified
  diagnostic."
- "With corrected section-3 inputs B=14 and C=19, R2’s numerical revert condition is not triggered. This does not
  establish improved usefulness."
- "R1 and R4 remain pending because required hand-check dispositions are absent. R3 remains pending before matched
  grading. No overall shipping verdict is established."
- "The Qwen original-to-control regression contains directly evidenced acceptance-mediated blocked runs and
  acceptance-repair loops, alongside timeout failures. Timeout growth alone cannot explain the rc0 drop. The
  historical comparison does not identify the acceptance gate’s individual aggregate effect or exclude environmental
  contributions."
- "Control and 3b have verified matching keys, external requests and chain order, plus documented matching nominal
  configurations. Full operational equivalence is not established by the retained manifests. Neither improved answer
  quality nor diagnostic-caused failures may be concluded from this table alone."

### 3b revert criteria R1-R4 (final)

Sources: operator `sc_dogfood/dsg/ladder/measure_3b.md`, Codex hunt `sc_dogfood/dsg/reviews/3b_w8_errorhunt.md`,
adjudication hunt `sc_dogfood/integrate/reviews/adj_hunt.md`; FINDINGS "3b R1 ADJUDICATED and 3b CLOSED"
(2026-10-12).

- **R2: not triggered.** Body-aware B=14 vs C=19 (the raw `-o` count B=13 is kept separately).
- **R3: not triggered.** ms4 ctl 5.435 vs 3b 5.177, ctl−3b +0.258; the paired 3b−ctl mean is −0.258 (per-task bootstrap 95% [-0.889, 0.367]);
  q36a3 ctl 7.762 vs 3b 7.966, −0.204.
- **R4: not triggered.** 0 of 22 reviewed sessions were caused by the diagnostic (22/22 no; 0 failed turns).
- **R1: not triggered under the adopted interpretation.** "R1 adjudication is complete under the coordinator's
  post-outcome file-deliverable interpretation: 0/317 false positives (0%), zero warned correct-short-answer
  sessions, and zero unknown R1 dispositions. All 39 initially positive diagnostic items and the one initially
  positive short-answer item received 'no'. R1's revert condition is not triggered under that interpretation."
  The first checker's result is retained: 39/317 (12.303%), all from three file-deliverable sessions ending in a
  bare "Task completed", with one warned correct-short-answer session. The second checker rejected all 16
  re-checked positives.
- **Adjudication disclosure (verbatim):** "The coordinator adopted this file-deliverable interpretation after
  seeing the first checker’s 39 positive classifications and the second checker’s rejection of all 16 positives
  rechecked. The third checker applied that interpretation with arm identities withheld. This introduces potential
  outcome-informed adjudication bias; it does not establish intentional bias. Under the adopted interpretation, R1
  is 0/317 false positives and zero warned correct-short-answer sessions. The first-checker result remains 39/317
  (12.303%), with one warned correct-short-answer session. The numerical thresholds were unchanged; the operational
  interpretation was specified after outcomes were visible."
- **Limitations:** the 3b warning was visible in 23 ms4_3b grading transcripts (imperfect blinding); graders
  treated answers given only in earlier turns inconsistently (28 cases, both arms); ctl-vs-3b binary identity rests
  on tarball sha256 only (the recorded hash was the bin/codex wrapper; codex.real was not hashed; ctl runners
  deleted).

**3b verdict:** 3b (`64e6176ed`) stays, with the follow-on `9eddc1af4`. Usefulness is not established by these
criteria.

**OPEN:** q36a3 critical-error rate rose from 3.7% (ctl, 4/108) to 9.3% (3b, 10/108). No registered trigger; it may
be noise. It is to be checked explicitly in the slice-5 matched release rerun.

### Gate-bypass causal check (q36a3, 20 t1/s1 keys)

Codex hunt: `sc_dogfood/integrate/reviews/gb_hunt.md`. The bypass build is commit
`35847f16902fed2a9f0e0978386e59b2d4a6d07d`, parent `7c470723062e42f1f279f8bcb80403bbbbba782a` (the control). Its
complete diff changes two files, +15/−4, covering the extension's Proceed decision and runtime acceptance-policy
rejection checks. Guidance and ledger tools are unchanged; verification leases, revision checks and preceding
evaluation machinery remain. The recorded `codex.real` sha256 is
`14c51de6788137961f8d87f52eae181b5798c2db0af161d3d64a502006bb7499`; this is consistent recorded provenance, not an
independently reproduced build. Ctl's real executable was not separately hashed. The predeclared threshold (≥10/20)
was recorded in `sc_dogfood/integrate/EXEC_LOG.md` before the run. The hunt independently selected the first 20
alphabetic task keys and reproduced every per-key field and aggregate.

| Metric | Original | ctl | gb |
|---|---:|---:|---:|
| Sessions | 20 | 20 | 20 |
| rc0 | 16 | 0 | 19 |
| rc1 | 4 | 13 | 1 |
| rc124 | 0 | 7 | 0 |
| `turn.started` events | 166 | 101 | 45 |
| Sessions with acceptance stderr lines | 0 | 19 | 0 |
| Acceptance stderr lines | 0 | 277 | 0 |
| Sessions with literal “completion refused” lines | 0 | 19 | 0 |
| Literal “completion refused” lines | 0 | 32 | 0 |
| Nonempty answer files | 20 | 13 | 20 |
| Summed session seconds | 7,329 | 52,069 | 2,889 |

“Acceptance lines” means stderr lines containing that substring. Zero such lines does not mean no acceptance
ledger: gb’s completed snapshot results still disclose unmet omission proposals.

Corrected wording (verbatim from the hunt):
1. “The bypass result strongly supports the acceptance completion gate decision as the main cause of the q36a3
   exit-status regression on these 20 t1/s1 keys. It does not isolate the effect from load and time, establish
   recovery across the full ladder, or establish usefulness recovery.”
2. “Summed session durations were 7,329 / 52,069 / 2,889 seconds; these are not arm elapsed wall-clock times.”
   Gb’s actual launch-to-last-session interval was 270 seconds, 17:07:52–17:12:22 UTC.
3. “The gate-bypass report was generated by gb/gb_report.py and independently reproduced from sessions.txt, parsed
   JSONL events, stderr, and answer files.”
4. “Gb produced 19 successful process exits and 20 nonempty answer files. Sampled snapshots contain substantial
   deliverables, but some final answer files omit the deliverable or its location. Usefulness has not been
   established by these counts.” Examples: the gb `dbmemo` answer is only an outcome block, though its snapshot
   contains a 670-word `memo.md`; the gb `qaconcept` answer omits the explanation and `math.isclose()` example that
   appear earlier in the JSONL; the gb `memdecide` answer contains the requested substantive summary.

**Load.** The bypass arm ran later, after a GPU restart, at different concurrency. Lower load plausibly reduces
latency and timeout risk, especially for ctl’s seven rc124 sessions (`code`, `csv`, `decisions`, `deck`, `finance`,
`memcontinue`, `postmortem`), and it cannot be dismissed. However, reconstructed snapshot state shows ctl ended
15 Blocked / 5 Running: all 13 rc1 sessions were Blocked, and two rc124 sessions, `decisions` and `memcontinue`,
were already Blocked. Logs show deterministic acceptance problems involving workspace identification, checker
admission, omission proposals and repair contracts, which more throughput does not directly repair. Gb ended
19 Completed / 1 Blocked, with `migration` remaining Blocked. Load alone is therefore a weak explanation for the
entire 0→19 recovery, though this experiment cannot quantify its contribution.

**Fewer turns.** All 45 gb turn starts have matching `turn.completed` events. Sampled snapshots contain `lesson.md`
and `quiz.md`, the database memo, a PowerPoint deck and CSV analysis artifacts; there is no evidence of wholesale
truncation. 89 of the 121 fewer turns come from four original failures that each consumed 25 turns (`math`,
`memcontinue`, `memdecide`, `qaconcept`); their gb total is 11 turns. The reduction therefore substantially
reflects avoided continuation exhaustion, alongside shorter successful runs.

Supported conclusion (hunt): strong evidence for gate-mediated termination failure on this subset, with unresolved
load/time attribution and no demonstrated usefulness recovery.

Backlog: STATEFUL_CODEX_CURRENT_STATE.md section 7, "Acceptance-mediated failures in Qwen control". Evidence:
sc_dogfood/dsg/ladder/measure_3b.py, measure_3b.md, handchecks_3b.json, r3_w8.md, manifests/ (including
manifests/raw/q36a3_gb/), runs/{ms4_ctl,ms4_3b,q36a3_ctl,q36a3_3b,q36a3_gb}/logs/ and snap/, gb/gb_report.py;
sc_dogfood/integrate/reviews/threeway_hunt.md, adj_hunt.md, gb_hunt.md; sc_dogfood/dsg/reviews/3b_w8_errorhunt.md.

## Landing record: slice 1 step F1 (2026-10-12)

Step F1 of the answer-safety slice (inert runtime foundations) landed on `stateful/main` as `f256e307e`: four commits, +1582/−56 across 13 files. It is inert: nothing calls the new obligation operation, and live answer admission is unchanged, pinned by a base-limit fixture. The Linux gate on the pre-merge head (job `20261012T204900Z-79accdb67`) had 0 new failures, 0 known failures and 0 flaky tests across the runtime, extension, exec and protocol crates. The confirmation verdict was LANDABLE WITH NAMED LIMITATIONS; the full record is in the campaign ledger (FINDINGS). The TUI and app-server downstream checks used earlier base logs, not a fresh base rebuild, and are not covered by this gate.

## Landing record: repository cleanup (2026-10-12)

Four commits, `git log 4209ce013..175b77ee1` (`git diff --stat 4209ce013 175b77ee1`: 8 files changed, +10/−460). No behavior change is intended: three of the four commits delete code or files with no remaining reference, and the fourth is documentation.

- `4d6706d6a`, unused prompt files removed: `codex-rs/core/gpt-5.1-codex-max_prompt.md` and `codex-rs/core/gpt-5.2-codex_prompt.md`, 160 lines. A search of the parent tree finds no reference to either filename.
- `d4f74daf7`, the base-instructions prompt now has one source. `BASE_INSTRUCTIONS` in `codex-rs/models-manager/src/model_info.rs` points at `codex_protocol::models::BASE_INSTRUCTIONS_DEFAULT` instead of `include_str!("../prompt.md")`; the duplicate `codex-rs/models-manager/prompt.md` (275 lines) and its `BUILD.bazel` compile-data entry are removed. At the parent commit, `models-manager/prompt.md` and `protocol/src/prompts/base_instructions/default.md` have the same SHA-256 (`ac8ae107a0d7…`), so the served text is unchanged.
- `01aeba725`, unused ConPTY helper removed: `create_conpty` in `codex-rs/windows-sandbox-rs/src/conpty/mod.rs`, 18 lines, marked `#[allow(dead_code)]`. No other `create_conpty` reference exists at `175b77ee1`; the similarly named `create_conpty_handles` in `utils/pty` is a different function.
- `175b77ee1`, master plan reconciled to the landed state (3b landed and closed, P1 landed at `b8dcee46f`, slice 1 step F1 landed at `f256e307e` with activation pending, cleanup landed), and `codex-rs/docs/protocol_v1.md` marked deprecated with `protocol.rs` named as the source of truth.

Verification:

- Linux gate, crate `codex-models-manager`, job `20261012T210519Z-01aeba725` (rev `01aeba72539f…`): 54 passed, 0 failed, 0 flaky, 0 skipped; baseline comparison "0 known (quarantined) failures, 0 NEW, 0 flaky". Source: `sc_dogfood/cloud/ci/runs/20261012T210519Z-01aeba725/summary.txt` and `meta.json`.
- Windows sandbox crate: the `codex-windows-sandbox` compile check finished with `EXIT=0` (log `sc_dogfood/integrate/logs/winsandbox_check.log`). This is a compile check only; no Windows tests were run for this removal.
- Not covered: no other crate was run on Linux for these commits, and the `core` prompt-file removal rests on the absence of references.

## HAL acceptance journey: first harness findings (2026-10-14)

HAL is the offline, on-premises deployment requirement; its document tasks run as Stateful runs. This is the first end-to-end attempt: Marathi-to-English translation of scanned CIDCO documents on Gemma-4-31B (model key `gem31`, served by vLLM on a remote H100 and reached through a Responses bridge). Figures come from the operator's logs in `hal-suite/docs_2026-10-15/` (`logs/stateful_runs.jsonl`, `logs/mt_headtohead_2026-10-15.md`, `BLOCKERS.md`) and are operator-reported, not re-measured here. They come from one model and one set of runs.

**(a) Context.** The first Stateful run (thread `01a13a0a-9efa-7f02-894c-7591767c5458`, one OCR page of 13 blocks) failed with a context-window error: the model was served at 16,384 tokens and the request held at least 16,385 input tokens. The log does not split that total between the Codex harness prompt and the page text. The VM was restarted with `--max-model-len 32768 --kv-cache-dtype fp8` (`vm/sc-models.json`), and the page then ran at a 32,768 window (thread `01a13a16-8497-72d0-a8ce-5720c17bc4e7`, auto-compact limit 26,000). Operator-reported, n=1. This run exceeded the 16,384-token input window. The logs do not quantify the harness prefix separately from the task and OCR text; deployments should measure and budget the complete request, including output headroom.

**(b) Goal-loop root cause** (code-traced at `9eddc1af4`; trace `sc_dogfood/integrate/reviews/hal_goalloop_analysis.md`; FINDINGS entry beginning "[HAL goal loop, code-traced 2026-10-14"). On thread `01a13a16-…` the run did not stop after its first complete 13-object JSON response. Coverage and number checks passed, but the operator’s review identified translation errors.

1. The first message was the full 13-object JSON answer (6,557 bytes) with no `[stateful-outcome]` trailer. `answered.rs:75-78` returns "the final message has no outcome block" and `answered.rs:162-167` returns `NotHandled`, so the run stays Running.
2. The model then emitted 12 byte-identical trailer-only finals. `answered.rs:79-84` rejects them ("no answer precedes the outcome block") because only the current message is parsed. It was not a blank-line, fence, casing or whitespace mismatch.
3. Each rejected final left the run Running, so `autonomy.rs:134-174` and `552-576` claimed another continuation (`storage.rs:282-285`).
4. Two compactions occurred, at rollout lines 66 and 196. Before the first, the retained first answer was already incomplete because of the 900-token capture limit (`core/src/guardian/mod.rs:74`; `core/src/context_manager/history_assistant_delivery.rs:66-86`). At the second, the eight retained assistant records contained only trailers (`history/src/retained_context.rs:24`, `199-207`). Neither replacement history preserved the exact JSON deliverable; the original remained available in the rollout.
5. `exec -o` writes the last message (`exec/src/cli.rs:91-98`), so later prose displaced the JSON. The same run also ended with a separate hydration error (exit 1) on a binary that predates P1.

Fix mapping: slice 1 activation (trailer-free Answered; freeze lines 28-30 and 79-87) addresses the first part, points 1 and 3, and by stopping on the first JSON keeps it in `-o` for this run. It does not fix the retained-answer truncation: exact answer retention across compaction (a bounded durable answer reference separate from the reviewer excerpt) is a new backlog item. Trailer-only declarations for already-running sessions would need a bounded, run-bound answer candidate (optional). (At the time of writing slice 1 activation was pending; it landed at `73f2d82da`.)

**(c) Recovery.** Every Stateful run that produced output needed its answer recovered from the run log rather than read from the harness's final message: the page run (JSON taken from rollout line 11, status `partial`) and all four translation-batch runs (status `recovered`, policy "first assistant rollout message"). The head-to-head used two of those batches.

**(d) Head-to-head (operator-reported).** Gemma-4-31B scored chrF++ 49.3 (95% CI 45.8 to 53.1; paired bootstrap, 1,000 samples, seed 20261015) on n=33 aligned pairs (target 200, not reachable without padding). The Marathi side is raw Tesseract OCR, so the score combines OCR and translation. IndicTrans2 Marathi-to-English is blocked: both `indic-en` repos return HTTP 403 (gated), so there is no head-to-head yet and Gemma is the only scored system. Number exactness is 84.38% (27/32; one pair excluded for a reference issue). Three misses (`mr_en_0005`, `0011`, `0033`) arise from the checker removing digit-internal commas and merging date components (for example 12 and 2007 into 122007). In `mr_en_0031`, the model changes the unseparated OCR digit run `802` to `8.02`; this is a source-number normalization, not a comma-removal artifact. In `mr_en_0013`, it changes OCR `1016` to `2016`, matching the human reference. Both model changes remain flagged under the source-number preservation policy. Throughput is 11.7 tokens/s end to end (101.42 s wall over both batches including harness startup and recovery; sacrebleu 13a tokens, not model tokens) on a remote H100 with an fp8 KV cache; this is not a controlled hardware comparison.

**(e) Image-input gap.** Plain `exec` had no `-i` image flag, so the OCR correction step was text-only and the model could not check the page. The operator’s review records errors that remained in this text-only run, including ‘FHA Point’ for ‘Nariman Point’; image-assisted correction was not tested. The flag for plain `exec` has now landed on stateful/main as `bbb9146b1` (commits `f3ced422a` and `bbb9146b1`; Linux gate job `20261014T135331Z-38775f78f`, codex-exec 179 pass, 0 new, 0 flaky).

## Landing records: image flag, workspace identity stages 2C, 2A and 2B (2026-10-14)

Status: a fresh Codex error hunt of this section (`sc_dogfood/integrate/reviews/eval_landings_hunt.md`) found four errors and several missing qualifications; all are applied below. Figures come from the cloud-gate summaries under `sc_dogfood/cloud/ci/runs/<job>/` and from `git` at `c5a8f39eb`.

**Exec image flag** (`f3ced422a`, `bbb9146b1`). Plain `codex exec` accepts `-i/--image`, which the HAL image-reading path needs. Linux gate, job `20261014T135331Z-38775f78f`, crate `codex-exec`: 179 passed, 0 new failures, 0 flaky. The gate ran on the branch head before landing.

**Stage 2C, directory-boundary check execution** (`8cf034a07` to `36dacb2bf`, four commits, +1027/−74 across 22 files). Check directories are normalised against the first root, canonical checker targets are admitted, and declared artifact and checker spellings are preserved. Review verdict: landable with named limitations. Linux gate, job `20261014T141751Z-a2fadd96c`, lane s2c, crates `codex-stateful-extension` and `codex-stateful-runtime`: extension 250 passed, runtime 63 passed, 0 new failures, 0 flaky. Open nit: a positive tool-path integration test for the host-root handoff (not yet written).

**Stage 2A, typed workspace identity for git roots** (`850643841` to `e9316e0eb`, ten commits, +1439/−217 across 12 files relative to the 2C head). Review verdict: landable with named limitations. First gate, job `20261014T141344Z-0f401c434`: runtime 57 passed; extension 266 passed and one new failure, `w18_dangling_ancestor_is_not_absence`, whose expected kind was Windows-only (on Linux a dangling-symlink ancestor reports `EscapingLink`; both kinds refuse the entry as the design requires). Test-only fix, then job `20261014T144326Z-b321b5ad8`: extension 267 passed, runtime 57 passed, 0 new failures. Because 2A and 2C were gated on separate bases, the combined head was gated before the push: job `20261014T144543Z-e9316e0eb`: extension 270 passed, runtime 63 passed, 0 new failures.

**Stage 2B, bounded content identity outside git** (`4bd1da0ed` to `c5a8f39eb`, 10 commits including the integration fixes, +1475/−52 across 16 files relative to the 2A head). The first review stopped it with three blockers (an invalid `.git` directory fell through to parent discovery, the deep-path fixture exceeded OS path limits, and overflow eligibility depended on enumeration order). A repair round fixed them. A confirmation review then found one more blocking defect (the entry bound was checked before duplicate keys were recognised, so overlapping roots were order-dependent); a narrow fix and an unreadable-root-marker regression were added. Landing needed four further test-only integration fixes because the 2B tests predate 2C's added `project_root` argument and the Python witness needs the host root and the `python3` interpreter on Linux. Four earlier gate runs failed before the final one: two did not compile (jobs `20261014T152744Z-125f77db6` and `20261014T153426Z-fc59bd122`, a changed call signature: three call sites missing the new argument, then two) and two failed the Python witness (jobs `20261014T153535Z-b79d4d33e`, checker files unreadable without the host root, and `20261014T153733Z-cc6b4f7d1`, no `python` binary on Linux); logs are `integrate/logs/s2b_gate.log` to `s2b_gate4.log`. Final gate, job `20261014T153925Z-c5a8f39eb`: extension 299 passed, runtime 63 passed, 0 new failures, 0 flaky. Limitation: the narrow fix for the confirmation blocker was verified by the cloud gate only; a fresh Codex re-read of it is still owed. The 5,000-level deep fixture runs only on Windows and uses elevated test limits (128 MiB retained-path bytes, a 32,000-byte pathname limit, a 900-second identity budget), so it is not a success under the production caps; the portable 200-level coverage also injects limits; production caps are unchanged. The forced-order overlapping-root regression passed in the final gate. The unreadable-root-marker test also reported a process pass, but it returns early when its capability preflight fails, so that pass does not establish that its denial assertion ran. Marker-case limitation: an embedded `.GIT` directory on a case-insensitive filesystem can be walked as ordinary content instead of refused as an embedded repository.

**Qualifications from the reviews.** Workspace identity is not an atomic snapshot: accepted replacement (ABA) races remain, blocking filesystem calls can retain worker slots, and inherited stdout can retain detached reader threads (`integrate/reviews/slice2a_review.md`, `integrate/reviews/slice2b_impl_report.md`). The identity gates ran the extension and runtime crates only: app-server adaptations and public exec acceptance journeys were not gated for these stages (the image flag's exec gate is separate), and the 2C review's baseline-red classifications are code traces. Windows coverage differs from the Linux gates: local Windows junction and ACL witnesses ran, but native symlink privilege preflights failed, and five filtered repair test processes returned after failed preflights and are not behavioural passes.

**What these landings do not show.** No benchmark or user-journey result exists for the new head: the Luna hands-on run below was run while the checkout was at `c753a903d`, before any of them. The workspace-identity work is the precondition for the release check (matched 216-session rerun) and has not been exercised by it.

**Hands-on summary (Luna, `gpt-6-luna` high, a copy of the local debug executable tested while the checkout was at `c753a903d`, 2026-10-14; hunt-corrected; `campaign2/handson_20261014/RESULTS.md`).** The executable's hash and its equality to the local debug build were established; its exact build-source identity was not independently established (neither its version output nor embedded strings name the commit). Five scenarios, run as a user: plain question PASS; CSV task in a folder that is not a git repository FAIL (work correct, the check was recorded as unavailable, run Blocked, exit 1); coding fix in a git repository PARTIAL (fix correct, exit 1, run Blocked after acceptance repair failed, a stray `check.sh` left behind); multi-session memory PARTIAL (explicit `/memory add` saved retained rules and a fresh thread recalled them; the tested natural-language message saved nothing in exec; after one Forget, duplicate lint entries were still active, so that observation alone does not isolate the cause; only after all four lint entries were tombstoned did a cold thread still state the rule although its active memory packet omitted them, which points to retained conversation continuity, a known limitation; an earlier apparent input bug was a test-driver artifact and is retracted); interrupt and resume PASS (TUI resume only; `codex exec resume` was not tested). The PASS labels omit user cost: the plain question logged a completion refusal and showed a visible outcome footer, and the resume scenario added four acceptance-related commits and two committed helper scripts and took about 6 minutes 24 seconds after the fifth requested commit to complete. Stage 2 targets the non-git failure (the check recorded as unavailable because the workspace could not be identified) and directory and checker boundary problems. The git scenario's Blocked ending followed acceptance-repair failures (artifact and checker overlap, absolute-path declarations) although its host receipts passed and its identity worked; nothing here shows these landings resolve that failure chain or Blocked endings in general. None of the journeys was re-run on the new head.

**Argon probe note (hosted model, reference only).** The probe's non-zero exits and two product items (long tool-call IDs, a stream-retry request ending on a model turn) are recorded in `campaign2/FINDINGS.md`; none is built or landed at this point.

## Hands-on at the new head (2026-10-14; Luna gpt-6-luna high; hunt-corrected; `campaign2/handson_20261014b/RESULTS.md`)

Executable: a copy of the local debug build (sha256 `1cabe2ef7c53...`) made with the checkout at `13b646183`; the build commit is inferred, not embedded, and the companion code-mode host differs from the prior run's. Four scenarios, one run each. Plain question: pass, with a refused completion and a visible outcome footer. CSV task in a plain folder: pass, exit 0 in 108 s against exit 1 in 215 s before; the run is Completed with an acceptance basis and nine passing host-command rows, where the earlier head recorded all seven as unavailable and ended Blocked. That is one run: the executable and host changed together, the earlier scenarios ran concurrently and this one sequentially, and nothing here attributes the change to a specific commit or shows the defect is eliminated. Git coding fix: pass with a caveat, exit 0 in 236 s against exit 1 in 445 s before; fix correct, but the run ended Completed through the answered-unverified path, its stored note says no criterion was checked although five passing receipts exist, the receipts describe older workspace generations, and the model deleted its checker script before completing. Image input (`exec -i`): pass; the answer reads the attached image. Memory and resume were not re-run on this head.

## Landing records: Argon extension fixes, stream-retry filtering, run-goal error, 2C tool-path test (2026-10-14)

Status: figures come from the cloud-gate summaries under `sc_dogfood/cloud/ci/runs/<job>/` and the logs in `sc_dogfood/integrate/logs/`; this section has not yet had its own fresh error hunt (see the next section when it is added). Origin: the Gemini 4 Argon probe (hosted model, reference only; `campaign2/FINDINGS.md`, section dated 2026-10-14 on the Argon harness findings, hunt-corrected there).

**Extension fixes** (stateful/main `b8aa1e5ce` to `40e68b588`, nine commits; the code tree equals the gated `af5a74f1f`). A tool call ID longer than the 512-byte provenance limit, or one containing control characters, padding or the reserved prefix, is now recorded as a fixed 88-byte digest source (`stateful:call:v1:sha256:` plus the SHA-256 of a domain-separated hash input), deterministic and collision-resistant but not injective; ordinary short IDs stay verbatim. The same mapping applies at the pending-command and host-evidence database boundaries, in-memory correlation keys and read-receipt hashes stay raw, and replay of entities stored with a raw ID still works because stored provenance is not rewritten. `openIssues` is accepted on a blocked update (running and failed still reject it) and the blocked reply is bounded to the response allowance with exact paging of the stored result through `stateful_run_read`; acceptance-tool rejections now name the action and the fields it takes (wording only, no validation tightened). Gates: first run job `20261014T161730Z-ad52fc0ed` had two new failures (the direct tool roster at 11,575 bytes over the 11,500 ceiling because of added schema text, and the blocked reply echoing the whole stored result over the 9,000-byte item bound); after a repair round, job `20261014T163936Z-af5a74f1f`: extension 309 passed, runtime 65 passed, 0 new failures. Review: STOP/CUT on the first version, then landable with named limitations: a very small response allowance still commits the blocked update without returning a retrieval cursor, and the read tool's description does not name blocked results. Not done: `just bazel-lock-update` after the extension's two new dev-dependencies on workspace crates (Cargo.lock edges added; no `MODULE.bazel.lock` change was seen by the reviewer, not verified by running the command).

**Stream-retry request filtering** (`1f329a28b`, one commit in `codex-rs/core`). When a response stream fails after a completed reasoning item, core had kept that item in history and the retry request ended on it; a provider bridge that rejects a request ending on a model turn then failed every retry. On a retry of the same sampling request, the trailing reasoning appended by the interrupted response is now omitted from the request copy only (durable history, rollout and tool accounting unchanged; first attempts and other trailing items unchanged). The commit body states that this behaviour is shared with upstream Codex and may conflict with the next upstream sync. Gates: job `20261014T164228Z-0526358a0` and a later run failed two of the new tests on assertions only (the reasoning items carry null content and per-turn metadata that the fixtures lacked; production was not at fault); after test-only repairs the filtered `codex-core` job `20261014T165928Z-cebf99433` passed 344 tests, 0 new failures. The full `codex-core` run, job `20261014T170107Z-cebf99433`, had 4,932 passed and 7 failed: 4 known quarantined, and 3 not in the quarantine baseline (`suite::remote_models::disabled_update_plan_preserves_custom_catalog_instructions`, `suite::remote_models::model_catalog_url_supplies_conversation_model_and_instructions`, `suite::scenarios::model_catalog_refresh_preserves_tools_and_history`), which also fail on the unmodified base `c5a8f39eb` and on `4209ce013` (before the cleanup landing), so they are older than this change and uninvestigated. Review: landable with named limitations (the test helper deletes reasoning `content` unconditionally; no runtime coverage of the request after a recovered retry, which may fail an incremental-prefix comparison once). Not verified: the recovery against a real Gemini bridge, which waits for the Argon rerun with the new bundle.

**Run-goal error** (`8ad3b2148`, one commit, two runtime files). A rejected run goal now names the failed condition (empty; over the 16,384-byte UTF-8 limit with the actual length; contains a NUL; leading or trailing whitespace); the JSON-RPC code stays -32602. Gate, job `20261014T171907Z-e47bb5d10`: runtime 67 passed, extension 309 passed, 0 new failures. Review: landable with named limitations (no test at exactly 16,385 bytes or for whitespace-only input; no app-server `stateful/run/start` test pinning the new message; app-server tests were not executed for this change).

**2C tool-path test** (`880145bb7`, test-only, 235 changed lines). Closes the open 2C review nit: tool-level tests now drive the acceptance tool through the first-root normalisation, second-root rejection, no-root and non-local refusal, and rejection of a model-supplied `projectRoot`. Gate, job `20261014T173448Z-6ef294ae9`: extension 311 passed, runtime 67 passed, 0 new failures. Review: landable with named limitations (mutation sensitivity was traced, not run; the remote case tests the environment-ID gate, not a remote transport; no symlink or alternate-case fixture).

**Bundles** (Linux x86_64, built from a clean checkout by the ladder operator, tarball sha256 verified): `c5a8f39eb` tarball `456028466457259c73061519d62d6d03bf095f3ede68d604dc62b3cf6bfdd0e4`; `40e68b588` tarball `e2b1d6483ea23f25301e91456c9b13a9d63967d9ee7dafcb23141b1908fa22ef`; `1f329a28b` tarball `3fa0765a168d2ce3292064c27ecfc77454f54d1317ab83a0f074d59621b98b46` (paths under `gs://sc-bench-iqidis-artifacts/builds/<sha>/`). None of the later landings had a bundle when this was written (snapshot; bundles for `73f2d82da`, `b59589c84` and others were built later the same day; see the note at the top).

**Still open.** At the time of writing slice 1 answer-safety activation was not landed (it landed later the same day at `73f2d82da`; see its landing record below). At the time of writing the Windows-side hands-on user test on the retry, goal-error and Argon-fix landings had not been run (later hands-on runs on descendant builds are recorded below).

## Landing record: slice 1 answer-safety activation (2026-10-14; not yet error-hunted)

Status: figures come from the cloud-gate logs `integrate/logs/s1a_gate*.log` and `s1a_gate_down2.log` and the reviews `integrate/reviews/s1a_review.md` and `s1a_confirm.md`; this record has had no fresh error hunt yet.

**What landed** (stateful/main `7fdb4251d`-equivalent series ending at `73f2d82da`, 16 commits applied by cherry-pick on top of `1e25a1614`; the `codex-rs` and `sdk` tree equals the gated `a783e6acf`). The activation switches the changed terminal routes on using the earlier inert foundations: a plain substantive final message in an Autonomous run now ends the run through the host-answer transaction as Answered, or as Partial when required criteria or blockers remain open, with the unmet items disclosed in the result; a trailer-only or status-only final keeps the run Running; a valid answer trailer overrides model progress; `obligation_update` gains `resolvedIssues` (with a reserved-prefix refusal); `openIssues` on a completion in an Autonomous run are retained for a Partial final answer instead of blocking the run; and the wire carries an answer outcome field. The implementation was started by a Droid that stopped without a report after four commits; Codex assessed the state against the frozen design, finished it and rebased it on `880145bb7`.

**Review and repairs.** The first fresh review stopped the series (STOP/CUT) on one defect: the Autonomous redirect discarded blockers submitted in `finalObligation.blockers`, so a run could end Answered without a blocker it had declared. The repair persists them through the redirect (a runtime-operation fix plus witnesses crossing contract and no-contract with empty and nonempty `openIssues`, with replay), adds a repeated-identical-trailer witness, changes the earlier app-server test of a refused check followed by a plain final to expect the frozen Partial outcome (it had expected Running; the old outcome passes on the base, so activation changed it, and the freeze requires Partial), and corrects the Python generated outcome type (a one-line hand edit matching the new generator postprocessor; full generation was not run). The confirmation review: landable with named limitations. A one-token compile error in the first repair and a roster byte-ceiling failure in the series (the direct tool roster grew past 11,500 bytes from the longer `openIssues` text; shortened to +1 byte) were caught by the cloud gates.

**Gates at the landed tree (`a783e6acf`).** Runtime 72 and extension 324 passed, 0 new failures (job logged in `s1a_gate6.log`). Downstream, one run earlier on the same code apart from a test literal: `codex-app-server` 161, `codex-app-server-protocol` 314, `codex-exec` 182, `codex-tui` 268 passed, 0 new failures (filtered app-server and tui sets, `s1a_gate_down2.log`). The deferred `obligation_update.resolvedIssues` adds 170 bytes to the total roster; ceilings 11,500 direct and 20,400 total hold.

**Not measured.** The release checks G1, G2 and G3 (matched-session rerun, floors, trivial-Blocked count, and the mean-turns fallback against both baselines) have not been run; the activation's effect on the open-model regression is therefore not yet measured. The Python SDK was hand-edited, not regenerated. Landing used a cherry-pick because applying the series as patches failed on a binary schema fixture; no content was altered.

## Hands-on at the activation head and the open-model completion question (2026-10-14; hunt-corrected where marked)

**Hands-on (Luna gpt-6-luna high; a copy of the local debug build tested with the checkout at `2e67cf5f8`, code head `73f2d82da`; hunt-corrected; `campaign2/handson_20261014c/RESULTS.md` and the matching FINDINGS section).** A plain question ends Answered in 14 s with no stderr and no footer. An always-failing check ends exit 0 with a stored Partial and no Blocked loop, but in the prompt as briefed the model rewrote the checker and said so while its footer claimed no open issues; a modified prompt that forbids touching the checker gave an honest Partial. The CSV task in a plain folder, three runs: exit 0 each, all evidence rows passed, one ended verified Completed and two ended Partial for ledger-internal reasons; one of those told the user a required tool was unavailable although the rollout shows promotion attempts. The git coding fix ended verified Completed (exit 0, 190 s) and left an admitted checker in the folder. Image input read the sign (with an added period inside the quotation). Memory: exec now saves statements as proposals, cold sessions answered correctly mainly through conversation continuity, and Forget did not remove the rule from a new thread's recall. Interrupt and resume continued without repeated steps but ended Partial with an extra checker commit. The over-long-goal error names the byte count and limit. All results are one run per scenario (three for the CSV task) on a loaded machine with a different executable than earlier runs (the companion host was unchanged), so no timing or causal claim about slice 1 follows.

**Why open-model sessions record no accepted completion on bundle P (single-pass Codex read of the code and 11 sw07P rollouts; not independently hunted).** The audit's zero counts (0 of 1,113 on P, 0 of 1,764 on H against 1,213 of 2,523 on B) measure verified Completed only. At this head an Autonomous run can end Answered or Partial with no completion call, so the audit does not measure ordinary successful answering. The sampled failing calls fail first on a missing `openIssues`, then on omission proposals, checks the plan admission cannot admit (shell expressions, `python -m`), stale receipts and unknown-effects refusals; one concrete recovery trap (a check that has run cannot be replaced) and misleading "valid completion calls" guidance are recorded as defects. A falsifiable prediction for a ten-session check on an activated bundle (Completed 0, Answered 10 for ordinary tasks) is recorded in FINDINGS; at the time of writing no such check had been run (two registered Gemma checks ran later; see the post-fix section).

## Post-fix check and hands-on at the usability head (2026-10-14; the second is hunt-corrected)

**Registered post-fix check (bundle `b59589c84`, ten identical prompts, same Gemma 4 31B node, fresh bridge key; executed by the ladder operator).** Actual equals the registered prediction on every count: Completed 0, Answered 10, Partial 0, Blocked 0, nonzero exits 0, no refused completion call, no run-update error line (before the fix: 8 of 10 sessions refused for a missing `openIssues`). Only 3 of 10 sessions called the completion tool at all, each once, each returning status running without refusal, so the effect of making the field optional and the effect of telling the model that no completion call is required are not separated. Raw artifacts under `gs://sc-bench-iqidis-artifacts/dsg/check10_b59589c84/`. Limits: simple prompts, one model, no required criteria, so Partial and the UNVERIFIABLE state are not exercised.

**Hands-on at the usability head (Luna gpt-6-luna high; executable copied from the local debug build, checkout `b59589c84`; `campaign2/handson_20261014d/RESULTS.md`; error hunt applied, details in `campaign2/FINDINGS.md`).** No completion call was refused for a missing `openIssues` in seven sessions, but that is not an improvement: the earlier Luna run also had none (the eight-of-ten baseline is the Gemma check). A plain question ends Answered, with the visible outcome footer back; with the original always-failing-check prompt the model again rewrote the checker and told the user the check passes (stored Partial); with "do not modify" it disclosed honestly; the CSV task twice, the git fix and an interrupted-and-resumed task all ended Partial with no verified Completed, and the resumed run's final message dropped the disclosure of the uncertified ledger. n=1 or 2 per scenario on a loaded machine, so timings are observations only.

## Project-store rebuild S0 baseline, 2026-10-15 (bootstrap executor)

The owner-directed rebuild is active; earlier hold text is historical. The
[baseline and custody receipt](./STATEFUL_PROJECT_STORE_BASELINE_20261015.md)
records the separate worktrees, concurrent-resolver collision, preserved sync WIP,
33-file migration inventory, exact installed launcher/binary hashes and private
online SQLite backups. Installed Luna exec answered `42`, made an exact LF scratch
file, and native memory add/list/Forget confirmed home-scoped storage plus active
CLI retirement. No project-local database, candidate source match, native conversion,
rollback rehearsal, compaction, TUI or offline/HAL acceptance is established.
S1a belongs to this location executor after the reciprocal handoff was explicitly
reconciled; the other executor owns the migration registry. Evidence work is in
`rebuild/project-store-bootstrap-2129`. Fresh Droid review remains a separate gate.

## Project-store S1a: explicit and bounded placement (2026-10-15)

`codex-project-store` provides canonical project-local `.stateful/memory.sqlite`
locations through explicit selection or workspace-bounded discovery, with no
engine-home fallback. Nested stores require explicit selection; redirected store
components and canonical workspace escapes refuse. No native caller is attached.

Windows and local WSL Linux targeted `just test -p codex-project-store` each passed
9 tests, zero skipped, retries disabled, jobs2 under the shared build mutex and
23,221,225,472-byte admission floor. Both Windows junction witnesses and Linux
symlink witnesses executed. Bazel lock regeneration returned0 with no lock drift;
scoped fix passed. Final `just fmt` and exact-Droid approval are separate receipts.
No tests are rerun after cleanup. The earlier archived root prototype's six-test
result and this implementation's earlier seven-test run are retained separately.

Actual launcher/Luna baseline checks are in the S0 receipt; the installed binary
is unchanged and has no call sites into this library. This is not a native-path,
neutral-CLI, database/bootstrap, migration, writable-copy, contention or HAL gate.
The next boundaries are reviewed registry/bootstrap, qualified local opening and
bounded neutral reads; native cutover stays behind S2/S3.

## HAL H1 preparation: exact-output baseline (2026-10-15)

The [preservation baseline](./STATEFUL_HAL_PRESERVATION_BASELINE_20261015.md)
records valid synthetic405byte Marathi/JSON inputs, two failed read-only copy
journeys and a failed explicit-UTF8 diagnostic (all finals329bytes/question marks),
a successful405byte full-access diagnostic control, and exact image770.23.
The profile control does not clear the failing read-only gate. CLM refused the
console setter; the cause/current source binding remains unqualified. An initial
fixture-corruption diagnosis was disproved by hashes/codepoints and corrected
without discarding failures. Scorer: deterministic bytes/JSON, no LLM judge.
Installed launcher/Luna only; no candidate, IT2, translation quality, customer
review, offline, compaction/restart or HAL acceptance claim.

## Project-store S1b1: connection-only identity registry (2026-10-15)

The independently reviewed final implementation is `d4860b6f8..a3731ab3f`
(499 changed lines). It retains the resolver and adds fixed bootstrap SQL,
stable project/store UUIDs, format/minimum-reader/minimum-writer versions, and
the domain/version/SHA256 registry. Initialization uses one immediate transaction;
inspection validates a read snapshot. Unknown schemas, histories, headers,
partial/malformed metadata and oversized projected fields refuse. This accepts
only the bootstrap domain, not arbitrary domain SQL or imported legacy content.

Recorded local `just test -p codex-project-store`: Windows19passed/0skipped and
WSL Linux19passed/0skipped, retries0/jobs2 under the shared build mutex and
23,221,225,472-byte HOST admission floor. The WSL guest had approximately8.3GB
total memory; this is not an equally sized Linux machine. The nineteen tests
combine resolver9 and registry10: independent initializers, read-only reopen,
isolated IDs, foreign/partial state, changed history, header/UUID corruption,
allocation-failure rollback, oversized fields and bounded busy recovery.
Scoped fix was warning-free; final `just fmt` and lock regeneration exited0,
with no MODULE lock diff. No tests ran after final cleanup. macOS and Bazel
build/test execution remain unrun; no CPU/heap bound for hostile SQLite is claimed.

Fresh Droid returned **APPROVE COMMIT AND INTEGRATION** for that exact source
slice and the additive facade/dependency resolution against resolver874eea815.
Approved HAL docs28b0693d2 then merged without conflicts; the codex-rs subtree
remained unchanged from a3731ab3f. Integration072a4360a fast-forwarded main and
pushed only to `stateful/main` and `stateful/rebuild/store-migration-registry`.
The earlier root prototype is archived, unlanded and separate from these gates.

External receipts: `sc_dogfood/lsr_drivers/reports/stateful_rebuild/`, especially
`registry_source_review1.txt`, `registry_combined_{win1,linux1,fix,bazel_lock,fmt}.log`
and `registry_final_manifest.json`. Final source/test-binary/launcher hashes are
inventoried; artifact paths/mtimes do not constitute cryptographic source-to-binary
attestation. The installed launcher/binaries were not replaced by this component.

Separate installed Luna baseline: arithmetic43passed; one file-edit trial claimed
creation while its tool cell remained running, but independent inspection found
no file. Count that trial failed, not a host-certified Completed. Baseline runs
used the existing engine home and wrote ordinary session/project state; no home
conversion occurred. Another executor's successful trial remains separate.
This kernel opens no files and changes no native attachment. Qualified path
opening, neutral knowledge reads, legacy closure import, old-writer barrier,
atomic completion/Forget and HAL acceptance remain outstanding milestone gates.
True child-process contention, killed-writer recovery and uncertain-commit
qualification are not established by the independent-connection fixtures.

## S1b2a: concrete-path filesystem admission (2026-10-15)

The unlanded opener2901 received CHANGES REQUIRED for checking only a local
project root while a descendant store could be an unsupported stable submount.
This smaller foundation qualifies a concrete existing file/directory and exposes
a distinct filesystem error; it opens no database or native route. Windows21 and
local Linux21 tests passed, zero skipped/retries. Caller selection of actual store
paths remains the following repaired-opener gate; these tests do not prove that
lifecycle. Arbitrary sync detection, namespace/resource and macOS/Bazel execution
remain unqualified. The held opener is not promoted by this foundation.

## S1b2b: repaired publication and inspection lifecycle (2026-10-15)

The first opener2901 was held after its27/27 platform gates: static review found
that qualifying only the project root missed stable unsupported submounts.
This repair retains that history and integrates concrete-path qualification.
Initialization qualifies .stateful before writes; inspection qualifies the directory,
database and existing WAL/SHM/journal sidecars before SQLite.
Private injected-probe witnesses model ordinary stable directory/file submounts;
a separately mounted WSL9p fixture is prepared for the following CLI witness.
Closed-stage/no-replace publication and read-only existing-store admission remain.
Final repaired Windows32/Linux32 gates passed with zero skips/retries; the file
probe uses malformed bytes so SQLite-before-policy ordering would fail distinctly.
Cleanup and fresh exact review remain separate receipts. No native/migration, actual killed-process,
power-loss, arbitrary-sync, hostile-SQLite or complete-milestone claim follows.

## S1c0: bounded standalone bootstrap inspection (2026-10-15)

The new `stateful-memory --project <dir> capabilities|root --json` executable
uses the reviewed project-store reader without engine authentication or home.
It exposes immutable bootstrap identity only, with explicit unavailable knowledge,
native attachment and semantic revisions. It creates no stores and is not S1c
knowledge retrieval or the complete first milestone. Unknown stores refuse.
Lossless ASCII-escaped JSON fits the 4096-byte envelope, including trailing LF;
this does not fix H1 exact source-byte transport.

Windows and WSL Linux each passed five tests, zero skips/retries: real reader
subprocesses over Rust-initialized stores, distinct identities, Unicode paths,
absent/foreign refusal, JSON expansion budgets and a reviewed output snapshot.
The first Windows attempt retained its one new-snapshot failure before explicit
review/acceptance. Lock regeneration, scoped fix and final format exited zero;
fix retained five test-only unwrap warnings. No tests ran after final cleanup.
The local candidate build exited zero; source/input and binary hashes are retained,
without claiming cryptographic build attestation. macOS/Bazel execution is unrun.

Installed launcher/Luna read two synthetic Python-seeded metadata stores through
that separate candidate and returned their exact distinct UUIDs. Deterministic
JSON equality and database hashes passed; this is no seeded knowledge-entry read,
legacy import or native attachment. A real ext4-root/9p-store mount refused with
unsupportedFilesystem; identical bytes on ext4 read successfully. The first mount
attempt returned invalidProject because its temporary fixture was absent; it is
retained, and the recreated persistent fixture supplied the qualifying witness.
The owned mount was removed. Receipts: external stateful_rebuild_cli_* logs and
stateful_rebuild_cli_hands_on_20261015/. Native launcher/binaries remain unchanged.

Fresh review held the initial reader for Bazel library/binary target collision,
then held its packaging repair for preserve_order-sensitive snapshot ordering.
The repaired reader disables library inference and sorts snapshot JSON objects.
Final Windows5/Linux5 gates explicitly enabled serde_json/preserve_order and passed
with zero skips/retries; scoped fix/final fmt exited zero. No post-cleanup tests.
A Bazel package query loaded binary/tests successfully; build/test execution is
still unrun. Both held histories and review findings remain preserved.

## S1c0b: explicit bootstrap administration (2026-10-15)

`init --storage local-unsynchronized` exposes the reviewed metadata initializer.
The required enum is an explicit operator assertion of host qualification, not
comprehensive synchronization detection. Reads still create no stores; unknown
existing files are refused. Initialization returns the same bounded identity
schema, with bootstrap-only coverage and no knowledge/native/legacy conversion.
The earlier read-only CLI review found a Bazel duplicate-target defect; disabling
library inference repaired it. A local Bazel package query then loaded the binary
and test targets; this query is not a Bazel build/test qualification.

Windows7/Linux7 passed with zero skips/retries: two spawned independent initializer
processes converge, repeat init/read preserves the database, abandoned stages stay
inactive, and missing storage assertion/foreign files refuse. These observations
do not prove an overlapping SQLite lock window, killed-process or power-loss
recovery. Initial init snapshot failure remains recorded; it was reviewed/accepted
before final passes. Scoped fix/final fmt/build exited zero, retaining five
test-helper unwrap warnings. Format waited when host RAM was below the floor.
No tests ran after cleanup; package dependencies/lock inputs did not change.

Installed launcher/Luna initialized two fresh scratch projects through the pinned
candidate, then read both identities and returned exact distinct UUIDs. Read-only
SQLite inspection and deterministic JSON equality confirmed the reply, integrity
and metadata-only schema; candidate/source hashes are retained. No synthetic seed
was used in this journey. Native launcher and installed Codex were unchanged.
Actual process-kill/power-loss, domain writes, writable-copy adoption, migration,
old-writer barrier, atomic completion, knowledge reads and full milestone remain
unqualified. Fresh exact Droid approval remains a separate promotion gate.

Integrated reader/init gates additionally passed Windows7/Linux7 with
serde_json/preserve_order enabled after sorting both snapshots. Conflict resolution
retains both histories and the reviewed handoff. An accidental CRLF rewrite was
normalized back to LF without changing logical text, followed by final format and
candidate build; no tests ran after cleanup. Exact combined review remains required.

## S1 follow-on: source-SQL composition preparation (2026-10-15)

The integrated CLI and both-parent merge received fresh exact approval before
stateful-only promotion. A later disposable Python experiment composed bootstrap
metadata plus all 33 application source SQL migrations: 67 tables including
internals, 49 triggers, integrity ok and no foreign-key violations. A deliberately
incomplete project-a copy matched three parent counts and passed SQLite checks
while losing its entry revision and runtime thread link. The current reader
refused the unregistered combined schema with unsupportedStore and unchanged DB
hash. Initial fixture seeding failed on required strategy_revision; that attempt
is retained separately from the corrected v2 witness. Full scope/receipts/limits:
STATEFUL_PROJECT_STORE_DOMAIN_FEASIBILITY_20261015.md. This is not Rust domain
installation, migration-history compatibility, conversion or effective-read proof.

## 2026-10-15 independent read-only reader witness

- Exact reader781c8e1e4, approved opener5d3, independent frozen leaf SHA25670b782778f89a71dc4c7879aefc8b7b85720235fba430cbf370bd8d85acdfa79. Local mutex/jobs2/23,221,225,472-byte host admission, build0; before/after source hashes equal. No cryptographic source-to-binary attestation or native installation.
- Independently reviewed scratch harness ran once: Python-seeded metadata only; two distinct identities, Unicode/non-Git projects, moved-root identity, direct SQLite/digest/integrity and refusal/immutability checks PASS. Registered children collected; process-lifetime overlap is not a forced SQLite race. Rust initialization was NOT RUN by this witness.
- Actual installed launcher/Luna session01a141ec-3118-7090-ad15-3f43ad38efba returned two completed exit0 reads and exact final JSON matching direct/tool UUIDs and paths. Fixture files and candidate hash unchanged; normal native session home writes expected. No model judge; explicit UTF-8 deterministic scoring.
- Migration inventory revision4 received fresh inventory-only approval:33 SQL files and disposable empty schemas are not populated SQLx migration qualification. Native/store-route map remains separately reviewed preparation. M0/S2/S3, native knowledge, atomic completion, legacy retirement, MCP/offline and HAL H1 remain open. Earlier native edit failure remains a failure.
- Details and retained local receipts: STATEFUL_PROJECT_STORE_WITNESS_20261015.md. This is not the first milestone or company-brain acceptance.

## 2026-10-15 cutover source inventory and authority pointers

The reviewed migration revision4 and attachment revision2 inventories are retained
in STATEFUL_PROJECT_STORE_CUTOVER_INVENTORY_20261015.md. They identify trigger and
backfill preservation, project closure, exact bytes, both cross-database guards,
native/app-server caches and direct evaluation readers. Source-only preparation,
not an importer design, populated SQLx conversion, native activation or runtime
result. Current-state/master-plan pointers now identify the owner rebuild order;
historical detail and all failed gates remain intact. No code or runtime changed.

## 2026-10-15 H1 child-shell diagnostic (not native qualification)

Using the pinned source UTF-8 prefix and exact synthetic405-byte fixture, disposable
FullLanguage/ConstrainedLanguage children isolate skipped file-reader defaults after
the forbidden console setter. WindowsPowerShell5.1 default/prefix CLM reads529bytes
of wrong decoded JSON; explicit file UTF-8 and a separate-default diagnostic retain
407bytes (source+CRLF). pwsh7.6 retains407bytes for all read variants. Corrected probes
show inherited codepage65001, so the native329-byte question-mark failure was NOT
reproduced/fixed. Initial ContainsKey CLM probe errors retained, not counted as
passes; all18 children collected. No native sandbox, source-matched native build,
policy change or product patch. Details: STATEFUL_HAL_CHILD_SHELL_DIAGNOSTIC_20261015.md.

## 2026-10-16 S1 domain manifest preparation and snapshot admission

- Inert project-store API prepares trusted compiled domain SQL before any destination lock, and admits an exact format2 schema/history under one snapshot. Bootstrap migration1 and schema remain immutable; actual domain version gaps and SHA256 SQL digests are retained. Existing bootstrap APIs still refuse application stores. No public destination installer in this first review unit.
- Final component gates Windows35/Linux35 passed, zero skips/retries; scoped fix clean. Final format receipt is recorded separately. No tests after cleanup. Synthetic fixtures cover schema/history/format refusal, kernel mutation, invalid manifests and read-only admission; this is not real-domain installation or populated conversion.
- Initial PowerShell5 wrapper stopped on just stderr; retained. Initial unsplit prototype Windows36 passed, then the code was split to keep each complex review unit below500lines. Those prototype results do not replace final-unit gates or add to their counts.
- No home conversion, installed-native change, effective knowledge recall, atomic completion, old-writer barrier, MCP, offline or HAL customer claim. Existing native edit and exact-Marathi failures remain open. Next unit adds explicit atomic installation with rollback/contention witnesses, followed by real-schema scratch qualification.

## 2026-10-16 S1 atomic domain installation

- PreparedDomains::install explicitly validates bootstrap and atomically applies all domain SQL, exact history records and format2 transition while preserving UUIDs. Existing registered stores require full manifest/schema equality. Foreign keys must be enabled; caller owns qualified placement and bounded busy timeout. No legacy SQLx history merge or native attachment.
- Windows38/Linux38 passed separately, zero skips/retries. New installer fixtures cover gapped/overlapping domain versions, stable identity/idempotence/read-only reopen, late trigger-injected failure rolling back all DDL/history/version state, and a held competing connection producing SQLITE_BUSY followed by a clean retry. These are independent connections, not independent-process critical-section witnesses.
- Shared mutex/jobs2/RAM floor retained; cleanup queued below the threshold then admitted after RAM recovered. Scoped fix and final fmt exit0; no post-cleanup tests. Fresh exact source review and real-schema subprocess/Luna evidence are separate gates, not implied by these counts.
- No populated migration, legacy retirement/quiescence, completion/Forget transaction, effective neutral knowledge reads, native installation or HAL/offline/customer acceptance. Cancellation, killed-writer, power-loss and domain upgrade qualification remain open.

## 2026-10-16 real-schema project-store witness

- Frozen Rust helper597ef108 on approved abac embeds all25intelligence+8runtime migrations. Local build1 failed absolute SQLx9 paths; repaired relative inputs source-reviewed, build2 passed with mutex/jobs2/RAM floor. Input/leaf/resolved-lock hashes retained; no cryptographic attestation.
- Reviewed independent harness once PASS: both full schemas equal Python-executed source oracle including67tables, exact UUID rows/non-nil canonical values,35digests/integrity/FKs/noSQLx ledger. Rust init. All13children recorded, including four diagnostic-specific normal exit1 refusals. Real held-writer busy refused within5seconds then retry passed; move hash and corruption no-repair hash passed. Launch-to-collection overlap only, not proven process/critical-section overlap.
- Actual unchanged launcher/Luna workspace-write six candidate bootstrap/install/read commands PASS on two fresh ASCII-path projects; exact final/helper/directSQLite UUID/path JSON. Both stores format2/67tables/35records, empty knowledge/run rows. Binary unchanged. Independent witness covers Unicode paths; manual journey does not qualify native Unicode transport. Deterministic scorer, no model judge.
- Packet STATEFUL_PROJECT_STORE_DOMAIN_WITNESS_20261016.md; receipts domain_fixtures/20261016T005722Z/ and luna_real_domains_20261016/. No native home conversion/installation, populated import/old-writer barrier, effective recall, WAL/atomic completion/Forget, process-kill/power-loss, MCP/offline/HAL/customer acceptance. Existing native edit and Marathi failures remain open.

## 2026-10-16 H1 independent PowerShell file-decoding defaults

- Split console encoding and the existing Get-Content/Select-String/Import-Csv UTF8 defaults into independent best-effort blocks. A refused console setter no longer skips file defaults. Explicit file encoding still wins; writing defaults and sandbox/approval policy are unchanged.
- Windows174/Linux192 passed separately with zero skips/retries. The Windows public integration test runs four real PowerShell5.1 CLM cases: Get-Content, Select-String, Import-Csv and explicit UTF16 override. It establishes console UTF8 BEFORE entering CLM to isolate file decoding; it is not native sandbox transport. Windows-only execution is absent from the Linux binary. Scoped fix/final fmt0; no post-cleanup tests.
- Actual installed-launcher/Luna diagnosis in fresh scratch: WindowsPowerShell5.1 CLM console/input437, pipeline20127, file default null; explicit UTF8 source read still331bytes with zero non-ASCII. chcp65001 did not preserve Marathi. Child-only PATH control selected existing portablePowerShell7.6.5, still CLM console437/pipeline65001 and lost Marathi. These are installed-baseline controls, not a source-matched candidate or a fixed native route. Source405byte fixture unchanged. First multiline batch-argument attempt failed255 before session; stdin retries and failures retained.
- Evidence: stateful_rebuild/h1_native_baseline_diag/ and h1_portable_shell_diag/ plus h1_windows_test1/h1_linux_test1/h1_fix1/h1_fmt1 logs. No new native binary, launcher install, translation/IndicTrans2, offline or customer qualification. Native329-byte failure remains open; transport needs a separately reviewed trusted startup solution.

## 2026-10-16 S1 registered read connection

- ProjectStoreReader opens one explicitly selected/qualified project-local file read-only, admits the complete known format2 schema/history under a snapshot, and owns one connection without pool/reconnect/home/native routing. A trusted domain facade consumes it only with matching name/actualversions/SQL/simple-transactional flags. Unknown/partial/changed claims close/refuse; no unchecked pool constructor or foreign SQL interface.
- Windows42/Linux42 passed separately, zero skips/retries; scoped fix/final fmt0, no post-cleanup tests. New fixtures cover real Rust domain init, identity-bound query/SQLite readonly8/write refusal with unchanged main bytes, domain mismatch, missing/bootstrap/changed-schema no-creation/no-repair, independent identities and explicit close/move/reopen. Existing mount/redirect/sidecar tests pass in the same gate. Dependencies/locks/build inputs unchanged.
- After cleanup the branch fast-forwarded unrelated reviewed H1 integration; project-store sources and Cargo lock unchanged. Admission is not knowledge authority/freshness. Domain eligibility/scope/snapshot checks remain mandatory, and namespace replacement by the same filesystem user is not a security boundary.
- Real-schema helper/Luna and exact source approval are separate gates. No application facade activation/effective recall, native caches, migration/quiescence, writable shared pool/atomic completion/Forget, MCP/offline/HAL/customer or first-milestone claim.

## 2026-10-16 source-matched native H1 and real-schema reader evidence

- Source-pinned e75 native bundle built locally after the default V8 sandbox Windows artifact/binding route failed. Existing Codex release matching archive/binding pair verified against published manifest and existing Bazel archive pin; no dependency, feature or lock change. Local locked/offline bundle build2 exit0, shared mutex/jobs2/RAM floor. Frozen CLI4c1fd373 plus four companion hashes retained, not cryptographic attestation.
- Actual launcher/Luna no-daemon read-only baseline/candidate metadata confirms file default null becomes utf8 under the same PS5.1 CLM/console437. Candidate exact-copy still329bytes/zero nonASCII, known failed c8cb532b hash versus unchanged405byte source. H1 remains FAIL. Temporary default-preserving child-only candidate selection restored original launcher exactly; installed CLI/host and four home schema/migration fingerprints unchanged at close. Normal home session rows/helper-cache writes may occur; this is not quiescence/data immutability or migration.
- Separately reviewed current-user self-imposed-CLM console prototype: run1 guarded out before child; run2 pre-resume AttachConsole failed6 with own child terminated/wait0; run3 owned sole-client console bootstrap65001 then inherited PowerShell retained exact405bytes+CRLF/114nonASCII and exit7, worker0/no timeout. CLIXML progress remains on stderr. No restricted-token/private-desktop/ACL/native acceptance; production bootstrap/cancellation/stream/packaging design remains next. All failures retained.
- Approved registered-reader source9e, separate reviewed helper662d9a8e built locally once. Two direct real-schema fixture reads and actual original-launcher/Luna two completed read-only helper calls match independent SQLite UUID/count expectations and exact final JSON; both67tables/35records, SQLite readonly8, unchanged database/helper hashes. Empty knowledge/run rows; no native attachment. Deterministic scorer, no model judge.
- Packets STATEFUL_HAL_NATIVE_QUALIFICATION_20261016.md and STATEFUL_PROJECT_STORE_READER_WITNESS_20261016.md retain custody/scope. Next-read handoff now marks generic format2 installer/owned reader landed, effective domain facade/CLI still next. M0/B0, S2/S3, populated conversion/old-writer barrier, effective recall, MCP/offline/HAL/customer and first-milestone acceptance remain open.

## 2026-10-16 next effective-read design and current-state reconciliation

- Staged design only: next source unit owns a validated read transaction, repeats complete admission/UUID checks in that snapshot and makes unfinished/cancelled owners close-only. Later PI extraction preserves native get_hit behavior; neutral coverage status, canonical fresh UUID ownership, bounded nested projections and identity-bound CLI contracts require their stated separate freezes. No facade, snapshot guard or effective CLI was implemented by this document.
- Current-state pointer now links the landed format2 installer/registered-reader and source-matched native evidence, retaining historical October12 detail. Native exact-copy FAIL, separate current-user console-prototype scope, M0/B0/S2/S3 and owner-first-milestone gaps remain explicit. This checkpoint adds no runtime result.

## 2026-10-16 S1 owned admitted read snapshots

- Additive ProjectDomainReader retains the exact compiled manifest/admission UUIDs and owns one readonly connection. begin_snapshot reuses complete schema/history/header/identity validation in the transaction held through trusted reads. Both UUIDs must match admission. Explicit finish awaits commit before reuse; refusal/cancellation/unfinished guard leaves the owner close-only. Raw trusted API and native paths stay compatible; no pool/reconnect, knowledge policy or foreign SQL authority.
- Windows45/Linux45 passed separately, zero skips/retries; scoped fix1/final fmt1 exit0, no post-cleanup tests. New fixtures prove stable view across an independent WAL connection's commit, next-snapshot visibility, readonly8, changed project/store UUID/schema/history/header refusal before domain access with no main repair, and unfinished guard close/move/reselect. No independent-process/crash/power-loss qualification. Async begin/finish cancellation was not separately injected; unfinished guard drop was exercised.
- New modules/tests remain below500lines and the complex source slice below500changed lines. Dependencies, locks and compile-time assets unchanged. Real-schema helper/Luna and exact source approval remain separate gates. No effective knowledge/Forget policy, model context, M0/S2/S3, populated migration, native attachment/MCP/offline/HAL/customer or first-milestone acceptance.

## 2026-10-16 owned snapshot real-schema/Luna witness

- Approved source4216c2c85 plus separately repaired/reviewed helper0306b76b built locally once with locked/offline/shared-mutex/jobs2/RAM gate, unchanged source/repo/helper-lock hashes. First harness review's package-name/awaited-close issues and original sources retained. Frozen artifact hashes are not cryptographic build attestation.
- Two direct exit0/no-stderr real-schema helper reads match independent SQLite identities/counts. Original restored launcher/Luna no-daemon/read-only made two completed exit0 calls; each helper validates/finishes two snapshots, requires readonly8 and awaits close. Exact tool/final JSON and unchanged main database/helper hashes PASS deterministically, no model judge. Both existing Rust-initialized stores67tables/35records with empty knowledge/run rows; separate helper, not native attachment.
- Snapshot witness packet and current/design/next-read pointers now mark Stage1 landed and keep PI effective extraction/bounded projection/CLI unimplemented. Ordinary home session writes possible. Async cancellation/commit-error injection, process races/crash, effective Forget/knowledge, M0/S2/S3/populated migration/native/MCP/offline/HAL/customer/first-milestone remain unaccepted. Known native Unicode FAIL unchanged.

## 2026-10-16 S1 behavior-preserving exact-hit extraction

Private effective_hit::read_on now owns the existing ModelTool eligibility-plus-full-hit materialization body; native get_hit still owns one transaction and preserves coverage-as-None behavior. New fixtures compare whole hits across an independent retirement commit and the next snapshot, plus foreign/missing/coverage refusal with retained archive. No new public API, neutral projection, source routing, pool, home selection or native behavior is activated.

Windows scoped just test: first compilation failed ambiguous test macro import; explicit imports repaired it. Gate2 passed137 runnable tests. Linux gate1 passed135 and timed out two pre-existing large fixtures at60seconds; unchanged timeout/retry settings, serial gate2 passed137. Both gates list one intentionally ignored support worker invoked by the passing cross-process parent test; no automatic retries. Scoped fix1 and final fmt1 exit0; no tests after cleanup. Receipts under external stateful_rebuild/effective_* logs retain failures and before/after source hashes. Review and separate frozen-helper/Luna witness remain independent gates; this entry asserts no neutral facade, M0/S2/S3, populated conversion, native Unicode repair, MCP/offline/HAL/customer or first-milestone acceptance.

## 2026-10-16 extraction helper and actual Luna witness

Approved ccd04ee04 extraction plus separately repaired/reviewed effective_harness built locally locked/offline with shared mutex/jobs2/RAM floor; helper SHA2641115e485a4459585f4e868ee31ebef167827b23cba186021f0911bcd523c6. Harness reviews1/2 repaired command/custody/type/counter/failure-record gaps; fresh review3 approved before one actual trial. Unchanged launcher/Luna no-daemon/ask/explicit existing unelevated profile, alpha workspace plus beta grant, made seven sequential helper init/read/forget calls. Strict trace/counter/typed JSON, delivered/final bytes, independent readonly SQLite including WAL, integrity/FKs and artifact hashes PASS. Alpha effective read hidden after Forget with archive retained, beta visible, foreign misses. Separate source-frozen helper uses writable LEGACY PI stores, not project-store/native attachment; installed CLI/host/helper/launcher hashes unchanged, ordinary home writes possible. Packet STATEFUL_PROJECT_STORE_EFFECTIVE_WITNESS_20261016.md retains scope and failures. No native restart/compaction/UI, neutral facade, conversion/M0/S2/S3, Unicode repair/MCP/offline/HAL/customer or first-milestone acceptance.

## 2026-10-16 H1 requirement preservation and console route inventory

Source-frozen HAL PDF3466d238bc8b3f6c301b9b74b479842500858a87401b8d0dbae4ab5cfcf6e395 platform/application pages read and page11 rendered. Inventory maps allH01-H16 and platformP01-P16 to future fixtures and explicit acceptance gaps; platform IDs are separate from perturbation IDs. Source confirms three pipe creation routes (direct capture, unified legacy, elevated runner), all NoWindow; stdio=None and ConPTY exceptions preserved. Current owner publisher-only policy supersedes historical lineage/written-policy wording. Fresh h1_requirements_review1 approved inventory only; no native transport repair, new specialist/ACL/offline/human/customer acceptance or cloud work. Packet STATEFUL_HAL_REQUIREMENT_PRESERVATION_20261016.md; retained native Marathi failure remains open.

## 2026-10-16 S1 project intelligence metadata reader

Additive ProjectKnowledgeReader consumes ProjectStoreReader through the actual compiled intelligence manifest; inspect preflights both admitted UUIDs, holds fresh kernel validation/typed clocks/current pointers/canonical30-table ownership/required edges and coverage in one readonly snapshot, finishes ordinary refusals and awaits explicit close. No entry text, neutral CLI, native pool/home routing or writer. Source pre-review repaired expected-ID precedence and orphan FTS dependencies. Windows final serial gate4 passed142 runnable tests; Linux final gate2 passed140 and timed out the same two legacy60second fixtures, including all5new tests passing. Baseline without this API reproduced both Linux timeouts; isolated baseline Windows2/2 passed. Full Linux suite gate remains HELD, not passed; one ignored worker is invoked by the passing process parent. All failures/zero-auto-retry receipts retained. Bazel lock refresh0/no MODULE drift, Cargo lock adds one first-party edge; scoped fix1/final fmt1 exit0, no post-cleanup tests. Argument-comment check handled SQLx on booleans; Bazel lint not run. Separate exact source approval/frozen-helper/Luna witness remain gates. No native migration/M0/S2/S3, effective knowledge/Unicode/MCP/offline/HAL/customer/first-milestone acceptance.

## 2026-10-16 H1 restricted-token console mechanism

Separately reviewed external production-readonly-token/inert-SID/private-desktop harness and strict controller built locally locked/offline under shared mutex/jobs2/RAM gate. Build1 import failure retained, repaired/reviewed build2 exit0. One frozen probe PASS_MECHANISM: natural CLM and denied unchanged write canary in both controls, NoWindow CP437 corrupts405-byte body, inherited worker CP65001 preserves exact body; target7/worker0, empty stderr, zero owned processes after stricter outer-job cleanup. Complete tracked build inputs and raw artifacts hash-bound; no production Windows changes. Packet STATEFUL_HAL_RESTRICTED_CONSOLE_WITNESS_20261016.md. Native405-to329-byte Marathi failure OPEN; ACL/WFP/IPC/account/package/cancellation/normal-descendant/offline/customer qualification absent.

## 2026-10-16 metadata reader frozen-helper/Luna witness

Source cc8fc834b plus separately reviewed metadata helper built locally locked/offline under shared mutex/jobs2/RAM gate. Build1 and cached source-rebinding builds2-5 exit0. Preparer first refused a miscopied installer pin before fixture creation; corrected against prior frozen authority and freshly approved. Journey reviews1-5 repaired full build/closed-fixture binding, pair/clocks, timeout logs, NoProfile and never-approve/user reviewer before one native trial. Two direct controls and actual unchanged-launcher/Luna two sequential read-only calls PASS strict typed JSON/trace/counters/final and independent SQLite checks. Distinct full-schema synthetic stores, alpha clocks above2^53, beta incomplete coverage; two finished snapshots per helper. Main DB/native/helper/build bytes unchanged,11 artifacts bound. Exact8861-input archive retained before synchronization. Windows142pass; Linux140pass+2baseline-reproduced timeouts, full Linux HELD. No effective-content/CLI/native attachment/migration/M0/S2/S3/Unicode/MCP/offline/customer or first-milestone claim. Packet STATEFUL_PROJECT_KNOWLEDGE_WITNESS_20261016.md.

## 2026-10-16 S1 bounded attributed-content reader and CLI

Three independently reviewed components add a private bounded scalar loader, an admitted readonly reader, and the engine-neutral `stateful-memory read` command. Source reviews private_review3, attachment_review2 and cli_review3 approve their exact slices through af3b98aaea. The private loader bounds SQLite TEXT before Rust allocation and retains ModelTool eligibility. The facade checks both canonical project/store identities and bounded entry ID before snapshot access; the same fresh snapshot checks kernel, clocks, current pointers, all intelligence owners/dependencies and coverage. Success and uniform missing replies disclose attributedContent/notEvaluated and six omissions, with lossless decimal-string clocks. The CLI uses actual compiled intelligence/runtime migration manifests and awaited close; no engine-home fallback. OS arguments stream before clap (32 including program; UTF-8 bytes plus64 each <=16KiB); replies are ASCII-safe and bounded8192 including newline (bootstrap4096). Discovery has the same six omissions.

Local verification: Windows PI attachment_windows2 passed149 runnable tests; one intentionally ignored process worker is exercised by its passing parent. Focused WSL attachment_linux1 passed7 new tests; full PI Linux NOT RUN this slice and prior full-suite qualification hold remains. Final CLI capability cycle cli_cap_windows2 and cli_cap_linux1 each passed11 with0skips. Dynamic two-project subprocess coverage, retirement and the accepted capabilities/read snapshot exercise actual persisted results. Scoped fix/final fmt passed; final equivalent comparison cleanup was typechecked, not retested. Five unchanged older CLI helper warnings remain. Cargo adds two existing package edges only; root just bazel-lock-update exited0 with no MODULE drift.

Windows Bazel is NOT QUALIFIED. Expanded binary/integration compile-data query passed, including migration SQL. First build failed incompatible native-target analysis; intended generated Windows build failed external time0.3.47 E0463 missing time_macros after10078 local actions. The independent existing time target reproduced the dependency error without CLI compilation; no passing baseline/full-suite inference or weakened constraints. All builds/tests were local, serialized with shared mutex/jobs2/RAM floor23221225472 bytes; no cloud build/test.

Retained failures include the initial PowerShell wrapper/encoding problems, provenance compile type error, predecessor timeout (unchanged-main isolated control passed; later full Windows passed without retry/timeout changes), fixture-clock conflicts repaired with explicit UPSERT and generation increments, two snapshot-pending runs, unsupported cargo-insta -p and first post-accept wrapper failing on the removed .snap.new. Source reviews repaired schema-valid supersession fixtures, Bazel compile-data routing and capability omission disclosure. Earlier failed/unapproved frozen candidates remain unexecuted.

One actual unchanged-launcher/Luna session failed final content fidelity: luna_trial1/failure.json and independent_audit.json record FAIL_FINAL_CONTENT_FIDELITY. Two auth-free direct CLI controls and both ordered profile-free native tool results matched the independent SQLite oracle exactly, including the405-byte source and clocks above2^53. The final answer and -o file double-escaped Marathi codepoints inside /alpha/result/entry/content; all other values matched. No retry, output normalization or weakened scoring accepted. Native exit0 is not a journey PASS. One session/two authoritative model tool calls, one completed turn, zero search/web/image calls; one fresh ChatGPT login control and stock-provider admission retained. Both databases remained byte-identical and logically identical. The separate read-only postfailure audit rechecked all8865 source inputs/ZIP members,20 live bindings and17 retained copies. Raw before/after DB receipts, native exit, stdout/answer/failure/diff and hash inventory retained. No judge was used. Exact final publication/delivery is an OPEN follow-up; the standalone bounded CLI component remains source-reviewed/Cargo-qualified only.

This is synthetic historical attribution, not effective truth or authenticated production ingress. No native/home attachment, shared-DB conversion, migration atomicity/old-writer barrier, compaction survival, verifiable edit/Forget ingress, native Unicode repair, MCP/offline/HAL/customer acceptance is established. The first company-brain milestone remains NOT FULFILLED. Native405-to329-byte Marathi failure stays OPEN; the new CLI witness cannot qualify the native console route. HAL publisher-only policy and IT2_NOT_RUN are preserved.


## 2026-10-16 S1 opt-in Windows GNU-host platform qualification

Source dc707b23a8af adds an opt-in native x86_64 GNU-hosted Rust1.95 path with GNU LLVM application targets, real pinned official GNU runtime/unwind archives, declared LLVM dlltool inputs, matching test bridge and OS workspace status. Shared Windows snapshot-root setup is independent of cwd; capabilities snapshot compares the complete parsed JSON object with explicit raw ASCII/4096-byte/newline checks. No Rust application/context/schema/launcher change. Fresh source_review_final1 APPROVE SOURCE scoped/no P1/P2. Windows Bazel gnu_cli_test8 passes7 cases/2 targets; windows_cli2 passes11/0skip; full WSL linux_pi1 passes149 runnable/one intentional worker skip at unchanged60-second limits and zero retries. Earlier Linux full-suite hold is resolved for this recorded unchanged-source run; earlier timeout cause/universal timing is not established. Diagnostic E0463, mixed-CRT, missing-runtime/tool, snapshot-path/order and my Windows-default-decoding failures retained. Final just fmt passes; no tests after formatting. Build-only reviewed-HEAD candidate4 passes with9577 identical input leaves and a hash-verified source archive.

One actual unchanged-launcher gpt-6-luna trial FAIL_NATIVE_TOOL_AVAILABILITY: the prepared code-mode-host=false configuration made the installed exec host unavailable. Native exit0, one completed turn, authoritative function0/custom1, zero command_execution items, final error objects instead of identities. Original validator failure prohibited/extra tool and independent_audit.json retained. All18 postflight entries and current pins match. No retry, repair or normalization; raw output, original failure and unconditional per-binding postflight retained. This does not qualify native usability or content fidelity. Effective native config/managed-requirements and stock ChatGPT login admitted without retaining raw auth/config; refresh-endpoint overrides removed, code mode disabled. Ordinary native engine-home session writes remain possible; immutability claims cover the closed project fixtures and pinned artifacts. Fresh frozen controller review precedes the sole model attempt. Evidence: C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/qualification_20261016.

Qualification is limited to the explicit short-output-base opt-in CLI lane; default Windows/MSVC/ARM64/full native Codex/offline/HAL remain unqualified. Native405-to329-byte Marathi and separate final nested double-escaping failures remain OPEN. No native project-store routing, populated shared-DB migration/old-writer barrier, atomic completion, compaction survival, verified edit/Forget ingress, MCP or customer acceptance. First company-brain milestone NOT FULFILLED. Separate upstream dirty B0 custody and historical refs preserved; no approved merge batch available.


## 2026-10-16 project-local short writer and held runtime adapter

Source ab8764ef5d plus separately reviewed argument-comment-only2c673ae7a9 adds an inactive trusted ProjectStoreWriter/guard: qualified existing project store, expected UUIDs, readonly admission before WAL, FK/FULL, two-second busy timeout, immediate reservation with repeated schema/history/identity validation, explicit commit/rollback and unfinished/error close-only ownership. No native lock removal or routing. Fresh source_review1 approves493-added-line component; runtime source_review1 separately approves the comment repair. Windows_store3/WSL_linux_store2 each51pass, zero skips/retries,9562-leaf maps stable; scoped fix/final fmt pass, no post-cleanup tests. Independent real-schema direct_trial2 PASS_COMPONENT_ONLY proves cross-domain commit/rollback/drop, typed Busy2.365s, another project committing while held, owned-helper termination and recovery with neither uncommitted write. Original direct_trial1 FAIL from live-SHM observer access remains retained; trial2 records operational sidecar access errors, while main/logical/integrity/FK checks remain mandatory. No physical live-sidecar immutability or power-loss claim.

One actual unchanged-launcher/Luna trial remains FAIL_LITERAL_CELL_CONTRACT_RETAINED: both raw code-mode cells appended one LF to their frozen literals. No retry/normalization/weakening. Separate diagnostic confirms both sequential profile-free exit0 tool outputs, whole final/-o JSON and authoritative counters exactly match; two raw calls/two results, prohibited0. All57 postflight entries and9562 live source leaves match; owned Job active0/rootExit0. Fresh frozen-controller review precedes this sole model attempt. Earlier preparations made no native/model call. Raw agent entries lack admitted identity-word projections and correctly return null, so neither content recall nor seeded-entry acceptance is established. Writer promotion is source/Cargo/direct-harness qualification only. Packet STATEFUL_PROJECT_STORE_WRITER_WITNESS_20261016.md; complete evidence writer_20261016.

Separate runtime adapter78989cce28 is SOURCE APPROVED but NOT promoted pending its own hands-on witness. Guard-consuming runtime completion retains legacy validation/acceptance and checks canonical run/project-tagged child ownership; late error rolls prior domain writes and lease/terminal mutations back. Caller still validates knowledge/eligibility/source bindings under the same guard. Windows/WSL runtime75 and store51 each pass, zero skips/retries;9564-leaf gate maps match. Final equivalent assertion/argument-comment cleanup typechecked by scoped fix, then formatted, not retested. Root Bazel lock refresh0/no MODULE drift; one existing first-party Cargo edge. Evidence runtime_20261016/qualification_audit.json and source_review1.txt; clean local branch retained. No native attachment, populated migration/old-writer barrier, whole-store certification, compaction/Forget/MCP/offline/HAL/customer or first company-brain milestone acceptance. Native405-to329Marathi and separate final nested escaping failures remain OPEN; protected upstream dirtyB0 untouched.


## 2026-10-16 runtime adapter preparation and retained oracle failure

Runtime source78989cce28 remains LOCAL/unpromoted. Its main-docs merge22c0c84a33 preserves the exact reviewed codex-rs tree65a8491b8d0f714574006c67cdc96d7baef5768f. External helper source44e94817763f4380e2371e44d499f336c6e55e9b171d166a01626e2d1d3f6e25 was freshly source-reviewed and built locally locked/offline in1m40s, with9565 unchanged source leaves and matching auxiliary hashes under shared mutex/jobs2/RAM floor. Original helper draft's SQLx string-type/idempotent-collision findings were repaired before build; original source/review retained. Fresh offline lock resolution initially refused yankedspin0.9.8; retained failure. Seeding the helper from the unchanged workspace lock and updating its root offline preserved the existing pin; no repo dependency/lock change.

Prepared proposal1 never executed: review found a missing hierarchy-trigger revision delta, and the operator hardened reparse checks with lstat. Fresh proposal2 received APPROVE FROZEN COMPONENT EXPERIMENT before one actual attempt. The success helper returned0; frozen oracle then refused an existing sqlite_sequence row at0, which fixture copying had initialized, instead of its assumed absent row. Trial2 remains FAIL_ORACLE_PRECONDITION_RETAINED; no retry, normalization or PASS. Stale and collision calls NEVER RAN. Separate diagnostic reconstruction from retained before/after images, including explicit seq0-to1, matches the entire successful image and whole reply; this does not upgrade acceptance. All28 postflight bindings and9565 source leaves match; owned Job active0/rootExit0. No runtime Luna/native/model call occurred. Manufactured host receipts are fixture data, not checker execution, migration or knowledge-policy certification. Packet STATEFUL_RUNTIME_ADAPTER_PREPARATION_20261016.md and external runtime_20261016/FINAL_HANDOFF.md preserve the closed failure and next gates. The first company-brain milestone and native/HAL gates remain open.

## 2026-10-16 runtime adapter component and actual Luna witness

Runtime source78989cce28 remains the499-line source-reviewed unit; doc-merge
facedae24f6d preserves Rust tree65a8491b8d0f714574006c67cdc96d7baef5768f.
Recorded Windows/WSL75runtime+51store each pass; final equivalent cleanup was
typechecked/formatted, not retested. New build3 and fixed-identity native helper
build1 pass locally locked/offline, mutex/jobs2/RAM23221225472, with9566 identical
source leaves and auxiliary maps. PowerShell5 stderr-wrapper failure retained.

Fresh direct_review6 precedes one3case PASS_SYNTHETIC_RUNTIME_ADAPTER_ONLY:
exact cross-domain success, stale ledger refusal, late conflicting-obligation
rollback. Full schema/header/67tables/35migration rows, lease and prior PI writes
are independently checked. Attempt2 stays FAIL_ORACLE_PRECONDITION; unexecuted
proposals3–5 retain startup/type/probe review findings and corrections. No retry
or normalization upgrades any old failure.

Fresh luna_review2 precedes ONE unchanged-launcher/gpt-6-luna trial3 PASS. Exactly2
ordered profile-free commands/raw custom calls, frozen one-LF cell terminators,
authoritative counters2/prohibited0, whole tool/final/-o JSON and full success/
late-conflict database deltas match. Job active0/rootExit0. Independent readonly
recheck passes64bindings/9566source leaves/10closed database images. Native
trials1/2 never executed (preparation controls ran); nonexistent alpha provider cwd was corrected and covered
by an actual-function in-memory RPC probe. Stock provider/requirements/ChatGPT
login and original launcher/CLI/host/config admitted; normal home session writes
are possible. Source-matched external helper, no new native installation or judge.

Packet STATEFUL_PROJECT_RUNTIME_WITNESS_20261016.md and runtime_20261016 retain
all raw artifacts. Synthetic host receipts/UUIDs/legacy seed copying are fixtures,
not checker execution, migration, ownership or native knowledge-policy evidence.
Exact closeout review precedes source promotion. Native project routing, knowledge/
eligibility/steering/source revalidation, populated migration/old-writer barrier,
native lock removal, compaction/Forget/MCP, Unicode/offline/HAL/customer and first
company-brain milestone remain OPEN. Writer LF and Marathi/final-escaping failures
remain failures. The separate in-progress knowledge-clock branch is not qualified
by this witness.

## 2026-10-16 same-guard project knowledge clocks

Inactive PI source2e90c89adc adds332lines: guard-consuming exact manifest/identity,
lossless project/retirement clocks and complete identity coverage under the final
writer reservation. No corpus scan or separate fence. Caller ownership/findings/
source/steering/runtime validation still required; further writes can invalidate
the check. Windows3/WSL1 each155PASS/1intentional worker skip with passing process
parent,9568equal maps. Initial missing-parent-method/test-uuid failure retained;
explicit runtime parent/public identity fixture fixed it. Scoped fix/fmt0 after
tests; no postcleanup retest. Fresh source/helper approval recorded.

Local locked/offline helper_build1 passes1m22, mutex/jobs2/RAM floor,9568equal
source/aux maps. Fresh direct_review1 precedes4case component PASS: exact success,
stale acceptance, late conflict, and synthetic generation1 refusal before runtime
call. Whole67table/35migration/schema/header/row/lease checks, no actual checker,
native Forget or production import. Native trial1 UNEXECUTED after review found
raw payload body/final comparison gap. Bound strict parser+1valid/12refusal probe
fix it without weakening earlier checks; fault/source-audit probes retained.

Fresh luna_review2 precedes ONE actual unchanged-launcher/gpt-6-luna trial2 PASS:
2exact sequential custom cells/commands, one-LF literals, raw/public stdout BYTES,
complete typed replies/raw-public finals/-o, counters2/prohibited0, exact DB deltas,
native0/Job0. Independent readonly recheck74pins/9568leaves/12closed DBs passes.
Source-matched external helper, stock provider/requirements/ChatGPT admission;
normal engine-home writes possible, no native install/judge/cloud build/test.
Main-docs merge67066be198 preserves Rust treeb9f104f4d72848372d15a290059ca4cd8eedbc35;
source frame2e90 is now historical. Packet STATEFUL_PROJECT_KNOWLEDGE_CLOCK_WITNESS_20261016.md.
Exact closeout precedes promotion; no native routing/import/old-writer barrier/
full certificate/fence removal/compaction/actual Forget/MCP/Unicode/offline/HAL/
customer or first company-brain milestone acceptance. Old failures stay failures.

## 2026-10-16 inactive HAL closed console codec

Closed request codec source1f14bac51bece417712a5821008175bb3de30be2:
38 mechanical plus482 complex changed lines, nine files/518 net additions.
Strict original-byte JSON after shape checks, bounded frames/encode, lossless u16,
explicit nonce/width/non-owning handle refs. No active launcher/worker/OS operation.
Windows2/WSL2 each5PASS0skip/9574equal inputs; Bazel-lock/scoped fix/fmt0,
no postcleanup retest. Source/helper review2 approved; locked/offline local helper
build3.96s under mutex/jobs2/RAM floor. Initial Serde defect and controller1
preparation failure retained; no helper/model ran in failed preparation1.
Corrected controller independently computes all bytes. Fresh frozen trial review1
precedes ONE actual unchanged-launcher/Luna trial2 PASS: two exact one-LF custom
cells, complete raw/public result/final bytes and typed JSON/-o, counters2/other0,
eight control/native artifacts match oracle, native0/Job0. Readonly recheck57pins/
9574source/eight artifacts passes. No judge/cloud/native install. All nine codec
files unchanged by automatic main merge; frozen whole-source claim is historical.
Packet STATEFUL_HAL_CONSOLE_CODEC_WITNESS_20261016.md; evidence console_wire_20261016.
Exact closeout precedes inactive promotion. Stream/responses/worker/lifecycle,
native Unicode/final escaping, offline/HAL/customer and first project database/
conversion/old-writer barrier/lock removal remain OPEN. Superseded unmerged draft
branch retained without blind promotion. No upstream batch approved or touched.


## 2026-10-16 prepared learning precondition and closed console stream

Inactive PI source6e8b34337d (345additions/6files) prepares an opaque negative
historical-learning result in one admitted read snapshot, then revalidates only
identity/clocks/coverage under the SAME final writer. Inclusive agent history,
retired/noncurrent revisions and relations retain legacy semantics; late writes
refuse/rollback. Caller still must bind persisted run/start and preserve other
policies. Stream2e23c2002f (221add/2delete/4files) validates exact header length
before allocation/body, handles short/interrupted IO, requires bounded EOF probe
and delegates strict codec. Caller must close/unblock/deadline the blocking pipe.
Fresh final_source_review4 APPROVES BOTH EXACT SOURCES; no native route activated.

Windows/WSL each162PI PASS/1intentional worker skip,11CLI PASS0skip,10stream PASS0skip.
Scoped fix/final fmt0, no postcleanup tests. Windows PI gate has one explicit
CLI-only two-error-arm source adjustment; PI closure unchanged, global equality
NOT claimed there. Both CLI/Linux PI/stream maps stable; qualification_audit.json
and fresh review verify. Old draft Linux timeout at60s remains failed/unexplained.

Local locked/offline helper build2 PASS8.52s/jobs2/globalmutex/RAM23221225472;
absolute-macro build1 failure retained. Fresh review precedes7case direct2
PASS_SYNTHETIC_COMPONENTS_ONLY: complete independent53table/167object/schema/history/
all-row three-image deltas including FTS commit boundaries, exact stream bytes,
natural bounded drain, zero active BEFORE/after cleanup and root0. Original
direct1 lifecycle activeBefore1 failure stays failed; postmortem diagnostic only.

Fresh frozen approval precedes ONE actual unchanged-launcher/gpt-6-luna trial2:
two exact sequential custom calls/other0, full raw/public helper replies, artifact
and rollback/frame checks match; activeBefore/after0/root0. Independent closed
recheck100pins/19158source and actual images PASS. The trial remains FAIL final
contract: final/-o preserve execution envelopes, not the expected parsed payloads.
The prompt's stdout-parsing ambiguity limits inference; no payload loss/new source
defect established, no retry/normalization, no model usability PASS or judge.

Packets STATEFUL_PROJECT_NO_LEARNING_PRECONDITION_WITNESS_20261016.md and
STATEFUL_HAL_CONSOLE_STREAM_WITNESS_20261016.md retain scope. External evidence
reports/stateful_rebuild/{learning_20261016,console_stream_20261016}. Exact closeout
precedes inactive source promotion. First project-owned database milestone/native
routing/populated import/old-writer barrier/full certificate/fence removal, actual
Forget/compaction/MCP, native Marathi/final escaping, offline/HAL/customer remain
OPEN. Protected upstream dirty B0 untouched; no approved batch or native install.


## 2026-10-16 persisted run-start binding precondition

Runtime919ded934503 adds201lines/4files, approved exact source and disjoint
main merge05aeb4d. revalidate_project_run_start consumes/returns the SAME guard,
requires actual runtime manifest, selected primary-key run/canonical project owner,
and exact nonnegative INTEGER/i64 created_at_ms. Missing/foreign/malformed/mismatch
refuse and roll back prior writes; caller still derives the timestamp from opaque
PI preparation and preserves all other policies under that same run/guard.

Actual local Windows/WSL just test runtime each80PASS0skip (including5new tests),
full9579-leaf source maps stable. Scoped fix/final fmt0; no postcleanup tests.
Disjoint main merge preserves runtime/dependency trees. Revised external helper
build2 locked/offline/local6.39s/jobs2/mutex/RAM passes. Fresh controller review
required zero boundary; match corrected to0; no previous failure overwritten.

Fresh direct1 PASS7cases (zero, mismatch, >2^53, REAL, missing, foreign, late agent),
independent67table/204object/35migration/all-row baseline and three-image deltas.
Success marker only, refusal images identical, natural drain/cleanup0/source pins.
Fresh frozen approval verifies102pins/all9583archive members/all direct evidence;
ONE unchanged-launcher/gpt-6-luna15.252s run passes2exact sequential custom cells,
whole raw/public replies/final/-o, exact STRING timestamp, complete database images,
zero active BEFORE/after cleanup/root0 and source/input postflight. Independent
closed audit repeats102pins/current9583leaves/actual images/raw payloads without
copying raw rollout. PASS_LUNA_INACTIVE_COMPONENTS_ONLY, no retry/normalization/judge.

This helper commits only a marker and always completionCertified:false,
syntheticOnly:true,nativeAttached:false. Earlier PI/stream final-contract FAIL
remains FAIL; explicit stdout parsing in this distinct unit does not regrade it.
Packet STATEFUL_PROJECT_RUN_START_BINDING_WITNESS_20261016.md and external
reports/stateful_rebuild/run_binding_20261016 retain exact limits/receipts.
First project-owned database milestone remains NOT FULFILLED: native routing,
populated import, old-writer barrier, full atomic completion/fence removal, actual
Forget/compaction/MCP/native Unicode/offline/HAL/customer gates stay OPEN.
No native installation, legacy conversion, AGENTS/sandbox policy edit or upstream
B0 touch. Exact closeout approval precedes inactive source promotion; closed full
source frames become HISTORICAL after these documentation changes.


## 2026-10-16: inactive terminal composition and HAL response fixtures

Two independently reviewed source slices, each under 500 changed lines, are
composed without conflicts: terminal fixture b6c645a8fc and response codec
ddfd2a9d89, merge 9fb1a6070ec0. No native routing or worker is attached.
[Exact scope and evidence](./STATEFUL_TERMINAL_RESPONSE_WITNESS_20261016.md).

All three new terminal fixtures pass on Windows and Linux. Complete extension
suites remain unclean: Windows 337/338 (Git-marker permission preflight failure),
Linux 338/339 (existing memory-history test at the unchanged 60-second timeout).
Both failures reproduced on clean main. Protocol tests pass 14/14 on both OSes.
Initial fixture errors, the relation-column repair and the stale Windows2
extension executable remain retained; the stale extension run is excluded from
current-source qualification, with clean/recompile proof for Windows3.

One SHA-frozen, freshly Droid-approved unchanged-launcher gpt-6-luna journey
passed seven exact custom exec cells in 34.610s. Independent closed audit passed
116 bound inputs and 9,585/9,586 archived source leaves, exact raw/public replies,
whole final outputs and natural zero-descendant cleanup. This is only
PASS_LUNA_INACTIVE_FIXTURES_ONLY: syntheticAcceptance:true, nativeRouting:false,
windowsSuiteClean:false, linuxSuiteClean:false. No retry, normalization or judge.
Earlier PI/stream final-contract FAIL results remain FAIL.

Scoped fix and fmt succeeded; no Rust tests followed cleanup. The source commits
retain separate reviewable units and merge_identity.json proves exact disjoint
crate composition, not merged runtime qualification. External exact-candidate
closeout review and promotion receipts determine landing; source archives retain
the exact executed pre-cleanup source after live documentation changes.

First project-owned database milestone remains NOT FULFILLED. Native project
routing, populated import, old-writer exclusion, real checker/source freshness,
full native atomic completion/fence removal, Forget, compaction, MCP/CLI reads,
aggregate steering bounds, and native Unicode/Marathi/offline/HAL/customer gates
remain OPEN. Protected dirty upstream B0 is untouched and not promoted.

## 2026-10-16 extension baseline repairs and inactive HAL response stream

The previous terminal-composition run's Windows Git-marker failure and WSL
memory-history timeout were independently reproduced baseline failures. Source
c945931e1c fixes their fixtures: inherited child-file denial plus the existing
preflight and a HEAD-denial assertion, and two Tokio workers for the unchanged
4160-entry/tool-batch/reopen/history/replay witness. No production acceptance
policy or timeout was weakened. Windows338/338 and WSL339/339 now PASS with zero
skips/retries; fan-in43.984s/48.500s. The explicit Windows success-output witness
executes the denial and restoration, with no preflight return. The historical
failures stay retained. A two-line explanatory comment changed during Windows
preparation; exact source maps and scope caveats are in the root gate witness.

The independent d74dd9f58f HAL source adds read_closed_response, sharing capped
closed framing with requests and retaining strict original response admission.
Windows/WSL19/19 PASS each, zero skips/retries. Partial/Interrupted reads,
truncation, preallocation size rejection, maximum body request, one trailing
probe, IO failures and strict payload/nonce cases are covered. Caller owns sender
closure/deadline; no authentication, lifecycle, job/drain or native worker claim.

Fresh Droid source approvals cover each source unit and the conflict-free merge
preserves exact extension/protocol subtree identities. The149-line acceptance
parity design d2b599a82f has fresh DESIGN APPROVE, not activation approval. It
keeps hashing/observation outside a short final writer and preserves durable
acceptance/steering gates, both learning dispositions and uncertain-COMMIT
reconciliation. B0 remains parallel-owned and unqualified; S1 -> M0-M3 -> S2 -> S3
ordering and populated import/old-writer exclusion gates remain controlling.

One installed-launcher Luna file journey PASS: exact Marathi JSON copy/final,
12.966s, no candidate installation. Separately, the source-pinned closed-file
response helper2/direct oracle and one actual Luna inspector PASS11 response
cases plus static helper IO refusal, exact whole final JSON and33bindings,
12.540s. Helper1's raw-diagnostic review finding and the launcher's pre-model
exit255 remain retained failures; revised helper review APPROVE and stdin prompt
controller are separate receipts. No regrading/normalization, judge, cloud build,
AGENTS/policy change, origin operation or native/HAL/customer acceptance claim.
Root witnesses link all logs. Scoped protocol/extension fix and final fmt pass;
fix made no source edits, with the existing extension large-enum warning retained.
Formatting changes layout only; no Rust tests were rerun afterward. Final
promotion requires its separate exact-candidate closeout and receipt.

## 2026-10-16 — inactive project-bound verification lease adapter

Two explicit guard-consuming runtime APIs share legacy verification SQL and
bind the exact run to the admitted project/runtime domain. Success commits prior
domain writes with the lease operation; refusal is rollback and close-only.
Same-owner takeover, competing-owner expiry and exact owner/attempt release
remain intact. Cancellation during COMMIT is uncertain and needs reconciliation.
No native routing or checker/acceptance authority was added.

Initial C382 runtime gates passed85/85 on Windows and WSL. Two follow-up
regressions first failed (overflow error ordering and a NUL-blind test observer),
then complete identical-source gates passed87/87 each with zero skips/retries.
Scoped fix and final fmt pass; no Rust tests followed cleanup. The merge with
approved main preserves component trees. Fresh source/closeout review and the
actual promotion receipt remain the authority for landing, not these test counts.

One frozen installed-launcher gpt-6-luna session passed in14.504s with one
inspector command, two project-local databases, complete durable-state checks,
strict output/attribution checks and9,620 archived bindings. Its external helper
was built from C382; it does not attest the later cleanup as a new binary or
native product behavior. Earlier helper/controller failures remain retained.
See [the witness](./STATEFUL_PROJECT_VERIFICATION_WITNESS_20261016.md).
The first project-database milestone, migration, old-writer exclusion, real
acceptance/source freshness, neutral MCP writes and offline/customer gates stay open.

## 2026-10-17 — inactive bounded project acceptance reader

Source units15b0024a6c/6bae03f2f8/2a30378852/156cc64284/a8835976e6 add actual
compiled-domain admission to read snapshots and guard-consuming runtime ledger
reads. Exact project/run binding and cap+one metadata scans precede allocation;
complete32criteria/128history/64reconciliations/4096pending/1MiB stored-field
budgets refuse valid legacy excess rather than silently omit binding state.
The hidden reconciled_at_ms sort-key regression first failed; the fix includes
it. Public legacy writers prove boundary/excess behavior without weaker checks.
No native routing, acceptance_request write, checker or knowledge authority added.

Windows/WSL project-store52/52 and runtime95/95 PASS, zero skips/retries. Runtime
and helper whole9601-file frames match and all executed bytes verify against
retained a883 Git objects. Store gates changed unrelated runtime preparation
only; project-store inputs match. Earlier compile/controller/coverage failures
remain retained. Scoped fix and final fmt PASS; no Rust tests after cleanup.

Direct1 failed CP1252 despite a stale PASS report; direct2/direct3 pass after
explicit UTF8 stdout. Successful bound exit/stdout/stderr/typed report now gate
Luna;15controls pass. Actual Luna1 remains FAIL_RETAINED for shell-rendering
allowlist mismatch,31inputs archived. Corrected prospective Luna2 PASS12.934s,
one command,34archived bindings,14unchanged reads/eight refusals/two stores,
exact whole tool/final JSON and complete durable snapshots. This is the a883
external helper through the unchanged launcher, not an installed cleanup binary.

Exact closeout/promotion receipts govern landing. First database/import/old-writer,
real acceptance/source freshness, Forget/compaction/MCP/offline/customer gates
remain open; B0 stays separate/unqualified. See the root acceptance-reader
witness and stateful_rebuild/acceptance_20261017 raw logs/frozen sessions/reviews.
