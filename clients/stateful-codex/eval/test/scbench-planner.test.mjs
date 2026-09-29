import assert from "node:assert/strict";
import test from "node:test";
import { loadScenarios } from "../harness/catalog.mjs";
import { planScenarios } from "../harness/planner.mjs";

test("plans the six surface conversations concurrently on a 32-CPU host", async () => {
  const scenarios = await loadScenarios({ selector: "surface.tui.*", env: { SCBENCH_FIXTURES: "C:/fixtures" } });
  const plan = planScenarios(scenarios, { tier: "surface", parallelism: 32 });
  assert.equal(plan.attempts, 6);
  assert.equal(plan.sessions, 6);
  assert.equal(plan.userTurns, 28);
  assert.equal(plan.jobs, 24);
  assert.equal(plan.maximumTheoreticalParallelism, 24);
  assert.deepEqual(plan.selectedSurfaces, ["tui"]);
  assert.equal(plan.privateFixtures[0].id, "surface.tui.data-room-correction-provenance");
});

test("operator ceilings are optional and explicit", async () => {
  const scenarios = await loadScenarios({ selector: "surface.tui.*", env: { SCBENCH_FIXTURES: "C:/fixtures" } });
  assert.throws(() => planScenarios(scenarios, { maxAttempts: 5 }), /max-attempts/);
  assert.equal(planScenarios(scenarios, { jobs: 2 }).jobs, 2);
});
