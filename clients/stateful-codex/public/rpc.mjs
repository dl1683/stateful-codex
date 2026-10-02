import { interpretReplyResult } from "./reply-result.mjs";

// The gateway authenticates this page with an HttpOnly same-site cookie set when the page was
// served; same-origin fetch and EventSource send it automatically.

export async function rpc(method, params = {}) {
  const response = await fetch("/rpc", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ method, params }),
  });
  const body = await response.json();
  if (!response.ok) {
    const error = new Error(body.error?.message ?? "Codex request failed");
    error.code = body.error?.code;
    error.data = body.error?.data;
    throw error;
  }
  return body.result;
}

// scope: { threadId, projectId } in a workspace; omit on the setup page (gateway events only).
export function subscribe(onMessage, scope = {}) {
  const query = new URLSearchParams();
  if (scope.threadId) query.set("thread", scope.threadId);
  if (scope.projectId) query.set("project", scope.projectId);
  const source = new EventSource(`/events?${query}`);
  source.onmessage = (event) => onMessage(JSON.parse(event.data));
  source.onerror = () =>
    onMessage({
      method: "gateway/error",
      params: { message: "Live connection was interrupted. Reconnecting…" },
    });
  return () => source.close();
}

// Answer a server request raised by this workspace's thread; the gateway rejects foreign replies.
export async function reply(threadId, response) {
  const result = await fetch("/reply", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ threadId, response }),
  });
  const body = result.ok ? null : await result.json().catch(() => null);
  return interpretReplyResult(result.status, body);
}
