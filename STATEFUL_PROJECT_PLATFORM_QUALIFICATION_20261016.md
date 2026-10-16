# S1 native Windows GNU-host qualification

Scope: an opt-in native x86_64 Windows Bazel build path for the existing neutral
project-memory CLI. This is build infrastructure and component qualification;
the first company-brain milestone remains **NOT FULFILLED**.

## Design and resulting source

`--config=windows-gnu-host` selects a real GNU-hosted Rust1.95 compiler for host
tools/proc macros while retaining the existing GNU LLVM application target.
The GNU execution platform inherits the host CPU and selects Windows/GNU/
MSVCRT; matching toolchains and the test bridge require x86_64. The existing rules_rust repository extension supplies normal and
bootstrap variants; the opt-in config gives those matching toolchains priority.
The main generated Cargo graph admits the GNU host triple. Default Windows,
MSVC CI and nightly toolchain selections do not enable this configuration.

The Rust stdlib assembly patch adds the matching official `rust-mingw` component
for GNU targets and declares its self-contained binaries as sysroot inputs.
Native GNU raw DLL imports receive the pinned LLVM distribution's actual
`llvm-dlltool.exe` through a path-mappable File-based codegen argument and explicit
action input. The Windows LLD tool declares that driver in its data. GNU binutils
`dlltool` from rust-mingw needs an absent assembler; the LLVM driver creates import
archives compatible with the actual LLD linker. The native GNU exec/target guard
limits this argument to the new host lane. This supplies real GCC unwind/runtime
archives and a declared import tool; no runtime stubs, removed features or disabled checks substitute
for missing dependencies. The new component's SHA256 is pinned:
`ac0b67b7ee8a35045c26b8e39a524f171b2dc4a18838bb15844308e4442802f2`.

The independent workspace-status repair makes the shell script Linux/macOS
specific, so automatic Windows configuration selects its CMD script. No Rust
application source, Cargo dependency, native launcher, store schema or model
context changes. The shared Windows test wrapper now sets the existing
`INSTA_WORKSPACE_ROOT` independently of its optional cwd change. The attributed
read test snapshots the complete parsed capabilities object because Cargo and
Bazel differ in serde_json object order; ASCII/4096-byte/newline wire checks are
explicit. It still compares every attributed content field and overflow refusal;
no native Luna output is repaired or normalized. Supported storage remains `<project>/.stateful/memory.sqlite`;
native attachment and migration are separate gates.

## Measured gates

Raw commands, inputs, admission/exit receipts, compiler/action records, failures
and review packets are retained at
`C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_rebuild/qualification_20261016`.

- `linux_pi1.log`: full WSL intelligence suite149 runnable tests PASS in210.255s;
  one intentionally ignored subprocess support worker is exercised by its parent.
  Both previously timed-out fixtures pass46.815s/56.978s under unchanged60s limits
  and zero retries. This does not establish the cause of earlier timeouts.
- `windows_cli1.log`: scoped `just test -p codex-stateful-memory`,11 PASS in4.892s,
  zero skips/retries. Rust application inputs remain unchanged in this slice.
- `gnu_actions2.json`: GNU host proc macro, GNU LLVM application and GNU compiler
  repositories resolve; `gnu_rustc_verbose_version.txt` confirms host GNU1.95.0.
- `gnu_time2_runtime.log`: dependency build PASS139.698s after runtime assembly.
- `gnu_cli_test8_utf8_snapshot.log`: both Bazel CLI test targets PASS18.047s,
  seven integration cases (five bootstrap/two read), zero skips/retries.
- `windows_cli2_portable_snapshot.log`: scoped Cargo rerun after test portability
  repair PASS11/11, zero skips/retries,4.494s test runtime/40.67s build.
- `fmt_final1.log`: required just fmt PASS after all tests. Snapshot storage
  then normalized to LF with exact parsed-object equality; no test rerun after
  formatting, per repository instructions. Exact committed source review: `source_review_final1.txt` APPROVE SOURCE scoped.
  Reviewed-HEAD build-only gates1-4 pass; gate4 binds the repaired controller and
  all9,577 input leaves in `candidate_source_inputs4.zip`.
- Frozen trial approval: `luna_frozen_review3.txt`; actual one-shot outcome and
  unconditional postflight: `luna_roots_trial1`. FAIL retained; no native usability qualification.

All Rust/Bazel builds/tests are local under `Local\StatefulB0LocalBuild`, jobs2 and sampled
host admission floor23,221,225,472 bytes. Windows and WSL Rust process checks,
fresh before/after source inventories and controlled RC/local-only flags are
retained. Public repository downloads were permitted; this is not an offline
qualification. Short output base `C:/b/sr_qual` is explicit in the command receipt.

## Retained failures and limits

Baseline E0463: the downloaded compiler was MSVC-hosted while the proc macro was
GNU LLVM; the compiler metadata locator names the exact rejected triple.
An unpromoted global MSVC-host experiment passed the small dependency under a
short base, but SQLx linking exposed native MinGW CRT/fortify symbols. Its default
bootstrap linker path was262characters; a short-base attempt was cancelled while
the wrong shell workspace-status command paused. The interrupt left the client
alive; only its verified read-only metadata CMD child was terminated with zero
Windows Rust processes present. Client exit and clean idle-server shutdown were
observed. No Rust PID was killed; the cancelled attempt is not a passing build.

The first GNU graph omitted its host triple; subsequent build lacked GCC archives;
the first full CLI build lacked dlltool. Later attempts failed on its assumed
prefixed filename, the GNU tool's absent assembler, and the LLVM driver missing
from declared C/C++ inputs. Patch hunk-count and multi-file-header
refresh failures are retained. Source changed before each corresponding new gate;
no failed result was erased or relabeled as acceptance. Once the CLI linked,
two targets failed snapshot lookup at a duplicated crate path. After the
workspace-root repair, one failed on capabilities JSON key order. During the
structured snapshot edit, an explicit diagnostic failed because Python used
Windows default decoding; reconstructing from original Git bytes with explicit
UTF-8 restored exact Marathi content equality. All generated failed snapshots
were moved into evidence; no blanket snapshot acceptance was used. The eighth
full CLI gate passes existing content/identity semantics and the portable snapshot.

Default Windows Bazel, MSVC/ARM64/macOS/full native Codex and disconnected HAL are
not qualified here. The native405-to329-byte Marathi failure and separate final
JSON double-escaping failure remain OPEN. No native project-store routing,
populated migration/old-writer barrier, short atomic completion, compaction
survival, verifiable edit/Forget ingress, MCP, offline or customer acceptance.
The separate upstream executor retains B0/M0-M3 custody; no approved batch was
available during initial inspection. Preserve that dirty work and historical refs.

## Native hands-on closeout

One actual unchanged-launcher gpt-6-luna trial FAIL_NATIVE_TOOL_AVAILABILITY: the prepared code-mode-host=false configuration made the installed exec host unavailable. Native exit0, one completed turn, authoritative function0/custom1, zero command_execution items, final error objects instead of identities. Original validator failure prohibited/extra tool and independent_audit.json retained. All18 postflight entries and current pins match. No retry, repair or normalization; raw output, original failure and unconditional per-binding postflight retained. This does not qualify native usability or content fidelity.
