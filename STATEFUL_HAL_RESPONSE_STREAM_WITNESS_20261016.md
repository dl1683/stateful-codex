# Inactive closed console response reader

Source `d74dd9f58f4501099cc040a6c835b5721c30158a` adds a bounded blocking
`read_closed_response` and shares the existing private frame collector with
requests. Fresh exact source review APPROVE (`response_stream_20261016/review1.txt`).
Evidence lives under
`C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/response_stream_20261016`.

The reader admits length1..=1MiB before allocation/body access, reads the exact
body, handles Interrupted reads, probes only one trailing byte and requires EOF.
It delegates original bytes to the unchanged strict response decoder. Static
errors retain framing/payload/admission distinctions; nonce correlation is not
authentication. The caller must close the sender and enforce a deadline that
unblocks arbitrary Read. This is not a persistent event pipe, worker, event-order
validator, handle owner, drain certificate or native launch route.

## Local component evidence

- Windows and WSL `just test -p codex-windows-console-protocol`:19/19 PASS each,
  zero skips/retries. Five new tests cover every event and full u32 values,
  partial/Interrupted IO, every truncated boundary, rejected header sizes before
  body access, maximum admitted body buffer, trailing probe, header/body/EOF
  errors and strict payload/nonce refusals. Existing request tests still cover
  lossless rawu16 Marathi and lone-surrogate transport.
- Source maps before/after each package gate match. Local shared mutex/jobs2/
  RAM floor23221225472; no cloud build, new dependency or schema change.
- External helper1 built and its11 happy/refusal file cases passed, but review1
  REQUEST_CHANGES found raw helper-level file-open diagnostics. This is retained,
  not upgraded to qualified evidence. Its29 bound inputs were archived before
  the helper changed. Candidate protocol source was unchanged by that repair.
- Helper2 uses static nonzero setup/IO failure markers; fresh helper review2
  APPROVE. Build2 locked/offline succeeded. `component_trial2` direct oracle
  passes11 response cases plus a missing-file static/nonzero check. It retains
  source/migration/helper-lock/binary bindings; this is not build attestation.
- First launcher attempt exited255 before any model event/tool call or final
  file. Its failure remains under `component_trial2/luna`; it earns no acceptance.
  The corrected controller passes the prompt through stdin, avoiding the batch
  launcher's multiline argument boundary.
- ONE actual unchanged-launcher gpt-6-luna session in `luna3`:12.540s, one
  completed inspector command and completed turn; exact whole final JSON agrees
  with the independent oracle and all33 frozen bindings match postflight.
  Status `PASS_LUNA_CLOSED_FILE_COMPONENT_ONLY`; no judge or normalization.

The finite-file helper does not establish pipe blocking, cancellation, deadlines
or real worker behavior; fragmented/error IO is unit-test evidence only. No
candidate installation or launcher/policy change occurred. Native Marathi,
worker spawn/ownership/lifetime, offline and customer HAL gates remain open.
The first project database/import/old-writer milestone remains unfulfilled.
Final scoped fix/format and promotion disposition are recorded separately.
