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
  const state = {
    ...workspaceFixture(),
    liveText: "",
    run,
    steering: [],
    steeringError: null,
    confirmedSteering: [],
  };
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

test("a second submission while one is pending is refused visibly and keeps its text", async () => {
  let release;
  const { control, submit, status, calls } = steeringWorkspace({
    rpc: (_method, params) => new Promise((resolve) => (release = () => resolve(saved(params.input)))),
  });
  type(control(), "First instruction");
  const first = submit();
  type(control(), "Second instruction");

  assert.equal(await submit(), false);
  assert.match(status().textContent, /Earlier steering is still being sent/);
  release();
  assert.equal(await first, true);
  assert.equal(control().value, "Second instruction");
  assert.equal(calls.length, 1);
});

test("if the form is replaced while steering is pending, a failure still shows the text", async () => {
  let reject;
  const { state, view, control, submit, status } = steeringWorkspace({
    rpc: () => new Promise((_resolve, rejectRequest) => (reject = rejectRequest)),
  });
  type(control(), "Keep the legacy module untouched");
  const pending = submit();
  state.run = { ...state.run, status: "completed" };
  view.update(state);
  assert.equal(control(), null);
  reject(new Error("run is closed."));

  assert.equal(await pending, false);
  assert.equal(
    status().textContent,
    "Steering was not saved: run is closed. Your text was: Keep the legacy module untouched",
  );
});
