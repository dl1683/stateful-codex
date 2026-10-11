import assert from "node:assert/strict";
import test from "node:test";

import { startSetup } from "../public/setup-controller.mjs";

// A minimal stand-in for the setup page: the #app element, its listeners and one form whose
// field values the test sets the way typing does.
function fakePage() {
  const listeners = {};
  const fields = new Map();
  const form = { id: "setup-form" };
  const app = {
    innerHTML: "",
    renders: 0,
    addEventListener: (type, listener) => {
      listeners[type] = listener;
    },
    querySelector: (selector) => (selector === "#setup-form" ? form : null),
  };
  let html = "";
  Object.defineProperty(app, "innerHTML", {
    get: () => html,
    set: (value) => {
      html = value;
      app.renders += 1;
    },
  });
  const dispatch = (type, target) =>
    listeners[type]({ type, target, preventDefault() {} });
  return { app, form, fields, dispatch };
}

function memoryStorage() {
  const values = new Map();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, String(value)),
    removeItem: (key) => values.delete(key),
  };
}

test("typing a question and clicking Open workspace in one step opens it", async () => {
  const page = fakePage();
  const calls = [];
  const project = { id: "project-1", name: "Demo", roots: [{ path: "C:/work/demo" }] };
  const rpc = async (method, params) => {
    calls.push(method);
    if (method === "account/read") return { account: { type: "chatgpt" } };
    if (method === "project/list") return { data: [project] };
    if (method === "thread/list") return { data: [] };
    if (method === "thread/start") return { thread: { id: "thread-1" } };
    throw new Error(`unexpected ${method} ${JSON.stringify(params)}`);
  };
  let opened = null;
  const setup = startSetup({
    app: page.app,
    rpc,
    subscribe: () => {},
    readForm: () => page.fields,
    storage: memoryStorage(),
    navigate: (path) => {
      opened = path;
    },
    newId: () => "id-1",
  });
  await setup.boot();
  page.fields.set("projectId", "project-1");
  await page.dispatch("change", { name: "projectId" });
  const rendersBefore = page.app.renders;

  // Type the question, then click: the click blurs the textarea first ("change"), which must
  // not replace the form (and the button under the pointer) before the click lands.
  page.fields.set("goal", "What did we decide about the database?");
  page.dispatch("input", { name: "goal" });
  page.dispatch("change", { name: "goal" });
  assert.equal(page.app.renders, rendersBefore);
  await page.dispatch("submit", page.form);

  assert.deepEqual(calls, ["account/read", "project/list", "thread/list", "thread/start"]);
  assert.equal(opened, "/workspace.html");
  assert.equal(setup.state.goal, "What did we decide about the database?");
  assert.equal(setup.state.mode, "ask");
});
