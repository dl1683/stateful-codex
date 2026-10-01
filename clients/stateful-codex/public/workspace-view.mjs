import {
  renderCommandApproval,
  renderFileChangeApproval,
} from "./approval-view.mjs";
import { describeAnswer, normalizeThreadItem } from "./answer-provenance.mjs";
import { renderMarkdown } from "./markdown.mjs";
import { effectiveFreshness, isRootUnavailable, rootKey } from "./source-availability.mjs";

const packetSections = [
  ["examined", "Examined"],
  ["rationale", "Why it matters"],
  ["learning", "Learned"],
  ["implication", "Implication"],
  ["strategy", "Current strategy"],
  ["changed", "What changed"],
  ["next", "Next"],
  ["uncertainty", "Uncertainty"],
  ["blockers", "Blockers"],
  ["requestedJudgment", "Your judgment could help"],
];

// Runtime updates replace one slot at a time (see workspace-dom.mjs). Forms live in slots whose
// markup changes only when the form itself must change, so drafts and focus survive refreshes.
export const WORKSPACE_SLOTS = {
  header: renderHeader,
  notices: renderNotices,
  intelligence: renderIntelligence,
  hierarchy: renderHierarchy,
  "routing-results": renderRoutingResults,
  obligation: (state) => renderObligation(state.obligations.at(-1)),
  strategy: renderStrategy,
  result: renderResult,
  activity: (state) => `${renderTrustLine(state)}${renderActivity(state.activity)}`,
  instruction: renderInstructionForm,
  "controls-state": renderControlsState,
  "mode-form": renderModeForm,
  "controls-detail": renderControlsDetail,
  measured: (state) => renderMeasuredWork(state.measurementSummary),
  "steering-form": renderSteeringForm,
  "steering-status": renderSteeringStatus,
  "steering-list": renderSteeringList,
  findings: renderFindings,
};

export function renderWorkspace(state) {
  return renderWorkspaceShell(state, (name) => WORKSPACE_SLOTS[name](state));
}

// slotHtml(name) supplies each slot's content, letting the runtime reuse memoized slots.
export function renderWorkspaceShell(state, slotHtml) {
  if (isLoadingScreen(state)) return renderLoading(state);
  const slot = (name) =>
    `<div class="slot" data-slot="${name}">${slotHtml(name)}</div>`;
  const html = `
    <main class="workspace-shell">
      ${renderPendingBar(state)}
      ${slot("header")}
      <div class="notices" data-slot="notices" role="status">${slotHtml("notices")}</div>
      <div class="workspace-grid">
        <aside class="stack-panel">
          ${slot("intelligence")}
          ${slot("hierarchy")}
          <section class="workspace-panel"><div class="panel-heading"><h2>Source routing</h2></div><form id="context-search" class="inline-form"><input name="query" aria-label="Search context map" placeholder="Find likely source material"/><button class="secondary">Search</button></form>${slot("routing-results")}</section>
        </aside>
        <section class="stack-panel main-work">
          ${renderRequests(state)}
          ${slot("obligation")}
          ${slot("strategy")}
          ${slot("result")}
          ${renderLive(state)}
          ${slot("activity")}
          ${slot("instruction")}
        </section>
        <aside class="stack-panel controls-panel">
          <section class="workspace-panel"><div class="panel-heading"><h2>Run controls</h2></div>${slot("controls-state")}${slot("mode-form")}${slot("controls-detail")}</section>
          ${slot("measured")}
          <section class="workspace-panel"><div class="panel-heading"><h2>Steer the work</h2></div>${slot("steering-form")}${slot("steering-status")}${slot("steering-list")}</section>
        </aside>
        <section class="findings-work">
          ${renderFindingFilter()}
          ${slot("findings")}
        </section>
      </div>
    </main>`;
  return html.replace(/[ \t]+\n/g, "\n");
}

export function isLoadingScreen(state) {
  return Boolean(state.loading && !state.project);
}

function renderNotices(state) {
  return `${state.error ? `<p class="banner error">${escapeHtml(state.error)}</p>` : ""}${state.notice ? `<p class="banner">${escapeHtml(state.notice)}</p>` : ""}${state.busyAction ? `<p class="working"><span></span>${escapeHtml(state.busyAction)}</p>` : ""}`;
}

function renderLoading(state) {
  return `<main class="boot"><p class="eyebrow">Stateful Codex</p><h1>Opening project intelligence.</h1>${state.error ? `<p class="banner error">${escapeHtml(state.error)}</p>` : ""}</main>`;
}

function renderHeader(state) {
  const run = state.run;
  return `<header class="workspace-header">
    <div><p class="eyebrow">Stateful Codex · ${escapeHtml(run?.mode ?? "preparing")}</p><h1>${escapeHtml(state.project?.name ?? "Project")}</h1><p class="path">${escapeHtml(state.project?.roots?.map((root) => root.path).join(" · ") ?? "")}</p>${renderMissingRoots(state)}</div>
    <div class="run-summary"><span class="badge ${escapeHtml(run?.status ?? "pending")}">${escapeHtml(run?.status ?? "preparing")}</span><span>strategy r${run?.strategyRevision ?? 0}</span><span>${run?.continuationsUsed ?? 0}/${run?.budget?.maxContinuations ?? 0} continuations</span><button class="text-button" data-action="refresh">Refresh</button></div>
  </header>`;
}

