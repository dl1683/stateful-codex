# H1 native qualification: file defaults fixed; Unicode transport still fails

This packet supersedes only the earlier statement that no source-matched native
candidate had been run. H1 customer acceptance remains open. The prefix source
e75db50113774f9baabfa30221fd8a2fed43c83c is integrated unchanged in main.

## Frozen local build and launcher journey

Receipts below are relative to the retained external directory
`C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/`.

- `h1_bundle_build1.log` failed: the default upstream V8 sandbox Windows archive
  and generated binding were unavailable. No sandbox feature or pin was disabled.
  Following `third_party/v8/README.md`, build2 used the existing Codex release
  archive plus its matching binding and checksum manifest. Archive SHA256
  732ec5da4243aa166799780c8519a5eea6f32f6e47657a323342794dc3c239d6
  matches the existing Bazel pin; binding SHA256
  dabf78ba1faac127660db9862b1d0354175c71b8db2d4fcb5bacbd9c93576b16
  matches that release's pair manifest. `v8_cache/verified_pair.json` retains
  paths, public URLs and hashes. V8 versions, features and lockfiles did not change.
- `h1_bundle_build2.log` passed local offline/locked compilation of CLI,
  code-mode-host and Windows sandbox, with jobs2, the shared build mutex and
  23,221,225,472-byte free-RAM admission. V8 was a downloaded prebuilt dependency;
  no remote build/test was run. Source inputs were hashed before/after.
- `h1_candidate_e75_20261016/manifest.json` freezes five executables. CLI SHA256
  4c1fd373e172aa07f774adee95d9331718924a7b0b3de7a46bb7b147853430b1.
  These hashes are custody evidence, not cryptographic build attestation.
- A separately source-reviewed, default-preserving launcher hook temporarily
  selected that absolute binary through a child-only environment variable. The
  actual `C:/Users/devan/bin/scodex.cmd` invoked Luna with `--no-daemon`, read-only
  sandbox and stdin prompts. The original launcher was restored byte-for-byte in
  `finally`. No installed executable was overwritten. Proposal, original bytes,
  qualification script and source-only review are retained externally.

`h1_native_qualification_e75/report.json`, its three run directories, prompts,
JSONL and final answers retain all results. Each launcher invocation exited0.
Baseline metadata and candidate metadata each used two completed commands;
candidate exact-copy used one. Both metadata runs report WindowsPowerShell
5.1.26100.9711, ConstrainedLanguage and console output codepage437. Baseline file
default is null; candidate `Get-Content:Encoding` is `utf8`. This qualifies the
narrow file-default fix through the native route.

The synthetic source is405bytes with114 non-ASCII bytes, SHA256
c4f90eae982f01c0c9308a99ac84c06545b1fa3e5e75e86b47c3620d48de5120.
Candidate copy still returns329bytes with zero non-ASCII bytes, SHA256
c8cb532b58429c50b537f822454a878661eadfef8e0b0ce055d617eebb184c69.
That is the known failed final-copy hash, not a transport pass. All source copies
remained unchanged. This fixture is not a translation or IndicTrans2 execution.

At qualification close, launcher SHA256 is
daf2303629de12cd1a049c791a141f154edddfeca4463ce38a903ecec9ea8fae.
Installed CLI/host hashes and four home schema/migration fingerprints match their
pre-run values. Fingerprints do not prove unchanged rows, cross-database
quiescence or rollback. Normal engine session writes and runtime helper-cache
materialization are allowed and may have occurred. No home conversion was done.

## Console startup mechanism experiment: separate from native acceptance

`h1_console_startup_probe.py` was reviewed before each changed variant. It uses
current-user children with self-imposed CLM, atomic job-list assignment, suspended
creation, hidden startup, explicit stream capture and an intentional exit7. It
does not reproduce restricted-token/private-desktop/read-only ACL execution.

- Run1 `h1_console_probe_20261016T012042Z/` stopped at the worker's original-console
  guard: CP437 existed despite CREATE_NO_WINDOW. No child hypothesis was tested.
- Run2 `h1_console_probe_20261016T013057Z/` used a detached worker. Control reproduced
  question marks. Attaching to a newly created suspended hidden-console child
  failed with ERROR_INVALID_HANDLE6 before resume. Its own child was terminated
  and wait0 recorded. Reject that proposed startup ordering.
- Run3 `h1_console_probe_20261016T021345Z/` retained those controls and added an owned
  no-window bootstrap. Before any encoding mutation, the worker proved its console
  client list contained exactly its own PID. It initialized CP65001, then created
  PowerShell inheriting that console. Child metadata says CP65001/CLM; stdout ends
  with all405 fixture bytes plus CRLF, retaining114 non-ASCII bytes, exit7,
  worker exit0/no timeout, and atomic job membership. CLIXML first-use progress
  remains on stderr: this is not clean-stream or end-to-end product acceptance.

Next H1 unit must design a native trusted console bootstrap and qualify actual
restricted-token creation, private console/desktop, job custody, cancellation,
stdin/stdout/stderr drainage, exit propagation and helper packaging. Do not weaken
CLM/sandbox policy or adopt the failed pre-resume AttachConsole approach. Neither
this mechanism witness nor the prefix fix establishes offline, model quality,
translation, customer delivery or a production transport repair.
