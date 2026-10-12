import assert from "node:assert/strict";
import test from "node:test";

import { scoreProjectState } from "../eval/evaluate-project-state.mjs";

test("scores lexical probe recall separately from current evidence support", () => {
  const manifest = {
    name: "fixture",
    expectedConcepts: [
      {
        name: "decision",
        kinds: ["decision"],
        termGroups: [["Alder"], ["fails"]],
      },
    ],
    forbiddenPhrases: ["Alder passes"],
  };
  const supported = {
    entry: {
      id: "entry-1",
      kind: "decision",
      content: "Alder fails the gate.",
      state: "active",
      evidence: [{ contextMapEntryId: "source-1" }],
    },
    effectiveVerification: "sourceVerified",
    evidenceFreshness: "current",
  };

  assert.deepEqual(scoreProjectState([supported], manifest), {
    schemaVersion: 2,
    manifest: "fixture",
    completeSnapshot: true,
    entries: { total: 1, currentlySupported: 1, storedSupportEntryRatio: 1 },
    concepts: {
      total: 1,
      matched: 1,
      currentlySupported: 1,
      lexicalProbeRecall: 1,
      storedSupportLexicalProbeRecall: 1,
      probes: [
        { name: "decision", matched: true, entryId: "entry-1", supported: true },
      ],
    },
    forbiddenClaims: [],
    passed: true,
  });
});
