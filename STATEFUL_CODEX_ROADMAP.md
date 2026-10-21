# Stateful Codex roadmap: 110-item inventory

Reconciled **2026-10-20** against runtime baseline
`997007527c5bb1d03c4d6b67cd5f8786b37b3e17`.
This preserves all **R01–R78, H01–H16 and P01–P16** from the October 17
council inventory, without republishing private council transcripts or inputs.
Later design directions D01–D26 refine these rows; they are not extra inventory
IDs or evidence of implementation.

**Implemented** credits only the named landed slice, not full product
acceptance. **Inactive** means source exists without relevant activation.
**Held** includes open, blocked, deferred and unqualified work.
**Rejected** identifies a deliberately discarded mechanism.
**Archived** identifies preserved experiments/drafts, not delivered capability.
Where several apply, the row states the controlling disposition and the limit.
The [current-state page][STATE] separates fresh checks from historical evidence.
Inventory totals: **14 implemented slices, 90 held, 4 archived, 1 rejected,
1 inactive = 110**. The 32 HAL rows are all held.

## Product and harness, 78 items

| ID | Item | Disposition | Evidence and remaining boundary |
| --- | --- | --- | --- |
| R01 | Native per-project root/fresh start | Implemented | [Launcher][LAUNCH] preserves native schemas/locks; no import or one-file conversion. |
| R02 | Neutral native reader/database guide | Implemented | [Native reader][NATIVE] is bounded, read-only and partial, not a complete authenticated graph. |
| R03 | Extra shared-empty assertion | Held | [Evaluation][EVAL]: original placement journey overall exit 1 retained; creator/accounting unresolved. |
| R04 | Git/plain-root/override/move/worktree selection | Implemented | [Launcher][LAUNCH] routing slice and tests; broad portability not qualified. |
| R05 | Writer contention/completion locks | Held | [Services][SERVICES] and [storage][BBSTORE] retain locks; placement alone does not solve same-project contention. |
| R06 | B0 inherited accounting/recovery | Held | [Upstream archive][CONSOLIDATION]; no promotion, dirty payloads are not in commit tags. |
| R07 | Fork/upstream migration 59/60 | Held | [Decisions][DECISIONS]; populated-history compatibility unqualified. |
| R08 | Upstream B1 | Held | [Batch policy][CONTRIBUTING]; no validated/promoted B1 claimed. |
| R09 | Upstream B2 | Held | [Batch policy][CONTRIBUTING]; depends on prior batch qualification. |
| R10 | Upstream B3 | Held | [Batch policy][CONTRIBUTING]; compaction replacement preservation unqualified. |
| R11 | Upstream B4 | Held | [Batch policy][CONTRIBUTING]; PartialAnswer/migration compatibility remains a gate. |
| R12 | Upstream B5 | Held | [Batch policy][CONTRIBUTING]; attribution/SDK/TUI/Windows validation remains. |
| R13 | Later upstream inventory/intake | Held | [Batch policy][CONTRIBUTING]; cached archive is not live catch-up. |
| R14 | Schema generation/Windows/Bazel lock debt | Held | [Testing][TESTING] names generators; recipes are checked, regeneration not requalified here. |
| R15 | Merge preservation of retries/context/phases/modes | Held | [Architecture][ARCH]; existing fixes do not qualify future upstream batches. |
| R16 | Full rich root after every compaction | Held | [World State][WORLD] is bounded; first ordinary-request visibility across all paths is not proved. |
| R17 | Delivery classification/packet restoration | Held | [Packet handling][PACKET]; persistence is not selected-checkpoint restoration. |
| R18 | Exact answers across repeated compaction | Held | [Evaluation][EVAL] and [decisions][DECISIONS] retain failed historical probes. |
| R19 | Bounded exact history/E2 | Held | [Decisions][DECISIONS]; paused attribution/eligibility contract, no new model transcript route. |
| R20 | Capsules/salience/duplicate-context experiment | Archived | [Offline capsule][CAPSULE] remains inactive; archived work is not current context delivery. |
| R21 | Explicit add/list/receipts/cold rules | Implemented | [Memory add][ADD], [protocol][MEMORY]; native cold recall not reverified in this pass. |
| R22 | Correction/replacement/supersession | Held | [Controls][CONTROLS] exist; current trim behavior and exact-input archived repair prevent blanket exactness claims. |
| R23 | Forget/Undo/no resurrection | Held | [Controls][CONTROLS], [receipts][RECEIPTS], [native reads][NATIVE]; full route/replay contract remains unqualified. |
| R24 | Plain-words memory and rule application | Held | [Capture admission][CAPTURE]; ambiguity/quotations are not authorization. |
| R25 | Lean3a stage1a inert proposal preparation | Held | [Decisions][DECISIONS]; frozen direction does not grant activation. |
| R26 | Lean3a Apply/status/Dismiss/eviction | Held | [Receipts][RECEIPTS] retain deliberate controls; broad activation/release prerequisite remains. |
| R27 | Capture usefulness trial/trigger | Held | [Evaluation][EVAL]; no fresh usefulness trial or automatic broad-capture mandate. |
| R28 | Conversation decisions/rejected approaches | Held | [Knowledge context][KNOWLEDGE] supports categories; capture quality/attribution incomplete. |
| R29 | Input/span origin and instruction admission | Rejected | Historical broad origin attempt was cut; [capture admission][CAPTURE] remains, narrower future design deferred. |
| R30 | Byte freshness/exact citation routes | Implemented | [Evidence reader][EVIDENCE], [freshness][FRESHNESS]; conclusion freshness is separate. |
| R31 | Controlling-source/current conclusion coverage | Held | [Context map][MAP] cannot establish complete controlling-source coverage. |
| R32 | Topic/deeper-board/file/exact-source recall | Held | [Map][MAP]/[blackboard tools][TOOLS] exist; decisive low-frequency routing remains incomplete. |
| R33 | Index publication economics | Held | [Indexer][INDEX]; profile actual costs before redesign, no savings inferred. |
| R34 | Foreign read/schema/projection contract | Implemented | [Native reader][NATIVE] supplies a bounded eligible slice; no complete history or authenticated feed. |
| R35 | Foreign agent create/revise/retire | Held | [Decisions][DECISIONS]; read-only directive, no safe foreign writer activation. |
| R36 | Stdio MCP over neutral operations | Held | [Reader CLI][READER]; no approved second writer/store or qualified MCP adapter. |
| R37 | Codex→Droid→Codex company brain | Held | [State][STATE]; placement/seeded reads do not fulfill the journey. |
| R38 | Stateful/latent routing bridge | Held | [Decisions][DECISIONS]; supporting research slice only, not live learning or truth authority. |
| R39 | Open-model A1/A2 usability | Implemented | [Answered handling][ANSWERED] and [evaluation][EVAL]; narrow historical successes, no general reliability claim. |
| R40 | A3 inert installation codec/schema | Held | [Decisions][DECISIONS]; migration allocation/compatibility must precede activation. |
| R41 | A4 safe readers/Partial disclosure | Held | [Decisions][DECISIONS]; missing/UNVERIFIABLE outcomes require complete projection. |
| R42 | A5 command-start revision provenance | Archived | [Acceptance observation][OBSERVATION] predecessor exists; unqualified draft preserved in [archive][CONSOLIDATION]. |
| R43 | A6 desired/current/admitted directory identity | Held | [Acceptance workspace][WORKSPACE]; full design/activation gates remain. |
| R44 | A9 inert integration scaffolding | Held | [Decisions][DECISIONS]; must precede A7/A8, not an activation shortcut. |
| R45 | A7 UNVERIFIABLE add/refine/accept | Held | [Decisions][DECISIONS]; incomplete installation/reader/provenance dependencies. |
| R46 | A8 admit/noCheck/restoration | Held | [Decisions][DECISIONS]; old passing evidence must not silently revive. |
| R47 | PartB immutable checks/constrained adapters | Held | [Decisions][DECISIONS]; no generic equivalence authority or weaker replacement check. |
| R48 | Opt-in Work verification v3 | Held | [Runtime][RUN]; unreviewed activation design, not ordinary-question certification. |
| R49 | P2 completion-stop/typed terminal recovery | Held | [Completion][COMPLETION]; refreeze after outcome contract, retain child/drain and false-completion risks. |
| R50 | Matched-216 release/non-Git journey | Held | [Evaluation][EVAL]; original denominator retained, no new release measurement. |
| R51 | 3b final carries deliverable | Implemented | [Deliverable handling][DELIVERABLE], [evaluation][EVAL]; omission/error caveats and release remeasurement remain. |
| R52 | Lossless tool-argument normalization | Held | [Evaluation][EVAL]; portable exact failing cases still needed. |
| R53 | Provider role adapter/fail-fast | Held | [Decisions][DECISIONS]; do not demote developer authority silently. |
| R54 | Bounded exec hydration/reconciliation | Implemented | [Evaluation][EVAL] retains P1 limitations; not all resume/backfill routes qualified. |
| R55 | Prefix/schema/retry cost measurement | Held | [Evaluation][EVAL]; measure real requests before trimming context. |
| R56 | Overload/concurrency/queue/TTFT | Held | [Evaluation][EVAL]; serving confounds not separated by fixture tests. |
| R57 | Trajectory/terminal/lifetime accounting | Held | [Measurement][MEASUREMENT] partial; normal-home reconciliation and full denominator remain. |
| R58 | Ask/Work admission/modes/steering | Implemented | [CLI modes][MODES], [run context][RUNWORLD], [client][CLIENT]; broad native journeys still unverified. |
| R59 | Resume/fork/cross-surface continuity | Held | [Evaluation][EVAL]; existing partial fixes do not qualify all routes. |
| R60 | Browser isolation/Host/Origin/auth | Implemented | [Gateway policy][GATEWAY], [event router][ROUTER]; selected security slice, native multi-session journey unverified. |
| R61 | Hierarchy paging/large-tree/responsive UI | Held | [Client][CLIENT]; no 21k-node qualification or broad redesign claim. |
| R62 | Matched packages/first use/model/goal diagnostics | Held | [Development][DEV] records pairing and blocked native build; release/Windows guarantees incomplete. |
| R63 | Images/modality-aware evidence | Implemented | [Evaluation][EVAL] records exec-image slice; original-document verification remains partial. |
| R64 | Project versus personal/global memory | Held | [Configuration][CONFIG] distinguishes pipelines; comprehensive retention boundaries unqualified. |
| R65 | Durable versus ephemeral retention | Held | [Configuration][CONFIG]; no universal non-retention/cold-restart guarantee. |
| R66 | Crash-safe outbox/exactly-once admission | Archived | [Decisions][DECISIONS]; historical deferred design, not a compaction prerequisite. |
| R67 | Prompt-valid forks/explicit project transfer | Held | [Decisions][DECISIONS]; never infer authorization to transfer history. |
| R68 | Tamper/multi-user/sharing/purge | Held | [Client security][CLIENT]; cooperative same-user SQLite/cookies are not RBAC. |
| R69 | Hygiene/invalidation/index repair | Implemented | [Freshness][FRESHNESS]/[checkout][CHECKOUT] pieces; broader bounded repair depends on demonstrated need. |
| R70 | Semantic consolidation/contradictions/insights | Inactive | [State][STATE]; shadow experiments, no direct-write maintainer activation. |
| R71 | Longitudinal usefulness/lifetime economics | Held | [Evaluation][EVAL] preserves mixed/negative results; mechanisms do not prove product benefit. |
| R72 | External benchmarks | Archived | [Evaluation history][HISTORY]; unofficial historical runs, new program paused. |
| R73 | Providers/logins/computer use/audio/video | Held | [Roadmap scope][DECISIONS]; later expansion, reuse existing seams without implied acceptance. |
| R74 | Retire obsolete rebuild/docs simplification | Implemented | [Reading map][README], [state][STATE], [archive ledger][CONSOLIDATION]; no cleanup/publication/cold-follow claim. |
| R75 | Native Marathi/Unicode/JSON/image delivery | Held | [Evaluation][EVAL]; narrow codec passes do not erase native 405→329-byte failure. |
| R76 | Restricted execution/job/guardian/cancellation | Held | [State][STATE]; components do not qualify complete native policy/descendant drain. |
| R77 | Translation/OCR/layout/human delivery | Held | [Evaluation][EVAL]; drafts are not customer-ready, disputed digits/dates and review remain. |
| R78 | Offline model appliance/capacity | Held | [Decisions][DECISIONS]; no disconnected cold start or production capacity qualification. |

