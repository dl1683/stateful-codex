use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use pretty_assertions::assert_eq;
use tokio::sync::oneshot;

use super::IndexGates;
use super::IndexOperation;

const SHORT: Duration = Duration::from_millis(50);
const LONG: Duration = Duration::from_secs(10);

#[tokio::test]
async fn an_operation_within_its_deadline_returns_its_output() {
    let gates = IndexGates::default();
    let outcome = gates.run("project-1", LONG, |_| async { 7 }).await;
    assert_eq!(outcome, IndexOperation::Finished(7));
}

/// A slow scan past the deadline is reported pending, keeps the permit until it actually
/// exits, and no second operation starts meanwhile; after it exits a retry runs.
#[tokio::test]
async fn timed_out_work_holds_the_permit_until_it_exits_and_then_allows_a_retry() {
    let gates = IndexGates::default();
    let starts = Arc::new(AtomicUsize::new(0));
    let (release, released) = oneshot::channel::<()>();
    let (exited, exit) = oneshot::channel::<()>();
    let first_starts = Arc::clone(&starts);
    let first = gates
        .run("project-1", SHORT, move |_| async move {
            first_starts.fetch_add(1, Ordering::SeqCst);
            let _ = released.await;
            let _ = exited.send(());
        })
        .await;
    assert_eq!(first, IndexOperation::Pending);

    let second_starts = Arc::clone(&starts);
    let second = gates
        .run("project-1", SHORT, move |_| async move {
            second_starts.fetch_add(1, Ordering::SeqCst);
        })
        .await;
    assert_eq!(
        (second, starts.load(Ordering::SeqCst)),
        (IndexOperation::Pending, 1)
    );

    release.send(()).expect("slow operation is still waiting");
    exit.await.expect("slow operation exits");
    let retry_starts = Arc::clone(&starts);
    let retry = gates
        .run("project-1", LONG, move |_| async move {
            retry_starts.fetch_add(1, Ordering::SeqCst)
        })
        .await;
    assert_eq!(retry, IndexOperation::Finished(1));
}

/// Abandoned work is asked to stop at the background ceiling instead of running forever.
#[tokio::test]
async fn the_background_ceiling_cancels_abandoned_work_cooperatively() {
    let gates = IndexGates::default();
    let (cancelled, observed) = oneshot::channel::<bool>();
    let outcome = gates
        .run_with_ceiling("project-1", SHORT, SHORT, move |cancellation| async move {
            while !cancellation.is_cancelled() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            let _ = cancelled.send(true);
        })
        .await;
    assert_eq!(outcome, IndexOperation::Pending);
    assert_eq!(observed.await, Ok(true));
}

#[tokio::test]
async fn projects_do_not_share_a_permit() {
    let gates = IndexGates::default();
    let (_hold, held) = oneshot::channel::<()>();
    let busy = gates
        .run("project-1", SHORT, move |_| async move {
            let _ = held.await;
        })
        .await;
    let other = gates.run("project-2", LONG, |_| async { "indexed" }).await;
    assert_eq!(
        (busy, other),
        (IndexOperation::Pending, IndexOperation::Finished("indexed"))
    );
}
