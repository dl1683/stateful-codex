// Starting a run's first turn safely, and following up a closed outcome on the same thread.
//
// A follow-up starts a new run on the same thread, exactly as setup's Continue does. Every step
// can lose its response (the request may or may not have taken effect), so each is recorded
// before it is sent and reconciled before it is retried:
// - the follow-up's start request (goal, mode, budget, idempotency key) is saved first, and
//   replaying it with the same key returns the run it created, if any;
// - before a first turn is (re)sent, the thread is checked for that message having arrived.

const PENDING_FOLLOW_UP = "stateful-follow-up";

export function readPendingFollowUp(storage) {
  try {
    const pending = JSON.parse(storage.getItem(PENDING_FOLLOW_UP) ?? "null");
    return pending?.key && pending.goal ? pending : null;
  } catch {
    return null;
  }
}

// Starts a follow-up, or finishes an earlier one whose outcome was never confirmed. An earlier
// unconfirmed follow-up with different text is finished first and this text is not sent: the
// error says so, and the caller keeps the draft.
export async function startFollowUp({ state, rpc, storage, goal, mode, onRunChanged }) {
  const pending = readPendingFollowUp(storage);
  if (pending && (pending.goal !== goal || pending.mode !== mode)) {
    await resumePendingFollowUp({ state, rpc, storage, onRunChanged });
    throw new Error(
      `An earlier follow-up ("${pending.goal}") had not finished starting; it has now been started. Your new text was not sent and is kept; send it again once that work is done.`,
    );
  }
  if (!pending) {
    storage.setItem(
      PENDING_FOLLOW_UP,
      JSON.stringify({ goal, mode, budget: state.run.budget, key: crypto.randomUUID() }),
    );
  }
  await resumePendingFollowUp({ state, rpc, storage, onRunChanged });
}

// Completes the saved follow-up, if any: (re)issues its idempotent start, then its first turn.
// Returns whether there was one.
export async function resumePendingFollowUp({ state, rpc, storage, onRunChanged }) {
  const pending = readPendingFollowUp(storage);
  if (!pending) return false;
  await ensureThreadLoaded(rpc, state.threadId);
  const { run } = await rpc("statefulRun/start", {
    projectId: state.projectId,
    threadId: state.threadId,
    goal: pending.goal,
    mode: pending.mode,
    budget: pending.budget,
    idempotencyKey: pending.key,
  });
  // Obligations and steering belong to the closed run.
  Object.assign(state, {
    run,
    obligations: [],
    steering: [],
    confirmedSteering: [],
    steeringError: null,
    runGeneration: (state.runGeneration ?? 0) + 1,
  });
  storage.setItem("stateful-mode", run.mode);
  storage.setItem("stateful-goal", run.goal);
  storage.setItem("stateful-run-key", pending.key);
  storage.setItem("stateful-created-run-id", run.id);
  storage.removeItem("stateful-initial-turn-sent");
  onRunChanged?.();
  await sendFirstTurn({ rpc, storage, threadId: state.threadId, run, text: run.goal });
  storage.removeItem(PENDING_FOLLOW_UP);
  return true;
}

// Sends a run's first turn unless the thread shows it already arrived (a lost response), and
// records it as sent either way.
export async function sendFirstTurn({ rpc, storage, threadId, run, text }) {
  await ensureThreadLoaded(rpc, threadId);
  if (!(await firstTurnArrived(rpc, threadId, run, text))) {
    await rpc("turn/start", {
      threadId,
      input: [{ type: "text", text, text_elements: [] }],
    });
  }
  storage.setItem("stateful-initial-turn-sent", run.id);
}

// After an app-server restart the thread is not loaded and turns would fail; resume it first.
export async function ensureThreadLoaded(rpc, threadId) {
  const { thread } = await rpc("thread/read", { threadId });
  if (thread?.status?.type === "notLoaded") {
    await rpc("thread/resume", { threadId, excludeTurns: true });
  }
}

// Whether the newest user message on the thread is this text, sent after the run was created.
async function firstTurnArrived(rpc, threadId, run, text) {
  const page = await rpc("thread/items/list", {
    threadId,
    cursor: null,
    limit: 20,
    sortDirection: "desc",
  }).catch((error) => {
    if (error.code === -32601) return { data: [] };
    throw error;
  });
  const latest = page.data.find((entry) => (entry?.item ?? entry)?.type === "userMessage");
  if (!latest) return false;
  const item = latest.item ?? latest;
  const sentText = (item.content ?? [])
    .filter((part) => part.type === "text")
    .map((part) => part.text)
    .join("");
  const startedAt = latest.startedAtMs ?? item.startedAtMs;
  const afterRun = startedAt === undefined || startedAt >= (run.createdAt - 1) * 1000;
  return sentText === text && afterRun;
}
