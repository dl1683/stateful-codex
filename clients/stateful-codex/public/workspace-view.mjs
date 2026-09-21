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

export function renderWorkspace(state) {
  if (state.loading && !state.project) return renderLoading(state);
  const latestObligation = state.obligations.at(-1);
  const html = `
    <main class="workspace-shell">
      ${renderHeader(state)}
      ${state.error ? `<p class="banner error">${escapeHtml(state.error)}</p>` : ""}
      ${state.notice ? `<p class="banner">${escapeHtml(state.notice)}</p>` : ""}
      ${state.busyAction ? `<p class="working"><span></span>${escapeHtml(state.busyAction)}</p>` : ""}
      <div class="workspace-grid">
        <aside class="stack-panel">
          ${renderIntelligence(state)}
          ${renderHierarchy(state)}
          ${renderSourceRouting(state)}
        </aside>
        <section class="stack-panel main-work">
          ${renderObligation(latestObligation)}
          ${renderStrategy(state)}
          ${renderResult(state)}
          ${renderLive(state)}
          ${renderActivity(state.activity)}
          ${renderInstructionForm()}
        </section>
        <aside class="stack-panel">
          ${renderControls(state)}
          ${renderSteering(state)}
          ${renderFindings(state)}
        </aside>
      </div>
      ${renderRequests(state.pendingRequests)}
    </main>`;
  return html.replace(/[ \t]+\n/g, "\n");
}

function renderLoading(state) {
  return `<main class="boot"><p class="eyebrow">Stateful Codex</p><h1>Opening project intelligence.</h1>${state.error ? `<p class="banner error">${escapeHtml(state.error)}</p>` : ""}</main>`;
}

function renderHeader(state) {
  const run = state.run;
  return `<header class="workspace-header">
    <div><p class="eyebrow">Stateful Codex · ${escapeHtml(run?.mode ?? "preparing")}</p><h1>${escapeHtml(state.project?.name ?? "Project")}</h1><p class="path">${escapeHtml(state.project?.roots?.map((root) => root.path).join(" · ") ?? "")}</p></div>
    <div class="run-summary"><span class="badge ${escapeHtml(run?.status ?? "pending")}">${escapeHtml(run?.status ?? "preparing")}</span><span>strategy r${run?.strategyRevision ?? 0}</span><span>${run?.continuationsUsed ?? 0}/${run?.budget?.maxContinuations ?? 0} continuations</span><button class="text-button" data-action="refresh">Refresh</button></div>
  </header>`;
}

function renderIntelligence(state) {
  const status = state.status;
  return panel(
    "Project intelligence",
    status
      ? `<div class="metrics"><div><strong>${status.blackboardEntryCount}</strong><span>understandings</span></div><div><strong>${status.contextMapEntryCount}</strong><span>source routes</span></div><div><strong>${status.fileCount}</strong><span>files mapped</span></div><div><strong>${status.missingSourceCount}</strong><span>missing sources</span></div></div><p class="microcopy">Knowledge revision ${status.revision}. Root-promoted: ${status.promotedEntryCount}.</p>`
      : empty("Project intelligence has not been initialized."),
  );
}

