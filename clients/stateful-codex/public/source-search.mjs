import { findUnavailableRoots } from "./source-availability.mjs";

// Source search and map refresh for the workspace. Each search supersedes earlier ones; a map
// refresh invalidates results found against the old index and repeats the newest submitted
// search, even one submitted while the refresh was running.
//
// Dependencies: `state` (the workspace state), `rpc`, `action` (runs an operation with a busy
// notice), `render(sections)`, `refresh()` (the workspace refresh), and `projectId`.
export function createSourceSearch({ state, rpc, action, render, refresh, projectId }) {
  // A failure is shown beside the results, which are cleared rather than left describing an
  // index they may no longer match. A response from an older search is ignored.
  async function search(text, { replay = false } = {}) {
    const generation = ++state.searchGeneration;
    state.latestQuery = text;
    try {
      const response = await action("Searching source map", () =>
        rpc("contextMap/query", { projectId, text, limit: 20 }),
      );
      if (generation !== state.searchGeneration) return;
      state.contextHits = response.data;
      state.lastSearch = text;
      state.searchError = null;
    } catch (error) {
      if (generation !== state.searchGeneration) return;
      state.contextHits = [];
      state.lastSearch = null;
      state.searchError = replay
        ? `The map was refreshed, but repeating the search for “${text}” failed (${error.message}). Search again.`
        : `The search failed (${error.message}). Try again.`;
    }
    render(["routing-results"]);
  }

  // Re-checks the folders first, so a restored folder can be indexed again. A workspace
  // refresh failure after a successful re-index is reported after the search is repeated.
  async function refreshMap() {
    state.unavailableRoots = await findUnavailableRoots(rpc, state.project?.roots);
    render(["header", "intelligence", "routing-results", "findings"]);
    const roots = state.project?.roots ?? [];
    if (roots.length && state.unavailableRoots.size === roots.length) {
      state.error =
        "The project folder is missing, so the source map can't be refreshed. Restore the folder at its recorded path first.";
      render(["notices"]);
      return;
    }
    let workspaceError = null;
    await action("Refreshing source map", async () => {
      await rpc("contextMap/refresh", { projectId });
      // Results and source errors from before the re-index no longer describe it.
      state.contextHits = [];
      state.evidenceError = null;
      render(["routing-results", "findings"]);
      await refresh().catch((error) => {
        workspaceError = error;
      });
    });
    if (state.latestQuery) await search(state.latestQuery, { replay: true });
    if (workspaceError) throw workspaceError;
  }

  return { search, refreshMap };
}
