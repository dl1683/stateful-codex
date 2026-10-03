// Passive memory status, read from the server's journal-backed projection
// (statefulMemory/summary): nothing here is tallied from notifications, and nothing asks the
// model.


// "Memory: 2 rules · 1 decision · 1 open check", plus what this page's session changed.
export function memoryStatusLine(summary) {
  if (!summary) return "Memory: checking…";
  if (summary.error) return "Memory status unavailable";
  const counts = summary.counts ?? {};
  const parts = [
    plural(counts.rules, "rule"),
    plural(counts.pendingRules, "task-limited rule"),
    plural(counts.decisions, "decision"),
    plural(counts.openChecks, "open check"),
    plural(counts.background, "note about you", "notes about you"),
    plural(counts.commits, "remembered commit"),
    plural(counts.unverifiedRules, "rule not in your words (not applied)", "rules not in your words (not applied)"),
    plural(counts.other, "other entry", "other entries"),
  ].filter(Boolean);
  const held = parts.length ? parts.join(" · ") : "nothing saved yet";
  const session = sessionChanges(summary.since);
  const late = summary.sessionStartedLate
    ? " · changes before memory status was available are not counted for this session"
    : "";
  return `Memory: ${held}${session ? ` · this session: ${session}` : ""}${late}`;
}

// What changed since the session's start, in plain words; empty when nothing did.
export function sessionChanges(since) {
  if (!since) return "";
  return [
    since.saved ? `saved ${since.saved}` : "",
    since.commitsRemembered ? `${plural(since.commitsRemembered, "commit")} remembered` : "",
    since.corrected ? `corrected ${since.corrected}` : "",
    since.forgotten ? `forgot ${since.forgotten}` : "",
    since.invalidated ? `${since.invalidated} no longer current` : "",
    since.captureIncomplete ? `${plural(since.captureIncomplete, "capture")} could not finish` : "",
  ]
    .filter(Boolean)
    .join(", ");
}

export function renderMemoryStatus(state) {
  return `<p class="memory-status microcopy" data-memory-status>${escapeHtml(memoryStatusLine(state.memorySummary))}</p>`;
}

function plural(value, one, many = `${one}s`) {
  if (!value) return "";
  return `${value} ${value === 1 ? one : many}`;
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
