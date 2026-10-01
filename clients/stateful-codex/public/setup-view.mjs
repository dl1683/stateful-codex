// The setup form is mounted once from renderSetup(); every later state change goes through
// updateSetup(), which edits attributes, text and option lists in place. Inputs the user types
// into (directory, name, goal, budget) are never re-created, so their drafts, focus and caret
// survive choice changes and asynchronous thread loading.

export function renderSetup(state) {
  const selectedProject = findProject(state);
  return `
    <main class="setup-shell">
      <p class="eyebrow" data-field="account">${escapeHtml(accountLine(state))}</p>
      <h1>Choose the work. Keep the understanding.</h1>
      <p class="lede">The directory defines the project. You choose the thread view and working mode. Stateful Codex carries the project intelligence across both.</p>
      <form id="setup-form">
        <div class="setup-grid">
          <section class="card stack">
            <h2><span class="step">01</span> Project</h2>
            <label>Existing project
              <select name="projectId">${projectOptions(state)}</select>
            </label>
            <p class="status" data-field="scope"${selectedProject ? "" : " hidden"}>${escapeHtml(scopeLine(selectedProject))}</p>
            <fieldset class="field-group stack" data-group="new-project"${selectedProject ? " hidden disabled" : ""}>
              <label>Project name<input name="projectName" value="${escapeHtml(state.projectName)}" placeholder="Research, product, matter…" required /></label>
              <label>Project directory<input name="rootPath" value="${escapeHtml(state.rootPath)}" placeholder="Absolute path" required /></label>
            </fieldset>
          </section>
          <section class="card stack">
            <h2><span class="step">02</span> Thread view</h2>
            <div class="choice-row">
              ${choice("thread", "create", "Create", "Start a clean conversation view.", state.threadAction)}
              ${choice("thread", "continue", "Continue", "Reopen an existing view.", state.threadAction)}
              ${choice("thread", "fork", "Fork", "Branch from an existing view.", state.threadAction)}
            </div>
            <fieldset class="field-group" data-group="thread-select"${state.threadAction === "create" ? " hidden disabled" : ""}>
              <label>Thread<select name="threadId" required>${threadOptions(state)}</select></label>
            </fieldset>
          </section>
          <section class="card wide stack">
            <h2><span class="step">03</span> Working mode</h2>
            <div class="choice-row">
              ${choice("mode", "autonomous", "Autonomous", "Keep working within the explicit budget; update me at meaningful changes.", state.mode)}
              ${choice("mode", "collaborative", "Collaborative", "Execute normally while making learning and strategy easy to steer.", state.mode)}
              ${choice("mode", "socratic", "Socratic", "Question and synthesize first; wait for my explicit transition to execution.", state.mode)}
            </div>
            <label>Desired outcome<textarea name="goal" placeholder="What should this work accomplish?" required>${escapeHtml(state.goal)}</textarea></label>
            <fieldset class="field-group setup-grid" data-group="budget"${state.mode === "autonomous" ? "" : " hidden disabled"}><label>Maximum continuations<input name="maxContinuations" type="number" min="1" max="1000" value="${state.maxContinuations}" /></label><label>Maximum elapsed seconds<input name="maxElapsedSeconds" type="number" min="60" max="604800" value="${state.maxElapsedSeconds}" /></label></fieldset>
          </section>
        </div>
        <div class="actions">
          <p class="status${state.error ? " error" : ""}" data-field="status" role="status">${escapeHtml(statusLine(state))}</p>
          <button class="primary" type="submit"${submitDisabled(state) ? " disabled" : ""}>${submitLabel(state)}</button>
        </div>
      </form>
    </main>`;
}

const optionsCache = new WeakMap();

