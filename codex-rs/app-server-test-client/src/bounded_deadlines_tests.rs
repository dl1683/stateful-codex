use super::*;
use pretty_assertions::assert_eq;
use std::cell::Cell;
use std::future::pending;
use std::future::ready;
use std::rc::Rc;

fn initialized() -> JourneyDeadlines {
    let mut clock = JourneyDeadlines::new();
    clock.begin_phase(Phase::Initialization).unwrap();
    clock.finish(Scope::Phase).unwrap();
    clock
}

async fn pending_poll<F: Future>(mut future: std::pin::Pin<&mut F>) {
    poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn explicit_completion_and_invalid_transitions_never_replace_ends() {
    let mut clock = JourneyDeadlines::new();
    let invalid = Err(DeadlineError::InvalidState);
    assert_eq!(clock.begin_phase(Phase::Turn), invalid);
    clock.begin_phase(Phase::Initialization).unwrap();
    let end = clock.end(Scope::Phase).unwrap();
    assert_eq!(
        clock.wait(Scope::Phase, ready(Err::<(), _>("inner"))).await,
        Ok(Err("inner"))
    );
    assert_eq!(clock.begin_phase(Phase::Initialization), invalid);
    assert_eq!(clock.finish(Scope::Rpc), invalid);
    assert_eq!(clock.end(Scope::Phase), Ok(end));
    clock.finish(Scope::Phase).unwrap();
    assert_eq!(clock.begin_phase(Phase::Initialization), invalid);
    clock.begin_phase(Phase::Rpc).unwrap();
    assert_eq!(clock.begin_rpc(), invalid);
    clock.finish(Scope::Phase).unwrap();
}

#[tokio::test(start_paused = true)]
async fn all_phase_limits_bound_pending_work_and_latch_failures() {
    for (phase, seconds, error) in [
        (
            Phase::Initialization,
            60,
            DeadlineError::InitializationExpired,
        ),
        (Phase::Rpc, 30, DeadlineError::RpcExpired),
        (Phase::Turn, 180, DeadlineError::TurnExpired),
        (Phase::Compaction, 180, DeadlineError::CompactionExpired),
    ] {
        let mut clock = if phase == Phase::Initialization {
            JourneyDeadlines::new()
        } else {
            initialized()
        };
        clock.begin_phase(phase).unwrap();
        let mut wait = Box::pin(clock.wait(Scope::Phase, pending::<()>()));
        pending_poll(wait.as_mut()).await;
        tokio::time::advance(Duration::from_secs(seconds - 1)).await;
        pending_poll(wait.as_mut()).await;
        tokio::time::advance(Duration::from_secs(/*secs*/ 1)).await;
        assert_eq!(wait.await, Err(error));
    }
}

#[tokio::test(start_paused = true)]
async fn exact_expiry_rejects_ready_and_pending_repolls_before_inner_work() {
    for initial_poll in [false, true] {
        let mut clock = initialized();
        clock.begin_phase(Phase::Rpc).unwrap();
        let polls = Cell::<usize>::default();
        let inner = poll_fn(|_| {
            polls.set(polls.get() + 1);
            Poll::<()>::Pending
        });
        let mut wait = Box::pin(clock.wait(Scope::Phase, inner));
        if initial_poll {
            pending_poll(wait.as_mut()).await;
        }
        tokio::time::advance(Duration::from_secs(/*secs*/ 30)).await;
        assert_eq!(wait.as_mut().await, Err(DeadlineError::RpcExpired));
        assert_eq!(polls.get(), usize::from(initial_poll));
    }
    let mut clock = initialized();
    clock.begin_phase(Phase::Rpc).unwrap();
    tokio::time::advance(Duration::from_secs(/*secs*/ 30)).await;
    assert_eq!(
        clock.wait(Scope::Phase, ready("late")).await,
        Err(DeadlineError::RpcExpired)
    );
}

#[tokio::test(start_paused = true)]
async fn crossing_completing_poll_drops_output_before_retained_wrapper_returns() {
    struct Output(Rc<Cell<usize>>);
    impl Drop for Output {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let drops = Rc::new(Cell::default());
    let mut clock = initialized();
    clock.begin_phase(Phase::Rpc).unwrap();
    let mut advance = Box::pin(tokio::time::advance(Duration::from_secs(/*secs*/ 30)));
    let inner = poll_fn(|cx| {
        assert!(advance.as_mut().poll(cx).is_pending());
        Poll::Ready(Output(Rc::clone(&drops)))
    });
    let mut wait = Box::pin(clock.wait(Scope::Phase, inner));
    assert!(matches!(
        wait.as_mut().await,
        Err(DeadlineError::RpcExpired)
    ));
    assert_eq!(drops.get(), 1);
    drop(wait);
    assert_eq!(drops.get(), 1);
}

#[tokio::test(start_paused = true)]
async fn crossing_pending_poll_is_rejected_without_another_inner_poll() {
    let mut clock = initialized();
    clock.begin_phase(Phase::Rpc).unwrap();
    let polls = Cell::<usize>::default();
    let mut advance = Box::pin(tokio::time::advance(Duration::from_secs(/*secs*/ 30)));
    let inner = poll_fn(|cx| {
        polls.set(polls.get() + 1);
        assert!(advance.as_mut().poll(cx).is_pending());
        Poll::<()>::Pending
    });
    assert_eq!(
        clock.wait(Scope::Phase, inner).await,
        Err(DeadlineError::RpcExpired)
    );
    assert_eq!(polls.get(), 1);
}

#[tokio::test(start_paused = true)]
async fn notifications_and_dropped_waits_preserve_original_phase_slots() {
    for phase in [Phase::Initialization, Phase::Turn, Phase::Compaction] {
        let mut clock = if phase == Phase::Initialization {
            JourneyDeadlines::new()
        } else {
            initialized()
        };
        clock.begin_phase(phase).unwrap();
        let end = clock.end(Scope::Phase).unwrap();
        drop(Box::pin(clock.wait(Scope::Phase, pending::<()>())));
        {
            let mut wait = Box::pin(clock.wait(Scope::Phase, pending::<()>()));
            pending_poll(wait.as_mut()).await;
        }
        for notification in 0..3 {
            tokio::time::advance(Duration::from_secs(/*secs*/ 10)).await;
            assert_eq!(
                clock.wait(Scope::Phase, ready(notification)).await,
                Ok(notification)
            );
            assert_eq!(clock.end(Scope::Phase), Ok(end));
        }
        assert_eq!(
            clock.begin_phase(Phase::Rpc),
            Err(DeadlineError::InvalidState)
        );
        tokio::time::advance(end - Instant::now()).await;
        assert!(clock.finish(Scope::Phase).is_err());
    }
}

#[tokio::test(start_paused = true)]
async fn nested_rpc_bounds_parent_waits_and_cannot_renew_parent() {
    let mut clock = initialized();
    clock.begin_phase(Phase::Turn).unwrap();
    clock.begin_rpc().unwrap();
    let child = clock.end(Scope::Rpc).unwrap();
    assert_eq!(clock.begin_rpc(), Err(DeadlineError::InvalidState));
    assert_eq!(clock.finish(Scope::Phase), Err(DeadlineError::InvalidState));
    assert_eq!(clock.end(Scope::Phase), Ok(child));
    let mut wait = Box::pin(clock.wait(Scope::Phase, pending::<()>()));
    pending_poll(wait.as_mut()).await;
    tokio::time::advance(Duration::from_secs(/*secs*/ 30)).await;
    assert_eq!(wait.await, Err(DeadlineError::RpcExpired));
    let mut clock = initialized();
    clock.begin_phase(Phase::Compaction).unwrap();
    let parent = clock.end(Scope::Phase).unwrap();
    clock.begin_rpc().unwrap();
    assert_eq!(clock.wait(Scope::Rpc, ready("reply")).await, Ok("reply"));
    clock.finish(Scope::Rpc).unwrap();
    assert_eq!(clock.end(Scope::Phase), Ok(parent));
    tokio::time::advance(Duration::from_secs(/*secs*/ 179)).await;
    clock.begin_rpc().unwrap();
    assert_eq!(clock.end(Scope::Rpc), Ok(parent));
    tokio::time::advance(Duration::from_secs(/*secs*/ 1)).await;
    assert_eq!(
        clock.finish(Scope::Rpc),
        Err(DeadlineError::CompactionExpired)
    );
}

#[tokio::test(start_paused = true)]
async fn sequential_phases_and_idle_time_leave_only_remaining_total() {
    for pending_work in [false, true] {
        let mut clock = initialized();
        for _ in 0..4 {
            clock.begin_phase(Phase::Turn).unwrap();
            tokio::time::advance(Duration::from_secs(/*secs*/ 179)).await;
            clock.finish(Scope::Phase).unwrap();
        }
        tokio::time::advance(Duration::from_secs(/*secs*/ 170)).await;
        clock.begin_phase(Phase::Compaction).unwrap();
        let inner = poll_fn(|_| {
            if pending_work {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        });
        let mut wait = Box::pin(clock.wait(Scope::Phase, inner));
        if pending_work {
            pending_poll(wait.as_mut()).await;
        }
        tokio::time::advance(Duration::from_secs(/*secs*/ 13)).await;
        if pending_work {
            pending_poll(wait.as_mut()).await;
        }
        tokio::time::advance(Duration::from_secs(/*secs*/ 1)).await;
        assert_eq!(wait.await, Err(DeadlineError::TotalExpired));
        assert_eq!(
            clock.wait(Scope::Phase, ready("late")).await,
            Err(DeadlineError::TotalExpired)
        );
    }
}

#[tokio::test(start_paused = true)]
async fn total_then_parent_then_child_precedence_is_sticky() {
    for seconds in [30, 870] {
        let mut clock = if seconds == 30 {
            JourneyDeadlines::new()
        } else {
            initialized()
        };
        clock
            .begin_phase(if seconds == 30 {
                Phase::Initialization
            } else {
                Phase::Turn
            })
            .unwrap();
        if seconds == 870 {
            clock.finish(Scope::Phase).unwrap();
            tokio::time::advance(Duration::from_secs(/*secs*/ 870)).await;
            clock.begin_phase(Phase::Turn).unwrap();
        } else {
            tokio::time::advance(Duration::from_secs(/*secs*/ 30)).await;
        }
        clock.begin_rpc().unwrap();
        tokio::time::advance(Duration::from_secs(/*secs*/ 30)).await;
        let error = if seconds == 30 {
            DeadlineError::InitializationExpired
        } else {
            DeadlineError::TotalExpired
        };
        assert_eq!(clock.begin_rpc(), Err(error));
        tokio::time::advance(Duration::from_secs(/*secs*/ 900)).await;
        assert_eq!(clock.finish(Scope::Phase), Err(error));
    }
    for (seconds, error) in [
        (60, DeadlineError::InitializationExpired),
        (900, DeadlineError::TotalExpired),
    ] {
        let mut clock = JourneyDeadlines::new();
        clock.begin_phase(Phase::Initialization).unwrap();
        clock.begin_rpc().unwrap();
        tokio::time::advance(Duration::from_secs(seconds)).await;
        assert_eq!(clock.begin_rpc(), Err(error));
        tokio::time::advance(Duration::from_secs(/*secs*/ 900)).await;
        assert_eq!(clock.finish(Scope::Phase), Err(error));
    }
}

#[tokio::test(start_paused = true)]
async fn shutdown_after_total_exhaustion_has_unclipped_success_and_finality() {
    let mut clock = JourneyDeadlines::new();
    tokio::time::advance(Duration::from_secs(/*secs*/ 900)).await;
    clock.begin_shutdown().unwrap();
    let end = clock.end(Scope::Shutdown).unwrap();
    assert_eq!(
        clock.begin_phase(Phase::Initialization),
        Err(DeadlineError::TotalExpired)
    );
    tokio::time::advance(Duration::from_secs(/*secs*/ 4)).await;
    assert_eq!(
        clock
            .wait(Scope::Shutdown, ready(Ok::<_, ()>("cleanup")))
            .await,
        Ok(Ok("cleanup"))
    );
    clock.begin_shutdown().unwrap();
    assert_eq!(clock.end(Scope::Shutdown), Ok(end));
    clock.finish(Scope::Shutdown).unwrap();
    tokio::time::advance(Duration::from_secs(/*secs*/ 900)).await;
    assert_eq!(clock.begin_shutdown(), Err(DeadlineError::Closed));
    assert_eq!(clock.finish(Scope::Shutdown), Err(DeadlineError::Closed));
    assert_eq!(
        clock.begin_phase(Phase::Turn),
        Err(DeadlineError::TotalExpired)
    );
}

#[tokio::test(start_paused = true)]
async fn shutdown_permanently_closes_live_slots_and_preserves_failure() {
    for initialize in [false, true] {
        let mut clock = if initialize {
            initialized()
        } else {
            JourneyDeadlines::new()
        };
        assert_eq!(clock.end(Scope::Shutdown), Err(DeadlineError::InvalidState));
        assert_eq!(
            clock.finish(Scope::Shutdown),
            Err(DeadlineError::InvalidState)
        );
        clock.begin_shutdown().unwrap();
        assert_eq!(
            clock.begin_phase(Phase::Initialization),
            Err(DeadlineError::Closed)
        );
        assert_eq!(clock.begin_rpc(), Err(DeadlineError::Closed));
        tokio::time::advance(Duration::from_secs(/*secs*/ 900)).await;
        assert_eq!(clock.finish(Scope::Phase), Err(DeadlineError::Closed));
    }
    for phase in [
        Phase::Initialization,
        Phase::Rpc,
        Phase::Turn,
        Phase::Compaction,
    ] {
        let mut clock = if phase == Phase::Initialization {
            JourneyDeadlines::new()
        } else {
            initialized()
        };
        clock.begin_phase(phase).unwrap();
        if phase != Phase::Rpc {
            clock.begin_rpc().unwrap();
        }
        clock.begin_shutdown().unwrap();
        assert_eq!(clock.end(Scope::Phase), Err(DeadlineError::Closed));
        assert_eq!(clock.finish(Scope::Rpc), Err(DeadlineError::Closed));
        assert_eq!(
            clock.wait(Scope::Shutdown, ready("cleanup")).await,
            Ok("cleanup")
        );
    }
    let mut clock = initialized();
    clock.begin_phase(Phase::Turn).unwrap();
    tokio::time::advance(Duration::from_secs(/*secs*/ 180)).await;
    assert_eq!(clock.finish(Scope::Phase), Err(DeadlineError::TurnExpired));
    clock.begin_shutdown().unwrap();
    tokio::time::advance(Duration::from_secs(/*secs*/ 900)).await;
    assert_eq!(
        clock.end(Scope::Shutdown),
        Err(DeadlineError::ShutdownExpired)
    );
    assert_eq!(clock.begin_rpc(), Err(DeadlineError::TurnExpired));
}

#[tokio::test(start_paused = true)]
async fn shutdown_pending_drop_and_repeat_cannot_renew_original_end() {
    let mut clock = JourneyDeadlines::new();
    clock.begin_shutdown().unwrap();
    let end = clock.end(Scope::Shutdown).unwrap();
    drop(Box::pin(clock.wait(Scope::Shutdown, pending::<()>())));
    {
        let mut wait = Box::pin(clock.wait(Scope::Shutdown, pending::<()>()));
        pending_poll(wait.as_mut()).await;
        tokio::time::advance(Duration::from_secs(/*secs*/ 2)).await;
        pending_poll(wait.as_mut()).await;
    }
    clock.begin_shutdown().unwrap();
    assert_eq!(clock.end(Scope::Shutdown), Ok(end));
    assert_eq!(
        clock.wait(Scope::Shutdown, ready("notification")).await,
        Ok("notification")
    );
    tokio::time::advance(Duration::from_secs(/*secs*/ 2)).await;
    let mut wait = Box::pin(clock.wait(Scope::Shutdown, pending::<()>()));
    pending_poll(wait.as_mut()).await;
    tokio::time::advance(Duration::from_secs(/*secs*/ 1)).await;
    assert_eq!(wait.await, Err(DeadlineError::ShutdownExpired));
    assert_eq!(clock.begin_shutdown(), Err(DeadlineError::ShutdownExpired));
    assert_eq!(
        clock.finish(Scope::Shutdown),
        Err(DeadlineError::ShutdownExpired)
    );
    assert_eq!(
        clock.begin_phase(Phase::Initialization),
        Err(DeadlineError::Closed)
    );
}

#[tokio::test(start_paused = true)]
async fn shutdown_exact_expiry_rejects_ready_before_inner_poll() {
    for initial_pending in [false, true] {
        let mut clock = JourneyDeadlines::new();
        clock.begin_shutdown().unwrap();
        let polls = Cell::<usize>::default();
        let ready = Cell::<bool>::default();
        let inner = poll_fn(|_| {
            polls.set(polls.get() + 1);
            if ready.get() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        });
        let mut wait = Box::pin(clock.wait(Scope::Shutdown, inner));
        if initial_pending {
            pending_poll(wait.as_mut()).await;
        }
        tokio::time::advance(Duration::from_secs(/*secs*/ 5)).await;
        ready.set(/*val*/ true);
        assert_eq!(wait.await, Err(DeadlineError::ShutdownExpired));
        assert_eq!(polls.get(), usize::from(initial_pending));
        assert_eq!(clock.begin_shutdown(), Err(DeadlineError::ShutdownExpired));
    }
}

#[tokio::test(start_paused = true)]
async fn shutdown_crossing_poll_discards_output_and_latches_only_shutdown() {
    struct Output(Rc<Cell<usize>>);
    impl Drop for Output {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let drops = Rc::new(Cell::default());
    let mut clock = JourneyDeadlines::new();
    clock.begin_shutdown().unwrap();
    let mut advance = Box::pin(tokio::time::advance(Duration::from_secs(/*secs*/ 5)));
    let inner = poll_fn(|cx| {
        assert!(advance.as_mut().poll(cx).is_pending());
        Poll::Ready(Output(Rc::clone(&drops)))
    });
    let mut wait = Box::pin(clock.wait(Scope::Shutdown, inner));
    assert!(matches!(
        wait.as_mut().await,
        Err(DeadlineError::ShutdownExpired)
    ));
    assert_eq!(drops.get(), 1);
    drop(wait);
    assert_eq!(drops.get(), 1);
    assert_eq!(clock.begin_shutdown(), Err(DeadlineError::ShutdownExpired));
    tokio::time::advance(Duration::from_secs(/*secs*/ 900)).await;
    assert_eq!(
        clock.begin_phase(Phase::Initialization),
        Err(DeadlineError::Closed)
    );
}
