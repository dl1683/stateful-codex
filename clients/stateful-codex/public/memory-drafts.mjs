// Corrections and additions the person is writing, kept apart from the panel that is
// re-rendered on every refresh: a draft survives refreshes, live events and a page reload in
// the same tab (session storage), and is cleared only after that draft is saved or the person
// cancels it. A correction draft remembers the revision it started from, so a change made
// elsewhere meanwhile is shown as a conflict instead of being overwritten.

// The most the server keeps for one entry; a longer draft is kept whole and refused on
// submit, never cut short.
export const MAX_ENTRY_BYTES = 2000;

// Whether `text` fits one entry (counted in UTF-8 bytes, as the server counts it).
export function fitsEntry(text) {
  return new TextEncoder().encode(text).length <= MAX_ENTRY_BYTES;
}

export function createDrafts(storage, threadId, projectId) {
  const key = `stateful-memory-drafts:${JSON.stringify([projectId, threadId])}`;
  const drafts = load(storage, key);

  const save = () => {
    try {
      storage?.setItem(key, JSON.stringify(drafts));
    } catch {
      // Storage may be full or unavailable; the in-memory drafts still survive re-renders.
    }
  };

  return {
    // The correction draft for an entry, if one is open.
    correction(entryId) {
      const draft = drafts.corrections[entryId];
      return draft ? { ...draft } : null;
    },
    corrections() {
      return { ...drafts.corrections };
    },
    // Opens a correction from the entry as it was shown, unless one is already open.
    open(entryId, baseRevision, content) {
      if (!drafts.corrections[entryId]) {
        drafts.corrections[entryId] = { baseRevision, content: String(content) };
        save();
      }
    },
    edit(entryId, content) {
      const draft = drafts.corrections[entryId];
      if (!draft) return;
      draft.content = String(content);
      save();
    },
    close(entryId) {
      delete drafts.corrections[entryId];
      save();
    },
    addition() {
      return { ...drafts.addition };
    },
    // Changing what is to be added makes it a new action: a retry of the earlier words
    // (which may have been saved before a timeout) never carries the new ones.
    editAddition(field, value) {
      const before = request(drafts.addition);
      drafts.addition = { ...drafts.addition, [field]: String(value) };
      if (field !== "actionId" && request(drafts.addition) !== before) {
        delete drafts.addition.actionId;
      }
      save();
    },
    // Clears the addition only if it is still the one submitted under `actionId`.
    settleAddition(actionId) {
      if (drafts.addition.actionId !== actionId) return false;
      drafts.addition = emptyAddition();
      save();
      return true;
    },
    clearAddition() {
      drafts.addition = emptyAddition();
      save();
    },
  };
}

function load(storage, key) {
  const restored = { corrections: Object.create(null), addition: emptyAddition() };
  try {
    const stored = JSON.parse(storage?.getItem(key) ?? "null");
    if (stored?.corrections && typeof stored.corrections === "object" && !Array.isArray(stored.corrections)) {
      for (const [entryId, draft] of Object.entries(stored.corrections)) {
        if (draft && Number.isSafeInteger(draft.baseRevision) && draft.baseRevision > 0 &&
            typeof draft.content === "string") {
          restored.corrections[entryId] = { baseRevision: draft.baseRevision, content: draft.content };
        }
      }
    }
    const addition = stored?.addition;
    if (addition && ["rule", "background", "decision", "note"].includes(addition.kind) &&
        typeof addition.content === "string" && typeof addition.reason === "string") {
      restored.addition = { kind: addition.kind, content: addition.content, reason: addition.reason };
      if (typeof addition.actionId === "string" && addition.actionId.trim() &&
          new TextEncoder().encode(addition.actionId).length <= 128) {
        restored.addition.actionId = addition.actionId;
      }
    }
  } catch {
    // Unreadable storage starts empty; malformed members never hide valid saved drafts.
  }
  return restored;
}

// What an addition asks for, as the server compares it (trimmed words).
function request(addition) {
  return JSON.stringify([
    addition.kind,
    String(addition.content ?? "").trim(),
    addition.kind === "decision" ? String(addition.reason ?? "").trim() : "",
  ]);
}

function emptyAddition() {
  return { kind: "rule", content: "", reason: "" };
}
