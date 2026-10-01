import { belongsToWorkspace, eventScope } from "./event-scope.mjs";

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
    return { sections: ["requests"], refresh: false };
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
