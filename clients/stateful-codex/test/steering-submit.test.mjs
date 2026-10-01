import assert from "node:assert/strict";
import test from "node:test";

import { submitSteering } from "../public/steering-submit.mjs";
import { createWorkspaceDom } from "../public/workspace-dom.mjs";
import { assertSameNode, createDocument, type } from "./mini-dom.mjs";
import { workspaceFixture } from "./workspace-fixture.mjs";

function steeringWorkspace({ run = workspaceFixture().run, rpc } = {}) {
  const document = createDocument();
  const root = document.createElement("div");
  document.body.append(root);
  const state = { ...workspaceFixture(), liveText: "", run, steering: [], steeringError: null };
  const view = createWorkspaceDom(root, { schedule: () => {} });
  view.update(state);
  const calls = [];
  const control = () => root.querySelector('[name="steering"]');
  const submit = () =>
    submitSteering({
      state,
      control: control(),
      drafts: view.drafts,
      rpc: async (method, params) => {
        calls.push({ method, params });
        return rpc(method, params);
      },
      action: (_label, operation) => operation(),
    }).then((saved) => {
      view.update(state, ["steering-status", "steering-list"]);
      return saved;
    });
  const status = () => root.querySelector('[data-slot="steering-status"]');
  return { root, state, view, calls, control, submit, status };
}

const saved = (input) => ({
  steering: { id: "steering-1", input, status: "pending", reason: null },
});

test("steering submitted before the run is loaded is kept, explained, and not sent", async () => {
  const workspace = steeringWorkspace({ run: null, rpc: async (_m, params) => saved(params.input) });
  const { root, state, view, calls, control, submit, status } = workspace;
  const textarea = control();
  const button = root.querySelector("#steering-form button");
  assert.equal(button.disabled, true);
  type(textarea, "Do not touch textkit/legacy");

  assert.equal(await submit(), false);
  assert.deepEqual(calls, []);
  assert.equal(textarea.value, "Do not touch textkit/legacy");
  assert.equal(status().querySelector("[role=alert]").textContent.startsWith("The run is still being prepared"), true);

  state.run = workspaceFixture().run;
  view.update(state);
  assertSameNode(control(), textarea);
  assert.equal(button.disabled, false);
  assert.equal(await submit(), true);
  assert.deepEqual(calls.map((call) => [call.method, call.params.runId, call.params.input]), [
    ["steering/submit", "run-1", "Do not touch textkit/legacy"],
  ]);
  assert.equal(textarea.value, "");
  assert.equal(status().textContent, "");
  assert.match(root.querySelector(".steering-list").textContent, /Do not touch textkit\/legacy/);
});

test("a failed steering request keeps the text and shows the error", async () => {
  const { control, submit, status } = steeringWorkspace({
    rpc: async () => {
      throw new Error("run revision conflict.");
    },
  });
  type(control(), "Prefer the new parser");

  assert.equal(await submit(), false);
  assert.equal(control().value, "Prefer the new parser");
  assert.equal(
    status().textContent,
    "Steering was not saved: run revision conflict. Your text is kept.",
  );
});

test("a response without a saved steering record is treated as a failure", async () => {
  const { control, submit, status } = steeringWorkspace({ rpc: async () => ({}) });
  type(control(), "Stop after the tests pass");

  assert.equal(await submit(), false);
  assert.equal(control().value, "Stop after the tests pass");
  assert.match(status().textContent, /did not confirm that it saved the steering/);
});
