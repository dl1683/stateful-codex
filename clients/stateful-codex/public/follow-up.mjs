// A follow-up to a closed outcome starts a new run on the same thread, exactly as setup's
// Continue does. It is recorded in session storage the same way, so a reload finds the new run
// and retries its first turn if that turn was never sent.
export async function startFollowUp({ state, rpc, storage, goal, mode, sendTurn }) {
  const idempotencyKey = crypto.randomUUID();
  storage.setItem("stateful-run-key", idempotencyKey);
  const started = await rpc("statefulRun/start", {
    projectId: state.projectId,
    threadId: state.threadId,
    goal,
    mode,
    budget: state.run.budget,
    idempotencyKey,
  });
  state.run = started.run;
  // Obligations and steering belong to the closed run.
  state.obligations = [];
  state.steering = [];
  state.confirmedSteering = [];
  state.steeringError = null;
  storage.setItem("stateful-mode", state.run.mode);
  storage.setItem("stateful-goal", goal);
  storage.setItem("stateful-created-run-id", state.run.id);
  storage.removeItem("stateful-initial-turn-sent");
  await sendTurn(goal);
  storage.setItem("stateful-initial-turn-sent", state.run.id);
}