// Bring a mounted setup form in line with state without replacing any user-editable control.
export function updateSetup(root, state) {
  const selectedProject = findProject(state);
  setText(root.querySelector('[data-field="account"]'), accountLine(state));
  for (const button of root.querySelectorAll("[data-choice-group]")) {
    const current =
      button.dataset.choiceGroup === "mode" ? state.mode : state.threadAction;
    const selected = String(button.dataset.choiceValue === current);
    if (button.getAttribute("data-selected") !== selected) {
      button.setAttribute("data-selected", selected);
    }
    if (button.getAttribute("aria-pressed") !== selected) {
      button.setAttribute("aria-pressed", selected);
    }
  }
  syncOptions(
    root.querySelector('select[name="projectId"]'),
    projectOptions(state),
    state.projectId,
  );
  const scope = root.querySelector('[data-field="scope"]');
  scope.hidden = !selectedProject;
  setText(scope, scopeLine(selectedProject));
  setGroup(root, "new-project", !selectedProject);
  setGroup(root, "thread-select", state.threadAction !== "create");
  const threadSelect = root.querySelector('select[name="threadId"]');
  syncOptions(threadSelect, threadOptions(state), state.threadId);
  threadSelect.disabled = Boolean(state.threadsLoading);
  setGroup(root, "budget", state.mode === "autonomous");
  const status = root.querySelector('[data-field="status"]');
  status.classList.toggle("error", Boolean(state.error));
  setText(status, statusLine(state));
  const submit = root.querySelector('button[type="submit"]');
  submit.disabled = submitDisabled(state);
  setText(submit, submitLabel(state));
}

function findProject(state) {
  return state.projects.find((project) => project.id === state.projectId);
}

function accountLine(state) {
  return `Stateful Codex · local ${state.account ? "account connected" : "login required"}`;
}

function scopeLine(project) {
  return project ? `Scope: ${project.roots.map((root) => root.path).join(", ")}` : "";
}

function statusLine(state) {
  return (
    state.error ??
    (state.account
      ? "Uses the cached ChatGPT sign-in from Codex CLI. No API key is requested or stored here."
      : "Run codex login in a terminal, then restart this client.")
  );
}

function submitDisabled(state) {
  return Boolean(state.busy || !state.account);
}

function submitLabel(state) {
  return state.busy ? "Opening…" : "Open workspace";
}

function projectOptions(state) {
  return `<option value="new"${state.projectId === "new" ? " selected" : ""}>Create from a directory</option>${state.projects.map((project) => `<option value="${escapeHtml(project.id)}"${project.id === state.projectId ? " selected" : ""}>${escapeHtml(project.name)} · ${escapeHtml(project.roots[0]?.path ?? "no root")}</option>`).join("")}`;
}

function threadOptions(state) {
  const placeholder = state.threadsLoading
    ? "Loading project threads…"
    : "Select a project thread";
  return `<option value="">${placeholder}</option>${state.threads.map((thread) => `<option value="${escapeHtml(thread.id)}"${thread.id === state.threadId ? " selected" : ""}>${escapeHtml(thread.name || thread.preview || thread.id)}</option>`).join("")}`;
}

// Option lists are rebuilt only when their content changes; the select element itself stays.
function syncOptions(select, html, value) {
  const key = html.replace(/ selected/g, "");
  if (optionsCache.get(select) !== key) {
    select.innerHTML = html;
    optionsCache.set(select, key);
  }
  if (select.value !== value) select.value = value;
}

function setGroup(root, name, active) {
  const group = root.querySelector(`[data-group="${name}"]`);
  if (group.hidden === !active && group.disabled === !active) return;
  group.hidden = !active;
  group.disabled = !active;
}

function setText(element, text) {
  if (element.textContent !== text) element.textContent = text;
}

function choice(group, value, label, detail, selected) {
  return `<button class="choice" type="button" data-choice-group="${group}" data-choice-value="${value}" data-selected="${value === selected}" aria-pressed="${value === selected}"><strong>${label}</strong><small>${detail}</small></button>`;
}

function escapeHtml(value) {
  return String(value ?? "").replace(
    /[&<>"']/g,
    (character) =>
      ({
        "&": "&amp;",
        "<": "&lt;",
        ">": "&gt;",
        '"': "&quot;",
        "'": "&#39;",
      })[character],
  );
}
