import { belongsToWorkspace, eventScope } from "./event-scope.mjs";

const REFRESH_METHODS =
  /^(statefulRun|statefulAttribution|obligation|steering|blackboard|project|thread|turn)\//;

// Apply one app-server message to the workspace state. Returns what the caller should do next;
// messages owned by another thread, project, or run leave the state untouched.
export function applyWorkspaceEvent(state, message) {
  const scope = eventScope(message);
  const owner = {
    threadId: state.threadId,
    projectId: state.projectId,
    runId: state.run?.id ?? null,
  };
  if (!belongsToWorkspace(scope, owner)) {
    // Another thread's event can still invalidate shared project data (for example a sibling
    // thread's attribution changes the project-wide measurement totals): refresh, never render.
    const sharedInvalidation =
      scope.kind === "thread" &&
      message.params?.projectId === state.projectId &&
      REFRESH_METHODS.test(message.method ?? "");
    return { render: false, refresh: sharedInvalidation };
  }

  if (scope.kind === "threadRequest") {
    state.pendingRequests = [
      ...state.pendingRequests.filter((item) => item.id !== message.id),
      message,
    ];
    return { render: true, refresh: false };
  }
  if (message.method === "gateway/pendingRequests") {
    // Authoritative on (re)connect: requests answered elsewhere while away disappear.
    if (message.params?.threadId !== state.threadId) return { render: false, refresh: false };
    state.pendingRequests = [...(message.params?.requests ?? [])];
    return { render: true, refresh: false };
  }
  if (message.method === "gateway/error") {
    state.notice = message.params?.message ?? null;
    return { render: true, refresh: false };
  }
  if (message.method === "serverRequest/resolved") {
    const requestId = message.params?.requestId;
    state.pendingRequests = state.pendingRequests.filter(
      (item) => item.id !== requestId,
    );
    return { render: true, refresh: false };
  }
  // Orders this workspace's events against the history pages it requests (see askAnswer).
  state.eventSeq = (state.eventSeq ?? 0) + 1;
  let render = false;
  let completed = false;
  if (message.method === "item/agentMessage/delta") {
    const delta = message.params?.delta ?? "";
    state.liveText += delta;
    noteLocalAnswer(state, message.params?.itemId ?? null).text += delta;
    render = true;
  }
  if (
    message.method === "item/started" &&
    message.params?.item?.type === "agentMessage"
  ) {
    noteLocalAnswer(state, message.params.item.id);
    render = true;
  }
  if (
    message.method === "item/completed" &&
    message.params?.item?.type === "agentMessage"
  ) {
    const local = noteLocalAnswer(state, message.params.item.id);
    local.text = message.params.item.text ?? "";
    local.completed = true;
    completed = true;
    render = true;
  }
  if (message.method === "turn/started") state.liveText = "";
  const reconnected = message.method === "gateway/connected";
  // Deltas or a completion may have been missed while disconnected: nothing local is trusted.
  if (reconnected) state.localAnswer = null;
  // A reconnect re-reads the thread's items, and so does every completion, so the panel always
  // converges to authoritative history.
  return {
    render: render || reconnected,
    refresh: reconnected || completed || REFRESH_METHODS.test(message.method ?? ""),
  };
}

// The newest assistant item this client saw through events: its id, the sequence number of its
// first event, its text so far and whether it completed. A different item's event replaces it.
function noteLocalAnswer(state, id) {
  if (state.localAnswer?.id !== id) {
    state.localAnswer = { id, firstSeq: state.eventSeq, text: "", completed: false };
  }
  return state.localAnswer;
}

// Applies a history page requested when the event count was `requestSeq`. A page that lists an
// assistant answer decides the Answer panel: the local item survives only if its first event
// arrived after the page was requested AND the page does not list it; otherwise the page's
// newest completed answer wins, whatever the local event order. A page with no answer decides
// nothing.
export function applyHistoryPage(state, items, requestSeq) {
  state.activity = items;
  const listed = items.map((entry) => entry?.item ?? entry);
  if (!listed.some((item) => item?.type === "agentMessage") || !state.localAnswer) return;
  const local = state.localAnswer;
  const newer = local.firstSeq > requestSeq;
  const unlisted = !listed.some((item) => item?.id === local.id);
  if (!(newer && unlisted)) state.localAnswer = null;
}
