import { belongsToWorkspace, eventScope } from "./public/event-scope.mjs";

const UNSUPPORTED_REQUEST = -32601;

// Delivers app-server messages only to the browser subscriptions that own them, and keeps
// server-originated requests (approvals, user input) bound to the thread that raised them so a
// reply from another workspace is rejected.
export class EventRouter {
  constructor({ replyToServer }) {
    this.replyToServer = replyToServer;
    this.subscriptions = new Set();
    this.outstanding = new Map();
  }

  // scope: { threadId, projectId } for a workspace, {} for the setup page (gateway events only).
  subscribe(sink, scope = {}) {
    const subscription = {
      sink,
      scope: { threadId: scope.threadId ?? null, projectId: scope.projectId ?? null, runId: null },
    };
    this.subscriptions.add(subscription);
    for (const { threadId, message } of this.outstanding.values()) {
      if (threadId === subscription.scope.threadId) sink.write(message);
    }
    return () => this.subscriptions.delete(subscription);
  }

  route(message) {
    const scope = eventScope(message);
    if (scope.kind === "globalRequest" || scope.kind === "unknownRequest") {
      this.replyToServer({
        id: message.id,
        error: {
          code: UNSUPPORTED_REQUEST,
          message: `${message.method} is not supported by the Stateful Codex web gateway`,
        },
      });
      return;
    }
    if (scope.kind === "threadRequest") {
      this.outstanding.set(requestKey(message.id), { threadId: scope.threadId, message });
    }
    if (message.method === "serverRequest/resolved") {
      this.outstanding.delete(requestKey(message.params?.requestId));
    }
    for (const subscription of this.subscriptions) {
      if (this.delivers(scope, message, subscription.scope)) subscription.sink.write(message);
    }
  }

  // Forward a browser's answer only if the request is still open and belongs to its thread.
  reply({ threadId, response }) {
    const key = requestKey(response?.id);
    const request = this.outstanding.get(key);
    if (!request) return { accepted: false, reason: "unknown or already answered request" };
    if (request.threadId !== threadId) {
      return { accepted: false, reason: "request belongs to another thread" };
    }
    this.outstanding.delete(key);
    this.replyToServer(response);
    return { accepted: true };
  }

  delivers(scope, message, owner) {
    if (belongsToWorkspace(scope, owner)) return true;
    // A sibling thread's notification can invalidate shared project data in this workspace.
    return (
      scope.kind === "thread" &&
      Boolean(owner.projectId) &&
      message.params?.projectId === owner.projectId
    );
  }
}

// JSON-RPC ids may be numbers or strings; keep the type in the key so 1 and "1" stay distinct.
function requestKey(id) {
  return `${typeof id}:${id}`;
}
