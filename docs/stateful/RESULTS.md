# Stateful Codex results

Single evidence ledger, consolidated 2026-10-22. Unless explicitly marked as
a cleanup or consolidation check, results below are **historical recorded measurements**, not
reruns on today's checkout. Source presence, installed binaries, component tests,
model delivery and product acceptance are different claims.
Method/commands: [EVALUATION](EVALUATION.md). Contracts:
[ARCHITECTURE](ARCHITECTURE.md). Lessons/recovery: [FAILURES](FAILURES.md).

Original tracked records are recoverable at local safety tag
`pre-doclayout-20261022` (commit `43a2c15951b1fd93d4c9f6be2545712634bdaecf`)
and the private cleanup archive. Exact frozen protocols, hashes and failed-attempt
receipts remain there; external raw campaigns are access-controlled and not
distributed with this repository. A tag does not preserve external or dirty data.
No historical failure is regraded here.

## Honest takeaway

Fresh-thread reuse has promising task-specific results. Single-thread continuity,
exact historic recall, stale-current claims, full lifetime economics and reliable
Work completion remain weak or unqualified. Neither the open-model ladder nor
unmatched public benchmarks has a Stateful-off control. They cannot establish
Stateful's causal contribution or today's release readiness.

## Current integration checks, October 22

Pinned Stateful baseline `43a2c15951` and upstream `a3d93e1e69`. **The integration
is not qualified for push or release.** These are local Windows checks, not an
independent clean-clone or native account/model/memory acceptance journey.

| Check | Observed result |
| --- | --- |
| Core/history/state, five Stateful storage/runtime crates, extension all-target checks | Passed |
| Broader workspace compilation excluding voice-host | Passed; unexcluded workspace requires missing pkg-config and GLib/GStreamer SDK |
| Native CLI and code-mode-host build | Passed with eight jobs and matched sandbox V8 archive/binding; CLI version and companion help exited 0 |
| Config, stable/experimental app-server schemas; Bazel lock regeneration | Passed; Bazel under WSL |
| Final scoped Clippy / repository formatting | Both exited 0; Clippy warnings retained. Tests were not rerun after fix/fmt |
| Browser and launcher | 132/132 and 10/10 passed, with their fixture/mocked scope |
| Python SDK with the newly built CLI | 270 passed, 41 intentionally skipped after test API/platform repairs |
| Initial six-crate Stateful suite | 721 run: 716 passed, 4 migration failures, 1 timeout; 1 skipped. The 5000-directory witness passed in 898 seconds |
| Migration repair | Fork SQL bytes retained as versions 62/63 after upstream 61; 20 migration tests passed, including fresh/reopen and byte-identical refusal of an old fork 59/60 database. All four failing extension migration cases passed on recheck |
| Focused upstream/Stateful integrations | 1081 run: 1063 passed, 17 failed, 1 timed out; 7869 filtered. Nextest marked 41 passing cases leaky. Fixture/environment repairs and remaining failures are retained in the private consolidation report |
| Fan-in recheck without concurrent Rust compilation | Timed out on both 60-second attempts; no deadline or production bound was relaxed |

A focused fixture/environment follow-up passed 50/54; its four remaining
fixture defects then passed 4/4 after repairs. The history fixtures now wait for
actual background indexing; malformed-rollout fixtures initialize an unindexed
store. Guardian hooks use a real Python 3 executable, verified-answer assertions
exclude ordinary assistant delivery events, and the empty-DB fixture explicitly
marks its intended indexing state. No production authority rule was relaxed.

The October 22 compaction checkpoint and request-scoped continuity assertions
failed, and fan-in checks timed out. Existing populated stores are not
automatically converted by renumbering the fork SQL; preserve them and use fresh
disposable homes. All existing product and migration holds remain.

The October 23 Windows comparison at pre-merge Stateful `43a2c15951` ran 18
historically failing or timing-out cases: **4 passed, 12 failed, 2 timed out**.
Both predecessor fan-in checks exceeded the unchanged 60-second deadline on
both attempts before the merge. Nine rollout/indexing fixtures, the guardian
source-call-eviction case and two answered-turn fixtures also failed before the
merge; they passed in the merged focused recheck. Two guardian hook fixtures
passed on both revisions after the merged fixture repair. The empty-database
case was absent at the baseline and passed with its merged indexing fixture.
The baseline passed the project-root and request-scope compaction tests;
their merged failures therefore required a separate compaction repair.
The repair preserved upstream's initial-context checkpoint rebuild, retained
self-contained request deferral and reset only the replacement window's scope
note charge. All **3/3** focused app-server compaction/scope tests then passed,
including repeated manual compaction, root revision and cold resume. Context
limits, authority and sandbox policy were unchanged; separate compaction and
native-memory qualification holds remain.

## Final integration regression classification, October 23

The resumed 2,060-case union produced 2,059 terminal results: **2,055
passed, 4 timed out, 0 assertion failures**, with traversal unfinished and no
final Nextest summary. Passing cases included 40 process-leak warnings and two
retry passes. The interrupted earlier run is not an additional population.

Matched finish checks ran 9 merged tests: **8 passed, 1 timed out**, exit 1;
5 baseline tests passed, exit 0. Traversal passed baseline in 927.990 seconds
and merged in 1,034.100 seconds. Across the union, the latest observed outcome
per case is **2,059 passed, 1 timed out, 0 unfinished**. This combines broad
and focused checks; it is not a new clean 2,060-test run.

The persistent app-server predecessor fan-in timeout also occurs on baseline
`43a2c15951`. Extension fan-in timed out on baseline and broadly after merging,
then passed focused in 59.020 seconds. Exec-image timeout sensitivity reproduced
on both revisions, with retry passes. PI fan-in timed out only in the broad
merged run and passed focused on both revisions (46.059 seconds baseline,
47.724 seconds merged). **Baseline attribution of that broad-only PI timeout
is uncertain**; a concurrency-dependent performance regression is not ruled out.
PI identity/proposal retry passes do not establish that their timing flakes
are fixed. The repaired compaction and request-scope cases passed both broad
and focused rechecks. No new reproducible merge-induced failure was identified.