## HAL applications, 16 items

These are acceptance responsibilities, not promises of 16 new frameworks.
Historical draft/component evidence is in [evaluation][EVAL]; customer input,
expert review, eligible data/models and actual deployment tests remain necessary.

| ID | Item | Disposition | Evidence and remaining boundary |
| --- | --- | --- | --- |
| H01 | Chatbot/virtual assistant | Held | [Harness architecture][ARCH]; HAL-grounded acceptance absent. |
| H02 | OCR/language translation | Held | [Evaluation][EVAL]; draft-only, disputed digits/date/source crops require expert review. |
| H03 | Document summarization | Held | [Evaluation][EVAL]; general capability is not HAL vertical acceptance. |
| H04 | Drafting/voice typing | Held | [Evaluation][EVAL]; specialist evidence and edit lineage unqualified. |
| H05 | Dashboards | Held | [Decisions][DECISIONS]; authorized schemas and reproducible calculations required. |
| H06 | Speech-to-text/text-to-speech | Held | [Decisions][DECISIONS]; local models, speakers/numbers and language quality unqualified. |
| H07 | Meeting minutes/discussion records | Held | [Decisions][DECISIONS]; source timestamps/speakers and attribution review needed. |
| H08 | Video generation/assembly | Held | [Decisions][DECISIONS]; authorized media/local dependencies/temporal lineage needed. |
| H09 | Coding assistant | Held | [Architecture][ARCH]; restricted offline customer execution not qualified. |
| H10 | Tender/vendor/invoice/resume scrutiny | Held | [Decisions][DECISIONS]; domain rubric and adverse-conclusion evidence needed. |
| H11 | Financial viability | Held | [Decisions][DECISIONS]; assumptions/calculations and expert inputs needed. |
| H12 | Visual parts inspection | Held | [Decisions][DECISIONS]; blocked on approved labels/images and engineering review. |
| H13 | Assisted product design | Held | [Decisions][DECISIONS]; blocked on drawings/constraints/checks, no production authority. |
| H14 | Predictive maintenance | Held | [Decisions][DECISIONS]; blocked on forward-time data and outcomes. |
| H15 | Demand forecasting | Held | [Decisions][DECISIONS]; authorized history, forward split and uncertainty required. |
| H16 | Aircraft-on-ground trends | Held | [Decisions][DECISIONS]; approved history/outcomes/experts, never aircraft-release authority. |

