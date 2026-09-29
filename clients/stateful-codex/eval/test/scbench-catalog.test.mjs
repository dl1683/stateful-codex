import assert from "node:assert/strict";
import test from "node:test";
import { loadScenarios, validateScenario } from "../harness/catalog.mjs";

test("the six promoted conversations validate without reading fixture contents", async () => {
  const scenarios = await loadScenarios({ selector: "surface.tui.*", env: { SCBENCH_FIXTURES: "C:/fixtures" } });
  assert.equal(scenarios.length, 6);
  assert.equal(scenarios.reduce((total, scenario) => total + scenario.conversation.turns.length, 0), 22);
  assert.equal(scenarios.find((scenario) => scenario.fixture.private).fixture.resolvedSource, "C:/fixtures/e2-dataroom");
});

test("missing hash, timeout, capture, or message is rejected", async () => {
  const base = (await loadScenarios({ selector: "surface.tui.key-rotation-scope-change", env: { SCBENCH_FIXTURES: "C:/fixtures" } }))[0];
  for (const change of [
    { fixture: { ...base.fixture, sha256: "" } },
    { conversation: { ...base.conversation, turnTimeoutSeconds: 0 } },
    { capture: ["screens"] },
    { conversation: { ...base.conversation, turns: [""] } },
  ]) await assert.rejects(validateScenario({ ...base, ...change }), /invalid/);
});

test("unresolved fixture roots fail clearly and private records contain no corpus", async () => {
  const scenarios = await loadScenarios({ selector: "surface.tui.*", env: { SCBENCH_FIXTURES: "C:/fixtures" } });
  await assert.rejects(() => loadScenarios({ selector: "surface.tui.key-rotation-scope-change", env: {} }), /unresolved environment variable SCBENCH_FIXTURES/);
  assert.equal(Object.prototype.hasOwnProperty.call(scenarios.find((scenario) => scenario.fixture.private), "contents"), false);
});
