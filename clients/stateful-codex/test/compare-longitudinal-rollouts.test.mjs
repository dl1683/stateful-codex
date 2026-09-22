import assert from "node:assert/strict";
import test from "node:test";

import { compareLongitudinalSuite } from "../eval/compare-longitudinal-rollouts.mjs";
import { simpleRollout, usage } from "./longitudinal-rollout-fixture.mjs";

function observation(withState) {
  const visibleDimensions = [
    "correctness",
    "evidenceTraceability",
    "decisiveDetail",
    "uncertaintyCalibration",
    "contradictionAndFreshness",
    "usefulness",
  ];
  const durableDimensions = [
    "semanticFidelity",
    "evidenceTraceability",
    "compressionValue",
    "uncertaintyPreservation",
    "futureUsability",
  ];
  const scores = (dimensions) => Object.fromEntries(dimensions.map((field) => [field, 4]));
  const rationales = (dimensions) =>
    Object.fromEntries(dimensions.map((field) => [field, `${field} rationale`]));
  const quality = {
    blinded: true,
    rubricVersion: "v1",
    grader: { id: "grader-1", model: "test-model", independent: true },
    evidenceReferences: [
      { path: "source.md", locator: "lines 1-3", note: "supports the score" },
    ],
    scores: {
      visibleAnswer: scores(visibleDimensions),
      durableState: withState ? scores(durableDimensions) : null,
    },
    rationales: {
      visibleAnswer: rationales(visibleDimensions),
      durableState: withState ? rationales(durableDimensions) : null,
    },
  };
  const sourceAudit = {
    method: "manual-v1",
    auditor: "auditor-1",
    uniqueFilesRead: 1,
    exactRegionsRead: 1,
    repeatedRegions: 0,
    broadReads: 0,
    failedReads: 0,
    corpusRevision: `sha256:${"0".repeat(64)}`,
  };
  const state = {
    method: "sqlite-v1",
    observedAtMs: 1,
    projectRevision: 3,
    hierarchyNodes: 2,
    blackboardEntries: 3,
    contextMapEntries: 1,
    relationships: 1,
    rootEntries: 1,
    candidateEntries: 0,
    staleRecords: 0,
    maintenanceRecords: 0,
  };
  return {
    cases: {
      q1: {
        quality,
        sourceAudit,
        ...(withState ? { state } : {}),
      },
      q2: {
        quality,
        sourceAudit,
        ...(withState ? { state: { ...state, blackboardEntries: 5 } } : {}),
      },
    },
  };
}

test("compares continuous sequences with cumulative deltas and clustered aggregation", () => {
  const manifest = {
    name: "two-turn longitudinal",
    requiredObservations: {
      baseline: ["quality", "sourceAudit"],
      stateful: ["quality", "sourceAudit", "state"],
    },
    projects: [
      {
        id: "licensing",
        isolatedCopies: true,
        requireStatefulProjectState: false,
        requireDurableCompletion: false,
        cases: [
          {
            id: "q1",
            prompt: "Question 1?",
            expectedConcepts: [
              { name: "authority", termGroups: [["executed"], ["controls"]] },
            ],
          },
          {
            id: "q2",
            prompt: "Question 2?",
            expectedConcepts: [
              { name: "cap", termGroups: [["six"], ["percent"]] },
            ],
          },
        ],
      },
    ],
  };
  const baseline = simpleRollout("baseline", "baseline", [
    usage(90, 20, 10),
    usage(180, 100, 20),
  ]);
  const stateful = simpleRollout("stateful", "stateful", [
    usage(110, 40, 10),
    usage(130, 100, 10),
  ]);
  for (const event of baseline) {
    if (event.type === "turn_context") {
      event.payload.cwd = "C:/baseline";
      event.payload.workspace_roots = ["C:/baseline"];
    }
  }
  for (const event of stateful) {
    if (event.type === "turn_context") {
      event.payload.cwd = "C:/stateful";
      event.payload.workspace_roots = ["C:/stateful"];
    }
  }

  const report = compareLongitudinalSuite(
    manifest,
    new Map([["licensing", { baseline, stateful }]]),
    new Map([
      [
        "licensing",
        { baseline: observation(false), stateful: observation(true) },
      ],
    ]),
  );

  assert.equal(report.valid, true);
  assert.equal(report.projects[0].cases[0].delta.usage.totalTokens.absolute, 20);
  assert.equal(
    report.projects[0].cases[1].cumulative.delta.totalTokens.absolute,
    -40,
  );
  assert.equal(report.aggregate.baseline.usage.totalTokens, 300);
  assert.equal(report.aggregate.stateful.usage.totalTokens, 260);
  assert.equal(report.aggregate.projectWins.totalTokens, 1);
  assert.equal(
    report.projects[0].cases[0].stateful.diagnosticAnswerCheck.diagnosticOnly,
    true,
  );
  assert.deepEqual(report.projects[0].cases[1].missingMeasurements, []);
  assert.match(report.inferenceBoundary, /clustered units/);
});

test("rejects empty observation objects and partial quality scores", () => {
  const manifest = {
    name: "strict observations",
    requiredObservations: {
      baseline: ["quality", "sourceAudit"],
      stateful: ["quality", "sourceAudit", "state"],
    },
    projects: [
      {
        id: "project",
        requireStatefulProjectState: false,
        requireDurableCompletion: false,
        cases: [{ id: "q1", prompt: "Question 1?" }],
      },
    ],
  };
  const baseline = simpleRollout("baseline", "baseline", [usage(90, 20, 10)]);
  const stateful = simpleRollout("stateful", "stateful", [usage(80, 30, 10)]);
  const incomplete = {
    cases: {
      q1: {
        quality: {
          blinded: true,
          rubricVersion: "v1",
          scores: { visibleAnswer: { correctness: 4 } },
        },
        sourceAudit: {},
        state: {},
      },
    },
  };
  const report = compareLongitudinalSuite(
    manifest,
    new Map([["project", { baseline, stateful }]]),
    new Map([["project", { baseline: incomplete, stateful: incomplete }]]),
  );

  assert.equal(report.valid, false);
  assert.ok(
    report.projects[0].cases[0].missingMeasurements.includes(
      "baseline observation.quality.grader",
    ),
  );
  assert.ok(
    report.projects[0].cases[0].missingMeasurements.includes(
      "stateful observation.state.projectRevision",
    ),
  );
});

test("reports missing external measurements instead of inferring them", () => {
  const manifest = {
    name: "measurement boundary",
    requiredObservations: { stateful: ["quality", "state"] },
    projects: [
      {
        id: "project",
        requireStatefulProjectState: false,
        requireDurableCompletion: false,
        cases: [{ id: "q1", prompt: "Question 1?" }],
      },
    ],
  };
  const baseline = simpleRollout("baseline", "baseline", [usage(90, 20, 10)]);
  const stateful = simpleRollout("stateful", "stateful", [usage(80, 30, 10)]);
  const report = compareLongitudinalSuite(
    manifest,
    new Map([["project", { baseline, stateful }]]),
  );

  assert.equal(report.valid, false);
  assert.deepEqual(report.projects[0].cases[0].missingMeasurements, [
    "stateful observation.quality",
    "stateful observation.state",
  ]);
  assert.ok(
    report.projects[0].protocolErrors.includes(
      "q1: missing stateful observation.state",
    ),
  );
});
