// Corrections and additions the person is writing, kept apart from the panel that is
// re-rendered on every refresh: a draft survives refreshes, live events and a page reload in
// the same tab (session storage), and is cleared only after that draft is saved or the person
// cancels it. A correction draft remembers the revision it started from, so a change made
// elsewhere meanwhile is shown as a conflict instead of being overwritten.

const MAX_DRAFT_CHARS = 2000;

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
    editAddition(field, value) {
      drafts.addition = { ...drafts.addition, [field]: bounded(String(value)) };
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
  return String(text).slice(0, MAX_DRAFT_CHARS);
}
