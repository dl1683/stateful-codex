// A small, safe Markdown renderer for agent prose. All source text is escaped before any markup
// is added, so the output contains only the elements produced here. Supported: paragraphs with
// line breaks, ATX headings, nested bullet and numbered lists, block quotes, fenced code,
// pipe tables, horizontal rules, and inline code, bold, italic and links. Web links open in a
// new tab; any other link target (usually a local file path) is shown as a path, because the
// browser cannot open the agent's filesystem.
//
// Agent output is untrusted and can be large, so every step is linear in its input: no regular
// expression can backtrack across a line, each loop consumes input, nesting is capped, and
// tables are bounded (content beyond the bounds stays visible as plain text).

const MAX_NESTING = 8;
const MAX_TABLE_COLUMNS = 20;
const MAX_TABLE_CELLS = 2_000;
// How far a link's label or destination may extend; longer brackets are left as text.
const MAX_LINK_PART = 512;

export function renderMarkdown(source) {
  const lines = String(source ?? "")
    .replace(/\r\n?|[\u2028\u2029]/g, "\n")
    .split("\n");
  return renderBlocks(lines, 0);
}

function renderBlocks(lines, depth) {
  const blocks = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index];
    if (!line.trim()) {
      index += 1;
      continue;
    }
    const fence = fenceOpening(line);
    if (fence) {
      const body = [];
      index += 1;
      while (index < lines.length && !lines[index].trimStart().startsWith(fence.marker)) {
        body.push(lines[index]);
        index += 1;
      }
      index += 1;
      const language = fence.language ? ` data-language="${escapeHtml(fence.language)}"` : "";
      blocks.push(`<pre class="md-code"${language}><code>${escapeHtml(body.join("\n"))}</code></pre>`);
      continue;
    }
    const heading = headingOf(line);
    if (heading) {
      // Panels already use h2, so prose headings start one level below.
      const level = Math.min(heading.level + 2, 6);
      blocks.push(`<h${level} class="md-heading">${renderInline(heading.text)}</h${level}>`);
      index += 1;
      continue;
    }
    if (isRule(line)) {
      blocks.push("<hr/>");
      index += 1;
      continue;
    }
    if (isTableStart(lines, index)) {
      index = renderTable(lines, index, blocks);
      continue;
    }
    if (isQuote(line)) {
      const quoted = [];
      while (index < lines.length && isQuote(lines[index])) {
        quoted.push(lines[index].replace(/^ {0,3}> ?/, ""));
        index += 1;
      }
      blocks.push(
        depth < MAX_NESTING
          ? `<blockquote>${renderBlocks(quoted, depth + 1)}</blockquote>`
          : `<blockquote>${renderParagraph(quoted)}</blockquote>`,
      );
      continue;
    }
    if (listMarker(line)) {
      index = renderList(lines, index, blocks, depth);
      continue;
    }
    // A paragraph always takes its first line, so the loop always advances.
    const paragraph = [line];
    index += 1;
    while (index < lines.length && lines[index].trim() && !startsBlock(lines, index)) {
      paragraph.push(lines[index]);
      index += 1;
    }
    blocks.push(renderParagraph(paragraph));
  }
  return blocks.join("");
}

function renderParagraph(lines) {
  return `<p>${lines.map((line) => renderInline(line.trim())).join("<br/>")}</p>`;
}

function fenceOpening(line) {
  const match = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(line);
  if (!match) return null;
  const language = match[2].trim().split(/\s/, 1)[0];
  if (match[1][0] === "`" && language.includes("`")) return null;
  return { marker: match[1], language };
}