// A missing project folder is a project-level condition: say so once, plainly, at the top.
function renderMissingRoots(state) {
  const missing = unavailableRootPaths(state);
  if (!missing.length) return "";
  const all = missing.length === (state.project?.roots ?? []).length;
  const one = missing.length === 1;
  const paths = missing.map((path) => `<code>${escapeHtml(path)}</code>`).join(", ");
  const consequence = all
    ? `Saved understandings and history are still readable, but sources can't be checked or re-indexed until the ${one ? "folder is" : "folders are"} restored at ${one ? "that path" : "those paths"}.`
    : `Sources under ${one ? "it" : "them"} can't be checked until ${one ? "it is" : "they are"} restored; the project's other folders work normally.`;
  return `<p class="banner error project-missing">${all ? `Project ${one ? "folder" : "folders"}` : `${one ? "One project folder" : "Some project folders"}`} not found on disk: ${paths}. ${consequence}</p>`;
}

function unavailableRootPaths(state) {
  if (!state.unavailableRoots?.size) return [];
  return (state.project?.roots ?? [])
    .map((root) => root.path)
    .filter((path) => state.unavailableRoots.has(rootKey(path)));
}

// Each count names the population it measures. contextMapEntryCount includes file and region
// routes, so it is never presented as a region count; regions come from the last refresh.
function renderIntelligence(state) {
  const status = state.status;
  if (!status) {
    return panel(
      "Project intelligence",
      empty("Project intelligence has not been initialized."),
    );
  }
  const refresh = status.lastRefresh;
  const tiles = [
    metric(status.blackboardEntryCount, "saved understandings", "blackboard entries"),
    metric(status.contextMapEntryCount, "source index entries", "file and region routes"),
    metric(status.fileCount, "files mapped"),
    metric(status.missingSourceCount, "missing sources"),
    metric(
      refresh ? refresh.regionsIndexed : null,
      "indexed source regions at last refresh",
      refreshDetail(refresh),
      "metric-wide",
    ),
  ];
  return panel(
    "Project intelligence",
    `<div class="metrics">${tiles.join("")}</div>${unavailableRootPaths(state).length ? `<p class="microcopy">These counts describe the last index. The project folder is currently missing, so they may no longer match what is on disk.</p>` : ""}${renderRefreshHealth(refresh)}<p class="microcopy">Knowledge revision ${status.revision}. Root-promoted: ${status.promotedEntryCount}.</p>`,
  );
}

function refreshDetail(refresh) {
  if (!refresh) return "no completed refresh recorded";
  const when = refresh.completedAt ? formatTimestamp(refresh.completedAt) : "time not recorded";
  if (!refresh.inventoryComplete) return `${when} · file inventory incomplete`;
  if (!refresh.regionCoverageComplete) return `${when} · region coverage partial`;
  return when;
}

function renderRefreshHealth(refresh) {
  if (!refresh) {
    return `<p class="banner">No completed full source-map refresh is recorded. File inventory and region coverage are not yet known to be complete.</p>`;
  }
  if (!refresh.inventoryComplete) {
    return `<p class="banner">File inventory incomplete · ${refresh.filesSkipped} skipped · ${refresh.filesIndexed} indexed. Startup will retry; unindexed files must not be treated as absent.</p>`;
  }
  if (!refresh.regionCoverageComplete) {
    return `<p class="banner">File inventory complete, but searchable region coverage is partial. Exact source reads may still be required.</p>`;
  }
  return `<p class="microcopy">Last full refresh completed: ${formatNumber(refresh.filesIndexed)} files and ${formatNumber(refresh.regionsIndexed)} searchable regions.</p>`;
}

function renderHierarchy(state) {
  const children = new Map();
  for (const node of state.hierarchy) {
    const key = node.parentId ?? "root";
    let bucket = children.get(key);
    if (!bucket) children.set(key, (bucket = []));
    bucket.push(node);
  }
  const roots = children.get("root") ?? [];
  const rows = roots.flatMap((node) => renderNode(node, children, 0, state));
  return panel(
    "Hierarchy",
    rows.length
      ? `<div class="tree">${rows.join("")}</div>`
      : empty("Refresh the source map to build the hierarchy."),
    `<button class="text-button" data-action="refresh-map">Refresh map</button>`,
  );
}

function renderNode(node, children, depth, state) {
  const label = node.relativePath || state.project?.name || "Project";
  const selected = state.selectedNodeId === node.id;
  const row = `<button class="tree-row" style="--depth:${depth}" data-action="node" data-node-id="${escapeHtml(node.id)}" data-selected="${selected}"><span>${kindGlyph(node.kind)}</span><span>${escapeHtml(label)}</span>${node.lifecycle === "active" ? "" : `<em>${escapeHtml(node.lifecycle)}</em>`}</button>`;
  return [
    row,
    ...(children.get(node.id) ?? []).flatMap((child) =>
      renderNode(child, children, depth + 1, state),
    ),
  ];
}

