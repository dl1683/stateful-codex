// Project memory as the person reviews it: their rules, task-limited and unverified rules,
// what they said about themselves, decisions and other knowledge, each with Forget and
// Correct, plus a form to add an entry and "Show more" for long lists. Browsing, adding and
// correcting never start a model turn (statefulMemory/read, /add, /forget and /correct).
// Correction text comes from the drafts the person is writing, never from the re-rendered
// entry, so a refresh cannot erase or silently re-base it.

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
  const drafts = state.memoryDrafts ?? {};
  const items = memory.items.map((item) => ({ ...item, section: sectionOf(item) }));
  const listed = new Set(items.map((item) => item.entryId));
  // A draft whose entry left the list (corrected or forgotten elsewhere) is kept, not lost.
  const orphans = Object.entries(drafts)
    .filter(([entryId]) => !listed.has(entryId))
    .map(([entryId, draft]) => renderOrphan(entryId, draft))
    .join("");
  const body = SECTIONS.map(([section, title, note]) => {
    const inSection = items.filter((item) => item.section === section);
    if (!inSection.length) return "";
    return `<div class="memory-section"><h3>${title}</h3>${note ? `<p class="microcopy">${note}</p>` : ""}<div class="finding-list">${inSection
      .map((item) => renderItem(item, drafts[item.entryId] ?? null))
      .join("")}</div></div>`;
  }).join("");
  const more = memory.cursor
    ? `<div class="control-row"><button class="secondary" data-action="memory-more">Show more</button><span class="microcopy">${memory.items.length} shown so far.</span></div>`
    : "";
  const list = items.length ? body : empty("Nothing is saved yet.");
  return panel(
    "Project memory",
    `${orphans}${list}${more}${renderAddForm(state.memoryAddition)}<p class="microcopy">Adding, forgetting and correcting change memory directly; no model turn is used.</p>`,
  );
}

const ADD_KINDS = [
  ["rule", "A rule (applies to all work)"],
  ["background", "About me"],
  ["decision", "A decision"],
  ["note", "A note"],
];

function renderAddForm(addition) {
  const draft = addition ?? { kind: "rule", content: "", reason: "" };
  const options = ADD_KINDS.map(
    ([kind, label]) => `<option value="${kind}"${draft.kind === kind ? " selected" : ""}>${label}</option>`,
  ).join("");
  return `<form class="stack memory-add" data-memory-add="1"><h3>Add to memory</h3><label>What it is <select name="kind">${options}</select></label><textarea name="content" aria-label="Words to save" required>${escapeHtml(draft.content)}</textarea><input name="reason" aria-label="Reason (for a decision)" placeholder="Reason (for a decision)" value="${escapeHtml(draft.reason)}"><div class="control-row"><button class="primary">Add</button></div></form>`;
}

function renderOrphan(entryId, draft) {
  const id = escapeHtml(entryId);
  return `<article class="memory-conflict" data-memory-entry="${id}"><p>Your unsaved correction is kept: the entry it was for changed or was removed elsewhere.</p><textarea readonly aria-label="Your unsaved correction">${escapeHtml(draft.content)}</textarea><div class="control-row"><button type="button" class="secondary" data-action="memory-cancel" data-entry-id="${id}">Discard</button></div></article>`;
}

// Older servers report background under knowledge; recognise it by its identity.
function sectionOf(item) {
  if (item.section === "knowledge" && item.source === "user" && item.entryId.startsWith("stateful-user-background-")) {
    return "background";
  }
  return item.section;
}

function renderItem(item, draft) {
  const id = escapeHtml(item.entryId);
  const editing = Boolean(draft);
  const replaces = item.replaces?.length
    ? `<small>Replaces: ${escapeHtml(item.replaces[0].content)}</small>`
    : "";
  const scope = item.scopeTitle
    ? `<small>Only in the investigation: ${escapeHtml(item.scopeTitle)}</small>`
    : "";
  const attributed = item.attributedTo
    ? `<small>${escapeHtml(item.attributedTo)}'s words you passed on, not your rule.</small>`
    : "";
  // A draft started from an older revision is kept and flagged; saving it would be refused.
  const conflict = draft && draft.baseRevision !== item.revision
    ? `<p class="microcopy">This entry changed elsewhere since you began; your text is kept below. Saving is refused for the old version: copy your text, Cancel, then Correct again.</p>`
    : "";
  // A shortened entry cannot be corrected here: saving the excerpt would drop its unseen tail.
  const correctButton = item.contentTruncated
    ? `<small>Too long to correct here; forget it and state the new wording instead.</small>`
    : `<button class="text-button" data-action="memory-correct" data-entry-id="${id}">Correct</button>`;
  const correct = editing && !item.contentTruncated
    ? `<form class="stack memory-correct" data-memory-correct="${id}" data-revision="${draft.baseRevision}">${conflict}<textarea name="content" aria-label="Corrected text" required>${escapeHtml(draft.content)}</textarea><div class="control-row"><button class="primary">${item.section === "unverifiedRule" ? "Correct and apply as your rule" : "Save correction"}</button><button type="button" class="secondary" data-action="memory-cancel" data-entry-id="${id}">Cancel</button></div></form>`
    : `<div class="control-row">${correctButton}<button class="text-button" data-action="memory-forget" data-entry-id="${id}" data-revision="${item.revision}">Forget</button></div>`;
  return `<article data-memory-entry="${id}"><p>${escapeHtml(item.content)}${item.contentTruncated ? "…" : ""}</p>${scope}${attributed}${replaces}${correct}</article>`;
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