function headingOf(line) {
  const match = /^ {0,3}(#{1,6})(?:[ \t]+(.*))?$/.exec(line);
  if (!match) return null;
  // A closing run of # is decoration.
  const text = (match[2] ?? "").replace(/[ \t]+#+[ \t]*$/, "").replace(/^#+[ \t]*$/, "").trim();
  return { level: match[1].length, text };
}

function isRule(line) {
  const compact = line.replace(/[ \t]/g, "");
  return /^ {0,3}[-*_]/.test(line) && compact.length >= 3 && /^(-+|\*+|_+)$/.test(compact);
}

function isQuote(line) {
  return /^ {0,3}>/.test(line);
}

function startsBlock(lines, index) {
  const line = lines[index];
  return Boolean(
    fenceOpening(line) ||
      headingOf(line) ||
      isQuote(line) ||
      listMarker(line) ||
      isRule(line) ||
      isTableStart(lines, index),
  );
}

// A list item: its indentation, kind, and the column where its text starts.
function listMarker(line) {
  const match = /^( *)([-*+]|\d{1,9}[.)])[ \t]+(?=\S)/.exec(line);
  if (!match) return null;
  return {
    indent: match[1].length,
    kind: /\d/.test(match[2]) ? "ordered" : "bullet",
    contentStart: match[0].length,
  };
}

function indentOf(line) {
  return line.length - line.trimStart().length;
}

// Items at one indentation form a list. More deeply indented items belong to the item above
// them as a nested list; more deeply indented text continues it. Nesting deeper than the cap
// stays visible as continuation text.
function renderList(lines, start, blocks, depth) {
  const first = listMarker(lines[start]);
  const items = [];
  let index = start;
  while (index < lines.length) {
    const marker = listMarker(lines[index]);
    if (!marker || marker.kind !== first.kind || marker.indent > first.indent + 1) break;
    if (marker.indent < first.indent) break;
    const text = [lines[index].slice(marker.contentStart)];
    const children = [];
    index += 1;
    while (index < lines.length) {
      const line = lines[index];
      if (!line.trim()) {
        // A blank line ends the item unless indented content follows the blank run.
        let next = index + 1;
        while (next < lines.length && !lines[next].trim()) next += 1;
        if (next >= lines.length || indentOf(lines[next]) <= first.indent) break;
        index = next;
        continue;
      }
      if (indentOf(line) <= first.indent) break;
      const nested = listMarker(line);
      if (nested && depth < MAX_NESTING) {
        const nestedBlocks = [];
        index = renderList(lines, index, nestedBlocks, depth + 1);
        children.push(...nestedBlocks);
        continue;
      }
      text.push(line.trim());
      index += 1;
    }
    items.push(`<li>${text.map(renderInline).join("<br/>")}${children.join("")}</li>`);
  }
  const tag = first.kind === "ordered" ? "ol" : "ul";
  blocks.push(`<${tag}>${items.join("")}</${tag}>`);
  return index;
}

function isTableStart(lines, index) {
  return (
    lines[index].includes("|") &&
    index + 1 < lines.length &&
    isTableSeparator(lines[index + 1])
  );
}

function isTableSeparator(line) {
  if (!line.includes("-")) return false;
  const cells = tableCells(line);
  return cells.length > 0 && cells.every((cell) => /^:?-{3,}:?$/.test(cell));
}

// Cells split on pipes outside code spans; an escaped pipe is part of the cell.
function tableCells(line) {
  let text = line.trim();
  if (text.startsWith("|")) text = text.slice(1);
  if (text.endsWith("|") && !text.endsWith("\\|")) text = text.slice(0, -1);
  const cells = [];
  let cell = "";
  let inCode = false;
  for (let position = 0; position < text.length; position += 1) {
    const character = text[position];
    if (character === "\\" && text[position + 1] === "|") {
      cell += "|";
      position += 1;
    } else if (character === "`") {
      inCode = !inCode;
      cell += character;
    } else if (character === "|" && !inCode) {
      cells.push(cell.trim());
      cell = "";
    } else {
      cell += character;
    }
  }
  cells.push(cell.trim());
  return cells;
}

// A table larger than its bounds keeps its excess rows as plain text below it.
function renderTable(lines, start, blocks) {
  const header = tableCells(lines[start]);
  const columns = Math.min(header.length, MAX_TABLE_COLUMNS);
  const maxRows = Math.max(Math.floor(MAX_TABLE_CELLS / columns) - 1, 0);
  const rows = [];
  let index = start + 2;
  while (index < lines.length && lines[index].includes("|") && lines[index].trim()) {
    if (rows.length >= maxRows) break;
    rows.push(tableCells(lines[index]));
    index += 1;
  }
  const cellsOf = (cells) => {
    const shown = cells.slice(0, columns);
    while (shown.length < columns) shown.push("");
    // Extra cells are kept in the last column rather than dropped.
    if (cells.length > columns) shown[columns - 1] = cells.slice(columns - 1).join(" | ");
    return shown;
  };
  blocks.push(
    `<div class="md-table"><table><thead><tr>${cellsOf(header).map((cell) => `<th>${renderInline(cell)}</th>`).join("")}</tr></thead><tbody>${rows.map((row) => `<tr>${cellsOf(row).map((cell) => `<td>${renderInline(cell)}</td>`).join("")}</tr>`).join("")}</tbody></table></div>`,
  );
  const overflow = [];
  while (index < lines.length && lines[index].includes("|") && lines[index].trim()) {
    overflow.push(lines[index]);
    index += 1;
  }
  if (overflow.length) blocks.push(renderParagraph(overflow));
  return index;
}

// Code spans are cut out first so their content is never interpreted as other markup.
export function renderInline(text) {
  const source = String(text);
  let html = "";
  let plain = "";
  let position = 0;
  while (position < source.length) {
    const tick = source.indexOf("`", position);
    if (tick < 0) break;
    let run = tick;
    while (source[run] === "`") run += 1;
    const fence = source.slice(tick, run);
    const close = source.indexOf(fence, run);
    // An unmatched run of backticks is literal text.
    if (close < 0 || source[close + fence.length] === "`") {
      plain += source.slice(position, run);
      position = run;
      continue;
    }
    plain += source.slice(position, tick);
    html += renderLinks(plain);
    plain = "";
    const content = source.slice(run, close);
    html += `<code>${escapeHtml(content.length > 2 && content.startsWith(" ") && content.endsWith(" ") ? content.slice(1, -1) : content)}</code>`;
    position = close + fence.length;
  }
  return html + renderLinks(plain + source.slice(position));
}

// Links are found in one left-to-right pass over "[" and "](" tokens: a link's label is the
// text after the latest "[" before its "](", and every attempt is bounded.
function renderLinks(text) {
  let html = "";
  let start = 0;
  let open = -1;
  const tokens = /\[|\]\(/g;
  for (let token = tokens.exec(text); token; token = tokens.exec(text)) {
    if (token[0] === "[") {
      open = token.index;
      continue;
    }
    const link = open >= start ? linkAt(text, open, token.index) : null;
    if (!link) continue;
    html += renderEmphasis(text.slice(start, open));
    html += renderLink(link.label, link.target);
    start = link.end;
    open = -1;
    tokens.lastIndex = link.end;
  }
  return html + renderEmphasis(text.slice(start));
}

// [label](target), [label](<target with spaces>), [label](target "title"); targets may hold
// balanced parentheses. Returns null for anything else.
function linkAt(text, open, labelEnd) {
  if (labelEnd - open > MAX_LINK_PART) return null;
  const label = text.slice(open + 1, labelEnd);
  if (!label.trim() || label.includes("]")) return null;
  let position = labelEnd + 2;
  const limit = Math.min(text.length, position + MAX_LINK_PART);
  let target = "";
  if (text[position] === "<") {
    const close = text.indexOf(">", position + 1);
    if (close < 0 || close >= limit) return null;
    target = text.slice(position + 1, close);
    position = close + 1;
  } else {
    let depth = 0;
    const begin = position;
    for (; position < limit; position += 1) {
      const character = text[position];
      if (character === "(") depth += 1;
      else if (character === ")") {
        if (depth === 0) break;
        depth -= 1;
      } else if (character === " " || character === "\t") break;
    }
    target = text.slice(begin, position);
  }
  if (!target) return null;
  // An optional quoted title is accepted and not shown.
  if (text[position] === " ") {
    const quote = text.indexOf('"', position);
    const titleEnd = quote === position + 1 ? text.indexOf('"', quote + 1) : -1;
    if (titleEnd < 0 || titleEnd >= limit) return null;
    position = titleEnd + 1;
  }
  if (text[position] !== ")") return null;
  return { label, target, end: position + 1 };
}

function renderLink(label, target) {
  if (/^https?:\/\//i.test(target)) {
    return `<a href="${escapeHtml(target)}" target="_blank" rel="noopener noreferrer">${renderEmphasis(label)}</a>`;
  }
  // A local path: show the label, keep the full target available on hover.
  const path = target.replace(/^file:\/+/i, "");
  return `<code class="md-path" title="${escapeHtml(decodeTarget(path))}">${escapeHtml(label)}</code>`;
}

function decodeTarget(target) {
  try {
    return decodeURI(target);
  } catch {
    return target;
  }
}

// Bold (**text**) and italic (*text*): delimiters pair left to right, and a delimiter that
// cannot open or close (next to a space) stays literal. Underscores are never emphasis, so
// identifiers such as parse_iso_date read correctly.
function renderEmphasis(text) {
  return pairDelimiters(escapeHtml(text), "**", "strong", (inner) =>
    pairDelimiters(inner, "*", "em", (plain) => plain),
  );
}

function pairDelimiters(text, delimiter, tag, renderInner) {
  let html = "";
  let position = 0;
  while (position < text.length) {
    const open = findDelimiter(text, delimiter, position, "open");
    if (open < 0) break;
    const close = findDelimiter(text, delimiter, open + delimiter.length, "close");
    if (close < 0) break;
    html += renderInner(text.slice(position, open));
    html += `<${tag}>${renderInner(text.slice(open + delimiter.length, close))}</${tag}>`;
    position = close + delimiter.length;
  }
  return html + renderInner(text.slice(position));
}

// The next delimiter that can open (followed by non-space) or close (preceded by non-space).
// For a single asterisk, part of a double asterisk never counts.
function findDelimiter(text, delimiter, from, role) {
  let position = text.indexOf(delimiter, from);
  while (position >= 0) {
    const before = text[position - 1];
    const after = text[position + delimiter.length];
    const single = delimiter === "*" && (before === "*" || after === "*");
    const fits =
      role === "open"
        ? after !== undefined && !/\s/.test(after)
        : position > from && before !== undefined && !/\s/.test(before);
    if (fits && !single) return position;
    position = text.indexOf(delimiter, position + 1);
  }
  return -1;
}

function escapeHtml(value) {
  return String(value ?? "").replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[character],
  );
}