function renderRoutingResults(state) {
  const heading = state.lastSearch
    ? `<p class="microcopy">Results for “${escapeHtml(state.lastSearch)}”</p>`
    : "";
  const searchError = state.searchError
    ? `<p class="inline-error" role="alert">${escapeHtml(state.searchError)}</p>`
    : "";
  const hits = state.contextHits.length
    ? `${heading}${state.contextHits.map((hit) => renderContextHit(hit, state)).join("")}`
    : state.searchError
      ? ""
      : `<p class="microcopy">Search descriptions and routing terms, then open exact source evidence.</p>`;
  return `${searchError}${hits}${state.evidence ? renderEvidence(state.evidence) : ""}`;
}

function renderContextHit(hit, state) {
  const freshness = effectiveFreshness(hit.freshness, state.unavailableRoots, hit.source.projectRoot);
  const errorKey = `hit:${hit.entryId}`;
  const missingRoot = isRootUnavailable(state.unavailableRoots, hit.source.projectRoot);
  return `<article class="route"><div><strong>${escapeHtml(hit.source.relativePath || hit.source.projectRoot)}</strong>${freshnessBadge(freshness)}</div><p>${escapeHtml(hit.description)}</p><button class="text-button" data-action="evidence" data-entry-id="${escapeHtml(hit.entryId)}" data-error-key="${escapeHtml(errorKey)}" ${freshness !== "current" ? "disabled" : ""}>Verify exact source</button>${renderUnverifiable(freshness, { missingRoot, saved: false })}${renderEvidenceError(state, errorKey)}</article>`;
}

const FRESHNESS_LABELS = {
  current: "source current",
  stale: "source changed",
  sourceUnavailable: "source missing",
  notApplicable: "no source linked",
};

function freshnessBadge(freshness) {
  return `<span class="badge ${escapeHtml(freshness)}">${escapeHtml(FRESHNESS_LABELS[freshness] ?? freshness)}</span>`;
}

// A disabled source action says why, and offers an action only where it can help. A search
// hit describes the index, so re-indexing repairs it. A saved understanding keeps the source
// fingerprint it was saved with, so re-indexing cannot repair it; it needs re-checking.
function renderUnverifiable(freshness, { missingRoot, saved }) {
  const refreshMap = `<button class="text-button" data-action="refresh-map">Refresh map</button>`;
  if (freshness === "sourceUnavailable" && missingRoot) {
    return `<p class="microcopy unverifiable">Its project folder is missing (see the notice at the top).</p>`;
  }
  if (saved && freshness === "stale") {
    return `<p class="microcopy unverifiable">Its source changed after this understanding was saved. Refreshing the map does not update a saved understanding; ask the agent to re-check it against the current file.</p>`;
  }
  if (saved && freshness === "sourceUnavailable") {
    return `<p class="microcopy unverifiable">Its saved source can no longer be found. Ask the agent to re-check this understanding.</p>`;
  }
  if (freshness === "stale") {
    return `<p class="microcopy unverifiable">This file changed after it was indexed, so it can't be checked against the index. ${refreshMap} to re-index it.</p>`;
  }
  if (freshness === "sourceUnavailable") {
    return `<p class="microcopy unverifiable">This file is no longer where it was indexed. ${refreshMap} to update the index.</p>`;
  }
  return "";
}

// An exact-source failure is shown beside the control that was used, not at the page top.
function renderEvidenceError(state, errorKey) {
  const error = state.evidenceError;
  if (!error || error.key !== errorKey) return "";
  return `<p class="inline-error" role="alert">${escapeHtml(error.message)}</p>`;
}

function renderEvidence(evidence) {
  const lines = evidence.firstLine
    ? ` · lines ${evidence.firstLine}–${evidence.lastLine}`
    : "";
  return `<article class="evidence"><div><strong>Exact evidence</strong><span class="badge verified">${escapeHtml(evidence.encoding)} · ${evidence.bytesReturned}/${evidence.totalBytes} bytes${lines}${evidence.truncated ? " · truncated" : ""}</span></div><p class="path">${escapeHtml(evidence.source.relativePath || evidence.source.projectRoot)}</p><pre>${escapeHtml(evidence.content)}</pre></article>`;
}

function renderObligation(obligation) {
  if (!obligation) {
    return panel(
      "Current obligation",
      empty("The agent has not published a semantic work update yet."),
    );
  }
  const sections = packetSections
    .filter(([key]) => obligation.packet[key]?.length)
    .map(
      ([key, label]) =>
        `<section><h3>${label}</h3><ul>${obligation.packet[key].map((item) => `<li>${escapeHtml(item)}</li>`).join("")}</ul></section>`,
    )
    .join("");
  return panel(
    `Current obligation · ${obligation.sequence}`,
    `<div class="packet">${sections || empty("This update contains no semantic fields.")}</div>`,
    `<span class="badge">revision ${obligation.revision}</span>`,
  );
}

