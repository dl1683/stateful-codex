// Ask: a run-less, project-attached thread. Its workspace never creates, adopts, changes or
// reads a run, so nothing in a run's lifecycle (acceptance, Blocked, continuations) applies to
// it. Work modes keep the run route. The choice is made at setup, before any run exists, and
// lives in sessionStorage, so refresh and reconnect reopen the same route.

export const ASK_MODE = "ask";
export const ASK_SENT_KEY = "stateful-ask-sent";

const OPEN_RUN_STATUSES = ["pending", "running", "paused", "blocked"];

export function isAskWorkspace(mode) {
  return mode === ASK_MODE;
}

// A thread whose run is still open binds every new turn to that run, so it cannot be asked
// run-free; a fresh thread can.
export function askRefusal(run) {
  return run && OPEN_RUN_STATUSES.includes(run.status)
    ? `This thread has an open ${run.mode} run, so a question here would join it. Open a new thread to ask, or continue the run in its own mode.`
    : null;
}

// Sends the setup question at most once per thread. The mark is written before the request,
// so a refresh or reconnect during submission never repeats it; a failed request clears the
// mark and the composer stays available to ask again.
export async function sendInitialQuestion({ storage, threadId, question, sendTurn }) {
  if (storage.getItem(ASK_SENT_KEY) === threadId) return false;
  storage.setItem(ASK_SENT_KEY, threadId);
  try {
    await sendTurn(question);
  } catch (error) {
    storage.removeItem(ASK_SENT_KEY);
    throw error;
  }
  return true;
}
