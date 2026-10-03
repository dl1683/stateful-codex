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

export function createDrafts(storage, threadId) {
  const key = `stateful-memory-drafts:${threadId}`;
  let drafts = load(storage, key);

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
      return drafts.corrections[entryId] ?? null;
    },
    corrections() {
      return { ...drafts.corrections };
    },
    // Opens a correction from the entry as it was shown, unless one is already open.
    open(entryId, baseRevision, content) {
      if (!drafts.corrections[entryId]) {
        drafts.corrections[entryId] = { baseRevision, content: bounded(content) };
        save();
      }
    },
    edit(entryId, content) {
      const draft = drafts.corrections[entryId];
      if (!draft) return;
      draft.content = bounded(content);
      save();
    },
    close(entryId) {
      delete drafts.corrections[entryId];
      save();
    },
    addition() {
      return drafts.addition;
    },
    // Changing what is to be added makes it a new action: a retry of the earlier words
    // (which may have been saved before a timeout) never carries the new ones.
    editAddition(field, value) {
      drafts.addition = { ...drafts.addition, [field]: bounded(String(value)) };
      if (field !== "actionId") delete drafts.addition.actionId;
      save();
    },
    clearAddition() {
      drafts.addition = emptyAddition();
      save();
    },
  };
}

function load(storage, key) {
  try {
    const stored = JSON.parse(storage?.getItem(key) ?? "null");
    if (stored && typeof stored === "object" && stored.corrections && stored.addition) {
      return stored;
    }
  } catch {
    // A damaged or missing record starts empty.
  }
  return { corrections: {}, addition: emptyAddition() };
}

function emptyAddition() {
  return { kind: "rule", content: "", reason: "" };
}

function bounded(text) {
  return String(text);
}
