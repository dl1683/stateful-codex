use pretty_assertions::assert_eq;

use super::State;
use super::ThreadMemory;
use codex_protocol::ThreadId;

/// Journal sequences are per project: each thread is counted against its own project's
/// watermark, with the session's threads of that project. A thread moved to another project
/// is counted there from then on, and stays among its earlier project's threads.
#[test]
fn each_project_keeps_its_own_watermark_and_threads() {
    let a = ThreadId::new();
    let a2 = ThreadId::new();
    let b = ThreadId::new();
    let plain = ThreadId::new();
    let unread = ThreadId::new();
    let mut state = State::default();
    state.bind(a, "p1".to_string(), /*head*/ 100);
    state.bind(a2, "p1".to_string(), /*head*/ 120);
    state.bind(b, "p2".to_string(), /*head*/ 5);
    state.threads.insert(plain, ThreadMemory::NotStateful);
    state.threads.insert(unread, ThreadMemory::Unavailable);
    let sorted = |mut threads: Vec<String>| {
        threads.sort();
        threads
    };
    let scope = |state: &State, thread| {
        state
            .scope_of(thread)
            .map(|(project, since, threads)| (project, since, sorted(threads)))
    };
    assert_eq!(
        (
            scope(&state, a),
            scope(&state, b),
            scope(&state, plain),
            scope(&state, unread)
        ),
        (
            Some((
                "p1".to_string(),
                100,
                sorted(vec![a.to_string(), a2.to_string()])
            )),
            Some(("p2".to_string(), 5, vec![b.to_string()])),
            None,
            None
        )
    );

    // Moving `a` to p2 counts it with p2 from p2's watermark; p1 still lists it.
    state.bind(a, "p2".to_string(), /*head*/ 9);
    assert_eq!(
        (
            scope(&state, a),
            sorted(state.projects["p1"].threads.iter().cloned().collect())
        ),
        (
            Some((
                "p2".to_string(),
                5,
                sorted(vec![a.to_string(), b.to_string()])
            )),
            sorted(vec![a.to_string(), a2.to_string()])
        )
    );
}
