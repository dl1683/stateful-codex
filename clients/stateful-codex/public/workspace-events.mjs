import { belongsToWorkspace, eventScope } from "./event-scope.mjs";

const REQUEST_ITEM_TYPES = new Set(["fileChange", "commandExecution"]);
const REQUEST_ITEM_LIMIT = 50;
const FILE_CHANGE_APPROVAL = "item/fileChange/requestApproval";

const REFRESH_METHODS =
  /^(statefulRun|statefulAttribution|obligation|steering|blackboard|project|thread|turn)\//;

// Apply one app-server message to the workspace state. Returns what the caller should do next:
// `sections` names the workspace slots to update ("requests" reconciles request cards),
// `delta` carries streamed text for the live panel, `turnStarted` resets it, and `refresh`
// asks for a project refresh. Messages owned by another thread, project, or run leave the
// state untouched.
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
    return { sections: [], refresh: sharedInvalidation };
  }

  if (scope.kind === "threadRequest") {
    state.pendingRequests = [
      ...state.pendingRequests.filter((item) => item.id !== message.id),
      message,
    ];
    // A file-change approval names only its item; if that item has not been seen live,
    // refresh so the recorded activity can supply the changed files.
    const unknownItem =
      message.method === FILE_CHANGE_APPROVAL &&
      !state.requestItems?.has(message.params?.itemId);
    return { sections: ["requests"], refresh: unknownItem };
  }
  if (message.method === "gateway/pendingRequests") {
    // Authoritative on (re)connect: requests answered elsewhere while away disappear.
    if (message.params?.threadId !== state.threadId) return { sections: [], refresh: false };
    state.pendingRequests = [...(message.params?.requests ?? [])];
    return { sections: ["requests"], refresh: false };
  }
  if (message.method === "gateway/error") {
    state.notice = message.params?.message ?? null;
    return { sections: ["notices"], refresh: false };
  }
  if (message.method === "serverRequest/resolved") {
    const requestId = message.params?.requestId;
    state.pendingRequests = state.pendingRequests.filter(
      (item) => item.id !== requestId,
    );
    return { sections: ["requests"], refresh: false };
  }
  const refresh = REFRESH_METHODS.test(message.method ?? "");
  if (rememberRequestItem(state, message)) {
    return { sections: ["requests"], refresh };
  }
  if (message.method === "item/agentMessage/delta") {
    const { turnId = null, itemId = null, delta = "" } = message.params ?? {};
    return { sections: [], refresh, delta: { turnId, itemId, delta } };
  }
  if (message.method === "turn/started") {
    const turnId = message.params?.turn?.id ?? message.params?.turnId ?? null;
    return { sections: [], refresh, turnStarted: turnId };
  }
  return { sections: [], refresh };
}

// Keep the latest file-change and command items so approval cards can show what they approve.
// Returns true when a pending request refers to the updated item.
function rememberRequestItem(state, message) {
  const params = message.params ?? {};
  let item = null;
  if (["item/started", "item/completed"].includes(message.method)) {
    if (REQUEST_ITEM_TYPES.has(params.item?.type)) item = params.item;
  } else if (message.method === "item/fileChange/patchUpdated") {
    const known = state.requestItems?.get(params.itemId);
    item = { ...known, type: "fileChange", id: params.itemId, changes: params.changes };
  }
  if (!item?.id) return false;
  state.requestItems ??= new Map();
  state.requestItems.delete(item.id);
  state.requestItems.set(item.id, item);
  if (state.requestItems.size > REQUEST_ITEM_LIMIT) {
    state.requestItems.delete(state.requestItems.keys().next().value);
  }
  return state.pendingRequests.some((request) => request.params?.itemId === item.id);
}
