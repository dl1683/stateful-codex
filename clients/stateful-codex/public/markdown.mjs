// A small, safe Markdown renderer for agent prose. All source text is escaped before any markup
// is added, so the output contains only the elements produced here. Supported: paragraphs with
// line breaks, ATX headings, bullet and numbered lists, block quotes, fenced code, simple
// pipe tables, horizontal rules, and inline code, bold, italic and links. Web links open in a
// new tab; any other link target (usually a local file path) is shown as a path, because the
// browser cannot open the agent's filesystem.
export function renderMarkdown(source) {
  const lines = String(source ?? "").replace(/\r\n?/g, "\n").split("\n");
  const blocks = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index];
    const fence = line.match(/^\s{0,3}(`{3,}|~{3,})\s*([^`\s]*)/);
    if (fence) {
      const body = [];
      index += 1;
      while (index < lines.length && !lines[index].trimStart().startsWith(fence[1])) {
        body.push(lines[index]);
        index += 1;
      }
      index += 1;
      const language = fence[2] ? ` data-language="${escapeHtml(fence[2])}"` : "";
      blocks.push(`<pre class="md-code"${language}><code>${escapeHtml(body.join("\n"))}</code></pre>`);
      continue;
    }
    if (!line.trim()) {
      index += 1;
      continue;
    }
    const heading = line.match(/^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/);
    if (heading) {
      // Panels already use h2, so prose headings start one level below.
      const level = Math.min(heading[1].length + 2, 6);
      blocks.push(`<h${level} class="md-heading">${renderInline(heading[2])}</h${level}>`);
      index += 1;
      continue;
    }
    if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) {
      blocks.push("<hr/>");
      index += 1;
      continue;
    }
    if (isTableStart(lines, index)) {
      const header = tableCells(line);
      const rows = [];
      index += 2;
      while (index < lines.length && lines[index].includes("|") && lines[index].trim()) {
        rows.push(tableCells(lines[index]));
        index += 1;
      }
      blocks.push(
        `<div class="md-table"><table><thead><tr>${header.map((cell) => `<th>${renderInline(cell)}</th>`).join("")}</tr></thead><tbody>${rows.map((row) => `<tr>${header.map((_, column) => `<td>${renderInline(row[column] ?? "")}</td>`).join("")}</tr>`).join("")}</tbody></table></div>`,
      );
      continue;
    }
    if (/^\s{0,3}>/.test(line)) {
      const quoted = [];
      while (index < lines.length && /^\s{0,3}>/.test(lines[index])) {
        quoted.push(lines[index].replace(/^\s{0,3}>\s?/, ""));
        index += 1;
      }
      blocks.push(`<blockquote>${renderMarkdown(quoted.join("\n"))}</blockquote>`);
      continue;
    }
    const listKind = listItemKind(line);
    if (listKind) {
      const items = [];
      while (index < lines.length && listItemKind(lines[index]) === listKind) {
        const text = [lines[index].replace(LIST_MARKERS[listKind], "")];
        index += 1;
        // Indented continuation lines (including nested items) stay with their item.
        while (
          index < lines.length &&
          lines[index].trim() &&
          /^\s{2,}/.test(lines[index]) &&
          listItemKind(lines[index].trimStart()) === null
        ) {
          text.push(lines[index].trim());
          index += 1;
        }
        items.push(text.map(renderInline).join("<br/>"));
        // A nested list is flattened into its own run of items at the same level.
        while (index < lines.length && /^\s{2,}/.test(lines[index]) && listItemKind(lines[index].trimStart())) {
          items.push(renderInline(lines[index].trimStart().replace(/^([-*+]|\d{1,9}[.)])\s+/, "")));
          index += 1;
        }
      }
      const tag = listKind === "ordered" ? "ol" : "ul";
      blocks.push(`<${tag}>${items.map((item) => `<li>${item}</li>`).join("")}</${tag}>`);
      continue;
    }
    const paragraph = [];
    while (index < lines.length && lines[index].trim() && !startsBlock(lines, index)) {
      paragraph.push(lines[index].trim());
      index += 1;
    }
    blocks.push(`<p>${paragraph.map(renderInline).join("<br/>")}</p>`);
  }
  return blocks.join("");
}

const LIST_MARKERS = {
  bullet: /^\s{0,3}[-*+]\s+/,
  ordered: /^\s{0,3}\d{1,9}[.)]\s+/,
};

function listItemKind(line) {
  if (LIST_MARKERS.bullet.test(line)) return "bullet";
  if (LIST_MARKERS.ordered.test(line)) return "ordered";
  return null;
}

function startsBlock(lines, index) {
  const line = lines[index];
  return (
    /^\s{0,3}(`{3,}|~{3,}|#{1,6}\s|>)/.test(line) ||
    listItemKind(line) !== null ||
    isTableStart(lines, index)
  );
}

function isTableStart(lines, index) {
  return (
    lines[index].includes("|") &&
    index + 1 < lines.length &&
    /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?\s*$/.test(lines[index + 1])
  );
}

function tableCells(line) {
  return line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((cell) => cell.trim());
}

// Code spans are cut out first so their content is never interpreted as other markup.
export function renderInline(text) {
  const parts = String(text).split(/(`+[^`]*?`+)/);
  return parts
    .map((part, index) =>
      index % 2 === 1 && /^(`+)[^`]*\1$/.test(part)
        ? `<code>${escapeHtml(part.replace(/^`+|`+$/g, ""))}</code>`
        : renderEmphasisAndLinks(part),
    )
    .join("");
}

function renderEmphasisAndLinks(text) {
  let html = "";
  let rest = text;
  const link = /\[([^\]]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/;
  for (let match = rest.match(link); match; match = rest.match(link)) {
    html += renderEmphasis(rest.slice(0, match.index));
    html += renderLink(match[1], match[2]);
    rest = rest.slice(match.index + match[0].length);
  }
  return html + renderEmphasis(rest);
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

function renderEmphasis(text) {
  return escapeHtml(text)
    .replace(/\*\*(?=\S)(.+?)(?<=\S)\*\*/g, "<strong>$1</strong>")
    .replace(/__(?=\S)(.+?)(?<=\S)__/g, "<strong>$1</strong>")
    .replace(/(^|[^*\w])\*(?=\S)([^*]+?)(?<=\S)\*(?![*\w])/g, "$1<em>$2</em>")
    .replace(/(^|[^_\w])_(?=\S)([^_]+?)(?<=\S)_(?![_\w])/g, "$1<em>$2</em>");
}

function escapeHtml(value) {
  return String(value ?? "").replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[character],
  );
}
