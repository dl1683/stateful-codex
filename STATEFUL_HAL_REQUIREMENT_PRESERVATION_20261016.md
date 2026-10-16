# H1 requirement-to-regression preparation

Inventory/design only; no new execution, specialist or customer acceptance. Source:
[HAL harness requirements](C:/Users/devan/Downloads/HAL_Harness_Requirements_and_Test_Plan.pdf),
SHA256 `3466d238bc8b3f6c301b9b74b479842500858a87401b8d0dbae4ab5cfcf6e395`.
The 34-page specification distinguishes procurement capabilities, added research
tests and open inputs; the stable application IDs below are its distinct modules.
Physical pages2-4 and11-17 were read for the platform/application scope; page11
was rendered/visually inspected. Extracted UTF8 text/render are retained externally
under stateful_rebuild/hal_requirements_source. The first default-encoding text
extraction was not UTF8 and is retained, not used as the source oracle.

The current owner requires all HAL use cases, local/offline customer operation
and non-Chinese developer/publisher eligibility. That current criterion supersedes
older written-policy/teacher-lineage wording in historical material. Publisher
eligibility, actual local deployment, quality and customer acceptance are separate
gates. Hosted Luna is a development check and cannot establish disconnected use.
No new model recommendation, cloud work, customer data access or HAL repo change
is performed by this inventory.

## Application scope and Stateful preservation boundary

All rows are requirements/preparation, not test results. Each acceptance must retain
source/revision, permission scope, model/tool provenance, failures and omissions.
The next fixture column names concrete future witnesses, not implemented tests.

| ID / application | Protected behavior / next fixture | Remaining acceptance |
|---|---|---|
| H01 Chatbot/assistant | Two users with disjoint documents; wrong-version policy and subsequent revocation; exact citation and truthful unavailable-tool answer. | Authenticated principals/ACL-aware reads, real source retrieval and long-session evidence. Project UUID alone is not tenant security. |
| H02 OCR/translation | Preserve Marathi/Odia/Latin serials, engineering units, digits, block IDs and layout lineage through tools and final files; retain disputed digit crop. | Native405-byte Marathi preservation remains failed; real adapter invocation/quality, field/layout metrics and qualified adjudication remain separate. |
| H03 Summarization | Late-page caveat, conflicting revisions, bounded multi-window processing, all-page ledger and claim citations; valid exports. | Exact publication/compaction survival, source/page accounting and final output reconciliation; no claim from a summary preview. |
| H04 Drafting/voice typing | Marathi/English amount correction, missing template field and cancelled draft; retain user approval and template numbering. | Voice adapter, valid rendered files and complete source-grounded fields, beyond Unicode transport. |
| H05 Dashboards | Read-only query, restricted department metric, changed date filter and source-matching chart/export after refresh. | Enforced DB credentials, query limits, row permissions and real refresh/export evidence. |
| H06 STT/TTS | Overlapping speakers, engineering accent/noise and interrupted bilingual stream; exact timestamps/subtitles. | Real speech/diarization/voice adapters and language/noise metrics; text-tool smoke does not qualify audio. |
| H07 Meeting minutes | Unnamed owner and contradictory due dates; source timestamp/speaker, explicit missing action attribution. | Audio/transcript lineage and reviewed decisions, owners, dates and valid export. |
| H08 Video assembly | Missing asset/invalid codec and multilingual captions/narration; provenance and timing through a rendered artifact. | Lawful assets, real media adapter/output/CPU traces; optional avatar is separate. |
| H09 Coding | Independent failing checker, prohibited checker edit, malicious README and bounded loop; useful truthful Partial when unmet. | Original checks, sandbox/no-exfiltration and source/diff/dependency evidence; no replacement-checker certification. |
| H10 Tender/vendor/invoice/resume scrutiny | Transliteration alias, expired certificate, altered-date duplicate invoice and wrong bank field; clause citations and deterministic3-way match. | Ground-truth fields, monetary/date correctness and transparent matching; retrieval alone is insufficient. |
| H11 Financial viability | Negative cash flow, multiple IRRs, fiscal/time-index differences and lakh/crore units with inspectable formulas. | Reference-spreadsheet parity, numerical/zero-division handling and auditable assumptions; model prose is not a calculator witness. |
| H12 Part inspection | Unseen lot, lighting drift and small boundary scratch; calibrated reject/uncertainty with image provenance. | Real labeled imaging/domain acceptance, per-part recall/false alarms and latency. Numeric image770.23 is only a protected smoke. |
| H13 Product design | Conflicting design revisions, changed tolerance and missing material data; no proprietary geometry egress. | CAD/PLM metadata/version/unit traceability and engineering review; no certified CAE claim. |
| H14 Predictive maintenance | New machine, missing/delayed sensor, policy change and false-alarm burst; read-only/shadow inputs. | Forward-time validation, lead time/asset-hour false alarms, RUL calibration and drift; dedicated specialist/data needed. |
| H15 Demand forecasting | Seasonality/part supersession/no-history/rare costly demand and procurement break across horizons. | Leakage-free forecast metrics, schedule/quantity validity and uncertainty by segment. |
| H16 AOG trend | Changed flight cycles, delayed spares, unseen tail and censored recovery; preserve uncertainty and source age. | Forward-time risk/duration calibration, lead time/out-of-fleet acceptance and explicit aviation safety boundary. |

