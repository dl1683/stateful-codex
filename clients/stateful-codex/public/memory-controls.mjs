import { createDrafts, fitsEntry, MAX_ENTRY_BYTES } from "./memory-drafts.mjs";

// All reads and mutations bind the project the user actually opened. Draft revisions never
// follow a refreshed item silently; the server remains authoritative for stale-write refusals.
export function createMemoryControls({ state, rpc, storage }) {
  const { projectId, threadId } = state;
  const binding = { threadId, expectedProjectId: projectId };
  const drafts = createDrafts(storage, threadId, projectId);
  const saving = new Set();
  let readSequence = 0;

  return {
    drafts,
    async refresh() {
      const sequence = ++readSequence;
      try {
        const response = await rpc("statefulMemory/read", {
          ...binding, cursor: null, limit: 50, backgroundSection: true,
        });
        if (response.projectId !== projectId) throw new Error("Memory belongs to another project");
        if (sequence === readSequence) state.memory = {
          items: response.data, cursor: response.nextCursor, pages: 1, error: null,
        };
      } catch (error) {
        if (sequence === readSequence) state.memory = { items: [], error: error.message };
      }
    },
    open(entryId) {
      const item = state.memory?.items.find((entry) => entry.entryId === entryId);
      if (!item || item.contentTruncated) throw new Error("An excerpt cannot be corrected");
      drafts.open(entryId, item.revision, item.content);
    },
    async correct(entryId) {
      const draft = drafts.correction(entryId);
      const item = state.memory?.items.find((entry) => entry.entryId === entryId);
      if (!draft || !item || item.contentTruncated) throw new Error("Read the complete entry before correcting");
      const content = draft.content.trim();
      if (!content || !fitsEntry(content)) throw new Error(`Nothing saved: use 1-${MAX_ENTRY_BYTES} UTF-8 bytes; your text is kept`);
      if (saving.has(entryId)) throw new Error("This correction is already saving");
      saving.add(entryId);
      try {
        const response = await rpc("statefulMemory/correct", {
          ...binding, entryId, expectedRevision: draft.baseRevision, content, backgroundSection: true,
        });
        const current = drafts.correction(entryId);
        if (current?.baseRevision === draft.baseRevision && current.content.trim() === content) drafts.close(entryId);
        state.notice = `Corrected: "${response.item.content}".`;
      } finally {
        saving.delete(entryId);
      }
    },
    async forget(entryId, expectedRevision) {
      await rpc("statefulMemory/forget", { ...binding, entryId, expectedRevision });
      state.notice = "Forgotten. The entry stays in history but no longer applies.";
    },
  };
}
