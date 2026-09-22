import assert from "node:assert/strict";
import test from "node:test";

import { compareLongitudinalSuite } from "../eval/compare-longitudinal-rollouts.mjs";
import { simpleRollout, usage } from "./longitudinal-rollout-fixture.mjs";

function observation(withState) {
  const quality = {
    blinded: true,
    rubricVersion: "v1",
    scores: {
      visibleAnswer: { correctness: 4 },
      durableState: withState ? { correctness: 4 } : null,
    },
  };
  const sourceAudit = { exactRegions: 1, repeatedReads: 0 };
  return {
    cases: {
      q1: {
        quality,
        sourceAudit,
        ...(withState ? { state: { blackboardEntries: 3 } } : {}),
      },
      q2: {
        quality,
        sourceAudit,
        ...(withState ? { state: { blackboardEntries: 5 } } : {}),
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