## HAL platform, 16 items

| ID | Item | Disposition | Evidence and remaining boundary |
| --- | --- | --- | --- |
| P01 | Offline/air-gap | Held | [Development][DEV]; no blocked-egress/disconnected cold-start qualification. |
| P02 | Shared local serving/APIs | Held | [Architecture][ARCH]; no authenticated customer load/capacity qualification. |
| P03 | Replaceable models | Held | [Evaluation][EVAL]; bridges exist, HAL model/license/capability acceptance absent. |
| P04 | Grounded retrieval | Held | [Evidence reader][EVIDENCE]; HAL entailment and complete controlling-source coverage unqualified. |
| P05 | Identity/history/RBAC/audit | Held | [Client][CLIENT]; local cookie and caller labels do not establish multi-user principals. |
| P06 | Files/exports | Held | [Evaluation][EVAL]; draft PDF/DOCX/JSON capability is not exact customer delivery. |
| P07 | Read-only Oracle | Held | [Decisions][DECISIONS]; needs DB-enforced read-only identity and approved queries. |
| P08 | Admin console | Held | [Decisions][DECISIONS]; actual operator roles/authorization absent. |
| P09 | Component scanning | Held | [Development][DEV]; full appliance/model/license manifest not qualified. |
| P10 | Offline update/rollback | Held | [Decisions][DECISIONS]; signed matched bundles and write-preserving recovery required. |
| P11 | Production-matched UAT | Held | [Evaluation][EVAL]; not run, approved workload/hardware/roles/experts needed. |
| P12 | Version register | Held | [State][STATE]; source pin is not complete appliance/tool/model register. |
| P13 | Latency/capacity/operations | Held | [Evaluation][EVAL]; 400/600/800-user scenarios not measured. |
| P14 | Governed integration | Held | [Contributing][CONTRIBUTING]; external writes require explicit authority. |
| P15 | Disconnected deployment | Held | [Development][DEV]; site/dependency/login/update qualification absent. |
| P16 | Training/support | Held | [Decisions][DECISIONS]; customer roles, accepted workflow and handoff outstanding. |