function renderStrategy(state) {
  const changes = state.obligations
    .flatMap((obligation) => obligation.packet.changed)
    .slice(-5);
  return panel(
    "Strategy & changes",
    `<p class="strategy-current">${escapeHtml(state.run?.strategy ?? "No durable strategy has been published yet.")}</p>${changes.length ? `<ol class="timeline">${changes.map((change) => `<li>${escapeHtml(change)}</li>`).join("")}</ol>` : `<p class="microcopy">Material strategy changes will appear here with their rationale.</p>`}`,
  );
}

function renderResult(state) {
  if (!state.run?.result) return "";
  return panel(
    "Run result",
    `<div class="result-copy prose">${renderMarkdown(state.run.result)}</div><p class="microcopy">A result is not automatically verified. Review its linked findings, uncertainty, and exact evidence.</p>`,
  );
}

export const LIVE_TAIL_CHARACTERS = 64 * 1024;

// The runtime streams into this panel's text nodes directly; the markup is rendered once.
function renderLive(state) {
  const text = state.liveText ?? "";
  return `<section class="workspace-panel" data-live${text ? "" : " hidden"}><div class="panel-heading"><h2>Live response</h2></div><div class="live-copy prose" data-live-copy>${text ? `<span data-live-item="">${escapeHtml(text)}</span>` : ""}</div><p class="microcopy" data-live-truncated hidden>Showing the latest ${LIVE_TAIL_CHARACTERS / 1024}K characters of streamed text. Each complete message appears under <a href="#recorded-messages">Recorded agent messages</a> once it finishes.</p><p class="microcopy">The agent's reply as it streams; finished messages are formatted. Structured progress stays in the obligation and project understanding.</p></section>`;
}

function renderActivity(items) {
  const normalized = items.map(normalizeThreadItem);
  const recent = normalized.filter(isSupportingActivity).slice(-12);
  const messages = normalized
    .filter((item) => item.type === "agentMessage" && item.text)
    .slice(-3);
  return `${renderRecordedMessages(messages)}<details class="workspace-panel"><summary data-disclosure="activity">Supporting activity · ${recent.length} recent items</summary><div class="activity-list">${recent.length ? recent.map((item) => `<div><span>${escapeHtml(activityLabel(item))}</span><small>${escapeHtml(activityDetail(item))}</small></div>`).join("") : empty("No supporting activity yet.")}</div></details>`;
}

// What the latest answer on this thread rested on, as far as the client can observe: saved
// memory in context and its source freshness (from the turn's measurement), exact source
// reads, and the commands, patches and other tools recorded for the turn. It claims nothing
// about mechanisms it cannot see, and is hidden while a newer turn is under way.
export function renderTrustLine(state) {
  const answer = describeAnswer(state);
  if (!answer) return "";
  const atLeast = answer.partial ? "at least " : "";
  const plural = (count, word, words = `${word}s`) =>
    `${formatNumber(count)} ${count === 1 ? word : words}`;
  const facts = [
    answer.usedMemory ? "used saved project memory" : "no saved project memory in context",
    answer.changedSources ? "some of its saved sources had changed since they were saved" : "",
    answer.missingSources ? "some of its saved sources were missing" : "",
    plural(answer.sourceReads, "exact source read"),
    `${atLeast}${plural(answer.commands, "command")} run`,
    answer.declined ? `${plural(answer.declined, "action")} declined` : "",
    `${atLeast}${plural(answer.patches, "patch", "patches")} applied`,
    answer.otherTools ? `${atLeast}${plural(answer.otherTools, "other tool call")}` : "",
  ].filter(Boolean);
  const unchecked =
    (answer.changedSources || answer.missingSources) &&
    !answer.sourceReads &&
    !answer.commands &&
    !answer.otherTools &&
    !answer.partial;
  const status = answer.status === "completed" ? "" : ` (turn ${escapeHtml(answer.status)})`;
  return `<p class="trust-line${unchecked ? " warn" : ""}"><strong>Latest answer${status}:</strong> ${facts.join(" · ")}.${unchecked ? " No source reads, commands or other tools were recorded for it, so parts of it may be out of date; ask it to check the files." : ""}</p>`;
}

// Completed agent messages from the recorded thread history; the live panel shows only a tail.
function renderRecordedMessages(messages) {
  return `<details class="workspace-panel" id="recorded-messages"><summary data-disclosure="recorded-messages">Recorded agent messages · ${messages.length} recent</summary>${messages.length ? messages.map((item) => `<div class="recorded-message prose">${renderMarkdown(item.text)}</div>`).join("") : empty("No completed agent messages are recorded yet.")}</details>`;
}

