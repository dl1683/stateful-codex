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
  if (!belongsToWorkspace(scope, owner)) return { render: false, refresh: false };

  if (scope.kind === "threadRequest") {
    state.pendingRequests = [
      ...state.pendingRequests.filter((item) => item.id !== message.id),
      message,
    ];
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
    state.liveText += message.params?.delta ?? "";
    render = true;
  }
  if (message.method === "turn/started") state.liveText = "";
  return { render, refresh: REFRESH_METHODS.test(message.method ?? "") };
}
