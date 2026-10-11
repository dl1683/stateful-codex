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
  let render = false;
  if (message.method === "item/agentMessage/delta") {
    const delta = message.params?.delta ?? "";
    const itemId = message.params?.itemId ?? null;
    state.liveText += delta;
    // The message now streaming, by item, so its completed text can replace what arrived.
    if (itemId !== state.liveItemId) {
      state.liveItemId = itemId;
      state.liveItemText = "";
    }
    state.liveItemText += delta;
    render = true;
  }
  if (
    message.method === "item/completed" &&
    message.params?.item?.type === "agentMessage"
  ) {
    const { id, text } = message.params.item;
    state.completedAnswer = { id, text };
    render = true;
  }
  if (message.method === "turn/started") {
    state.liveText = "";
    state.liveItemId = null;
    state.liveItemText = "";
  }
  // A reconnect may have missed deltas or a completion: re-read the thread's items.
  const reconnected = message.method === "gateway/connected";
  return {
    render,
    refresh: reconnected || REFRESH_METHODS.test(message.method ?? ""),
  };
}
