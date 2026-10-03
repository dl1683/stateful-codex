import { belongsToWorkspace, eventScope } from "./event-scope.mjs";
import { noteConnectionLost, noteTurnEvent } from "./execution-state.mjs";
import { groupReceipt, knowledgeReceipt } from "./memory-receipts.mjs";

export { knowledgeReceipt };

const REQUEST_ITEM_LIMIT = 50;
// Per remembered item; a larger patch is kept truncated and flagged, never silently.
const REQUEST_ITEM_DIFF_CHARACTERS = 256 * 1024;
const FILE_CHANGE_APPROVAL = "item/fileChange/requestApproval";
const RECEIPT_LIMIT = 3;

// Recent receipts shown as banners. Totals never come from these: the memory status line
// counts the journal (statefulMemory/summary).
const REFRESH_METHODS =
  /^(statefulRun|statefulAttribution|statefulKnowledge|obligation|steering|blackboard|project|thread|turn)\//;

// Apply one app-server message to the workspace state. Returns what the caller should do next:
// `sections` names the workspace slots to update ("requests" reconciles request cards),
// `delta` carries streamed text for the live panel, `completedMessage` carries a finished
// agent message to format in place, `turnStarted` resets the panel, and `refresh`
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
    return { sections: ["requests"], refresh: missingApprovalItems(state) };
  }
  if (message.method === "gateway/pendingRequests") {
    // Authoritative on (re)connect: requests answered elsewhere while away disappear.
    if (message.params?.threadId !== state.threadId) return { sections: [], refresh: false };
    state.pendingRequests = [...(message.params?.requests ?? [])];
    // Sent on every (re)connect: events may have been missed, so re-read what is running.
    return { sections: ["requests", "header"], refresh: true };
  }
  if (message.method === "gateway/error") {
    state.notice = message.params?.message ?? null;
    if (state.execution) noteConnectionLost(state.execution);
    return { sections: ["notices", "header"], refresh: false };
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
  if (message.method === "item/completed" && message.params?.item?.type === "agentMessage") {
    const { turnId = null, item } = message.params;
    return {
      sections: [],
      refresh,
      completedMessage: { turnId, itemId: item.id ?? null, text: item.text ?? "" },
    };
  }
  if (message.method === "turn/started") {
    const turnId = message.params?.turn?.id ?? message.params?.turnId ?? null;
    if (state.execution) noteTurnEvent(state.execution, message.method, message.params);
    return { sections: ["header"], refresh, turnStarted: turnId };
  }
  if (message.method === "turn/completed") {
    if (state.execution) noteTurnEvent(state.execution, message.method, message.params);
    return { sections: ["header"], refresh };
  }
  if (message.method === "statefulKnowledge/captured") {
    const receipt = knowledgeReceipt(message.params);
    if (!receipt) return { sections: [], refresh: true };
    state.receipts = [...(state.receipts ?? []), receipt].slice(-RECEIPT_LIMIT);
    return { sections: ["notices"], refresh: true };
  }
  if (message.method === "statefulKnowledge/groupCaptured") {
    const receipt = groupReceipt(message.params);
    if (!receipt) return { sections: [], refresh: true };
    state.receipts = [...(state.receipts ?? []), receipt].slice(-RECEIPT_LIMIT);
    return { sections: ["notices"], refresh: true };
  }
  return { sections: [], refresh };
}

// A file-change approval names only its item. When a pending approval has no complete item,
// a refresh loads recorded activity, which carries the changed files.
function missingApprovalItems(state) {
  return state.pendingRequests.some(
    (request) =>
      request.method === FILE_CHANGE_APPROVAL &&
      !(state.requestItems?.get(request.params?.itemId)?.partial === false) &&
      !recordedItem(state, request.params?.itemId),
  );
}

function recordedItem(state, itemId) {
  return state.activity?.some((entry) => (entry?.item ?? entry)?.id === itemId) ?? false;
}

// Keep the latest file-change items, size-bounded, so approval cards can show what they
// approve. Canonical items (item/started, item/completed) replace streamed partial patches.
// Returns true when a pending request refers to the updated item.
function rememberRequestItem(state, message) {
  const params = message.params ?? {};
  let item = null;
  if (["item/started", "item/completed"].includes(message.method)) {
    if (params.item?.type === "fileChange") item = { ...boundedItem(params.item), partial: false };
  } else if (message.method === "item/fileChange/patchUpdated") {
    if (state.requestItems?.get(params.itemId)?.partial === false) return false;
    item = {
      ...boundedItem({ type: "fileChange", id: params.itemId, changes: params.changes }),
      partial: true,
    };
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

const encoder = new TextEncoder();
// ignoreBOM keeps a leading U+FEFF, which is part of an added or deleted file's content.
const decoder = new TextDecoder("utf-8", { ignoreBOM: true });

function copyString(text) {
  return decoder.decode(encoder.encode(text));
}

function boundedItem(item) {
  let budget = REQUEST_ITEM_DIFF_CHARACTERS;
  const changes = (item.changes ?? []).map((change) => {
    const diff = String(change.diff ?? "");
    // Copy through an encoder: a plain slice can keep the original huge string alive.
    const sliced = diff.slice(0, Math.max(budget, 0));
    const kept = copyString(sliced);
    budget -= sliced.length;
    return {
      path: change.path,
      kind: change.kind,
      diff: kept,
      diffTruncated: sliced.length < diff.length,
    };
  });
  return { type: "fileChange", id: item.id, status: item.status, changes };
}