The baseline used Rust 1.95.0/V8 150.4.0; merged used Rust 1.99.1/V8 152.2.0.
Finish checks ran concurrently on a shared Windows host with separate targets;
they are behavior/deadline observations, not controlled performance measurements.
Original failures, warnings, unchanged deadlines and all product/migration holds
remain. This classification does not qualify a release or native acceptance.

## V8 artifact verification, October 22

Recorded 2026-10-22 during integration of Stateful `43a2c15951b1fd93d4c9f6be2545712634bdaecf`
and upstream `a3d93e1e692f1144cf8685175a5d1f6bf7294f57` (uncommitted integration tree).
The public `rusty-v8-v152.2.0` Windows x86_64 MSVC pointer-compression/sandbox
release manifest, gzip library and matching source binding were downloaded and
SHA-256 verified. The manifest matched the pinned source
[trusted manifest digest](../../third_party/v8/rusty_v8_152_2_0_release_manifests.sha256);
both artifacts matched that manifest. The retained verification receipt and
artifact bytes were rechecked on October 23 with the same hashes.

| Verified input | Bytes | SHA-256 |
| --- | ---: | --- |
| Manifest | 257 | `e16bf2d7bc4b64a96a89e87048d61b9c466040e4c82ae64cfe9f7f2052ef3179` |
| Gzip archive | 41630799 | `cd829415695b30ad61b306b23a381e11276f14bb0b46815ac5ed8b96711d002d` |
| Matching binding | 39400 | `572ed16ff09bb7944f4b35c659ff62dcd3faf43a0351e3f94f011e01b2f92a62` |

