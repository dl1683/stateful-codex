// Approval cards must show what is being approved. A file-change request carries only its item
// ID, so the changed paths and diff come from the matching fileChange item; a command request
// carries its command, which is shown without the shell wrapper and with its working directory.

export const DIFF_PREVIEW_LINES = 40;
export const DIFF_MAX_LINES = 2000;

export function renderFileChangeApproval(request, item) {
  const reason = request.params.reason
    ? `<p>${escapeHtml(request.params.reason)}</p>`
    : "";
  const root = request.params.grantRoot
    ? `<p class="banner">Also asks to allow writes under <code>${escapeHtml(request.params.grantRoot)}</code> for this session.</p>`
    : "";
  if (!item?.changes?.length) {
    return {
      ready: false,
      html: `<p><strong>File change</strong></p>${reason}${root}<p class="banner error">The changed files for this request are not available yet. Approval stays disabled until they can be shown.</p>`,
    };
  }
  const files = item.changes.map(renderChange).join("");
  return {
    ready: true,
    html: `<p><strong>File change · ${item.changes.length} ${item.changes.length === 1 ? "file" : "files"}</strong></p>${reason}${root}${files}`,
  };
}

export function renderCommandApproval(request) {
  const { command, cwd, reason } = request.params;
  const shown = command ? displayCommand(command) : null;
  return `<p><strong>Run command</strong></p>${reason ? `<p>${escapeHtml(reason)}</p>` : ""}${shown ? `<pre class="approval-command">${escapeHtml(shown)}</pre>` : `<p class="banner">The agent requests approval to continue; no command was supplied.</p>`}${cwd ? `<p class="path">in ${escapeHtml(cwd)}</p>` : ""}${shown && shown !== command ? `<details><summary>Exact command line</summary><pre class="approval-command">${escapeHtml(command)}</pre></details>` : ""}`;
}

// Strip a PowerShell or POSIX shell wrapper ("pwsh.exe -NoProfile -Command '...'",
// "bash -lc '...'") so the user reads the command that will actually do the work.
export function displayCommand(command) {
  const match = command.match(
    /^\s*(?:"[^"]*?(?:pwsh|powershell)(?:\.exe)?"|\S*?(?:pwsh|powershell)(?:\.exe)?)\s+(?:-(?!c\b|command\b)\w+\s+)*-(?:c|command)\s+([\s\S]+)$/i,
  ) ??
    command.match(
      /^\s*\S*?\/?(?:bash|sh|zsh)(?:\.exe)?\s+-\w*c\w*\s+([\s\S]+)$/i,
    );
  return match ? unquote(match[1].trim()) : command;
}

function unquote(text) {
  const quote = text[0];
  if ((quote === "'" || quote === '"') && text.at(-1) === quote && text.length > 1) {
    const inner = text.slice(1, -1);
    return quote === "'" ? inner.replaceAll("''", "'") : inner;
  }
  return text;
}

function renderChange(change) {
  const kind = change.kind?.type ?? "update";
  const label = { add: "Add", delete: "Delete", update: "Edit" }[kind] ?? kind;
  const moved = change.kind?.move_path ?? change.kind?.movePath;
  const lines = String(change.diff ?? "").split("\n");
  const bounded = lines.slice(0, DIFF_MAX_LINES);
  const preview = bounded.slice(0, DIFF_PREVIEW_LINES).join("\n");
  const rest =
    bounded.length > DIFF_PREVIEW_LINES
      ? `<details><summary>Show all ${bounded.length} lines</summary><pre class="approval-diff">${escapeHtml(bounded.join("\n"))}</pre>${lines.length > DIFF_MAX_LINES ? `<p class="microcopy">Diff truncated after ${DIFF_MAX_LINES} lines. Review the full change before approving.</p>` : ""}</details>`
      : "";
  return `<section class="approval-file"><p><span class="badge">${escapeHtml(label)}</span> <code>${escapeHtml(change.path)}</code>${moved ? ` → <code>${escapeHtml(moved)}</code>` : ""}</p><pre class="approval-diff">${escapeHtml(preview)}</pre>${rest}</section>`;
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
