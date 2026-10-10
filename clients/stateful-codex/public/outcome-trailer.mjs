// Presentation only: hides a trailing Stateful outcome block from assistant text. The raw
// message, events and history keep it unchanged. Mirrors the stateful_outcome module of
// codex-app-server-client, which the TUI and exec use.

const OPEN = "[stateful-outcome]";
const CLOSE = "[/stateful-outcome]";
const MAX_TRAILER_BYTES = 4096;

const bodyLine = (line) =>
  line.startsWith("disposition:") ||
  line.startsWith("open-issues:") ||
  line.startsWith("- ");

const byteLength = (text) => new TextEncoder().encode(text).length;

// Lines of `text` with their start offsets; `partial` marks a last line with no newline yet.
function lines(text) {
  const result = [];
  let start = 0;
  for (;;) {
    const end = text.indexOf("\n", start);
    if (end === -1) {
      if (start < text.length) {
        result.push({ start, line: text.slice(start), partial: true });
      }
      return result;
    }
    result.push({ start, line: text.slice(start, end), partial: false });
    start = end + 1;
  }
}

// Offset where the trailing outcome block starts, or -1. A complete message must end with a
// closed block (body lines, the close line, then only blank lines). A streaming prefix also
// withholds an open block whose lines so far fit, and a last line that may become its opener.
function trailerStart(text, streaming) {
  const all = lines(text);
  const tail = all.at(-1);
  const pendingOpener =
    streaming && tail?.partial && OPEN.startsWith(tail.line) ? tail.start : -1;
  let opener = -1;
  for (let index = all.length - 1; index >= 0; index -= 1) {
    const { line, partial } = all[index];
    if ((!partial || !streaming) && line.trimEnd() === OPEN) {
      opener = index;
      break;
    }
  }
  if (opener === -1 || byteLength(text.slice(all[opener].start)) > MAX_TRAILER_BYTES) {
    return pendingOpener;
  }
  let closed = false;
  for (const { line, partial } of all.slice(opener + 1)) {
    if (streaming && partial) break;
    const trimmed = line.trimEnd();
    if (closed) {
      if (trimmed.trim() !== "") return pendingOpener;
    } else if (trimmed === CLOSE) {
      closed = true;
    } else if (!bodyLine(trimmed)) {
      return pendingOpener;
    }
  }
  return closed || streaming ? all[opener].start : -1;
}

// `text` without its trailing outcome block. `streaming` marks a message still arriving.
export function visibleAnswer(text, { streaming = false } = {}) {
  const value = String(text ?? "");
  const start = trailerStart(value, streaming);
  return start === -1 ? value : value.slice(0, start).trimEnd();
}