## What happens next

Keep memory correctness and compaction qualification separate from upstream
migration qualification, inactive installation/Work designs and customer/HAL
delivery. Reuse the archived designs as evidence, not as authority to activate.
First resolve resource/artifact admission and fresh independent verification.
Do not repeat a design council or create a new framework merely to relabel
an unrun gate. No timeline or universal acceptance follows from this inventory.

[README]: ./README.md
[STATE]: ./STATEFUL_CODEX_CURRENT_STATE.md
[ARCH]: ./ARCHITECTURE.md
[CONFIG]: ./CONFIGURATION.md
[DEV]: ./DEVELOPMENT.md
[TESTING]: ./TESTING.md
[CONTRIBUTING]: ./CONTRIBUTING.md
[DECISIONS]: ./STATEFUL_CODEX_DECISIONS.md
[CONSOLIDATION]: ./STATEFUL_CODEX_CONSOLIDATION.md
[EVAL]: ./STATEFUL_CODEX_EVALUATION.md
[HISTORY]: ./STATEFUL_CODEX_EVALUATION_HISTORY_2026_09_TO_10_08.md
[LAUNCH]: ./scripts/scodex.py
[NATIVE]: ./codex-rs/project-intelligence/src/blackboard_storage/native_read.rs
[READER]: ./codex-rs/stateful-memory/src/main.rs
[BBSTORE]: ./codex-rs/project-intelligence/src/blackboard_storage.rs
[SERVICES]: ./codex-rs/ext/stateful/src/services.rs
[WORLD]: ./codex-rs/ext/stateful/src/world_state.rs
[PACKET]: ./codex-rs/core/src/context_manager/history_conversation_packet.rs
[CAPSULE]: ./codex-rs/ext/stateful/src/working_set/mod.rs
[ADD]: ./codex-rs/ext/stateful/src/memory_add.rs
[CONTROLS]: ./codex-rs/ext/stateful/src/memory_controls.rs
[RECEIPTS]: ./codex-rs/ext/stateful/src/memory_receipts.rs
[MEMORY]: ./codex-rs/app-server-protocol/src/protocol/v2/stateful_memory.rs
[CAPTURE]: ./codex-rs/ext/stateful/src/capture_admission.rs
[KNOWLEDGE]: ./codex-rs/project-intelligence/src/knowledge.rs
[EVIDENCE]: ./codex-rs/project-intelligence/src/evidence.rs
[FRESHNESS]: ./codex-rs/ext/stateful/src/source_freshness.rs
[MAP]: ./codex-rs/project-intelligence/src/context_map.rs
[TOOLS]: ./codex-rs/ext/stateful/src/tools/blackboard.rs
[INDEX]: ./codex-rs/project-intelligence/src/indexer.rs
[OBSERVATION]: ./codex-rs/ext/stateful/src/acceptance_observation.rs
[WORKSPACE]: ./codex-rs/ext/stateful/src/acceptance_workspace.rs
[RUN]: ./codex-rs/stateful-runtime/src/run.rs
[RUNWORLD]: ./codex-rs/ext/stateful/src/run_world_state.rs
[ANSWERED]: ./codex-rs/ext/stateful/src/answered.rs
[COMPLETION]: ./codex-rs/ext/stateful/src/completion.rs
[DELIVERABLE]: ./codex-rs/ext/stateful/src/deliverable.rs
[MEASUREMENT]: ./codex-rs/stateful-runtime/src/measurement.rs
[MODES]: ./codex-rs/utils/cli/src/stateful_mode_cli_arg.rs
[CLIENT]: ./clients/stateful-codex/README.md
[GATEWAY]: ./clients/stateful-codex/gateway-policy.mjs
[ROUTER]: ./clients/stateful-codex/event-router.mjs
[CHECKOUT]: ./codex-rs/ext/stateful/src/checkout.rs