function renderControlsState(state) {
  const run = state.run;
  if (!run) return empty("Preparing the run.");
  const buttons = [];
  if (run.status === "running") buttons.push(control("pause", "Pause"));
  if (run.status === "paused" || run.status === "pending") {
    buttons.push(
      control("resume", run.mode === "socratic" ? "Begin execution" : "Resume"),
    );
  }
  if (["pending", "running", "paused", "blocked"].includes(run.status)) {
    buttons.push(control("cancel", "Cancel"));
  }
  return `<p class="goal">${escapeHtml(run.goal)}</p><div class="control-row">${buttons.join("")}</div>${isTerminalRun(run) ? `<p class="microcopy">This outcome is closed. Its mode and record are preserved.</p>${newOutcomeLink()}` : ""}`;
}

function renderModeForm(state) {
  if (!state.run || isTerminalRun(state.run)) return "";
  // The persisted mode is applied to the select by the runtime, so a refresh never replaces
  // the editor (or an unsent choice in it).
  return `<form id="mode-form" class="mode-form"><label>Workflow mode<select name="mode">${modeOptions(null)}</select></label><button class="secondary">Change</button></form>`;
}

function renderControlsDetail(state) {
  const run = state.run;
  if (!run) return "";
  return `<dl><div><dt>Elapsed budget</dt><dd>${formatDuration(run.budget.maxElapsedSeconds)}</dd></div><div><dt>Run revision</dt><dd>${run.revision}</dd></div></dl>${renderRecovery(run, state.recovery)}${isTerminalRun(run) ? "" : `<button class="secondary full" data-action="maintain">Maintain project intelligence</button>`}`;
}

function renderMeasuredWork(summary) {
  if (!summary || !summary.measurementCount) {
    return panel(
      "Measured work",
      empty("No durable Stateful turn measurements are available yet."),
    );
  }
  const trajectory = summary.trajectory;
  const counters = summary.counters ?? {};
  const usage = summary.tokenUsage;
  const usageLine = usage
    ? `<p class="microcopy">Token coverage ${summary.turnsWithTokenUsage}/${summary.measurementCount} records · ${formatNumber(usage.inputTokens)} input (${formatNumber(usage.cachedInputTokens)} cached) · ${formatNumber(usage.outputTokens)} output.</p>`
    : `<p class="microcopy">Provider token usage is unavailable for this window.</p>`;
  const windowBoundary = summary.hasMore
    ? " Older measurements exist outside this window."
    : " This window includes every recorded project measurement.";
  const trajectoryCoverage = trajectory
    ? summary.terminalMeasurementCount < summary.measurementCount
      ? ` Recorded trajectory subtotal covers ${summary.terminalMeasurementCount}/${summary.measurementCount} records.`
      : ""
    : " Model-response, model-tool, and tool-output totals are unavailable because no terminal trajectory has been merged.";
  const trajectoryLine = trajectory
    ? `${formatNumber(trajectory.modelToolCalls)} model tool calls · ${formatNumber(trajectory.toolOutputBytes)} tool-output bytes.`
    : "Model tool calls and tool-output bytes unavailable.";
  return panel(
    "Measured work",
    `<div class="metrics">${metric(summary.measurementCount, "turn records")}${metric(summary.runCount, "runs represented")}${metric(trajectory ? trajectory.completedModelResponses : null, "model responses")}${metric(counters.materialFindingsReused ?? 0, "findings reused")}</div><p class="microcopy">Terminal coverage ${summary.terminalMeasurementCount}/${summary.measurementCount} · ${summary.completedTurns} completed · ${summary.failedTurns} failed · ${summary.abortedTurns} aborted · ${formatMilliseconds(summary.durationMs)} measured.${trajectoryCoverage}</p>${usageLine}<p class="microcopy">${trajectoryLine} ${formatNumber(counters.evidenceReadCalls ?? 0)} exact evidence reads.${windowBoundary} Exact observed counts only; no monetary cost is inferred.</p>`,
  );
}

function renderSteeringForm(state) {
  return isTerminalRun(state.run)
    ? `<p class="microcopy">Steering is closed with this outcome. Use Continue in this thread to give the agent a follow-up; steering opens again while it works.</p>`
    : `<form id="steering-form" class="stack"><textarea name="steering" placeholder="Follow this fact, connect these findings, or change direction…" required></textarea><button class="primary">Submit steering</button></form>`;
}

function renderSteeringStatus(state) {
  return state.steeringError
    ? `<p class="banner error" role="alert">${escapeHtml(state.steeringError)}</p>`
    : "";
}

function renderSteeringList(state) {
  const items = state.steering.slice(-5).reverse();
  return items.length
    ? `<div class="steering-list">${items.map((item) => `<article><p>${escapeHtml(item.input)}</p><span class="badge ${escapeHtml(item.status)}">${escapeHtml(item.status)}</span>${item.reason ? `<small>${escapeHtml(item.reason)}</small>` : ""}${item.statusStale ? `<small>Status as confirmed when submitted; later updates are not loaded.</small>` : ""}</article>`).join("")}</div>`
    : `<p class="microcopy">Your exact instruction and its application state remain visible.</p>`;
}

const FINDING_KINDS = [
  "instruction",
  "fact",
  "claim",
  "number",
  "decision",
  "strategy",
  "question",
  "contradiction",
  "failure",
  "rejectedApproach",
  "signal",
  "note",
];

