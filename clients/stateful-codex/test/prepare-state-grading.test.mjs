import assert from "node:assert/strict";
import test from "node:test";

import { stateArtifactHash } from "../eval/export-project-state.mjs";
import { verifyStateArtifact } from "../eval/prepare-state-grading.mjs";

function fixture() {
  const state = {
    formatVersion: "stateful-project-state-v1",
    projectId: "project-1",
    intelligenceRevision: 12,
    run: { id: "run-1", revision: 3, result: "Completed result" },
    obligations: [{ id: "obligation-1" }],
    steering: [],
    hierarchy: [],
    contextMap: [],
    blackboard: { entries: [], revisions: [], evidenceLinks: [], relations: [] },
    counts: {},
    corpusRevision: "sha256:corpus",
    capturedAtMs: 123,
  };
  state.snapshotSha256 = stateArtifactHash(state);
  const turn = {
    corpusRevision: state.corpusRevision,
    stateArtifact: {
      snapshotSha256: state.snapshotSha256,
      capturedAtMs: state.capturedAtMs,
      intelligenceRevision: state.intelligenceRevision,
      runRevision: state.run.revision,
    },
  };
  return { state, turn };
}

test("accepts an exact revision-pinned state artifact", () => {
  const { state, turn } = fixture();
  assert.doesNotThrow(() => verifyStateArtifact(state, turn));
});

test("rejects state changed after the recorded turn", () => {
  const { state, turn } = fixture();
  state.run.result = "Mutated later result";
  assert.throws(
    () => verifyStateArtifact(state, turn),
    /content does not match its hash/,
  );
});
