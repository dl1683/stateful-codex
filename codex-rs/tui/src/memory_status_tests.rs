use pretty_assertions::assert_eq;

use super::AttachOutcome;
use super::AttachResult;
use super::MemoryStatus;
use super::State;
use codex_protocol::ThreadId;

fn project(project_id: &str, head: u64) -> AttachResult {
    AttachResult::Project {
        project_id: project_id.to_string(),
        head,
    }
}

/// A result read before a project change is rejected when it is applied, success or error;
/// the change's own reading (no project) clears the view.
#[test]
fn results_queued_behind_a_project_change_are_dropped() {
    let status = MemoryStatus::default();
    let thread = ThreadId::new();
    let old = status.lock().begin_attach(thread);
    assert_eq!(
        status.lock().finish_attach(thread, old, project("a", 10)),
        AttachOutcome::Refresh
    );
    // Known and Unavailable events of `old` are queued; then the project is unlinked.
    status.project_changed(thread);
    let new = status.lock().begin_attach(thread);
    let old_accepted = status.accepts(thread, old);
    let cleared = status
        .lock()
        .finish_attach(thread, new, AttachResult::NotStateful);
    let new_accepted = status.accepts(thread, new);
    let scope = status.lock().scope_of(thread);
    assert_eq!(
        (old_accepted, cleared, new_accepted, scope),
        (false, AttachOutcome::Clear, true, None)
    );
}

/// Reconnecting makes every earlier attachment obsolete; a thread unlinked while this client
/// was disconnected is read again and cleared, and its earlier project stays only as history.
#[test]
fn reconnect_rereads_bindings_and_drops_delayed_results() {
    let status = MemoryStatus::default();
    let linked = ThreadId::new();
    let plain = ThreadId::new();
    let before = status.lock().begin_attach(linked);
    status
        .lock()
        .finish_attach(linked, before, project("a", 10));
    let plain_before = status.lock().begin_attach(plain);
    status
        .lock()
        .finish_attach(plain, plain_before, AttachResult::NotStateful);

    status.connection_changed();
    let after = status.lock().begin_attach(linked);
    let unlinked = status
        .lock()
        .finish_attach(linked, after, AttachResult::NotStateful);
    // The missed assignment of the plain thread is found the same way.
    let plain_after = status.lock().begin_attach(plain);
    let assigned = status
        .lock()
        .finish_attach(plain, plain_after, project("b", 3));
    let state = status.lock();
    assert_eq!(
        (
            status_accepts(&state, linked, before),
            unlinked,
            state.scope_of(linked),
            state.projects["a"].threads.contains(&linked.to_string()),
            assigned,
            state
                .scope_of(plain)
                .map(|(_, project, since, _)| (project, since)),
        ),
        (
            false,
            AttachOutcome::Clear,
            None,
            true,
            AttachOutcome::Refresh,
            Some(("b".to_string(), 3)),
        )
    );
}

/// Project changes A to B to C whose readings complete out of order: only C's reading binds
/// the thread, and only results requested under C's attachment may be applied.
#[test]
fn reordered_project_changes_only_apply_the_newest() {
    let status = MemoryStatus::default();
    let thread = ThreadId::new();
    let a = status.lock().begin_attach(thread);
    status.lock().finish_attach(thread, a, project("a", 1));
    let b = status.lock().begin_attach(thread);
    let c = status.lock().begin_attach(thread);
    let c_outcome = status.lock().finish_attach(thread, c, project("c", 30));
    let b_outcome = status.lock().finish_attach(thread, b, project("b", 20));
    let state = status.lock();
    assert_eq!(
        (
            c_outcome,
            b_outcome,
            [a, b, c].map(|generation| status_accepts(&state, thread, generation)),
            state
                .scope_of(thread)
                .map(|(_, project, since, _)| (project, since)),
            state.projects.contains_key("b"),
        ),
        (
            AttachOutcome::Refresh,
            AttachOutcome::Obsolete,
            [false, false, true],
            Some(("c".to_string(), 30)),
            false,
        )
    );
}

/// Journal sequences are per project: each thread is counted against its own project's
/// watermark, with the session's threads of that project.
#[test]
fn each_project_keeps_its_own_watermark_and_threads() {
    let mut state = State::default();
    let a = ThreadId::new();
    let a2 = ThreadId::new();
    let b = ThreadId::new();
    for (thread, project_id, head) in [(a, "p1", 100), (a2, "p1", 120), (b, "p2", 5)] {
        let generation = state.begin_attach(thread);
        state.finish_attach(thread, generation, project(project_id, head));
    }
    let scope = |thread| {
        state
            .scope_of(thread)
            .map(|(_, project, since, mut threads)| {
                threads.sort();
                (project, since, threads)
            })
    };
    let mut p1 = vec![a.to_string(), a2.to_string()];
    p1.sort();
    assert_eq!(
        (scope(a), scope(b)),
        (
            Some(("p1".to_string(), 100, p1)),
            Some(("p2".to_string(), 5, vec![b.to_string()]))
        )
    );
}

fn status_accepts(state: &State, thread: ThreadId, generation: u64) -> bool {
    state.is_current(thread, generation)
}
