# H1 restricted-token console mechanism witness, 2026-10-16

Status: PASS_MECHANISM only. Installed native exact Marathi-copy failure remains OPEN.

A separately reviewed external executable used the current production readonly-token constructor with an inert fixture SID and private desktop. A hidden sole-client worker initialized its console to65001. The control launched Windows PowerShell5.1 with NoWindow; the candidate inherited the worker console. Neither script set its language mode. Both reported natural ConstrainedLanguage, an unchanged denied-write canary, unchanged input, target exit7, worker exit0 and zero owned processes after outer-job cleanup.

The control reported input/output437 and corrupted the405-byte UTF-8 body. The candidate reported input/output65001 and preserved that entire body. Raw stdout includes framing/metadata, so full stream sizes448/528 differ from body sizes. Both stderr streams are empty. The workerOutputCpAfter receipt field is a constant65001, not a measured post-exit console value; no post-exit preservation claim follows. The deterministic scorer requires a valid constrained/denied pair AND the corruption/preservation contrast; it cannot pass on a failed control.

## Custody and retained evidence

- Frozen source HEAD: fa8b727ab2b336f9a73cad8951ff1774d50f8b0c; no production Windows source changed.
- Frozen helper SHA256: 27c3c5fd867dda17b3771fe9d5e516f4d6b99e1cec9e5e5fd1afc160cff5165f.
- External root: `C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild`.
- `h1_restricted_harness/src/main.rs`, Cargo.toml/lock: source review3 APPROVE MECHANISM HARNESS SOURCE. Build1 failed a misplaced API import; original source and failure retained. Fixed import and explicit existing feature, reviewed before build2.
- `h1_restricted_probe.py`: controller review3 APPROVE RESTRICTED PROBE CONTROLLER; prior unexecuted controllers/reviews retained.
- `h1_restricted_build2.log`: local locked/offline build exit0, shared build mutex, jobs2 and RAM admission/recheck. Full tracked AGENTS/codex-rs plus helper/controller source hashes captured before/after. Artifact hashes record custody, not reproducible-build attestation.
- `h1_restricted_candidate_20261016/manifest.json`: frozen binary/helper/controller/build-receipt hashes. Scorer independently requires the complete nonempty unique input set and verifies its current contents before launching.
- `h1_restricted_probe/report.json`: validPair=true, intendedContrast=true, binaryUnchanged=true, 8856 bound inputs; all subordinate raw artifacts hashed. `control/` and `candidate/` retain raw child streams, worker file/delivered receipts, canaries and controller cleanup receipts.

## Limits and next gate

The inert-SID token/private-desktop construction exercises a production primitive, not the full native account/ACL/WFP/network/IPC/package route. Outer probe cleanup deliberately terminates the owned job after normal exit and does not qualify product normal-exit descendant preservation. No cancellation/drain/backpressure/concurrent-console, elevated account, image packaging, native launcher or model-use qualification occurred. No cloud work or model judge.

A future trusted console worker must preserve all three pipe routes and their stdin/stderr, job/cancellation/normal-descendant contracts described in STATEFUL_HAL_CONSOLE_TRANSPORT_DESIGN_20261016.md. The installed product still fails the retained405-to329-byte Marathi copy. H1/customer/offline acceptance remains open; this result justifies the next transport implementation slice.
