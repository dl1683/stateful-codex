// Interpret the gateway's answer to a reply. A request that is no longer open (answered in
// another tab, resolved, or its turn ended) is retired quietly; anything else is an error.
export function interpretReplyResult(status, body) {
  if (status >= 200 && status < 300) return { retired: false };
  if (status === 409 && body?.code === "resolved") return { retired: true };
  throw new Error(body?.error?.message ?? body?.reason ?? "Codex reply failed");
}