// The filter lives outside the findings slot, so typing in it never loses focus or caret when
// the list below is re-rendered.
function renderFindingFilter() {
  return `<form id="finding-filter" class="inline-form finding-filter" role="search"><input name="text" aria-label="Filter saved understandings" placeholder="Filter saved understandings"/><select name="kind" aria-label="Kind of understanding"><option value="">All kinds</option>${FINDING_KINDS.map((kind) => `<option value="${kind}">${kindLabel(kind)}</option>`).join("")}</select></form>`;
}

// Newest first, current before superseded, narrowed by the node selection and the filter.
function renderFindings(state) {
  const selected = state.selectedNodeId;
  const filter = state.findingFilter ?? { text: "", kind: "" };
  const text = filter.text.trim().toLowerCase();
  const inNode = state.blackboard.filter((hit) => !selected || hit.entry.nodeId === selected);
  const entries = inNode
    .filter(
      (hit) =>
        (!filter.kind || hit.entry.kind === filter.kind) &&
        (!text || hit.entry.content.toLowerCase().includes(text)),
    )
    .sort((left, right) => entryTime(right.entry) - entryTime(left.entry));
  const current = entries.filter((hit) => (hit.entry.state ?? "active") === "active");
  const replaced = entries.filter((hit) => (hit.entry.state ?? "active") !== "active");
  const filtering = Boolean(text || filter.kind);
  // A capped page is disclosed wherever the list is shown, empty or not: absence from the loaded
  // page is not absence from the project.
  const capped = state.blackboardTruncated;
  const loaded = capped ? "loaded " : "";
  const parts = [
    filtering && entries.length ? `${entries.length} of ${inNode.length} ${loaded}match the filter.` : "",
    entries.some((hit) => entryTime(hit.entry)) ? "Most recently updated first." : "",
    capped
      ? `Only the ${formatNumber(state.blackboard.length)} understandings the server returned are loaded; others exist but are not shown here.`
      : "",
  ].filter(Boolean);
  const summary = parts.length ? `<p class="microcopy">${parts.join(" ")}</p>` : "";
  const list = (hits) =>
    `<div class="finding-list">${hits.map((hit) => renderFinding(hit, state)).join("")}</div>`;
  const body = entries.length
    ? `${summary}${current.length ? list(current) : empty(`No current ${loaded}understandings match.`)}${replaced.length ? `<details class="superseded"><summary data-disclosure="superseded-findings">Superseded or withdrawn · ${replaced.length}</summary>${list(replaced)}</details>` : ""}`
    : `${empty(
        filtering
          ? `No ${loaded}saved understandings match the filter.`
          : selected
            ? `No ${loaded}findings at this node.`
            : "No findings have been recorded yet.",
      )}${summary}`;
  return panel(
    selected
      ? "Selected-node understanding"
      : "Project understanding & open signals",
    body,
    selected
      ? `<button class="text-button" data-action="node" data-node-id="${escapeHtml(selected)}">Show all nodes</button>`
      : "",
  );
}

function entryTime(entry) {
  return entry.updatedAt ?? entry.createdAt ?? 0;
}

function kindLabel(kind) {
  const words = kind.replace(/([A-Z])/g, " $1").toLowerCase();
  return `${words[0].toUpperCase()}${words.slice(1)}`;
}

// Dates come from the record, not from its text: when it was saved and last changed.
function renderFindingDate(entry) {
  if (!entry.updatedAt && !entry.createdAt) return "";
  const saved = entry.createdAt ? formatDate(entry.createdAt) : null;
  const changed = entry.updatedAt ? formatDate(entry.updatedAt) : null;
  const label =
    saved && changed && saved !== changed
      ? `Saved ${saved} · updated ${changed} (UTC)`
      : `Saved ${saved ?? changed} (UTC)`;
  return `<small class="finding-date">${escapeHtml(label)}${entry.state && entry.state !== "active" ? ` · ${escapeHtml(entry.state)}` : ""}</small>`;
}

function formatDate(seconds) {
  return new Date(seconds * 1000).toISOString().slice(0, 10);
}

function renderFinding(hit, state) {
  const entry = hit.entry;
  const freshness = findingFreshness(hit, state);
  const evidence = entry.evidence
    .map((link) => {
      const range = link.lineRange ? `${link.lineRange.start}-${link.lineRange.end}` : "";
      const errorKey = `finding:${entry.id}:${link.contextMapEntryId}:${range}`;
      return `<button class="text-button" data-action="evidence" data-entry-id="${escapeHtml(link.contextMapEntryId)}" data-error-key="${escapeHtml(errorKey)}"${link.lineRange ? ` data-first-line="${link.lineRange.start}" data-last-line="${link.lineRange.end}"` : ""} ${freshness !== "current" ? "disabled" : ""}>Open evidence${link.lineRange ? ` · lines ${link.lineRange.start}–${link.lineRange.end}` : ""}</button>${renderEvidenceError(state, errorKey)}`;
    })
    .join("");
  const confirm =
    hit.effectiveVerification === "userConfirmed"
      ? ""
      : `<button class="text-button" data-action="confirm-knowledge" data-entry-id="${escapeHtml(entry.id)}" data-revision="${entry.revision}">Confirm this understanding</button>`;
  return `<article><div><span class="badge kind">${escapeHtml(entry.kind)}</span><span class="badge ${escapeHtml(hit.effectiveVerification)}">${escapeHtml(hit.effectiveVerification)}</span>${freshnessBadge(freshness)}</div>${renderFindingDate(entry)}<p>${escapeHtml(entry.content)}</p>${evidence}${entry.evidence.length ? renderUnverifiable(freshness, { missingRoot: allRootsMissing(state), saved: true }) : ""}${confirm}${hit.relations.length ? `<small>${hit.relations.length} linked relationship${hit.relations.length === 1 ? "" : "s"}</small>` : ""}</article>`;
}

