# HAL closed console request stream - 2026-10-16

INACTIVE SOURCE QUALIFIED; native final-contract trial FAIL retained.
Promotion requires a separate exact closeout approval/receipt.
Source2e23c2002f9821dde6d27cf3c217c0ce60e2ea1d adds221/deletes2 lines in four files.
Fresh final_source_review4 independently APPROVES EXACT STREAM SOURCE.

read_closed_request reads the exact four-byte header, validates nonzero length
at most1MiB BEFORE allocation/body access, reads exactly that bounded payload and
requires EOF with one-byte probing. Short/interrupted reads and transport failures
are handled; diagnostics expose only static Framing text. Existing strict closed
codec retains payload/direction/version/nonce/shape/handle-width/aliasing admission
and lossless rawu16 vectors. The shared length helper also serves the slice decoder.

The caller MUST close the sender and enforce a deadline that unblocks an arbitrary
blocking Read. This is a closed single-request transport, not a multi-frame status
stream. No worker/WinAPI/owning-handle/launcher/job/policy change or activation.
Received numbers remain unauthenticated non-owning data. Parent custody, rights,
paired-job authority, raw UTF16 handoff and lifecycle still need qualification.

Windows/WSL just test each10PASS/zero skips at unchanged defaults, identical full
9580-source maps. Tests cover real partial/interrupted IO, every truncated frame,
zero/oversize refusal before body access, a bounded trailing-byte probe, errors at
header/body/EOF and complete Marathi-codepoint/lone-surrogate rawu16 preservation.
Scoped fix/final fmt pass; no tests after cleanup. No dependency/schema change.

Shared external helper build2 locked/offline/local PASS8.52s; full PI and stream
source/auxiliary maps stable. direct_trial2 passes7synthetic cases, including exact
roundtrip/trailing frame bytes from an independent Python oracle. Natural job
quiescence is observed from exit, bounded5s/512samples; actual activeBefore/after0
and root0 required. Source/whole artifact postflight passes. The original direct1
lifecycle failure remains failed; no trial was normalized or regraded.

Shared Luna proposal luna_trial2 is prepared under manifest
c9e70ae5862963d917f5d6917b35e848014526479d826664d7fea1aeda271d62;
fresh frozen review APPROVED before ONE actual unchanged-launcher/Luna session.
Two exact sequential custom cells, complete raw/public helper payloads, exact
frame bytes and all100pins/19158source/closed-artifact checks agree; no other tools,
activeBefore/after0/root0. Trial stays FAIL_FINAL_OUTPUT_ENVELOPE_RETAINED: the final
and output file contain execution envelopes, failing the frozen parsed-payload
expectation. Prompt wording was ambiguous about stdout parsing; no payload-loss
or new source defect is established. No retry/normalization or usability PASS.
This harness uses an unchanged installed launcher and a source-matched EXTERNAL
helper; it does not install or exercise a newly implemented console worker.

Evidence: C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/console_stream_20261016
and ../learning_20261016 (shared witness). Response/worker/lifecycle, native405-to329
Marathi and nested final escaping, compaction/restart, offline/customer/HAL and
first project database/import/old-writer/full-certificate/fence-removal gates stay
OPEN. Publisher-only policy, no new cloud build/test and IT2_NOT_RUN are preserved.
