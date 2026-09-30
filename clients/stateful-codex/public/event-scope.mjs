// Decide which workspace an app-server message belongs to. One app-server serves every open
// tab, so a workspace must apply only its own thread's live events and requests, its own
// project's invalidations, and its own run's updates.

const GLOBAL_REQUEST_METHODS = new Set([
  "account/chatgptAuthTokens/refresh",
  "attestation/generate",
]);

export function eventScope(message) {
  const params = message?.params ?? {};
  const isRequest = Object.hasOwn(message ?? {}, "id") && Boolean(message?.method);
  if (message?.method?.startsWith("gateway/")) return { kind: "gateway" };
  if (isRequest && GLOBAL_REQUEST_METHODS.has(message.method)) {
    return { kind: "globalRequest" };
  }
  const threadId =
    params.threadId ?? params.conversationId ?? params.thread?.id ?? null;
  if (threadId) {
    return { kind: isRequest ? "threadRequest" : "thread", threadId };
  }
  if (params.projectId && params.runId) {
    return { kind: "run", projectId: params.projectId, runId: params.runId };
  }
  if (params.projectId) return { kind: "project", projectId: params.projectId };
  return { kind: isRequest ? "unknownRequest" : "unknown" };
}

// A workspace owns its thread, its project, and its current run. Unknown or global messages
// are not shown in any single workspace.
export function belongsToWorkspace(scope, { threadId, projectId, runId }) {
  switch (scope.kind) {
    case "gateway":
      return true;
    case "thread":
    case "threadRequest":
      return scope.threadId === threadId;
    case "project":
      return scope.projectId === projectId;
    case "run":
      return scope.projectId === projectId && (!runId || scope.runId === runId);
    case "globalRequest":
    case "unknownRequest":
    case "unknown":
      return false;
  }
  return false;
}