// Evidence links do not name their root, so a finding is overridden only when every project
// folder is missing.
function findingFreshness(hit, state) {
  return allRootsMissing(state) && ["current", "stale"].includes(hit.evidenceFreshness)
    ? "sourceUnavailable"
    : hit.evidenceFreshness;
}

function allRootsMissing(state) {
  const roots = state.project?.roots ?? [];
  return roots.length > 0 && unavailableRootPaths(state).length === roots.length;
}

function renderInstructionForm(state) {
  // A closed outcome can be followed up in place: the reply starts a new outcome on this same
  // thread, which keeps the conversation, project intelligence and (by default) the mode.
  if (isTerminalRun(state.run)) {
    return panel(
      "Continue in this thread",
      `<form id="continue-form" class="stack"><textarea name="followup" aria-label="Follow-up for this thread" placeholder="Ask a follow-up, correct something, or give the next piece of work…" required></textarea><div class="control-row"><label class="inline-label">Mode<select name="followup-mode">${modeOptions(state.run.mode)}</select></label><button class="primary">Continue</button></div></form><p class="microcopy">Starts a new outcome on this thread with the same project intelligence. To start fresh instead, <a class="text-button" href="/">open a new outcome</a>.</p>`,
    );
  }
  return panel(
    "Add an instruction",
    `<form id="message-form" class="inline-form"><textarea name="message" placeholder="Ask, clarify, or direct the active thread…" required></textarea><button class="primary">Send</button></form>`,
  );
}

function isTerminalRun(run) {
  return ["completed", "cancelled", "failed"].includes(run?.status);
}

function newOutcomeLink() {
  return `<a class="text-button" href="/">Start another outcome</a>`;
}

// The full request cards stay in the work column; this slim bar stays in view wherever the
// person has scrolled, announces waiting requests, and takes them to the cards.
function renderPendingBar(state) {
  const count = state.pendingRequests.length;
  // The status region is always exposed (only the visual bar hides), so assistive technology
  // announces the first request as well as later ones.
  return `<p class="visually-hidden" role="status" data-pending-announcement>${pendingSummary(count)}</p><div class="pending-bar" data-pending-bar${count ? "" : " hidden"}><span aria-hidden="true">${pendingSummary(count)}</span><button type="button" class="secondary" data-action="show-requests">Review</button></div>`;
}

export function pendingSummary(count) {
  if (!count) return "";
  return count === 1
    ? "The agent is waiting for your input."
    : `The agent is waiting for your input on ${count} requests.`;
}

// Pending requests sit at the top of the main work column, never over the page, so the
// steering and control panels beside them stay visible and usable while an approval waits.
function renderRequests(state) {
  const requests = state.pendingRequests;
  return `<aside class="request-drawer" aria-label="Agent needs input" data-requests${requests.length ? "" : " hidden"}><h2>Agent needs input</h2><div data-request-list>${requests.map((request) => renderRequestCard(request, state)).join("")}</div></aside>`;
}

// The thread item an approval refers to: a complete live item, then recorded activity, then a
// streamed partial patch (which keeps Approve disabled).
export function requestItem(state, itemId) {
  if (!itemId) return null;
  const live = state.requestItems?.get(itemId);
  if (live && !live.partial) return live;
  return (
    state.activity.map(normalizeThreadItem).find((item) => item?.id === itemId) ??
    live ??
    null
  );
}

// Server request IDs may be numbers or strings; the key keeps 11 and "11" distinct.
export function requestKey(id) {
  return JSON.stringify(id);
}

export function renderRequestCard(request, state) {
  const key = escapeHtml(requestKey(request.id));
  if (request.method === "item/tool/requestUserInput") {
    return `<form class="request-card stack" data-request-key="${key}">${request.params.questions.map(renderQuestion).join("")}<button class="primary">Reply</button></form>`;
  }
  if (
    [
      "item/commandExecution/requestApproval",
      "item/fileChange/requestApproval",
    ].includes(request.method)
  ) {
    const { ready, html } =
      request.method === "item/fileChange/requestApproval"
        ? renderFileChangeApproval(request, requestItem(state, request.params.itemId))
        : { ready: true, html: renderCommandApproval(request) };
    return `<article class="request-card" data-request-key="${key}">${html}<div class="control-row"><button class="primary" data-action="approve"${ready ? "" : " disabled"}>Approve</button><button class="secondary" data-action="decline">Decline</button></div></article>`;
  }
  return `<article class="request-card" data-request-key="${key}"><strong>${escapeHtml(request.method)}</strong><p>This request type is visible but must be handled by a compatible client.</p></article>`;
}

