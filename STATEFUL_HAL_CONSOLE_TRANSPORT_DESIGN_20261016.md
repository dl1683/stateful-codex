# H1 native console transport: route inventory and staged design

Design only. The retained 405-byte Marathi fixture still fails through the native
read-only route (329 ASCII bytes). No transport repair or restricted-token
prototype is claimed here. The source-matched file-default fix and the earlier
current-user console experiment retain their separate, narrow results.

## Route that must be repaired

The launcher's current configuration selects `windows.sandbox=unelevated`.
This is a configuration observation, not proof that every historical session
used it. `unified_exec/backends/legacy.rs::spawn_legacy_process` prepares the
existing restricted token and private desktop. Non-TTY execution calls
`process.rs::spawn_process_with_pipes` with Separate stderr and NoWindow console.
`create_process_as_user` whitelists stdio handles and assigns its job atomically
through STARTUPINFOEX. These restrictions must remain intact.

The elevated route uses `elevated/runner_client.rs`, launches the GUI-subsystem
`codex-command-runner`, authenticates its IPC, and `command_runner/win.rs` creates
the restricted child using the same pipe primitive with NoWindow. TTY uses ConPTY
and is a separate route. Changing only that runner would miss the currently
configured unelevated route. Other capture callers of the pipe primitive must be
enumerated before activation; do not assume a unified-exec-only patch fixes all
tool transports.

The current API's NoWindow flag does not supply an initialized UTF-8 console
inheritance contract. Parent/child console creation depends on subsystem and
creation flags. Microsoft documents [console creation](https://learn.microsoft.com/en-us/windows/console/creation-of-a-console),
[AllocConsole](https://learn.microsoft.com/en-us/windows/console/allocconsole),
and [CreateProcessAsUserW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw).
Those are API contracts, not evidence that this restricted private-desktop route
already works. The failed pre-resume AttachConsole ordering remains rejected.

## Selected implementation direction, pending mechanism qualification

Use a dedicated trusted console worker for non-TTY sandbox pipe launches, with an
owned hidden console initialized before creating the restricted target. Do not
change the main Codex process's console, detach the user's console, set a shared
console's code page, or perform an async per-command console mutation in-process.
Keep ConPTY and non-Windows execution unchanged.

The worker is a separately packaged companion, resolved by the existing trusted
bundle/registered-image mechanism, never PATH or a model-supplied executable.
There is no script execution mode in the worker's own unrestricted context.
Its only operation is launching the given target under the already prepared
restricted primary token, with the existing exact environment, cwd, private
desktop and pipe handles. It never constructs an unrestricted replacement token.

The parent creates/owns the outer kill-on-close job, desktop and all pipe ends.
The worker enters that job atomically at creation. Only explicit whitelisted
handles reach the worker: restricted token, bounded request/control channel,
target stdin/stdout/stderr and necessary job custody. The target's inherited
handle list contains stdio only; no token/control/parent IPC handles. Duplicate
only the minimal rights actually required, clear unwanted inheritance, and
retain explicit RAII cleanup on all early returns. Windows handle access and
cross-session limitations require a real restricted-token witness, not an
assumption based on ordinary subprocess inheritance.

The worker proves sole ownership of its console using GetConsoleProcessList and
its own PID before any code-page mutation. It sets and verifies input/output
CP65001 before spawning the target with console inheritance. Console setup or
ownership failure produces a bounded control error, never silent CP437 fallback.
No model command or CLM setter is responsible for the initialization.
How hidden creation/subsystem flags and private-desktop inheritance interact is
the next experimental gate; do not ship AllocConsole plus a visible-window hide
race, or claim CREATE_NO_WINDOW alone suffices.

Parent and worker exchange a versioned bounded startup protocol over inherited
control handles, not temporary files, command-line payloads or account secrets.
The request includes exact argv/environment/cwd, private desktop identity and
target handles. Validate counts/UTF-16 and frame lengths before allocation;
the concrete schema/byte caps and companion identity handshake require their
own source brief before implementation. Reject unknown versions/duplicate setup.
Never expose this control channel as a model tool or network listener.

The target's stdout/stderr flow directly through the existing separate pipes,
without CLIXML rewriting, lossy decoding, ASCII escape substitution or hidden
output suppression. Keep stdin half-close and pipe drainage semantics. Worker
diagnostics use the control channel, never contaminate target output. The worker
waits the target and propagates its real exit code; setup failure is a distinct
host error. Parent cancellation kills the owned job and awaits worker/target
termination plus drains, preserving live-child accounting and timeout behavior.
Normal target root exit (including nonzero exit) preserves the existing supported
descendant behavior: all applicable outer and inner jobs must explicitly complete
their preserve-descendants transition before final owning handles close. An armed
outer kill-on-close job must not undo an inner job's preservation. Qualify normal
zero/nonzero root exit with surviving descendants separately from cancellation,
timeout and worker/control failure. Live descendants still follow existing
accounting/drain rules; normal preservation cannot manufacture fully drained exit.
Worker crash/lost control/target spawn failure closes handles and kills only its
owned job; there is no fallback execution under a weaker profile.

## Reviewable implementation/qualification sequence

1. Freeze request/control bounds, exact handle rights/custody and launcher/bundle
   lookup changes after inventorying every pipe callsite. This document alone
   does not approve that unwritten wire protocol or companion integration.
2. Independently reviewed local mechanism helper: production restricted token
   and private desktop, atomic jobs, hidden owned console, unchanged stdio,
   exact Marathi bytes and intentional nonzero exit. Compare NoWindow control
   to inherited-console candidate; preserve every failure. Include denied writes
   outside the allowed fixture and CLM metadata, proving no permission weakening.
3. Inert companion/parent transport in separate modules, under 500 complex changed
   lines per coherent unit. No production activation until restricted witness,
   failure/cancel/large-output/stdin/descendant tests and packaging checks pass.
4. Activate both direct legacy capture and unified-exec pipe routes deliberately;
   elevated adaptation and managed-bundle identity remain separate reviewed gates.
   Never widen activation to ConPTY or non-Windows as an incidental refactor.
5. Source-pinned local native bundle, frozen hashes and actual unchanged-launcher
   Luna scratch journey. Read-only 405-byte output must match the original exact
   fixture; image `770.23`, JSON/tool Unicode, nonzero exit, restart and cancellation
   remain protected witnesses. Keep engine home/migration cohorts untouched.

Run local scoped Windows/Linux tests (platform absence explicit), fix/fmt, fresh
Droid source/merge review and unchanged sandbox policy. A current-user CLM probe
is not restricted-token/private-desktop acceptance. A helper pass is not a native
tool-route pass; native byte preservation is not translation quality, IndicTrans2,
CIDCO human adjudication, offline inference or customer delivery.
