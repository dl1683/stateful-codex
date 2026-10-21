import { createDrafts, fitsEntry, MAX_ENTRY_BYTES } from "./memory-drafts.mjs";

// All reads and mutations bind the project the user actually opened. Draft revisions never
// follow a refreshed item silently; the server remains authoritative for stale-write refusals.
export function createMemoryControls({ state, rpc, storage, newId = () => crypto.randomUUID() }) {
  const { projectId, threadId } = state;
  const binding = { threadId, expectedProjectId: projectId };
  const drafts = createDrafts(storage, threadId, projectId);
  const saving = new Set();
  let readSequence = 0;
  let wantedPages = 1;

  async function readPage(cursor) {
    const response = await rpc("statefulMemory/read", {
      ...binding, cursor, limit: 50, backgroundSection: true,
    });
    if (response.projectId !== projectId) throw new Error("Memory belongs to another project");
    return response;
  }

  const controls = {
    drafts,
    async refresh() {
      const sequence = ++readSequence;
      try {
        // A revision change between pages gets one fresh read, never a partial mixed list.
        for (let attempt = 0; attempt < 2; attempt += 1) {
          try {
            const items = [];
            let cursor = null;
            let pages = 0;
            do {
              const response = await readPage(cursor);
              items.push(...response.data);
              cursor = response.nextCursor;
              pages += 1;
            } while (cursor && pages < wantedPages);
            if (sequence === readSequence) state.memory = { items, cursor, pages, error: null };
            break;
          } catch (error) {
            if (attempt || error.code !== -32602 || !/memory changed/.test(error.message)) throw error;
          }
        }
      } catch (error) {
        if (sequence === readSequence) state.memory = { items: [], error: error.message };
      }
    },
    async more() {
      const memory = state.memory;
      if (!memory?.cursor) return;
      wantedPages = memory.pages + 1;
      const sequence = ++readSequence;
      try {
        const response = await readPage(memory.cursor);
        if (sequence !== readSequence) return;
        const known = new Set(memory.items.map((item) => item.entryId));
        state.memory = { items: [...memory.items, ...response.data.filter((item) => !known.has(item.entryId))],
          cursor: response.nextCursor, pages: wantedPages, error: null };
      } catch (error) {
        if (sequence !== readSequence) return;
        if (error.code !== -32602 || !/memory changed/.test(error.message)) throw error;
        await controls.refresh();
        state.notice = "Project memory changed; the opened pages were read again.";
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
    async add() {
      const addition = drafts.addition();
      const content = addition.content.trim();
      const reason = addition.kind === "decision" ? addition.reason.trim() || null : null;
      if (!content || !fitsEntry(reason ? `${content} Reason: ${reason}` : content))
        throw new Error(`Nothing added: text with its reason must fit ${MAX_ENTRY_BYTES} UTF-8 bytes; your text is kept`);
      if (!addition.actionId) {
        addition.actionId = newId();
        drafts.editAddition("actionId", addition.actionId);
      }
      if (saving.has(addition.actionId)) throw new Error("This addition is already saving");
      saving.add(addition.actionId);
      try {
        const response = await rpc("statefulMemory/add", {
          ...binding, kind: addition.kind, content, reason,
          clientActionId: addition.actionId, backgroundSection: true,
        });
        drafts.settleAddition(addition.actionId);
        state.notice = response.outcome === "added" ? "Added to project memory." : "Already saved in project memory.";
      } finally {
        saving.delete(addition.actionId);
      }
    },
  };
  return controls;
}