function renderQuestion(question) {
  const name = escapeHtml(question.id);
  const options = question.options
    ?.map(
      (option) =>
        `<option value="${escapeHtml(option.label)}">${escapeHtml(option.label)} — ${escapeHtml(option.description)}</option>`,
    )
    .join("");
  return `<label>${escapeHtml(question.header)}<span>${escapeHtml(question.question)}</span>${options ? `<select name="${name}">${options}${question.isOther ? `<option value="Other">Other</option>` : ""}</select>` : `<input name="${name}" type="${question.isSecret ? "password" : "text"}" required/>`}</label>`;
}

// A value and its label form one named group, so assistive technology and text extraction
// read "45 saved understandings" rather than a run of values followed by a run of labels.
function metric(value, label, detail = "", className = "") {
  const shown = value === null || value === undefined ? "—" : formatNumber(value);
  const name = value === null || value === undefined ? `${label} unavailable` : `${shown} ${label}`;
  return `<div class="metric${className ? ` ${className}` : ""}" role="group" aria-label="${escapeHtml(detail ? `${name} (${detail})` : name)}"><strong aria-hidden="true">${shown}</strong><span aria-hidden="true">${escapeHtml(label[0].toUpperCase() + label.slice(1))}${detail ? ` · ${escapeHtml(detail)}` : ""}</span></div>`;
}

function formatTimestamp(seconds) {
  const iso = new Date(seconds * 1000).toISOString();
  return `${iso.slice(0, 10)} ${iso.slice(11, 16)} UTC`;
}

function panel(title, content, action = "") {
  return `<section class="workspace-panel"><div class="panel-heading"><h2>${title}</h2>${action}</div>${content}</section>`;
}

function control(action, label) {
  return `<button class="secondary" data-action="${action}">${label}</button>`;
}

function modeOptions(selected) {
  return ["autonomous", "collaborative", "socratic"]
    .map(
      (mode) =>
        `<option value="${mode}" ${mode === selected ? "selected" : ""}>${mode[0].toUpperCase()}${mode.slice(1)}</option>`,
    )
    .join("");
}

function renderRecovery(run, recovery) {
  if (run.mode !== "autonomous") return "";
  if (!recovery?.previousTurnId) {
    return `<p class="recovery">Autonomous recovery is armed. No continuation has been claimed yet.</p>`;
  }
  const active =
    recovery.leaseExpiresAt && recovery.leaseExpiresAt > Date.now() / 1000;
  const lease = active
    ? ` Lease active until ${new Date(recovery.leaseExpiresAt * 1000).toLocaleTimeString()}.`
    : " The last lease is recoverable.";
  return `<p class="recovery">Recovery checkpoint: ${escapeHtml(recovery.previousTurnId)}.${lease}</p>`;
}

function empty(message) {
  return `<p class="empty">${escapeHtml(message)}</p>`;
}

function kindGlyph(kind) {
  return { project: "◉", directory: "□", file: "─", region: "·" }[kind] ?? "·";
}

function isSupportingActivity(item) {
  return !["agentMessage", "userMessage"].includes(item.type);
}

function activityLabel(item) {
  return (
    {
      commandExecution: "Command",
      fileChange: "File change",
      reasoning: "Reasoning",
      plan: "Plan",
      mcpToolCall: "Connected tool",
      dynamicToolCall: "Tool",
      contextCompaction: "Context compacted",
    }[item.type] ?? item.type
  );
}

function activityDetail(item) {
  if (item.type === "commandExecution")
    return `${item.status} · ${item.command}`;
  if (item.type === "fileChange")
    return `${item.status} · ${item.changes.length} changes`;
  if (item.type === "mcpToolCall" || item.type === "dynamicToolCall")
    return `${item.tool} · ${item.status}`;
  if (item.type === "plan") return item.text;
  if (item.type === "reasoning") return item.summary.join(" ");
  return item.id ?? "Recorded";
}

function formatDuration(seconds) {
  if (seconds < 3600) return `${Math.ceil(seconds / 60)} min`;
  return `${Math.round((seconds / 3600) * 10) / 10} hr`;
}

function formatMilliseconds(milliseconds) {
  if (milliseconds < 60_000)
    return `${Math.round(milliseconds / 100) / 10} sec`;
  if (milliseconds < 3_600_000)
    return `${Math.round(milliseconds / 6_000) / 10} min`;
  return `${Math.round(milliseconds / 360_000) / 10} hr`;
}

function formatNumber(value) {
  return Number(value ?? 0).toLocaleString("en-US");
}

function escapeHtml(value) {
  return String(value ?? "").replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        character
      ],
  );
}
