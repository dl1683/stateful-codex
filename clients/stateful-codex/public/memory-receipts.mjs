// The lines the page shows when memory changes: one per newly saved entry, one per counted
// capture group (several rules from one message, or commits found in the workspace history).
// Commits are remembered from history; who made them is not known, so the line never says
// "you committed".

const RECEIPT_LABELS = {
  rule: "Saved your rule",
  pendingRule: "Saved a task-limited rule (not applied)",
  decision: "Saved a decision",
  recipe: "Saved a project recipe",
  finding: "Saved a finding",
  background: "Saved what you said about yourself",
  commit: "Remembered a commit from workspace history",
};

const PREVIEW_CHARACTERS = 160;
const LISTED_COMMITS = 3;

// One line for something newly saved; a repeat of what was already saved says nothing.
export function knowledgeReceipt(params) {
  if (!params || params.outcome !== "stored") return null;
  if (params.category === "commit") return commitLine(params.text);
  const label = RECEIPT_LABELS[params.category] ?? "Saved";
  return `${label}: "${clipText(params.text ?? "", PREVIEW_CHARACTERS)}"`;
}

// One line for a counted group, from its committed counts; null when it changed and lost
// nothing.
export function groupReceipt(params) {
  if (!params) return null;
  const saved = params.saved ?? 0;
  const pending = params.pending ?? 0;
  const lost = (params.omitted ?? 0) + (params.failed ?? 0);
  if (saved + pending === 0 && lost === 0) return null;
  const stored = (params.items ?? []).filter((item) => item.outcome === "stored");
  if (params.category === "commit") {
    if (saved === 1 && stored.length === 1) return commitLine(stored[0].text);
    const listed = stored
      .slice(0, LISTED_COMMITS)
      .map((item) => clipText(item.text ?? "", 80))
      .join("; ");
    const more = stored.length > LISTED_COMMITS ? `; and ${stored.length - LISTED_COMMITS} more` : "";
    return `Remembered ${saved} commits from workspace history: ${listed}${more}`;
  }
  const noun = params.category === "rule" || params.category === "pendingRule" ? "rule" : "entry";
  const parts = [];
  if (saved) parts.push(`Saved ${count(saved, noun)}`);
  if (pending) parts.push(`kept ${count(pending, "rule")} for this task only (not applied later)`);
  if (params.alreadyPresent) parts.push(`${params.alreadyPresent} already saved`);
  if (params.omitted) parts.push(`${params.omitted} too long to keep whole, not saved`);
  if (params.failed) parts.push(`${params.failed} could not be saved`);
  const previews = stored
    .map((item) => `"${clipText(item.text ?? "", 80)}"`)
    .join("; ");
  return `${capitalize(parts.join(", "))}${previews ? `: ${previews}` : ""}`;
}

// "<shortsha> <subject>" as one plain, truthful line.
function commitLine(text) {
  const value = String(text ?? "").trim();
  const space = value.indexOf(" ");
  const sha = space < 0 ? value : value.slice(0, space);
  const subject = space < 0 ? "" : value.slice(space + 1);
  return `Remembered commit ${sha}${subject ? `: ${clipText(subject, PREVIEW_CHARACTERS)}` : ""} · from workspace history`;
}

// At most `limit` characters, cut between code points (never inside a surrogate pair), with an
// ellipsis when cut. Section signs, dashes and curly quotes pass through unchanged.
export function clipText(text, limit) {
  const characters = Array.from(String(text ?? ""));
  if (characters.length <= limit) return characters.join("");
  return `${characters.slice(0, Math.max(limit - 1, 0)).join("").trimEnd()}…`;
}

function count(value, noun) {
  return `${value} ${noun === "entry" ? (value === 1 ? "entry" : "entries") : value === 1 ? noun : `${noun}s`}`;
}

function capitalize(text) {
  return text ? `${text[0].toUpperCase()}${text.slice(1)}` : text;
}
