// Project memory as the person reviews it: their rules, task-limited and unverified rules,
// decisions and other knowledge, each with Forget and Correct. Browsing and correcting never
// start a model turn (statefulMemory/read, /forget and /correct).

import { executionLabel } from "./execution-state.mjs";

const SECTIONS = [
  ["userRule", "Your rules", "Applied to all work in this project."],
  ["pendingRule", "Task-limited rules", "Kept, not applied."],
  ["unverifiedRule", "Rules not in your words", "Not applied. Correcting one makes your words a standing rule."],
  ["background", "About you", "What you said about yourself and this work."],
  ["decision", "Decisions", ""],
  ["knowledge", "Other knowledge", ""],
];

export function renderMemory(state) {
  const memory = state.memory;
  if (!memory) return panel("Project memory", empty("Loading project memory."));
  if (memory.error) {
    return panel("Project memory", `<p class="microcopy">Project memory is unavailable: ${escapeHtml(memory.error)}</p>`);
  }
  const items = memory.items.map((item) => ({ ...item, section: sectionOf(item) }));
  if (!items.length) return panel("Project memory", empty("Nothing is saved yet."));
  const body = SECTIONS.map(([section, title, note]) => {
    const inSection = items.filter((item) => item.section === section);
    if (!inSection.length) return "";
    return `<div class="memory-section"><h3>${title}</h3>${note ? `<p class="microcopy">${note}</p>` : ""}<div class="finding-list">${inSection
      .map((item) => renderItem(item, state.memoryEditing === item.entryId))
      .join("")}</div></div>`;
  }).join("");
  const more = memory.more
    ? `<p class="microcopy">Showing the first ${memory.items.length} entries.</p>`
    : "";
  return panel(
    "Project memory",
    `${body}${more}<p class="microcopy">Forget and Correct change memory directly; no model turn is used.</p>`,
  );
}

// Background is stored as the user's own fact; the server reports it under knowledge.
function sectionOf(item) {
  if (item.section === "knowledge" && item.source === "user" && item.entryId.startsWith("stateful-user-background-")) {
    return "background";
  }
  return item.section;
}

function renderItem(item, editing) {
  const id = escapeHtml(item.entryId);
  const replaces = item.replaces?.length
    ? `<small>Replaces: ${escapeHtml(item.replaces[0].content)}</small>`
    : "";
  // A shortened entry cannot be corrected here: saving the excerpt would drop its unseen tail.
  const correctButton = item.contentTruncated
    ? `<small>Too long to correct here; forget it and state the new wording instead.</small>`
    : `<button class="text-button" data-action="memory-correct" data-entry-id="${id}">Correct</button>`;
  const correct = editing && !item.contentTruncated
    ? `<form class="stack memory-correct" data-memory-correct="${id}" data-revision="${item.revision}"><textarea name="content" aria-label="Corrected text" required>${escapeHtml(item.content)}</textarea><div class="control-row"><button class="primary">${item.section === "unverifiedRule" ? "Correct and apply as your rule" : "Save correction"}</button><button type="button" class="secondary" data-action="memory-cancel">Cancel</button></div></form>`
    : `<div class="control-row">${correctButton}<button class="text-button" data-action="memory-forget" data-entry-id="${id}" data-revision="${item.revision}">Forget</button></div>`;
  return `<article data-memory-entry="${id}"><p>${escapeHtml(item.content)}${item.contentTruncated ? "…" : ""}</p>${replaces}${correct}</article>`;
}

// The current execution and the run's durable state, said separately and plainly: a run that
// stays open is not work in progress, and nothing says "not working" before an authoritative
// read or live event has shown it.
export function runStateLabel(state) {
  const run = state.run;
  const now = executionLabel(state.execution, state.pendingRequests?.length ?? 0);
  if (!run) return now;
  const mode = `${run.mode[0].toUpperCase()}${run.mode.slice(1)}`;
  if (!["pending", "running", "paused"].includes(run.status)) return `${now} · ${mode} run ${run.status}`;
  return `${now} · ${durableRunLabel(run, mode)}`;
}

function durableRunLabel(run, mode) {
  if (run.status === "paused") return `${mode} run paused`;
  if (run.mode === "autonomous") return `${mode} run open; it may continue on its own`;
  return `${mode} run stays open between answers`;
}

// The document title carries waiting approvals so a background tab still shows them.
export function documentTitle(baseTitle, pendingCount) {
  return pendingCount ? `(${pendingCount}) Approval needed · ${baseTitle}` : baseTitle;
}

function panel(title, content) {
  return `<section class="workspace-panel"><div class="panel-heading"><h2>${title}</h2></div>${content}</section>`;
}

function empty(message) {
  return `<p class="empty">${escapeHtml(message)}</p>`;
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
