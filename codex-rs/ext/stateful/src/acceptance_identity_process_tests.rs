use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use pretty_assertions::assert_eq;

use super::records;
use crate::acceptance_identity_reason::IdentityGap;
use crate::acceptance_identity_worker::read_identity;
use crate::acceptance_observation::OUTSTANDING_READERS;

async fn fixture(action: &str) -> (Result<i32, IdentityGap>, Duration) {
    let baseline = OUTSTANDING_READERS.load(Ordering::SeqCst);
    let ready = Arc::new(AtomicBool::new(false));
    let received = Arc::clone(&ready);
    let mut command = Command::new(if cfg!(windows) { "python" } else { "python3" });
    command.args(["-c", &format!("import os,time; print('ready',os.getpid(),flush=True); {action}")]);
    let started = Instant::now();
    let result = read_identity(Duration::from_secs(10), move |_| {
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut pid = None;
        let result = records(command, b'\n', 4096, deadline, |record| {
            let text = std::str::from_utf8(record).unwrap().trim_end();
            pid = Some(text.strip_prefix("ready ").expect("ready").parse::<u32>().unwrap());
            received.store(true, Ordering::SeqCst);
            Ok(())
        });
        let pid = pid.expect("fixture started before the deadline");
        #[cfg(windows)]
        let alive = Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"]).output().unwrap().stdout.windows(pid.to_string().len()).any(|window| window == pid.to_string().as_bytes());
        #[cfg(unix)]
        let alive = Command::new("kill").args(["-0", &pid.to_string()]).status().unwrap().success();
        assert!(!alive, "child has exited");
        result
    }).await;
    let elapsed = started.elapsed();
    assert!(ready.load(Ordering::SeqCst), "reader received ready");
    assert_eq!(OUTSTANDING_READERS.load(Ordering::SeqCst), baseline);
    (result, elapsed)
}

#[tokio::test]
async fn w13_blocked_stdout_is_killed_at_deadline() {
    let (result, elapsed) = fixture("time.sleep(2)").await;
    assert_eq!(result, Err(IdentityGap::Time));
    assert!(elapsed < Duration::from_millis(1500), "{elapsed:?}");
}

#[tokio::test]
async fn w13_eof_before_exit_is_not_success() {
    let (result, elapsed) = fixture("os.close(1); time.sleep(2)").await;
    assert_eq!(result, Err(IdentityGap::Time));
    assert!(elapsed < Duration::from_millis(1500), "{elapsed:?}");
}

#[tokio::test]
async fn w13_ready_child_exits_successfully() {
    assert_eq!(fixture("pass").await.0, Ok(0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn w13_retained_workers_exhaust_slots_then_recover() {
    let baseline = OUTSTANDING_READERS.load(Ordering::SeqCst);
    let mut releases = Vec::new();
    for _ in 0..4 {
        let (release, blocked) = std::sync::mpsc::channel();
        let (entered, ready) = std::sync::mpsc::channel();
        releases.push(release);
        let worker = tokio::spawn(read_identity(Duration::from_millis(100), move |_| {
            entered.send(()).unwrap();
            blocked.recv().unwrap();
            Ok(())
        }));
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(worker.await.unwrap(), Err(IdentityGap::Time));
    }
    assert_eq!(read_identity(Duration::from_secs(1), |_| Ok(())).await, Err(IdentityGap::Slots));
    for release in releases { release.send(()).unwrap(); }
    for _ in 0..100 {
        if OUTSTANDING_READERS.load(Ordering::SeqCst) == baseline { break; }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(OUTSTANDING_READERS.load(Ordering::SeqCst), baseline);
    assert_eq!(read_identity(Duration::from_secs(1), |_| Ok(())).await, Ok(()));
}

#[test]
fn start_failure_is_git_not_a_vacuous_pass() {
    let result = records(Command::new("missing-identity-fixture-executable"), b'\n', 4096, Instant::now() + Duration::from_secs(1), |_| Ok(()));
    assert_eq!(result, Err(IdentityGap::Git));
}
