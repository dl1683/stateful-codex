use super::ChildError;
use super::OwnedChild;
use pretty_assertions::assert_eq;
use std::future::Future;
use std::future::poll_fn;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;
use std::task::Poll;
use std::time::Duration;
use tokio::time::Instant;
use uuid::Uuid;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn spawn() -> (Self, Child) {
        let root = std::env::temp_dir().join(format!("stateful-child-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let fixture = Self { root };
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "bounded_child::tests::native_child_fixture",
                "--test-threads=1",
            ])
            .env("STATEFUL_CHILD_FIXTURE", &fixture.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let end = std::time::Instant::now() + Duration::from_secs(/*secs*/ 10);
        while !fixture.root.join("ready").exists() {
            if std::time::Instant::now() >= end {
                let _ = child.kill();
                let _ = native_reap(&mut child);
                panic!("child readiness timed out");
            }
            assert_eq!(child.try_wait().unwrap(), None);
            std::thread::sleep(Duration::from_millis(/*millis*/ 5));
        }
        (fixture, child)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Exact unique directory created by this fixture, never a variable tree.
        for name in ["ready", "release"] {
            let _ = std::fs::remove_file(self.root.join(name));
        }
        let _ = std::fs::remove_dir(&self.root);
    }
}

fn native_reap(child: &mut Child) -> ExitStatus {
    let end = std::time::Instant::now() + Duration::from_secs(/*secs*/ 10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(std::time::Instant::now() < end, "native reap timed out");
        std::thread::sleep(Duration::from_millis(/*millis*/ 5));
    }
}

#[test]
#[ignore = "subprocess fixture invoked explicitly by native ownership tests"]
fn native_child_fixture() {
    let root = PathBuf::from(std::env::var_os("STATEFUL_CHILD_FIXTURE").unwrap());
    std::fs::write(root.join("ready"), b"ready").unwrap();
    let end = std::time::Instant::now() + Duration::from_secs(/*secs*/ 30);
    while !root.join("release").exists() {
        assert!(std::time::Instant::now() < end, "fixture watchdog");
        std::thread::sleep(Duration::from_millis(/*millis*/ 5));
    }
}

#[tokio::test]
async fn already_exited_preserves_complete_status_without_termination() {
    let (fixture, mut child) = Fixture::spawn();
    std::fs::write(fixture.root.join("release"), b"release").unwrap();
    let status = native_reap(&mut child);
    assert!(status.success());
    let mut owner = OwnedChild::new(child);
    let request = owner.request_shutdown(Instant::now() + Duration::from_secs(/*secs*/ 5));
    let reaped = owner.reap().await;
    assert_eq!(
        (request, reaped, owner.reaped(), owner.attempted),
        (Ok(()), Ok(status), Some(status), false)
    );
}

#[tokio::test]
async fn live_child_is_terminated_once_and_observed_reaped() {
    let (_fixture, child) = Fixture::spawn();
    let mut owner = OwnedChild::new(child);
    let request = owner.request_shutdown(Instant::now() + Duration::from_secs(/*secs*/ 5));
    let retained_stdin = owner.child.stdin.is_some();
    let reaped = owner.reap().await;
    let actual = native_reap(&mut owner.child);
    assert_eq!(
        (
            request,
            reaped,
            owner.reaped(),
            owner.attempted,
            retained_stdin
        ),
        (Ok(()), Ok(actual), Some(actual), true, false)
    );
}

#[tokio::test]
async fn unpolled_reap_does_not_undo_synchronous_request() {
    let (_fixture, child) = Fixture::spawn();
    let mut owner = OwnedChild::new(child);
    let before = owner.reap().await;
    let request = owner.request_shutdown(Instant::now() + Duration::from_secs(/*secs*/ 5));
    drop(owner.reap());
    let retained = (owner.attempted, owner.child.stdin.is_none());
    let reaped = owner.reap().await;
    let actual = native_reap(&mut owner.child);
    assert_eq!(
        (before, request, retained, reaped),
        (
            Err(ChildError::NotStarted),
            Ok(()),
            (true, true),
            Ok(actual)
        )
    );
}

#[tokio::test]
async fn pending_reap_cancellation_preserves_owner_and_end() {
    let (_fixture, child) = Fixture::spawn();
    let mut owner = OwnedChild::new(child);
    let end = Instant::now() + Duration::from_secs(/*secs*/ 5);
    let request = owner.request_shutdown(end);
    tokio::time::pause();
    {
        let future = owner.reap();
        tokio::pin!(future);
        poll_fn(|cx| {
            assert!(future.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    let retained = (owner.end, owner.attempted);
    tokio::time::resume();
    let reaped = owner.reap().await;
    let actual = native_reap(&mut owner.child);
    assert_eq!(
        (request, retained, reaped),
        (Ok(()), (Some(end), true), Ok(actual))
    );
}

#[tokio::test]
async fn repeated_request_cannot_renew_exact_expiry() {
    let (_fixture, child) = Fixture::spawn();
    let mut owner = OwnedChild::new(child);
    tokio::time::pause();
    let end = Instant::now() + Duration::from_secs(/*secs*/ 5);
    let first = owner.request_shutdown(end);
    let repeated = owner.request_shutdown(end + Duration::from_secs(/*secs*/ 10));
    tokio::time::advance(Duration::from_secs(/*secs*/ 5)).await;
    let expired = owner.reap().await;
    let restarted = owner.request_shutdown(end + Duration::from_secs(/*secs*/ 20));
    let actual = native_reap(&mut owner.child);
    assert_eq!(
        (
            first,
            repeated,
            expired,
            restarted,
            owner.end,
            owner.child.try_wait().unwrap()
        ),
        (
            Ok(()),
            Ok(()),
            Err(ChildError::Deadline),
            Err(ChildError::Deadline),
            Some(end),
            Some(actual)
        )
    );
}

#[tokio::test]
async fn expired_first_request_retains_live_child_and_stdin() {
    let (fixture, child) = Fixture::spawn();
    let mut owner = OwnedChild::new(child);
    tokio::time::pause();
    let end = Instant::now();
    let first = owner.request_shutdown(end);
    let repeated = owner.request_shutdown(end + Duration::from_secs(/*secs*/ 5));
    let retained = (
        owner.child.stdin.is_some(),
        owner.attempted,
        owner.child.try_wait().unwrap(),
    );
    std::fs::write(fixture.root.join("release"), b"release").unwrap();
    let actual = native_reap(&mut owner.child);
    assert_eq!(
        (first, repeated, retained, owner.reaped()),
        (
            Err(ChildError::Deadline),
            Err(ChildError::Deadline),
            (true, false, None),
            None
        )
    );
    assert!(actual.success());
}

#[tokio::test]
async fn recorded_reaping_remains_a_fact_after_deadline_refusal() {
    let (_fixture, child) = Fixture::spawn();
    let mut owner = OwnedChild::new(child);
    let request = owner.request_shutdown(Instant::now() + Duration::from_secs(/*secs*/ 5));
    let reaped = owner.reap().await;
    let actual = native_reap(&mut owner.child);
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(/*secs*/ 5)).await;
    let expired = owner.reap().await;
    assert_eq!(
        (request, reaped, expired, owner.reaped()),
        (Ok(()), Ok(actual), Err(ChildError::Deadline), Some(actual))
    );
}
