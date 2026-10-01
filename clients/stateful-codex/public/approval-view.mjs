// Approval cards must show what is being approved. A file-change request carries only its item
// ID, so the changed paths and diff come from the matching fileChange item; a command request
// carries its command, which is shown without the shell wrapper and with its working directory.

export const DIFF_PREVIEW_LINES = 40;
export const DIFF_LINE_CHARACTERS = 400;
export const DIFF_FILE_CHARACTERS = 64 * 1024;
export const DIFF_FILES_WITH_PREVIEW = 50;

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
  const streaming = item.partial
    ? `<p class="banner">The patch is still being prepared. Approval stays disabled until the complete change is known.</p>`
    : "";
  const files = item.changes
    .map((change, index) => renderChange(change, index < DIFF_FILES_WITH_PREVIEW, index))
    .join("");
  const unpreviewed =
    item.changes.length > DIFF_FILES_WITH_PREVIEW
      ? `<p class="microcopy">Diffs are previewed for the first ${DIFF_FILES_WITH_PREVIEW} files; every changed path is listed.</p>`
      : "";
  const truncated = item.changes.some(
    (change, index) =>
      change.diffTruncated ||
      (index < DIFF_FILES_WITH_PREVIEW && exceedsDisplay(change.diff)),
  )
    ? `<p class="banner">Part of this change is too large to show here. Review the full change before approving.</p>`
    : "";
  return {
    ready: !item.partial,
    html: `<p><strong>File change · ${item.changes.length} ${item.changes.length === 1 ? "file" : "files"}</strong></p>${reason}${root}${streaming}${truncated}${files}${unpreviewed}`,
  };
}

export function renderCommandApproval(request) {
  const { command, cwd, reason } = request.params;
  const shown = command ? displayCommand(command) : null;
  return `<p><strong>Run command</strong></p>${reason ? `<p>${escapeHtml(reason)}</p>` : ""}${shown ? `<pre class="approval-command">${escapeHtml(shown)}</pre>` : `<p class="banner">The agent requests approval to continue; no command was supplied.</p>`}${cwd ? `<p class="path">in ${escapeHtml(cwd)}</p>` : ""}${shown && shown !== command ? `<details><summary data-disclosure="exact-command">Exact command line</summary><pre class="approval-command">${escapeHtml(command)}</pre></details>` : ""}`;
}

const POWERSHELL = new Set(["pwsh", "powershell"]);
const POSIX_SHELLS = new Set(["bash", "sh", "zsh"]);

// The server sends shell-joined argv. When it is a shell running one script
// ("pwsh -NoProfile -Command <script>", "bash -lc <script>"), show the script itself;
// anything else, including unparseable text, is shown exactly as sent.
export function displayCommand(command) {
  const words = splitShellWords(command);
  if (!words || words.length < 3) return command;
  const program = words[0]
    .split(/[\\/]/)
    .at(-1)
    .toLowerCase()
    .replace(/\.exe$/, "");
  if (POWERSHELL.has(program)) {
    const flag = words.findIndex((word, index) => index > 0 && /^-(c|command)$/i.test(word));
    const options = words.slice(1, flag);
    if (flag > 0 && flag === words.length - 2 && options.every((word) => word.startsWith("-"))) {
      return words[flag + 1];
    }
    return command;
  }
  if (POSIX_SHELLS.has(program) && words.length === 3 && /^-[a-z]*c$/.test(words[1])) {
    return words[2];
  }
  return command;
}

// POSIX shell word splitting (single quotes, double quotes with backslash escapes, and
// backslash escapes). Returns null for unterminated quoting.
export function splitShellWords(text) {
  const words = [];
  let word = null;
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (/\s/.test(character)) {
      if (word !== null) words.push(word);
      word = null;
      continue;
    }
    word ??= "";
    if (character === "'") {
      const end = text.indexOf("'", index + 1);
      if (end < 0) return null;
      word += text.slice(index + 1, end);
      index = end;
    } else if (character === '"') {
      index += 1;
      for (; index < text.length && text[index] !== '"'; index += 1) {
        if (text[index] === "\\" && '$`"\\\n'.includes(text[index + 1] ?? "")) index += 1;
        word += text[index];
      }
      if (index >= text.length) return null;
    } else if (character === "\\") {
      index += 1;
      if (index >= text.length) return null;
      word += text[index];
    } else {
      word += character;
    }
  }
  if (word !== null) words.push(word);
  return words;
}

function exceedsDisplay(diff) {
  const text = String(diff ?? "");
  return (
    text.length > DIFF_FILE_CHARACTERS ||
    text.split("\n").some((line) => line.length > DIFF_LINE_CHARACTERS)
  );
}

function renderChange(change, withPreview, index) {
  const kind = change.kind?.type ?? "update";
  const label = { add: "Add", delete: "Delete", update: "Edit" }[kind] ?? kind;
  const moved = change.kind?.move_path ?? change.kind?.movePath;
  const heading = `<p><span class="badge">${escapeHtml(label)}</span> <code>${escapeHtml(change.path)}</code>${moved ? ` → <code>${escapeHtml(moved)}</code>` : ""}</p>`;
  if (!withPreview) return `<section class="approval-file">${heading}</section>`;
  const lines = String(change.diff ?? "")
    .slice(0, DIFF_FILE_CHARACTERS)
    .split("\n")
    .map((line) =>
      line.length > DIFF_LINE_CHARACTERS ? `${line.slice(0, DIFF_LINE_CHARACTERS)} …` : line,
    );
  const preview = lines.slice(0, DIFF_PREVIEW_LINES).join("\n");
  const rest =
    lines.length > DIFF_PREVIEW_LINES
      ? `<details><summary data-disclosure="diff-${index}">Show all ${lines.length} lines</summary><pre class="approval-diff">${escapeHtml(lines.join("\n"))}</pre></details>`
      : "";
  return `<section class="approval-file">${heading}<pre class="approval-diff">${escapeHtml(preview)}</pre>${rest}</section>`;
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
