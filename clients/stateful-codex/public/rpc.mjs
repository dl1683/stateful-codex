const sessionToken = document
  .querySelector('meta[name="stateful-session"]')
  .getAttribute("content");

export async function rpc(method, params = {}) {
  const response = await fetch("/rpc", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-Stateful-Session": sessionToken,
    },
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

export function subscribe(onMessage) {
  const source = new EventSource(
    `/events?token=${encodeURIComponent(sessionToken)}`,
  );
  source.onmessage = (event) => onMessage(JSON.parse(event.data));
  source.onerror = () =>
    onMessage({
      method: "gateway/error",
      params: { message: "Live connection was interrupted. Reconnecting…" },
    });
  return () => source.close();
}
