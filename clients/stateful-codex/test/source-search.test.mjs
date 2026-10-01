import assert from "node:assert/strict";
import test from "node:test";

import { createSourceSearch } from "../public/source-search.mjs";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

// A workspace with controllable RPC responses: each call waits until the test settles it.
function searchWorkspace({ refresh = async () => {} } = {}) {
  const state = {
    project: { roots: [{ path: "C:/work/project" }] },
    contextHits: [],
    evidenceError: { key: "hit:old", message: "old", rootUnavailable: false },
    searchGeneration: 0,
    latestQuery: null,
    lastSearch: null,
    searchError: null,
    unavailableRoots: new Set(),
  };
  const calls = [];
  const rendered = [];
  const rpc = (method, params) => {
    if (method === "fs/getMetadata") return Promise.resolve({ isDirectory: true });
    const call = { method, params, ...deferred() };
    calls.push(call);
    return call.promise;
  };
  const sources = createSourceSearch({
    state,
    rpc,
    action: (_label, operation) => operation(),
    render: (sections) => rendered.push(...sections),
    refresh,
    projectId: "project-1",
  });
  const settle = async () => new Promise((resolve) => setTimeout(resolve, 0));
  return { state, calls, rendered, sources, settle };
}

const hits = (query) => ({ data: [{ entryId: `${query}-hit` }] });

test("a search submitted during a map refresh is the one repeated, and its results win", async () => {
  const { state, calls, sources, settle } = searchWorkspace();
  const alpha = sources.search("alpha");
  calls[0].resolve(hits("alpha"));
  await alpha;

  const refreshing = sources.refreshMap();
  await settle();
  assert.equal(calls[1].method, "contextMap/refresh");
  const beta = sources.search("beta");
  calls[1].resolve({});
  await settle();
  // The repeat searches the newest query, superseding beta's in-flight request.
  assert.deepEqual(
    calls.slice(2).map((call) => [call.method, call.params.text]),
    [
      ["contextMap/query", "beta"],
      ["contextMap/query", "beta"],
    ],
  );
  calls[2].resolve(hits("stale-beta"));
  calls[3].resolve(hits("beta"));
  await Promise.all([beta, refreshing]);

  assert.equal(state.lastSearch, "beta");
  assert.deepEqual(state.contextHits, [{ entryId: "beta-hit" }]);
});

test("a re-index clears old results and errors even when the workspace refresh then fails", async () => {
  const { state, calls, rendered, sources, settle } = searchWorkspace({
    refresh: async () => {
      throw new Error("refresh failed");
    },
  });
  state.contextHits = [{ entryId: "old-hit" }];
  state.latestQuery = "alpha";

  const refreshing = sources.refreshMap();
  await settle();
  calls[0].resolve({});
  await settle();
  assert.deepEqual(state.contextHits, []);
  assert.equal(state.evidenceError, null);
  assert.ok(rendered.includes("findings"));

  // The search is still repeated; the refresh failure is reported afterwards.
  calls[1].reject(new Error("offline"));
  await assert.rejects(refreshing, /refresh failed/);
  assert.equal(
    state.searchError,
    "The map was refreshed, but repeating the search for \u201calpha\u201d failed (offline). Search again.",
  );
});