This verifies downloaded input identity only. It does not establish compilation,
V8 cell execution, native login/model/memory behavior or independent acceptance.
The separate build/check results above retain their own scope. The reproducible
download/hash recipe is in [DEVELOPMENT](DEVELOPMENT.md#windows-v8-archive-and-binding).

## Earlier documentation checks, October 22

Recorded prior documentation-only working tree based on `43a2c15951b1fd93d4c9f6be2545712634bdaecf`:
`npm --prefix clients/stateful-codex test` passed **132/132**, no skipped or failed
tests; `python -m unittest discover -s scripts -p test_scodex.py -v` passed
**10/10**. These repeat the lightweight checks, not native/model acceptance.
Canonical documentation links resolve; no deleted document is required by a
compile-time include. Existing upstream/fixture/prompt/license files are preserved.
Full Rust suites and native login/model journeys were not run for this cleanup.
The required `just fmt` gate exited **0**; no source/config/lock changes resulted.
Tests were not rerun after formatting.

## Most recent recorded onboarding checks

Runtime baseline `997007527c5bb1d03c4d6b67cd5f8786b37b3e17`;
test/fixture review-fix pin `b8d095182c703849c1371b35f9232d2cd4867d78`.
Package metadata `0.159.2` is not a tested release label.

| Layer, 2026-10-21 | Recorded evidence |
| --- | --- |
| Tools | Rust 1.95.0, Node 22.19.0, pnpm 10.34.5, Python 3.13.7, just 1.57.0 |
| Web | Windows 132/132 PASS, including discovered fixture modules, no skips; not 132 independent product assertions |
| Regression sensitivity | Six individual guard-removal mutations detected: cookie name, extra envelope keys, 1 MiB body limit, typed IDs, submission-time excerpts, retired Add actions |
| Launcher | 10/10 PASS in disposable/mocked launches |
| Gateway | Production HTTP handler with synthetic bridge; not native persistence/model qualification |
| Fresh Windows pair | PASS at intake `3e831c98ecfc7136f500feb1f7426c931aad1fde`; one-job fresh short local clone, verified public OpenAI V8 archive AND binding |
| Build duration/resource | Cargo exit 0, 60m02s; sampled minimum free RAM 17.87 GiB every two seconds, not a continuous guarantee |
| Native smoke | CLI 0.159.2/version and companion help exit 0; empty-home login status “Not logged in”, exit 1 as expected; app-server initialize id 1 and EOF shutdown exit 0 |
| Smoke limits | Backfill-pending/filesystem-fallback warning; no login/model/memory/cold-restart/V8 cell proof |
| Default V8 route | Deno sandbox Windows variant 404 retained as failure; matched CI-publisher override is a different successful route |
| Rust scoped suites | Not executed by the onboarding documentation pass |
| Linux/macOS/WSL newcomer route | UNVERIFIED |
| Independent clean-clone review | Earlier FAIL retained; author-repaired build/format route, independent re-review outstanding |

Debug artifact identities, not reproducible-release attestations:

| Binary | Bytes | SHA-256 |
| --- | ---: | --- |
| `codex.exe` | 364251136 | `985e1d234638e35e1b07155d72fe3eb9674bd896bd231b6e5ae5b15b315ebd13` |
| `codex-code-mode-host.exe` | 103872000 | `c20fac923914074055826ea7eede685eaac2055b54d8b4cd74bc62d8610391dd` |

October 20 browser artifact narratives: port 62635 was BLOCKED by connection
refusal; no application assertions ran. Separate isolated Chromium run on port
53758 passed fixture-backed memory correction/draft reload/Add/Forget/paging
and stale-refusal/orphan behaviors. Its stale setup required recovery after an
incorrect JSON-RPC envelope was correctly rejected, so the original-ID script
did not pass unchanged. Preliminary screenshots were not stale-refusal proof.
No native backend/account qualification follows from either run.

## Experiment register, September 21 to October 12

Dates in this table are 2026. Early SC-EVAL-001–022 generally used one native
Luna xhigh run per arm, matched prompts/build/policy and cached login.
“Full” means full lifetime tokens; “uncached” means uncached input plus output.
Early successful follow-ups exclude maturation unless explicitly included.

| Record | Date | Result and controlling limitation |
| --- | --- | --- |
| SC-EVAL-001 | 09-21 | Exact inventory correct; ordinary 69,900 full/22,540 uncached versus Stateful 85,477/32,741: +22.28%/+45.26%, despite one fewer read-bearing call |
| SC-EVAL-001R | 09-21 | Active-run tool reliability repaired; 130,251 full/30,923 uncached, +86.34%/+37.19% |
| SC-EVAL-002 | 09-21 | Procurement connection correct but all 8 files reopened; 237,459 versus 72,891 full (+225.77%); uncached +136.40% |
| SC-EVAL-002R | 09-22 | Compact root/batched writes: 149,360 full, still about twice ordinary; all 8 files reopened |
| SC-EVAL-003/003R | 09-22 | Optimized Stateful 83,056 versus same-binary ordinary 101,020 full (−17.78%); 32,368 versus 20,124 uncached (+60.84%); 4/6 files |
| SC-EVAL-004 | 09-22 | Held-out licensing: 146,500 versus 173,902 follow-up full (−15.76%); 35,908 versus 45,390 uncached (−20.89%); 4/10 files. Lexical 6/10 versus manual 10/10. Maturation 649,173 full/94,677 uncached/15 responses |
| SC-EVAL-005 | 09-22 | Three follow-ups: 291,780 versus 428,784 full (−31.95%) but 90,052 versus 73,456 uncached (+22.59%); lifetime still behind |
| SC-EVAL-005R | 09-22 | Deferred-tool remediation rejected/reverted: −47.12% full but +35.95% uncached |
| SC-EVAL-006 | 09-22 | Linked maturation 344,870 full (−46.88% from 004), 75,814 uncached, 17 findings/27 relationships |
| SC-EVAL-007 | 09-22 | Refresh-route maturation 279,247 full (−19.03% from 006), 69,071 uncached; 17 findings/29 relationships |
| SC-EVAL-008 | 09-22 | Resolved-evidence mechanisms worked; 417,784 full (+49.61% from 007), 75,256 uncached; two failed/retried persistence calls |
| SC-EVAL-009 | 09-22 | Exact provenance/terminal ordering gates passed; 250,849 full/68,065 uncached; 13 findings/19 relationships |
| SC-EVAL-010 | 09-22 | Follow-up full 248,456 versus 308,300 (−19.41%); uncached 83,592 versus 65,356 (+27.90%); one answer lost material carve-out |
| SC-EVAL-011 | 09-22 | Project prompt-cache mechanism passed; turn uncached 14,822 versus 27,155 (−45.42%); not a general economic result |
| SC-EVAL-012 | 09-22 | Final prose/obligation mechanism passed, durable material result failed; a post-completion checklist cannot repair it |
| SC-EVAL-013 | 09-22 | Selected material root findings preserved; one rejected opaque-reference retry added response/~3,022 uncached tokens |
| SC-EVAL-014 | 09-22 | Revision-bound aliases passed first-call completion and semantic coverage; 97,045 full/30,229 uncached; concurrency/lifetime qualification still open |
| SC-EVAL-015 | 09-22 | Stopped after maturation, no matched cases: alias cap/revision ambiguity caused three completion attempts |
| SC-EVAL-016 | 09-22 | Stopped after maturations, no matched cases: published anyOf accepted mutually exclusive evidence identities; runtime refused |
| SC-EVAL-017 | 09-22 | Twelve answers/six persisted results semantically correct; lexical misses retained. Reads 20→7, raw files 54→29. Follow-up full +12.83%, uncached −20.36%; including maturation full +130.07%, uncached +96.47%; uncached break-even only ESTIMATE around question 35 |
| SC-EVAL-018 | 09-22 | Single-call completion/full-token target passed; extra verification round and uncached target failed. Pair +5.83% full/+40.18% uncached versus ordinary; not release economics |
| SC-EVAL-019 | 09-22 | Submitted-result fidelity gates passed; continuity 84,033 full/13,377 uncached |
| SC-EVAL-020 | 09-22 | Retrieval passed, redundant workflow/economics failed; 81,003 full/15,467 uncached |
| SC-EVAL-021 | 09-22 | Semantic intermediate obligation failed; 102,833 full/27,057 uncached |
| SC-EVAL-022 | 09-22 | Remediated obligation still failed; 109,325 full/31,501 uncached; case frozen |
| SC-EVAL-023 | 09-22 | Six-project breadth directional only; graders preferred ordinary on 5/6 |
| SC-EVAL-024 | 09-23 | Source change correct under total-context stress; economics failed decisively |
| SC-EVAL-025 | 09-23 | `body_after_prefix` diagnostic −49.36% total; no compaction occurred, so not compaction evidence |
| SC-EVAL-026 | 09-23 | Historical retrieval mechanism shown, ungraded: 992,033 versus 134,154 full (+639.47%); uncached +213.34%; 27 versus 9 responses |
| SC-EVAL-027 | 09-23 | `fix-git`: Stateful 3/3, ordinary 0/1; 2.24–3.46× cost, tiny technical control |
| SC-EVAL-028 | 09-23 | Four-task smoke 4/4; one-attempt breadth 70/89 (78.65%); later regression 68/89, not the same run |
| SC-EVAL-029 | 09-23 | Concurrent-family smoke retired before results; manifest removed October 7 |
| SC-EVAL-030 | 09-23 | Feedback pilot: 50 attempts, 40 passes/10 failures, 2 persistence misses, 9 infra retries; fifth-attempt cost −22.0%, passes 7/8 versus 5/8; no ordinary repeated control |
| SC-EVAL-031 | 09-24 | Bix five-question gate 4/5 local; breadth 3/10 locally correct, 7/10 operationally valid, 2/10 both; debug-derived image, not official Bix score |
| SC-EVAL-032 | 09-24 | Ten-question Pramana continuous A/B quality level; Stateful 2.0× input/2.7× uncached, 4 versus 2 compactions; price-equivalent ESTIMATE $18.16 versus $9.02 |
| SC-EVAL-033 | 09-28/29 | Three replicated data-room studies favored fresh threads; notes control close, code-study ranking flipped, trivial asks overhead |
| SC-EVAL-034 | 09-30 | Sol 25-question study −13.4% list-price-equivalent cost; small batches cost more; dependent later batches and same-day stale-current failures |
| SC-EVAL-035 | 10-02/03 | Fresh-thread preferences/decisions helped; within-run evidence mixed/negative; all hands-on cells n=1 |
| SC-EVAL-036 | 10-07 | Historical public benchmark first pass, no ordinary arm; unmatched references cannot prove parity or advantage |
| SC-EVAL-037 | 10-08 | Multi-model extension; qemu exclusions/model-label repairs, self-judged scores withdrawn; partial/proxy results retained below |
| SC-EVAL-038 | 10-09/10 | Pre-cut candidates preserved rules/cold recall/Forget, but rejected hypothesis never proposed. Decision paraphrase failed first build, fixed second. A later change removed automatic model disclosure, so pre-cut cold row would not reproduce |
| SC-EVAL-039 | 10-10 | Plain Autonomous requirement failed: 1/2 trivial questions Blocked; other needed ten cells and exposed raw outcome |
| SC-EVAL-040 | 10-10 | Frozen restricted-read 12-turn n=1: ordinary 1.94M units/10 compactions, Stateful 2.27M/13; ordinary delivered all steps and better PR description |
| SC-EVAL-041 | 10-11 | Offline tuned gains failed held-out generalization; structural floor hard gates passed but recall weak; piggyback cut |
| SC-EVAL-042 | 10-11 | Cold recall/application passed after manual admission; spoken correction not supersession; forgotten rule reappeared as “current” from continuity |
| SC-EVAL-043 | 10-11/12 | Eleven-model ladder on bundle 83eee86b6 before acceptance-ledger gate; no Stateful-off arm |
| SC-EVAL-044 | 10-12 | Later gate regression: Qwen clean exits 98→3→2/108; Mistral 54→29→26. Gate bypass supports termination mechanism, not usefulness; release remeasurement held |

## Fresh threads, within-thread losses and accounting disagreements

SC-EVAL-033 data room: five sessions, 14 documents, 48-criterion rubric, same Luna
high build/model, three runs/arm and two proxy-judge scores/memo:

| Arm | Mean quality /48 | Uncached+output per run |
| --- | ---: | --- |
| Ordinary continuous | 13.5 | 236–275k |
| Stateful continuous | 14.3 | 428–444k |
| Stateful fresh | 18.2 | 402–465k |
| Ordinary fresh + hand NOTES | 16.8 | 435–624k |

Fresh Stateful led ordinary continuous twice/tied once. Its 1.33-criterion lead
over NOTES is within run-to-run noise; about 17% fewer uncached+output than NOTES,
but more than ordinary continuous. Short asks paid ~2× uncached/2.2× responses.

SC-EVAL-034 five batches: Stateful/ordinary list-price-equivalent ESTIMATE
$1.422/$1.641, wall 2,342/2,669 seconds. Batch cost deltas +89%, +55%, −25%,
−22%, −9%. Both scored 10/10 on objective first two batches only; later batches
shared an accumulating store. One run per arm and an incomplete-binary run excluded.
After an external status change, same-day first-answer correctness was Stateful
0/10 versus ordinary 7.5/10; historical numbers and instruction application failed.

SC-EVAL-035 20-session fresh-thread series (one run per arm/build):

| Build | Input S/O | Excluding ordinary re-asks | Compactions S/O | Standing rule S/O |
| --- | --- | --- | --- | --- |
| `9a3c2109d8` | 0.89× | 0.97× | 12/13 | 7/20 vs 1/20 |
| `27aa1988f8` | 0.73× | 0.83× | 3/15 | 20/20 vs 1/20 |
| `c466898af7` | 0.82× | 0.89× | 2/14 | 20/20 vs 1/20 |

Ordinary needed two conversation-only re-asks per series. Other single-pair
findings: main-issue repeats more consistent; analyst conclusions recalled 3/3
versus 0/3; swapped-license currentness improved but neither caught decisive
“uses”→“contains”; debugging fix improved but tests vacuous in both arms;
patent rules retained but ordinary final claims stronger; tutoring preference loss
repaired by verbatim capture; data analysis carried context but made three framing
errors versus none; essay attribution/spelling and literature decisions improved,
sometimes at higher cost. Multi-rule receipts, scoped rules, background capture,
ruled-out entries and source-index errors remained defects.

Within-run checks: 17-minute code task Stateful input 1.36× and ordinary preferred;
seven-hour research 1.12× input/1.23× uncached, ordinary preferred; one 12-turn
test preferred Stateful quality but cost 1.27× synthetic units, 23 versus 18
compactions and both lost next-step context. Exec-resume sandbox drift affected
both arms, so not a pure memory attribution. Research overhead decomposition:
66% memory writes/28% fixed prefix; 4/155 post-compaction packet items used before
re-derivation; ~90% rereads were the agent's own working set.

Separate design-reconciled evidence, not new runtime tests:

- SC-EVAL-032 frozen artifact sums disagree with the register's old table:
  input S/O 25,089,552/12,572,326; uncached 1,427,344/512,678; output
  149,511/107,259. Keep both denominators, never silently mix them.
- `cmp1`, three threads/arm, 45 actual compactions: both recovered historical
  counts; S/O deduplicated synthetic units 5,613,161.6/3,792,676.0 (~1.48×),
  compactions 29/16, wall ~1.31×. Shadow builder cost 9.7–13.5% of a thread per
  invocation without demonstrated downstream benefit.
- Harder `rp16` after three compactions: ordinary 0/18, Stateful 3/18, successful
  facts from initial outcomes. Probes correlated within three threads/arm;
  source re-derivation confound and disagreement with ceiling `cmp1` unresolved.
- `cap1`: marker-bearing promoted capture 14/36 standing cases out of 47 cases;
  exposure did not guarantee obedience. One simulated TUI day: 18 turns but
  four Stateful runs, no instruction entries, invented preferences persisted;
  2.27× total/1.43× uncached/1.51× output/1.50× busy time.
- Corrected Office E12: 12-session Stateful lifetime remained 7–48% higher;
  break-even beyond two re-asks in 3/4 runs. Cheap text rereads remained competitive.
- Shadow maintenance: critical omissions in 6/26 outputs despite some qualification
  improvements; guard amended after run, dangling references, no installed/downstream
  proof. Keep it shadow-only.
- Historical scoped RPC queries held, but Windows sibling/store/rollout reads
  persisted outside content; path reuse/junction repoint reused old memory.
  Those are existence proofs, not a browser rebinding reproduction.
- Fork matrix: 8/8 dead dangling-call forks versus 4/4 live controls. Source edits
  became “source-verified” without independent authority. A two-file synthetic
  same-size/mtime-restored edit plus new governing file disproved HEAD/size/mtime
  as complete freshness, not actual Codex-cache behavior.

## Historical public benchmarks

Bundle **83eee86b6**, before the acceptance-ledger completion gate. No ordinary arm.
Luna coding max/memory high; Gemini high. Proxy judges and unlike published
references prevent causal/equivalence claims. SC-EVAL-036/037 older snapshot:

| Benchmark | Luna | Gemini 3.7 | DeepSeek V4.1 | Gemini 3.8 partial |
| --- | --- | --- | --- | --- |
| TB2.1, scorable (all-89 basis) | 341/435 78.4% (341/445 76.6%), five trials | 76/87 87.4% (85.4%) | 68/87 78.2% (76.4%); max 73/87 83.9% (82.0%) | 66/86 76.7% (74.2%) |
| SWE-bench Verified | 458/500 91.6% | 407/498 81.7% | Not in old full set | Not run |
| Stratified Verified 100 | 87/100 | 79/100 | 94/100 | 61/77 |
| DeepSWE 1.1 | 79/111 71.2% | 64/104 61.5% | 69/113 61.1%, max | Not run |
| GAIA validation | 134/165 81.2% | 120/165 72.7% | Not in old set | 96/165 58.2% |
| TB4.0, 63 runnable | 5/58 8.6% | 7/61 11.5% | Not run | Not run |
| SWE-bench Pro V1 | 510/577 88.4%, 154 unscored | Not run | Not run | Not run |
| Historical Harvey proxy | 1/54 all-pass, mean criterion .851 | 66/1243 valid of 1251, .864 | Not run | Not run |
| LoCoMo J, common G3.8 judge | 84.0% (Luna proxy 84.1%), F1 .490, adversarial 50.2% | 94.7%, F1 .595 | 93.3% (Luna proxy 93.1%), F1 .293, adversarial 68.8% | Not run |
| LongMemEval-S subset 102 | 83.3% (Luna judge 81.2%) | 94/101 93.1%, one unanswered | Not run | Not run |
| MemoryArena physics 20, SR/PS | .65/.521 | .80/.727 | Not run | Not run |
| MemoryArena math 40, SR/PS | .25/.438 | .825/.891 | Not run | Not run |
| MemoryArena travel 270, SR/PS/sPS | .026/38.4/86.0 | .522/84.7/98.6 | Not run | Not run |

Luna TB2.1 all-task Wilson interval 72.5–80.3%, task-clustered SE 3.6 pp;
Verified Wilson 88.8–93.7%, GAIA 74.6–86.4%. These are not between-harness
equivalence tests. LoCoMo totals 1,986 questions; J judges 1,533 category-1–4.
MemoryArena travel is deterministic; other memory tracks use proxy judges.
Gemini travel used ~5.6× Luna tokens and wrote notes during ingestion.
Gemini math leakage check found no established leakage, but 3/40 tasks searched
benchmark source/problem text and both arms had open network: indicative only.

Corrections/denominators retained: qemu startup/SSH verifiers broken by mirror
404 were excluded in all arms, all-89 basis retained. Global-default model labels
were repaired without score changes except exclusions. Gemini self-judged physics
PS .758 was withdrawn; common explicit G3.8 judge supersedes it. Vertex 429s left
two Verified/nine DeepSWE tasks unscored. Five DeepSeek argv/pkill self-kills count
as failures. Luna infra replacements: TB2.1 24, Verified 54, GAIA zero; scored
failures/timeouts were not replaced. LongMemEval full-500, DolphinBench, AMA,
MemoryAgentBench, BEAM and MINTEval remained partial/unreported. Other blocked
programs are not missing successes.

Later DeepSeek/Gemini update, same historical bundle, distinct denominators:
TB2.1 202/261 77.4% versus 233/260 89.6%; stratified Verified trials DeepSeek
94/96/95 versus Gemini 79/78/81; GAIA 119/165 versus 120/165; DeepSWE 72/112
versus 69/113. DeepSeek GAIA replaced 50 serving-saturated timeouts at lower load;
Verified contamination is not ruled out. No variance estimate or parity claim.

Later 17-task/32-session descriptive head-to-head: DeepSeek quality 9.78 versus
Gemini 9.05, leads/ties/trails 11/5/1; input 24.4M/33.5M, output 571k/714k.
Dedicated-node cost $27.50 versus $24.69 Gemini list-price equivalent; counting
only 10.1-minute active window gives DeepSeek $6.66, a different cost scope.
One run/model/session; contract v2 visible from session 1, Gemini style recovered
from harness logs, operator checked final state, all graders Claude.
Updated raw redacted evidence has not been published.

## Open-model ladder and completion regression

SC-EVAL-043 scheduled 32 tasks × 2 trials, 108 sessions/model.
DeepSeek/MiniMax graded support 29 tasks/94 sessions after serving/bridge
exclusions; Step 107 sessions after Spot preemption; others 108.
Ladder blinding imperfect: MiniMax `<mm:think>` markers survived; some graders
viewed other score files after grading, no later edits reported. Across 186
regraded sessions mean grading range .50; no between-model uncertainty interval.

| Model | GPU | Quality /10 | Critical % | Retained GPU-lifecycle $ | Quality points/$ |
| --- | --- | ---: | ---: | ---: | ---: |
| DeepSeek V4.1 Flash | 8×B200 | 9.71 | 0 | 24.29 | 11.59 |
| Qwen3.8-27B, xhigh | 4×H100 | 9.63 | 0 | 24.04 | 12.82 |
| Qwen3.8-Flash-Next, xhigh | 4×H100 | 9.54 | 0 | 39.54 | 7.72 |
| MiMo-V2.6-Flash | half B200 node | 9.30 | 2.8 | 20.26 | 14.69 |
| GLM-5.3-Flash | half B200 node | 9.13 | .9 | 20.26 | 14.42 |
| MiniMax-M3, infra-affected | 8×H100 | 8.79 | 0 | 158.31 | 1.61, not comparable |
| Qwen3.6-35B-A3B | H100 | 8.02 | 11.1 | 5.64 | 45.48 |
| Step-3.7-Flash | 4×H100 | 7.32 | 10.3 | 54.72 | 4.28 |
| Granite-4.2-8B | 4×H100 | 6.54 | 5.6 | 43.81 | 4.77 |
| Qwen3.5-9B | H100 | 6.04 | 25.0 | 7.66 | 25.22 |
| Mistral-Small-4, role-rewrite confound | 2×H100 | 4.74 | 24.1 | 30.20 | 5.02 |

GPU lifecycle includes boot/smoke/idle of retained attempt, excludes runner CPU
and discarded attempts, priced at Spot list rates. Recomputed DeepSeek/Qwen-27B
inputs give $24.30/$24.03 versus frozen $24.29/$24.04 above; points/$ are frozen
ledger values. Not actual whole-program cost. Mistral's developer→system/user
rewrite/chat-completions bridge prevents a clean model comparison.

SC-EVAL-044 originals/control `7c4707230`/3b `64e6176ed` each had 108 sessions:
Qwen clean exits **98/3/2**, Mistral **54/29/26**. Qwen critical errors rose
3.7%→9.3%. 3b R1–R4 revert criteria did not trigger: R1 was initially 39/317,
then 0/317 after **post-outcome adjudication**, not an unchanged original checker.
Release not established. Gate-bypass 20-key Qwen subset clean exits 16/0/19;
acceptance stderr lines 0/277/0. Source/requests support gate-mediated termination
failure, but cache/load/time differed and usefulness recovery was not graded.

SC-EVAL-041 capsule details: tuned after 14 rules plain 4/5, Stateful 5/5;
held-out within3 2/5 both. Structural floor tuned 5/5 both, within3 2/5 both,
with zero wrong-active and zero rejected-working routes over 104 capsules.
82/104 empty command-route lines were separately repaired. Offline piggyback
three-way over 53 boundaries: 46 empty records, four missing trailers, two valid;
no recall gain, +33% units/+89% compaction latency. Hand upper bound 5/7 and
4/5 does not make it product evidence. Seven-needle rubric differs from old five.
Three leak-path failures led to cut `717fc9d61`; inactive scaffold archived.

## October 14 runtime and application checks

Historical landings, not fresh tests:

| Slice | Recorded result / qualification |
| --- | --- |
| Inert answer safety `f256e307e` | Linux runtime/extension/exec/protocol no new failures; TUI/app-server base logs not fresh |
| Prompt/unused-code cleanup | Models-manager 54 passed; Windows sandbox compile exit 0 only; absence-of-reference evidence for core prompts |
| Exec images | `bbb9146b1`, Linux exec 179 passed; not exact image-delivery acceptance |
| Workspace 2C/2A/2B | Final combined extension/runtime 299/63 passed after compile/fixture/review failures; privileged early-return tests are not denial assertions, deep fixtures use elevated limits; ABA/blocked-IO/readers remain |
| Tool IDs/replies | Digest long/invalid source IDs, blocked `openIssues` and bounded replies; extension/runtime 309/65 passed after roster/oversize failures; small allowance/cursor and Bazel-lock debt retained |
| Stream retry | Filtered core 344 passed; full 4,932 passed/7 failed, four quarantined and three reproduced on base; real bridge recovery unverified |
| Goal errors / tool-path test | Runtime/extension 67/309 then 67/311 passed; no full app-server or alternate-case/symlink qualification |
| Answer activation `73f2d82da` | Runtime/extension 72/324; downstream filtered app-server/protocol/exec/TUI 161/314/182/268. Blocker-loss redirect repaired. Python generated type hand-edited, full generation not run; matched native acceptance checks unrun |
| Gemma `b59589c84` simple check | 10 Answered, 0 Completed/Partial/Blocked/nonzero/refusals; one model/simple prompts/no required criteria, not general Work verification |

Hands-on descendants varied: CSV/git/image routes improved in individual runs;
other CSV/git/resume runs ended Partial, sometimes lost certification caveats or
left checker scripts/commits. “Always failing check” prompts caused the model to
rewrite the checker unless forbidden. Memory recall often came from continuity;
Forget did not purge that route. Executable/source identity was sometimes inferred,
companions/load/concurrency changed, so timings and cause cannot be attributed.

Operator-reported Gemma-4-31B OCR/translation in a restricted environment: first request exceeded 16,384
input window; 32,768 allowed the page. Prefix/task split unmeasured. One complete
6,557-byte JSON was followed by twelve identical trailer-only finals; completion
remained Running, two compactions lost exact delivery, `-o` took last message.
All output-bearing runs needed rollout recovery. Later trailer-free Answered
addresses stopping, not exact retained-answer preservation.

On 33 aligned raw-OCR pairs: chrF++49.3, bootstrap 95% CI45.8–53.1, target200 not
met; number exactness27/32=84.38% with one reference exclusion. Three checker
comma/date artifacts; source `802`→`8.02` and `1016`→`2016` remain flagged source
normalizations. Throughput11.7 sacrebleu tokens/s, not model tokens, on remote H100.
IndicTrans2 gated403, **IT2_NOT_RUN**; no controlled head-to-head. Text-only correction
missed source-image details. Draft output is not expert/customer acceptance.

## Canonical-store and restricted console component evidence, October 15–18

Every row below is **historical component-only evidence** on its recorded
candidate/helper, not native cutover, populated migration, first-database milestone,
MCP/foreign writes, offline or customer qualification. Windows/WSL counts overlap
across evolving suites and must not be summed. Helpers and source-hash custody are
not cryptographic build attestations. Initial failures remain in the archived
source records even where a distinct repaired run passed.

| Component / source or date | Recorded gates and witness | Limit or retained failure |
| --- | --- | --- |
| S0 native baseline, 10-15 | Installed Luna answered 42, exact LF file, home-scoped add/list/Forget | Not project-local conversion; separate edit claimed success while cell running/no file, FAIL |
| Location S1a | Windows/WSL9 each, junction/symlink executed | No attached native caller |
| Bootstrap registry `a3731ab3f` | Windows/WSL19 each; immutable metadata/rollback/busy | Independent connections not forced process overlap |
| Concrete filesystem checks | Windows/Linux21 each | Earlier opener27/27 held for descendant submount omission |
| Repaired opener | Windows/Linux32 each; policy before SQLite on actual paths | No arbitrary sync detection, hostile-SQLite/crash proof |
| Bootstrap reader/init | Reader5 then init7 each OS; real 9p submount refuses, ext4 same bytes read; actual Luna identities exact | Pending snapshots, Bazel target collision/order defects repaired; no knowledge read |
| SQL feasibility | 25 PI+8 runtime migrations, 67 tables/49 triggers, integrity/FKs | Count-matching incomplete copy lost revision/thread; empty schemas not populated SQLx preservation |
| Manifest preparation/install | Windows/Linux35 then38 each; late-trigger rollback, busy/retry | Real schema helper initially failed absolute SQLx path, repaired |
| Real-domain helper | 67 tables/35 records match independent oracle; two-store Luna exact | Empty knowledge/run rows; no native attachment |
| Registered reader | Windows/Linux42 each; readonly8/refusal/close-move | Same-user namespace replacement not security boundary |
| Owned snapshots | Windows/Linux45 each; stable WAL view then next-snapshot change, unfinished close-only | Begin/finish cancellation/commit errors not all injected |
| Effective-hit extraction `ccd04ee04` | Windows137; Linux first135+2timeouts, serial137; seven-call helper/Luna read/Forget isolation | Helper uses legacy PI; no neutral native activation |
| Metadata reader `cc8fc834b` | Windows142; Linux140+2 baseline timeouts, later unchanged-source149 pass | Typed clocks >2^53, incomplete coverage; exact helper identities, no effective-content claim |
| Attributed CLI `af3b98aaea` | PI149 Windows, focused Linux7; CLI11 each; two direct tool results exact | Sole model final double-escaped Marathi: **FAIL_FINAL_CONTENT_FIDELITY** despite exit0; no normalization |
| GNU-host opt-in `dc707b23a8af` | Windows Bazel7 cases/2 targets, CLI11; WSL PI149; default lane E0463/dependency failures retained | Sole model **FAIL_NATIVE_TOOL_AVAILABILITY** from prepared unavailable exec host; not MSVC/full native qualification |
| Writer `ab8764ef5d` | Store51 each; direct commit/rollback/drop/Busy and project independence | Direct1 live-SHM observer failure; model raw cells added LF: **FAIL_LITERAL_CELL_CONTRACT_RETAINED**, later matching JSON does not regrade |
| Runtime adapter `78989cce28` | Runtime75/store51 each; later direct3-case and Luna exact commit/stale/late rollback | Earlier **FAIL_ORACLE_PRECONDITION_RETAINED**, sqlite_sequence0 mistaken absent; synthetic acceptance only |
| Same-guard knowledge clocks `2e90c89adc` | PI155 each, one worker ignore; direct4-case and Luna exact | Caller still owns source/steering/findings/run checks; no actual native Forget |
| Negative-learning preparation `6e8b34337d` | PI162/CLI11 each; direct7-case combined stream | Model final kept exec envelopes instead of parsed payload: FAIL, parsing ambiguity retained |
| Run-start binding `919ded934503` | Runtime80 each; direct7-case and Luna exact lossless timestamps | Marker-only, syntheticOnly/nativeAttached:false/completionCertified:false |
| Terminal composition `b6c645a8fc` | Three new fixtures passed; extension Windows337/338, Linux338/339; protocol14 each; Luna fixtures PASS | Permission/timeout failures reproduced on base; synthetic checker, not host-certified completion |
| Extension fixture repair `c945931e1c` | Windows338/WSL339 clean, no skips/retries; denial assertion executes | Fixture fix, not weaker timeout/policy; exact Marathi file-copy journey separate, not native pipe repair |
| Lease adapter | Runtime85 then87 each, Luna two-store exact | Overflow ordering/NUL observer failures retained; uncertain COMMIT separate |
| Acceptance reader `a8835976e6` | Store52/runtime95 each, direct reads/refusals, prospective Luna2 PASS | Direct CP1252 and Luna1 shell mismatch FAIL retained; sort-key omission repaired |
| Request-parameter extraction `97411af572` | Runtime97 each; six legacy materializations, Luna exact | LEGACY parity only; Python unindexed VM steps1517/15017/150017 versus11 indexed is not Rust performance |
| Explicit domain upgrade `e8ec5a4f7f` | Store56 each; cancellation close/reconcile, 2×68-table fixture/Luna | Fixture index/history only; not arbitrary populated upgrade/old-client exclusion |
| Owner manifests `dd220f1165` | Runtime99/PI162/CLI11 each; Luna2 22 CLI calls/two67-table stores | Stale candidate and Luna1 Git-cache failures retained |
| Steering index | Runtime100/CLI11/extension338 Windows, 100/11/339 WSL; transition/busy/uncertain-ACK helper+Luna | Canonical-only index; native history/openers unchanged |
| Bounded project request `31aebbd` | Runtime105/CLI11 each; extension338/339 after baseline timeout;39 helper calls/project-bound deltas | No native activation; sparse ordinal/final population enforced |
| Atomic run creation `333902` | Runtime110/CLI11, extension338/339;29 calls, two stores,131072/131073-byte and64/65 bounds, rollback/replay | Synthetic host receipts; native import/old-writer/lock still open |
| Storage exclusion `7033` | Windows store56/PI167/ext339; WSL56/167/340 | Native Luna **FAIL_RETAINED**, two public files refreshed, zero queries and unsupported final |
| Schema/writer feasibility | Schema-only 163 PI/37 runtime DDL after exclusions; three Python SQLite cases PASS | Delivery **FAIL_RETAINED** malformed final; no native/full partition/import proof |
| Runtime quiescence `b17440e637` | Runtime116 each; native assertions passed row/registry refusal1811, replay/unrelated progress,45guards | First model failed literal/zero native calls; locked second wrote report then cp1252 UnicodeEncodeError. Whole journey FAIL |
| Native store-root launcher | Local build; reader13/13 after fixture repair; joint Python9 then10 | Initial177PASS/fixtureFAIL/2PItimeouts/1skip; timeouts reproduced base; sole model rule/cold read/neutral read passed, **overall exit1 shared-empty assertion retained**, creator unknown |

Restricted console rows:

| Component | Recorded evidence | Boundary |
| --- | --- | --- |
| Exact-byte baseline | Source405-byte Marathi JSON; read-only copies/finals329 bytes/question marks; full-access diagnostic405 | **FAIL** read-only; full-access control not repair |
| Child shell | PS5.1 CLM default529 wrong bytes; explicit/default UTF8 and pwsh407=source+CRLF;18 children reaped | Did not reproduce/fix native329-byte route; initial CLM probe errors retained |
| File defaults | Windows174/Linux192; four real PS5.1 CLM integration cases | Source-matched native candidate still329 bytes, no non-ASCII; file decoding fixed, native transport FAIL |
| Owned-console prototype | Current-user CLM CP65001 preserves405+CRLF/114nonASCII, target7/worker0 after initial attach6 failure | Not restricted-token/private-desktop/native acceptance |
| Restricted mechanism | Real readonly-token/inert SID/private desktop; CP437 corrupts, inherited CP65001 exact, denied write canary, cleanup0 | PASS_MECHANISM only; production ACL/WFP/IPC/package/lifetime still open |
| Requirement inventory | All16 applications+16platform items mapped | Design/inventory only; all held |
| Closed request codec | Protocol5 each; exact u16/strict JSON/helper+Luna | Inactive; no handle ownership/auth |
| Closed stream | Protocol10 each; length before allocation/short/interrupted/EOF | Caller owns closure/deadline; final-envelope model FAIL shared with learning row |
| Response codec/reader | Protocol14 then19 each;11 helper cases and Luna | Initial raw diagnostic/pre-model255 failures retained; inactive |
| Ordered responses | Protocol24 each;13 cases and Luna | Ready→Spawned→RootExited assertions, not process trust |
| Required response frame | Protocol29 each;12 oracles/Luna | No EOF probe/next-frame consumption; CRLF wrapper failure retained |
| External creation positives | Two one-test integration gates;3processes/5real exit cases0/7/259/high-bit,13 refusals, Luna | Actual restricted child, unpackaged creator; no production factory/guardian |
| Guarded lifetime | Native one-test/four-case PASS, direct PASS; sole Luna guardactive1 FAIL despite roots exit0 | Cause **UNKNOWN**, model returned `{}`; not regraded |
| ACK-boundary successor | Separate direct/Luna PASS; simulated ACK1 error5 and real pre-ACK3 wrapper termination retained FAIL_RETAINED | Not retained-query/live-body/controller crash or full recovery |
| Guard-drain diagnostic | Direct active counts[1,0,0], Luna[0,0], no intervention | Non-reproduction, not lifetime-failure explanation |

Inactive transport components on archived stack, October18:

| Stage | Recorded suite | Scope / retained limitation |
| --- | --- | --- |
| LF framing | 15/15 | 8MiB frame, deadline/cancel state/write poison; no caller |
| Stdout admission | 26/26 | 16MiB/64frames, leases/sticky EOF; initial OS5 gate failed |
| Stderr drain | 35/35 twice, overlapping | 64KiB raw ring/8KiB scratch, continuous drain; not native cleanup |
| Shared admission | First42PASS/1FAIL+retryFAIL; repaired43/43 | Test needed second Pending poll, no production weakening; payload credits not RSS/allocator bound |
| Leased writer | 53/53 | Pool-bound future retains frame through flush/drop; raw I/O not enforced |
| Work deadlines | Compile0tests then62/62; precedence repaired62/62 | Init60/RPC30/turn180/compact180/total900 seconds, inactive |
| Shutdown | 67/67 | Separate unclipped5s allowance, not absolute905s lifetime/native cancellation |
| Direct child | 74PASS/1intentional fixture ignore | Canceled-wait retention/observed reap; Drop best effort, no descendants |
| Pumps | 81PASS/1fixture ignore | Cancelled-join ownership/fixed end; observed join only; wrapper Completed not transport success |
| Old guard contrast | Separate1test/1child PASS expected config refusal | Original2PASS/1FAIL/3NOT RUN, old0/6 and65broader failures retained; original refusal cause UNKNOWN |

## Archived active-compaction acceptance, not integrated

| Historical normal suite | Result, every exit 1 |
| --- | --- |
| CLI | 459 PASS / 8 FAIL / 2 runner skips |
| App-server | 1845 PASS / 46 FAIL / 18 TIMEOUT / 11 runner skips |
| Stateful extension | 337 PASS / 2 FAIL / 1 TIMEOUT / 0 runner skips |
| Core | 4292 PASS / 26 FAIL / 77 runner skips |

Earlier broader-memory failure remains separate. Original installed-native
execution: **23 total / 8 PASS / 15 ERROR / exit 1**.
Release and independent native execution remain **unqualified**. Source review
does not establish runtime behavior. Guard early returns/flaky passes retain
their qualifications. Correction/Forget and upstream compatibility require
their own evidence; active compaction remains independently unqualified.

Exact recall/restoration, foreign-write exclusion, live upgrades, ordinary and
Work execution, populated migration59/60, matched evaluation, native home
placement, restricted deployment, offline operation, application quality and
capacity remain held. Consolidation checks do not supersede these limits.
