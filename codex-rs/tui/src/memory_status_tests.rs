use pretty_assertions::assert_eq;

use super::State;
use super::ThreadMemory;
use codex_protocol::ThreadId;

/// Journal sequences are per project: each thread is counted against its own project's
/// watermark, with the session's other threads of that project.
#[test]
fn each_project_keeps_its_own_watermark() {
    let a = ThreadId::new();
    let a2 = ThreadId::new();
    let b = ThreadId::new();
    let plain = ThreadId::new();
    let unread = ThreadId::new();
    let mut state = State::default();
    state.watermarks.insert("p1".to_string(), 100);
    state.watermarks.insert("p2".to_string(), 5);
    state
        .threads
        .insert(a, ThreadMemory::Project("p1".to_string()));
    state
        .threads
        .insert(a2, ThreadMemory::Project("p1".to_string()));
    state
        .threads
        .insert(b, ThreadMemory::Project("p2".to_string()));
    state.threads.insert(plain, ThreadMemory::NotStateful);
    state.threads.insert(unread, ThreadMemory::Unavailable);
    let mut p1_threads = vec![a.to_string(), a2.to_string()];
    p1_threads.sort();
    let scope = |thread| {
        state.scope_of(thread).map(|(since, mut threads)| {
            threads.sort();
            (since, threads)
        })
    };
    assert_eq!(
        (scope(a), scope(b), scope(plain), scope(unread)),
        (
            Some((100, p1_threads)),
            Some((5, vec![b.to_string()])),
            None,
            None
        )
    );
}
