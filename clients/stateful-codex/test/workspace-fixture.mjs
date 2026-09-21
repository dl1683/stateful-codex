export function workspaceFixture() {
  return {
    project: {
      name: "Decisive detail investigation",
      roots: [{ path: "C:/work/investigation" }],
    },
    run: {
      id: "run-1",
      mode: "autonomous",
      status: "running",
      goal: "Resolve the contradiction and preserve exact evidence.",
      continuationsUsed: 3,
      budget: { maxContinuations: 24, maxElapsedSeconds: 14400 },
      strategy:
        "Verify the changed clause, then test its effect on the conclusion.",
      strategyRevision: 4,
      revision: 8,
      result: "The current working conclusion depends on the amended clause.",
    },
    recovery: {
      leaseExpiresAt: 1,
      previousTurnId: "turn-3",
      lastContinuationClaimedAt: 1,
    },
    status: {
      revision: 12,
      blackboardEntryCount: 7,
      contextMapEntryCount: 18,
      fileCount: 6,
      missingSourceCount: 1,
      promotedEntryCount: 3,
    },
    hierarchy: [
      node("node-project", null, "project", ""),
      node("node-dir", "node-project", "directory", "sources"),
      node("node-file", "node-dir", "file", "sources/amendment.txt"),
    ],
    obligations: [
      {
        id: "obligation-9",
        sequence: 9,
        revision: 2,
        packet: {
          examined: ["The agreement and later amendment."],
          rationale: [
            "The amendment may reverse the agreement's default rule.",
          ],
          learning: [
            "Clause 7 changes the operative threshold from 40% to 60%.",
          ],
          implication: ["The earlier conclusion is no longer supported."],
          strategy: [
            "Recalculate affected cases and verify downstream references.",
          ],
          changed: [
            "Shifted from confirming the initial rule to tracing the amendment.",
          ],
          next: ["Check the calculation workbook and final memo."],
          uncertainty: ["One cited appendix is currently missing."],
          blockers: [],
          requestedJudgment: [
            "Confirm whether the missing appendix can be supplied.",
          ],
        },
      },
    ],
    blackboard: [
      hit(
        "node-file",
        "number",
        "The operative threshold is 60%, not 40%.",
        [{ contextMapEntryId: "map-1" }],
        "current",
        "sourceVerified",
        [{ id: "relation-1" }],
      ),
      hit(
        "node-project",
        "question",
        "Does the missing appendix create an exception?",
        [],
        "notApplicable",
        "unverified",
        [],
      ),
    ],
    steering: [
      {
        input: "Compare this amendment with the calculation workbook.",
        status: "applied",
        reason: null,
      },
    ],
    contextHits: [
      {
        entryId: "map-1",
        freshness: "current",
        description: "Threshold amendment and effective date.",
        source: source("sources/amendment.txt"),
      },
    ],
    evidence: {
      encoding: "utf8",
      bytesReturned: 43,
      totalBytes: 43,
      truncated: false,
      content: "Clause 7: the threshold is amended to 60%.",
      source: source("sources/amendment.txt"),
    },
    activity: [
      {
        type: "commandExecution",
        id: "item-1",
        status: "completed",
        command: "rg threshold sources",
      },
    ],
    pendingRequests: [
      {
        id: "request-1",
        method: "item/tool/requestUserInput",
        params: {
          questions: [
            {
              id: "appendix",
              header: "Missing source",
              question: "Can you provide the appendix?",
              isOther: true,
              isSecret: false,
              options: [
                { label: "Yes", description: "I can add it now." },
                { label: "No", description: "Proceed without it." },
              ],
            },
          ],
        },
      },
    ],
    liveText: "I found a threshold change that affects the working conclusion.",
    selectedNodeId: null,
    loading: false,
    busyAction: null,
    error: null,
    notice: null,
  };
}

function node(id, parentId, kind, relativePath) {
  return { id, parentId, kind, relativePath, lifecycle: "active" };
}

function source(relativePath) {
  return { projectRoot: "C:/work/investigation", relativePath };
}

function hit(
  nodeId,
  kind,
  content,
  evidence,
  evidenceFreshness,
  effectiveVerification,
  relations,
) {
  return {
    entry: { id: `${nodeId}-${kind}`, nodeId, kind, content, evidence },
    evidenceFreshness,
    effectiveVerification,
    relations,
  };
}
