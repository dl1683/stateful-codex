// Memory controls act directly with the user's authority, without starting a model turn.
const SECTIONS = [
  ["userRule", "Your rules", "Retained standing rules. Applicability is separate."],
  ["pendingRule", "Task-limited rules", "Historical scope is unsupported; kept, not applied."],
  ["unverifiedRule", "Rules not in your words", "Not applied. Correcting one makes your words a standing rule."],
  ["background", "About you", "What you said about yourself and this work."],
  ["decision", "Decisions", ""],
  ["knowledge", "Other knowledge", ""],
];

export function renderMemory(state) {
  const memory = state.memory;
  const drafts = state.memoryDrafts ?? {};
  const items = memory?.items ?? [];
  const listed = new Set(items.map((item) => item.entryId));
  const orphans = Object.entries(drafts).filter(([id]) => !listed.has(id)).map(([id, draft]) =>
    `<article class="memory-conflict"><p>Your unsaved correction is kept: its entry changed or was removed.</p><textarea readonly aria-label="Your unsaved correction">${escapeHtml(draft.content)}</textarea><button type="button" class="secondary" data-action="memory-cancel" data-entry-id="${escapeHtml(id)}">Discard</button></article>`,
  ).join("");
  const body = SECTIONS.map(([section, title, note]) => {
    const entries = items.filter((item) => item.section === section);
    if (!entries.length) return "";
    return `<div class="memory-section"><h3>${title}</h3>${note ? `<p class="microcopy">${note}</p>` : ""}<div class="finding-list">${entries.map((item) => renderItem(item, drafts[item.entryId])).join("")}</div></div>`;
  }).join("");
  const status = memory?.error
    ? `Project memory is unavailable: ${escapeHtml(memory.error)}`
    : !memory ? "Loading project memory." : !items.length ? "Nothing is saved yet." : "";
  return `<section class="workspace-panel"><div class="panel-heading"><h2>Project memory</h2></div>${status ? `<p class="microcopy">${status}</p>` : ""}${orphans}${body}<p class="microcopy">Forget and Correct change memory directly; no model turn is used.</p></section>`;
}

function renderItem(item, draft) {
  const id = escapeHtml(item.entryId);
  const conflict = draft && draft.baseRevision !== item.revision
    ? `<p class="microcopy">This entry changed elsewhere since you began; your text is kept. Cancel and Correct again to use its new revision.</p>` : "";
  const prior = item.replaces?.length
    ? `<small>Replaces: ${escapeHtml(item.replaces[0].content)}</small>` : "";
  const held = item.scopeState || item.exceedsApplyBound
    ? `<small>Kept for review; not applied to new work.</small>` : "";
  const correct = item.contentTruncated
    ? `<small>Too long to correct here; its excerpt cannot replace the unseen words.</small>${draft ? `<textarea readonly aria-label="Your unsaved correction">${escapeHtml(draft.content)}</textarea><button type="button" data-action="memory-cancel" data-entry-id="${id}">Discard draft</button>` : ""}`
    : draft
      ? `<form class="stack memory-correct" data-memory-correct="${id}">${conflict}<textarea name="content" data-entry-id="${id}" aria-label="Corrected text" required>${escapeHtml(draft.content)}</textarea><div class="control-row"><button class="primary">${item.section === "unverifiedRule" ? "Correct and apply as your rule" : "Save correction"}</button><button type="button" class="secondary" data-action="memory-cancel" data-entry-id="${id}">Cancel</button></div></form>`
      : `<button class="text-button" data-action="memory-correct" data-entry-id="${id}">Correct</button>`;
  return `<article data-memory-entry="${id}"><p>${escapeHtml(item.content)}${item.contentTruncated ? "…" : ""}</p>${held}${prior}${correct}<button class="text-button" data-action="memory-forget" data-entry-id="${id}" data-revision="${item.revision}">Forget</button></article>`;
}

function escapeHtml(value) {
  return String(value).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;").replaceAll("'", "&#39;");
}
