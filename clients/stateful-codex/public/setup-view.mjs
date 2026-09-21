export function renderSetup(state) {
  const selectedProject = state.projects.find(
    (project) => project.id === state.projectId,
  );
  return `
    <main class="setup-shell">
      <p class="eyebrow">Stateful Codex · local ${state.account ? "account connected" : "login required"}</p>
      <h1>Choose the work. Keep the understanding.</h1>
      <p class="lede">The directory defines the project. You choose the thread view and working mode. Stateful Codex carries the project intelligence across both.</p>
      <form id="setup-form">
        <div class="setup-grid">
          <section class="card stack">
            <h2><span class="step">01</span> Project</h2>
            <label>Existing project
              <select name="projectId">
                <option value="new" ${state.projectId === "new" ? "selected" : ""}>Create from a directory</option>
                ${state.projects.map((project) => `<option value="${escapeHtml(project.id)}" ${project.id === state.projectId ? "selected" : ""}>${escapeHtml(project.name)} · ${escapeHtml(project.roots[0]?.path ?? "no root")}</option>`).join("")}
              </select>
            </label>
            ${
              selectedProject
                ? `<p class="status">Scope: ${escapeHtml(selectedProject.roots.map((root) => root.path).join(", "))}</p>`
                : `
              <label>Project name<input name="projectName" value="${escapeHtml(state.projectName)}" placeholder="Research, product, matter…" required /></label>
              <label>Project directory<input name="rootPath" value="${escapeHtml(state.rootPath)}" placeholder="Absolute path" required /></label>
            `
            }
          </section>
          <section class="card stack">
            <h2><span class="step">02</span> Thread view</h2>
            <div class="choice-row">
              ${choice("thread", "create", "Create", "Start a clean conversation view.", state.threadAction)}
              ${choice("thread", "continue", "Continue", "Reopen an existing view.", state.threadAction)}
              ${choice("thread", "fork", "Fork", "Branch from an existing view.", state.threadAction)}
            </div>
            ${state.threadAction === "create" ? "" : `<label>Thread<select name="threadId" required><option value="">Select a project thread</option>${state.threads.map((thread) => `<option value="${escapeHtml(thread.id)}" ${thread.id === state.threadId ? "selected" : ""}>${escapeHtml(thread.name || thread.preview || thread.id)}</option>`).join("")}</select></label>`}
          </section>
          <section class="card wide stack">
            <h2><span class="step">03</span> Working mode</h2>
            <div class="choice-row">
              ${choice("mode", "autonomous", "Autonomous", "Keep working within the explicit budget; update me at meaningful changes.", state.mode)}
              ${choice("mode", "collaborative", "Collaborative", "Execute normally while making learning and strategy easy to steer.", state.mode)}
              ${choice("mode", "socratic", "Socratic", "Question and synthesize first; wait for my explicit transition to execution.", state.mode)}
            </div>
            <label>Desired outcome<textarea name="goal" placeholder="What should this work accomplish?" required>${escapeHtml(state.goal)}</textarea></label>
            ${state.mode === "autonomous" ? `<div class="setup-grid"><label>Maximum continuations<input name="maxContinuations" type="number" min="1" max="1000" value="${state.maxContinuations}" /></label><label>Maximum elapsed seconds<input name="maxElapsedSeconds" type="number" min="60" max="604800" value="${state.maxElapsedSeconds}" /></label></div>` : ""}
          </section>
        </div>
        <div class="actions">
          <p class="status ${state.error ? "error" : ""}">${escapeHtml(state.error ?? (state.account ? "Uses the cached ChatGPT sign-in from Codex CLI. No API key is requested or stored here." : "Run codex login in a terminal, then restart this client."))}</p>
          <button class="primary" type="submit" ${state.busy || !state.account ? "disabled" : ""}>${state.busy ? "Opening…" : "Open workspace"}</button>
        </div>
      </form>
    </main>`;
}

function choice(group, value, label, detail, selected) {
  return `<button class="choice" type="button" data-choice-group="${group}" data-choice-value="${value}" data-selected="${value === selected}"><strong>${label}</strong><small>${detail}</small></button>`;
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
