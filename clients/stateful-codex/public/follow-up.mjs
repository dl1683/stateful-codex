// Starting a run's first turn safely, and following up a closed outcome on the same thread.
//
// A follow-up starts a new run on the same thread, exactly as setup's Continue does. Every step
// can lose its response (the request may or may not have taken effect), so each is recorded
// before it is sent and reconciled before it is retried:
// - the follow-up's start request (workspace, goal, mode, budget, idempotency key) is saved
//   first, and replaying it with the same key returns the run it created, if any;
// - a first turn records when its dispatch began. A turn never dispatched is simply sent. One
//   whose dispatch outcome is unknown is resent only when the thread's history since that
//   moment is fully read and does not contain it; otherwise the person is asked to check.

const PENDING_FOLLOW_UP = "stateful-follow-up";
const DISPATCH = "stateful-turn-dispatch";
const HISTORY_PAGE = 50;
const MAX_HISTORY_PAGES = 10;
// Allowance for the browser and app-server clocks when comparing a dispatch time.
const CLOCK_SLACK_MS = 2 * 60 * 1000;

// The saved follow-up for this workspace. One saved by another workspace is discarded: it must
// never run in a different project or thread.
export function readPendingFollowUp(storage, { projectId, threadId }) {
  let pending = null;
  try {
    pending = JSON.parse(storage.getItem(PENDING_FOLLOW_UP) ?? "null");
  } catch {
    pending = null;
  }
  if (!pending?.key || !pending.goal) return null;
  if (pending.projectId !== projectId || pending.threadId !== threadId) {
    storage.removeItem(PENDING_FOLLOW_UP);
    return null;
  }
  return pending;
}

// Setup clears follow-up bookkeeping when a different workspace is opened.
export function clearFollowUpRecords(storage) {
  storage.removeItem(PENDING_FOLLOW_UP);
  storage.removeItem(DISPATCH);
}

// Starts a follow-up, or finishes an earlier one whose outcome was never confirmed. An earlier
// unconfirmed follow-up with different text is finished first and this text is not sent: the
// error carries it (refusedText) so the caller can keep it visible.
export async function startFollowUp({ state, rpc, storage, goal, mode, onRunChanged }) {
  const scope = { projectId: state.projectId, threadId: state.threadId };
  const pending = readPendingFollowUp(storage, scope);
  if (pending && (pending.goal !== goal || pending.mode !== mode)) {
    await resumePendingFollowUp({ state, rpc, storage, onRunChanged });
    const error = new Error(
      `An earlier follow-up ("${pending.goal}") had not finished starting; it has now been started. Your new text was not sent; it is in the instruction box, ready to send.`,
    );
    error.refusedText = goal;
    throw error;
  }
  if (!pending) {
    storage.setItem(
      PENDING_FOLLOW_UP,
      JSON.stringify({ ...scope, goal, mode, budget: state.run.budget, key: crypto.randomUUID() }),
    );
  }
  await resumePendingFollowUp({ state, rpc, storage, onRunChanged });
}

// Completes this workspace's saved follow-up, if any: (re)issues its idempotent start, then
// its first turn. Returns whether there was one.
export async function resumePendingFollowUp({ state, rpc, storage, onRunChanged }) {
  const pending = readPendingFollowUp(storage, {
    projectId: state.projectId,
    threadId: state.threadId,
  });
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
  if (storage.getItem("stateful-created-run-id") !== run.id) {
    storage.setItem("stateful-created-run-id", run.id);
    storage.removeItem("stateful-initial-turn-sent");
  }
  onRunChanged?.();
  if (storage.getItem("stateful-initial-turn-sent") !== run.id) {
    await sendFirstTurn({ rpc, storage, threadId: state.threadId, run, text: run.goal });
  }
  storage.removeItem(PENDING_FOLLOW_UP);
  return true;
}

// Sends a run's first turn and records it as sent. A dispatch that may already have reached the
// server is reconciled against the thread's history first.
export async function sendFirstTurn({ rpc, storage, threadId, run, text }) {
  await ensureThreadLoaded(rpc, threadId);
  const dispatch = readDispatch(storage, run.id);
  const arrived = dispatch ? await turnArrivedSince(rpc, threadId, text, dispatch.at) : false;
  if (arrived === null) {
    throw new Error(
      "It isn't certain whether the first instruction reached the agent, and the thread's recent history is too long to check. Look at the recorded messages before sending it again.",
    );
  }
  if (!arrived) {
    storage.setItem(DISPATCH, JSON.stringify({ runId: run.id, at: Date.now() }));
    await rpc("turn/start", {
      threadId,
      input: [{ type: "text", text, text_elements: [] }],
    });
  }
  storage.setItem("stateful-initial-turn-sent", run.id);
  storage.removeItem(DISPATCH);
}

function readDispatch(storage, runId) {
  try {
    const dispatch = JSON.parse(storage.getItem(DISPATCH) ?? "null");
    return dispatch?.runId === runId && Number.isFinite(dispatch.at) ? dispatch : null;
  } catch {
    return null;
  }
}

// After an app-server restart the thread is not loaded and turns would fail; resume it first.
export async function ensureThreadLoaded(rpc, threadId) {
  const { thread } = await rpc("thread/read", { threadId });
  if (thread?.status?.type === "notLoaded") {
    await rpc("thread/resume", { threadId, excludeTurns: true });
  }
}

// true: a user message with this text started after the dispatch began. false: the history
// since then was read completely and has none. null: that could not be established.
async function turnArrivedSince(rpc, threadId, text, dispatchedAt) {
  const boundary = dispatchedAt - CLOCK_SLACK_MS;
  let cursor = null;
  for (let page = 0; page < MAX_HISTORY_PAGES; page += 1) {
    let response;
    try {
      response = await rpc("thread/items/list", {
        threadId,
        cursor,
        limit: HISTORY_PAGE,
        sortDirection: "desc",
      });
    } catch (error) {
      if (error.code === -32601) return null;
      throw error;
    }
    for (const entry of response.data) {
      const item = entry?.item ?? entry;
      const startedAt = entry?.startedAtMs ?? item?.startedAtMs;
      if (!Number.isFinite(startedAt)) continue;
      if (startedAt < boundary) return false;
      if (item?.type === "userMessage" && messageText(item) === text) return true;
    }
    cursor = response.nextCursor;
    if (!cursor) return false;
  }
  return null;
}

function messageText(item) {
  return (item.content ?? [])
    .filter((part) => part.type === "text")
    .map((part) => part.text)
    .join("");
}