## Platform scope

These P01-P16 are the platform table in specification section2. They must not be
confused with identically numbered input perturbations later in the document.

| Platform requirement | Required evidence before acceptance |
|---|---|
| P01 Offline/local/no external core transmission | Disconnected cold start, blocked-egress trace and complete endpoint/dependency inventory; no hosted fallback. |
| P02 Shared serving and independent module APIs | Versioned local discovery/API contracts and simultaneous-module replay. |
| P03 Replaceable eligible models / HAL-authored agents | Offline model replacement drill and independent agent integration; neutral project reads are one prerequisite. |
| P04 Grounded retrieval / document storage | Exact file/page/section lineage plus freshness and coverage tests. |
| P05 Per-user history/authentication/RBAC/audit | Principal/role mapping, denied reads and exportable authenticated audit trail. |
| P06 Reliable files/exports/voice/languages | Valid structured/rendered outputs and sampled source-to-output reconciliation. |
| P07 Oracle safeguards | Enforced read-only credentials, rejected mutation, query budgets and appropriate plan/preview evidence. |
| P08 Administration and unified usage logs | Admin-role checks, versioned config export and observed usage. |
| P09 Component/model intake verification | Signed manifests, checksums and tampered/malicious package refusal. |
| P10 Staged offline updates and rollback | Validated update package, dry-run import and rollback rehearsal. |
| P11 Matched UAT before promotion | Exact topology/version comparison and promotion/rollback logs. |
| P12 Version/source/license register | Model/component revision, source, checksum and deployment register. |
| P13 Task performance/scalability/operations | Workload-specific measured latency/capacity, availability/maintenance and operator history;400/600/800 sessions are sizing scenarios, not measured throughput. |
| P14 Governed APIs/local access/isolation | Access matrix, permission-aware retrieval and cross-module denial/red-team evidence. |
| P15 Extensible disconnected deployment | Reproducible local/container manifests and health checks. |
| P16 Training/support/AMC/handover | Agreed operational training, offline-update/support checklist and rehearsal. |

## Local native transport callsite closure

Source-only search at main7e5 finds exactly three production callers controlling
pipe-process console flags in windows-sandbox-rs. This closes the enumeration
prerequisite of the prior console staging document; it approves no protocol/code.

| Route | Current path / non-TTY setting | Compatibility that must survive |
|---|---|---|
| Direct capture | lib.rs::run_windows_sandbox_capture_with_filesystem_overrides -> create_process_as_user, NoWindow | Existing restricted-token/ACL/private-desktop preparation, capture/timeout/cancel and normal-root descendant preservation. |
| Unified-exec legacy | unified_exec/backends/legacy.rs::spawn_legacy_process -> spawn_process_with_pipes, Separate stderr, NoWindow | Stdin lifecycle, separate output channels, job/desktop ownership and normal-vs-cancel descendant handling. |
| Elevated runner | bin/command_runner/win.rs::spawn_ipc_process -> spawn_process_with_pipes, NoWindow | Authenticated framed IPC, dedicated sandbox-account restriction, managed-image packaging and equivalent lifecycle/streams. |

process.rs owns the common CreateProcessAsUserW primitive. Its stdio=None case
does not apply CREATE_NO_WINDOW; that behavior must not be changed incidentally.
ConPTY uses its separate spawn path. Resolve every affected direct/unified caller
when activating a future trusted console worker; fixing only the GUI runner does
not cover the presently configured unelevated launcher.

Current engineering evidence remains limited to the retained protected-baseline
packet and source-matched qualification packet: file-default repair, native exact
Marathi copy failure, numeric image smoke and current-user console experiment.
The broker/envelope and specialist pilots belong to the separate HAL-eval owner;
consume its reviewed receipts without converting narrow qualification into scored
vertical-slice, source-meaning, offline appliance or customer acceptance. CIDCO
human flags/delivery review stay open; no ambiguity is resolved by this inventory.
