import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { validateStateEvidenceAssertions } from "./validate-state-evidence.mjs";

test("requires decisive terms to occur inside the persisted evidence range", async () => {
  const workspace = await mkdtemp(path.join(tmpdir(), "stateful-evidence-"));
  await writeFile(
    path.join(workspace, "policy.md"),
    "# Policy\nLaunch requires approval.\nThreshold is 10 or\nfewer.\n",
  );
  const source = await readFile(path.join(workspace, "policy.md"));
  const fingerprint = `sha256:${createHash("sha256").update(source).digest("hex")}`;
  const state = fixtureState(workspace, fingerprint);
  const benchmarkCase = {
    id: "learn-v1",
    stateEvidenceAssertions: [
      {
        name: "threshold",
        entryTermGroups: [["10"]],
        sourceTermGroups: [["10 or fewer"]],
      },
    ],
  };

  await assert.rejects(
    validateStateEvidenceAssertions({ state, benchmarkCase, workspace }),
    /no current sourceVerified blackboard entry/,
  );
  state.blackboard.evidenceLinks[0].lastLine = 4;
  assert.deepEqual(
    await validateStateEvidenceAssertions({ state, benchmarkCase, workspace }),
    [
      {
        name: "threshold",
        entryId: "threshold-entry",
        revision: 1,
        contextMapEntryId: "policy-route",
        relativePath: "policy.md",
        firstLine: 2,
        lastLine: 4,
      },
    ],
  );
});

function fixtureState(workspace, fingerprint) {
  return {
    hierarchy: [
      {
        id: "policy-node",
        projectRoot: workspace,
        relativePath: "policy.md",
        sourceFingerprint: fingerprint,
        lifecycle: "active",
      },
    ],
    contextMap: [
      {
        id: "policy-route",
        nodeId: "policy-node",
        sourceFingerprint: fingerprint,
      },
    ],
    blackboard: {
      entries: [{ id: "threshold-entry", revision: 1 }],
      revisions: [
        {
          entryId: "threshold-entry",
          revision: 1,
          verification: "source_verified",
          content: "The controlling threshold is 10.",
        },
      ],
      evidenceLinks: [
        {
          entryId: "threshold-entry",
          revision: 1,
          contextMapEntryId: "policy-route",
          sourceFingerprint: fingerprint,
          firstLine: 2,
          lastLine: 2,
        },
      ],
    },
  };
}
