// Passive memory status and the dated return card, both read from the server's journal-backed
// projections (statefulMemory/summary and statefulMemory/recap): nothing here is tallied from
// notifications, and nothing asks the model.

import { clipText } from "./memory-receipts.mjs";

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
  return `Memory: ${held}${session ? ` · this session: ${session}` : ""}`;
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

// The return card: dated, bounded, assembled from stored memory. With no recorded next step it
// invites the person to choose rather than inventing one.
export function renderRecap(state) {
  const recap = state.recap;
  if (!recap || recap.error || state.recapDismissed) return "";
  const hasContent =
    recap.lastWork ||
    recap.rules?.length ||
    recap.decisions?.length ||
    recap.openChecks?.length ||
    recap.commits?.length ||
    recap.captureIncomplete;
  if (!hasContent) return "";
  const rows = [];
  if (recap.lastWork) {
    const request = recap.lastWork.request ? ` — “${escapeHtml(clipText(recap.lastWork.request, 160))}”` : "";
    rows.push(`<p>Last finished ${escapeHtml(formatDate(recap.lastWork.finishedAt))}${request}</p>`);
  }
  rows.push(list("Rules that apply", recap.rules, recap.moreRules, (rule) => escapeHtml(clipText(rule, 200))));
  rows.push(
    list("Current decisions", recap.decisions, recap.moreDecisions, (decision) =>
      `${escapeHtml(clipText(decision.text, 200))}${decision.reason ? ` <small>Because: ${escapeHtml(clipText(decision.reason, 200))}</small>` : ""}`,
    ),
  );
  rows.push(
    list("Commits remembered since then (from workspace history)", recap.commits, recap.moreCommits, (commit) =>
      escapeHtml(clipText(commit, 160)),
    ),
  );
  rows.push(list("Open checks", recap.openChecks, recap.moreOpenChecks, (check) => escapeHtml(clipText(check, 200))));
  if (recap.captureIncomplete) {
    rows.push(
      `<p class="microcopy">${recap.captureIncomplete === 1 ? "One capture" : `${recap.captureIncomplete} captures`} could not finish since then; something you said may be missing from memory.</p>`,
    );
  }
  rows.push(`<p class="microcopy">No next step was recorded. Tell the agent what you would like to do next.</p>`);
  return `<section class="workspace-panel recap" data-recap><div class="panel-heading"><h2>Where things stand, as of ${escapeHtml(formatDate(recap.asOf))}</h2><button class="text-button" data-action="recap-dismiss">Hide</button></div>${rows.join("")}</section>`;
}

function list(title, items, more, renderItem) {
  if (!items?.length) return "";
  const extra = more ? `<li class="microcopy">and ${more} more</li>` : "";
  return `<div class="recap-list"><h3>${title}</h3><ul>${items.map((item) => `<li>${renderItem(item)}</li>`).join("")}${extra}</ul></div>`;
}

// Unix seconds as "2026-10-03 14:05 UTC": the same everywhere, independent of the browser locale.
export function formatDate(seconds) {
  if (!Number.isFinite(seconds)) return "an unknown time";
  const iso = new Date(seconds * 1000).toISOString();
  return `${iso.slice(0, 10)} ${iso.slice(11, 16)} UTC`;
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
