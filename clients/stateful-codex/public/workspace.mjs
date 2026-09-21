const app = document.querySelector("#app");
const projectId = sessionStorage.getItem("stateful-project");
const threadId = sessionStorage.getItem("stateful-thread");
const mode = sessionStorage.getItem("stateful-mode");

if (!projectId || !threadId || !mode) {
  window.location.replace("/");
} else {
  app.innerHTML = `
    <main class="setup-shell">
      <p class="eyebrow">Stateful Codex · ${escapeHtml(mode)}</p>
      <h1>Your project workspace is connected.</h1>
      <p class="lede">Project ${escapeHtml(projectId)} is attached to thread ${escapeHtml(threadId)}. Loading the semantic workspace…</p>
      <p><a href="/">Change project, thread, or mode</a></p>
    </main>`;
}

function escapeHtml(value) {
  return String(value).replace(
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
