import { acceptedReplyResult, isJsonObject, isRequestId } from "./gateway-policy.mjs";
import { belongsToWorkspace, eventScope } from "./public/event-scope.mjs";

const UNSUPPORTED_REQUEST = -32601;

// Thread requests the web workspace can actually answer; anything else is declined at once so
// the app-server never waits on a page that cannot respond.
const SUPPORTED_THREAD_REQUESTS = new Set([
  "item/commandExecution/requestApproval",
  "item/fileChange/requestApproval",
  "item/tool/requestUserInput",
]);

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
  // A workspace first receives an authoritative snapshot of its thread's open requests, so a
  // reconnecting page drops cards that another tab answered while it was away.
  subscribe(sink, scope = {}) {
    const subscription = {
      sink,
      scope: { threadId: scope.threadId ?? null, projectId: scope.projectId ?? null, runId: null },
    };
    this.subscriptions.add(subscription);
    if (subscription.scope.threadId) sink.write(this.pendingSnapshot(subscription.scope.threadId));
    return () => this.subscriptions.delete(subscription);
  }

  route(message) {
    const scope = eventScope(message);
    const unsupportedThreadRequest =
      scope.kind === "threadRequest" && !SUPPORTED_THREAD_REQUESTS.has(message.method);
    if (scope.kind === "globalRequest" || scope.kind === "unknownRequest" || unsupportedThreadRequest) {
      this.decline(message);
      return;
    }
    if (scope.kind === "threadRequest") {
      this.outstanding.set(requestKey(message.id), { threadId: scope.threadId, message });
    }
    if (message.method === "serverRequest/resolved") {
      this.outstanding.delete(requestKey(message.params?.requestId));
    }
    if (message.method === "turn/completed") this.retireTurn(scope.threadId, message.params?.turn?.id);
    for (const subscription of this.subscriptions) {
      if (this.delivers(scope, message, subscription.scope)) subscription.sink.write(message);
    }
  }

  // Forward a browser's answer only if the request is still open and belongs to its thread. The
  // outgoing JSON-RPC response is built here from the stored request's id and a result of exactly
  // the shape that request accepts; nothing else the browser sent is forwarded.
  reply({ threadId, response }) {
    const shaped =
      isJsonObject(response) &&
      Object.keys(response).length === 2 &&
      isRequestId(response.id) &&
      Object.hasOwn(response, "result");
    if (!shaped) {
      return { accepted: false, code: "invalid", reason: "a reply carries only an id and a result" };
    }
    const key = requestKey(response.id);
    const request = this.outstanding.get(key);
    if (!request) {
      return { accepted: false, code: "resolved", reason: "request is no longer open" };
    }
    if (request.threadId !== threadId) {
      return { accepted: false, code: "foreignThread", reason: "request belongs to another thread" };
    }
    const result = acceptedReplyResult(request.message, response.result);
    if (!result) {
      return { accepted: false, code: "invalid", reason: "this reply does not answer that request" };
    }
    this.outstanding.delete(key);
    this.replyToServer({ id: request.message.id, result });
    return { accepted: true };
  }

  // The app-server connection is gone: no open request can be answered any more.
  clear() {
    this.outstanding.clear();
  }

  pendingSnapshot(threadId) {
    const requests = [...this.outstanding.values()]
      .filter((request) => request.threadId === threadId)
      .map((request) => request.message);
    return { method: "gateway/pendingRequests", params: { threadId, requests } };
  }

  // An approval or question cannot outlive the turn that raised it.
  retireTurn(threadId, turnId) {
    if (!threadId || !turnId) return;
    for (const [key, request] of this.outstanding) {
      if (request.threadId === threadId && request.message.params?.turnId === turnId) {
        this.outstanding.delete(key);
      }
    }
  }

  decline(message) {
    this.replyToServer({
      id: message.id,
      error: {
        code: UNSUPPORTED_REQUEST,
        message: `${message.method} is not supported by the Stateful Codex web gateway`,
      },
    });
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