function renderHierarchy(state) {
  const children = new Map();
  for (const node of state.hierarchy) {
    const key = node.parentId ?? "root";
    children.set(key, [...(children.get(key) ?? []), node]);
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

function renderSourceRouting(state) {
  return panel(
    "Source routing",
    `<form id="context-search" class="inline-form"><input name="query" aria-label="Search context map" placeholder="Find likely source material"/><button class="secondary">Search</button></form>
    ${state.contextHits.length ? state.contextHits.map(renderContextHit).join("") : `<p class="microcopy">Search descriptions and routing terms, then open exact source evidence.</p>`}
    ${state.evidence ? renderEvidence(state.evidence) : ""}`,
  );
}

function renderContextHit(hit) {
  return `<article class="route"><div><strong>${escapeHtml(hit.source.relativePath || hit.source.projectRoot)}</strong><span class="badge ${escapeHtml(hit.freshness)}">${escapeHtml(hit.freshness)}</span></div><p>${escapeHtml(hit.description)}</p><button class="text-button" data-action="evidence" data-entry-id="${escapeHtml(hit.entryId)}" ${hit.freshness !== "current" ? "disabled" : ""}>Verify exact source</button></article>`;
}

function renderEvidence(evidence) {
  return `<article class="evidence"><div><strong>Exact evidence</strong><span class="badge verified">${escapeHtml(evidence.encoding)} · ${evidence.bytesReturned}/${evidence.totalBytes} bytes${evidence.truncated ? " · truncated" : ""}</span></div><p class="path">${escapeHtml(evidence.source.relativePath || evidence.source.projectRoot)}</p><pre>${escapeHtml(evidence.content)}</pre></article>`;
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
    `<p class="result-copy">${escapeHtml(state.run.result)}</p><p class="microcopy">A result is not automatically verified. Review its linked findings, uncertainty, and exact evidence.</p>`,
  );
}

function renderLive(state) {
  return state.liveText
    ? panel(
        "Live response",
        `<p class="live-copy">${escapeHtml(state.liveText)}</p><p class="microcopy">Supporting prose. Durable meaning is recorded in the obligation packet and project intelligence.</p>`,
      )
    : "";
}

function renderActivity(items) {
  const recent = items.filter(isSupportingActivity).slice(-12);
  return `<details class="workspace-panel"><summary>Supporting activity · ${recent.length} recent items</summary><div class="activity-list">${recent.length ? recent.map((item) => `<div><span>${escapeHtml(activityLabel(item))}</span><small>${escapeHtml(activityDetail(item))}</small></div>`).join("") : empty("No supporting activity yet.")}</div></details>`;
}

function renderControls(state) {
  const run = state.run;
  if (!run) return panel("Run controls", empty("Preparing the run."));
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
  return panel(
    "Run controls",
    `<p class="goal">${escapeHtml(run.goal)}</p><div class="control-row">${buttons.join("")}</div><form id="mode-form" class="mode-form"><label>Workflow mode<select name="mode">${modeOptions(run.mode)}</select></label><button class="secondary">Change</button></form><dl><div><dt>Elapsed budget</dt><dd>${formatDuration(run.budget.maxElapsedSeconds)}</dd></div><div><dt>Run revision</dt><dd>${run.revision}</dd></div></dl><button class="secondary full" data-action="maintain">Maintain project intelligence</button>`,
  );
}

function renderSteering(state) {
  const items = state.steering.slice(-5).reverse();
  return panel(
    "Steer the work",
    `<form id="steering-form" class="stack"><textarea name="steering" placeholder="Follow this fact, connect these findings, or change direction…" required></textarea><button class="primary">Submit steering</button></form>${items.length ? `<div class="steering-list">${items.map((item) => `<article><p>${escapeHtml(item.input)}</p><span class="badge ${escapeHtml(item.status)}">${escapeHtml(item.status)}</span>${item.reason ? `<small>${escapeHtml(item.reason)}</small>` : ""}</article>`).join("")}</div>` : `<p class="microcopy">Your exact instruction and its application state remain visible.</p>`}`,
  );
}

function renderFindings(state) {
  const selected = state.selectedNodeId;
  const entries = state.blackboard
    .filter((hit) => !selected || hit.entry.nodeId === selected)
    .filter((hit) =>
      [
        "fact",
        "claim",
        "number",
        "question",
        "contradiction",
        "signal",
        "failure",
      ].includes(hit.entry.kind),
    );
  return panel(
    selected ? "Selected-node findings" : "Findings & open signals",
    entries.length
      ? `<div class="finding-list">${entries.map(renderFinding).join("")}</div>`
      : empty(
          selected
            ? "No matching findings at this node."
            : "No findings have been recorded yet.",
        ),
    selected
      ? `<button class="text-button" data-action="node" data-node-id="${escapeHtml(selected)}">Clear filter</button>`
      : "",
  );
}

function renderFinding(hit) {
  const entry = hit.entry;
  const evidence = entry.evidence
    .map(
      (link) =>
        `<button class="text-button" data-action="evidence" data-entry-id="${escapeHtml(link.contextMapEntryId)}" ${hit.evidenceFreshness !== "current" ? "disabled" : ""}>Open evidence</button>`,
    )
    .join("");
  return `<article><div><span class="badge kind">${escapeHtml(entry.kind)}</span><span class="badge ${escapeHtml(hit.effectiveVerification)}">${escapeHtml(hit.effectiveVerification)}</span><span class="badge ${escapeHtml(hit.evidenceFreshness)}">${escapeHtml(hit.evidenceFreshness)}</span></div><p>${escapeHtml(entry.content)}</p>${evidence}${hit.relations.length ? `<small>${hit.relations.length} linked relationship${hit.relations.length === 1 ? "" : "s"}</small>` : ""}</article>`;
}

function renderInstructionForm() {
  return panel(
    "Add an instruction",
    `<form id="message-form" class="inline-form"><textarea name="message" placeholder="Ask, clarify, or direct the active thread…" required></textarea><button class="primary">Send</button></form>`,
  );
}

function renderRequests(requests) {
  if (!requests.length) return "";
  return `<aside class="request-drawer"><h2>Agent needs input</h2>${requests.map(renderRequest).join("")}</aside>`;
}

function renderRequest(request) {
  if (request.method === "item/tool/requestUserInput") {
    return `<form class="request-card stack" data-request-id="${escapeHtml(request.id)}">${request.params.questions.map(renderQuestion).join("")}<button class="primary">Reply</button></form>`;
  }
  if (
    [
      "item/commandExecution/requestApproval",
      "item/fileChange/requestApproval",
    ].includes(request.method)
  ) {
    const description =
      request.params.command ??
      request.params.reason ??
      "The agent requests approval to continue.";
    return `<article class="request-card"><p>${escapeHtml(description)}</p><div class="control-row"><button class="primary" data-action="approve" data-request-id="${escapeHtml(request.id)}">Approve</button><button class="secondary" data-action="decline" data-request-id="${escapeHtml(request.id)}">Decline</button></div></article>`;
  }
  return `<article class="request-card"><strong>${escapeHtml(request.method)}</strong><p>This request type is visible but must be handled by a compatible client.</p></article>`;
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

function escapeHtml(value) {
  return String(value ?? "").replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        character
      ],
  );
}
