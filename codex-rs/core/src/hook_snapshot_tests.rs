use std::collections::HashMap;
use std::sync::Arc;

use codex_exec_server::ExecutorCapabilityDiscoverySnapshot;

use super::*;
use crate::session::tests::make_session_and_context;

fn discovery() -> Arc<ExecutorCapabilityDiscoverySnapshot> {
    Arc::new(ExecutorCapabilityDiscoverySnapshot::new(
        &[],
        Vec::new(),
        HashMap::new(),
    ))
}

/// A step of `turn` whose own capability-root discovery is `discovery`.
fn step(
    turn: &Arc<TurnContext>,
    discovery: Option<Arc<ExecutorCapabilityDiscoverySnapshot>>,
) -> StepContext {
    let Ok(mut step) = Arc::try_unwrap(StepContext::for_test(Arc::clone(turn))) else {
        panic!("the test owns the only step reference");
    };
    step.executor_capability_discovery = discovery;
    step
}

/// A capability-root selection that changes mid-turn applies from the next turn: the turn's
/// executor-plugin hooks come from its first step's discovery.
#[tokio::test]
async fn a_turn_runs_the_executor_hooks_its_first_step_discovered() {
    // The first step selected no capability roots; a later step of the same turn selects one.
    let (session, turn) = make_session_and_context().await;
    let turn = Arc::new(turn);
    pin_turn_hooks(&session, &turn);
    pin_turn_executor_discovery(&turn, /*discovery*/ None);
    let later = discovery();
    pin_turn_executor_discovery(&turn, Some(&later));
    assert!(turn_executor_discovery(&step(&turn, Some(later))).is_none());

    // Otherwise the turn keeps its first step's discovery.
    let (session, turn) = make_session_and_context().await;
    let turn = Arc::new(turn);
    pin_turn_hooks(&session, &turn);
    let first = discovery();
    pin_turn_executor_discovery(&turn, Some(&first));
    pin_turn_executor_discovery(&turn, Some(&discovery()));
    let used = turn_executor_discovery(&step(&turn, Some(discovery()))).expect("pinned");
    assert!(Arc::ptr_eq(&used, &first));

    // A context no task started uses each step's own discovery.
    let (_, turn) = make_session_and_context().await;
    let turn = Arc::new(turn);
    let own = discovery();
    let used = turn_executor_discovery(&step(&turn, Some(Arc::clone(&own)))).expect("own");
    assert!(Arc::ptr_eq(&used, &own));
}
