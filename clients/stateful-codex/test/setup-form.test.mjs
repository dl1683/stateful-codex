import assert from "node:assert/strict";
import test from "node:test";

import { createSetupForm } from "../public/setup-form.mjs";
import { assertSameNode, createDocument, type } from "./mini-dom.mjs";

function setupState() {
  return {
    busy: false,
    error: null,
    account: { type: "chatgpt" },
    projects: [
      { id: "project-a", name: "Alpha", roots: [{ path: "C:/alpha" }] },
      { id: "project-b", name: "Beta", roots: [{ path: "C:/beta" }] },
    ],
    threads: [],
    projectId: "new",
    projectName: "",
    rootPath: "",
    threadAction: "create",
    threadId: "",
    mode: "collaborative",
    goal: "",
    maxContinuations: 24,
    maxElapsedSeconds: 14400,
  };
}

// thread/list responses are released by the test, so their completion order is explicit.
function deferredRpc() {
  const calls = [];
  const rpc = (method, params) =>
    new Promise((resolve, reject) => calls.push({ method, params, resolve, reject }));
  return { rpc, calls };
}

function mountSetup(rpc = deferredRpc().rpc) {
  const document = createDocument();
  const root = document.createElement("div");
  document.body.append(root);
  const state = setupState();
  const opened = [];
  const form = createSetupForm({
    root,
    state,
    rpc,
    openWorkspace: async (current) => opened.push({ ...current }),
  });
  form.update();
  const field = (name) => root.querySelector(`[name="${name}"]`);
  const card = (group, value) =>
    root.querySelector(`[data-choice-group="${group}"][data-choice-value="${value}"]`);
  return { document, root, state, opened, field, card };
}

function chooseProject(view, projectId) {
  const select = view.field("projectId");
  select.value = projectId;
  return select.dispatch("change").results[0];
}

test("a card selection takes effect on the first click, even right after a field change", () => {
  const view = mountSetup();
  const goal = view.field("goal");
  type(goal, "Map the decisive evidence");
  // Blurring the textarea fires change before the click lands on the card.
  goal.dispatch("change");
  const socratic = view.card("mode", "socratic");
  socratic.querySelector("small").dispatch("click");

  assert.equal(view.state.mode, "socratic");
  assertSameNode(view.card("mode", "socratic"), socratic);
  assert.deepEqual(
    ["autonomous", "collaborative", "socratic"].map((mode) => [
      view.card("mode", mode).getAttribute("data-selected"),
      view.card("mode", mode).getAttribute("aria-pressed"),
    ]),
    [
      ["false", "false"],
      ["false", "false"],
      ["true", "true"],
    ],
  );
  assert.equal(view.root.innerHTMLWrites, 1);
});

test("changing modes and thread actions preserves the goal and budget drafts", () => {
  const view = mountSetup();
  const goal = view.field("goal");
  goal.focus();
  type(goal, "Keep the draft");
  view.card("mode", "autonomous").dispatch("click");
  const budget = view.root.querySelector('[data-group="budget"]');
  assert.equal(budget.hidden, false);
  type(view.field("maxContinuations"), "7");
  view.card("mode", "socratic").dispatch("click");
  assert.equal(budget.hidden, true);
  assert.equal(budget.disabled, true);
  view.card("thread", "continue").dispatch("click");
  view.card("mode", "autonomous").dispatch("click");

  assertSameNode(view.field("goal"), goal);
  assert.equal(goal.value, "Keep the draft");
  assertSameNode(view.document.activeElement, goal);
  assert.equal(view.field("maxContinuations").value, "7");
  assert.equal(view.state.maxContinuations, 7);
  assert.equal(view.root.querySelector('[data-group="thread-select"]').hidden, false);
  assert.equal(view.root.innerHTMLWrites, 1);
});

test("asynchronous thread loading preserves unrelated input", async () => {
  const { rpc, calls } = deferredRpc();
  const view = mountSetup(rpc);
  const rootPath = view.field("rootPath");
  type(rootPath, "C:/draft/directory");
  type(view.field("projectName"), "Draft name");
  view.card("thread", "continue").dispatch("click");
  const loading = chooseProject(view, "project-a");
  const goal = view.field("goal");
  goal.focus();
  type(goal, "Typed while threads load");
  assert.equal(view.field("threadId").disabled, true);

  calls[0].resolve({ data: [{ id: "thread-a1", name: "Alpha thread" }] });
  await loading;

  assertSameNode(view.field("goal"), goal);
  assert.equal(goal.value, "Typed while threads load");
  assertSameNode(view.document.activeElement, goal);
  assert.equal(view.field("threadId").disabled, false);
  assert.deepEqual(
    view.field("threadId").querySelectorAll("option").map((option) => option.value),
    ["", "thread-a1"],
  );
  assert.equal(view.root.querySelector('[data-group="new-project"]').hidden, true);
  chooseProject(view, "new");
  assertSameNode(view.field("rootPath"), rootPath);
  assert.equal(rootPath.value, "C:/draft/directory");
  assert.equal(view.field("projectName").value, "Draft name");
});

test("an obsolete thread list cannot replace the current project's threads", async () => {
  const { rpc, calls } = deferredRpc();
  const view = mountSetup(rpc);
  view.card("thread", "fork").dispatch("click");
  const first = chooseProject(view, "project-a");
  const second = chooseProject(view, "project-b");
  assert.deepEqual(
    calls.map((call) => call.params.projectId),
    ["project-a", "project-b"],
  );

  calls[1].resolve({ data: [{ id: "thread-b1", name: "Beta thread" }] });
  await second;
  calls[0].resolve({ data: [{ id: "thread-a1", name: "Alpha thread" }] });
  await first;

  assert.equal(view.state.projectId, "project-b");
  assert.deepEqual(view.state.threads, [{ id: "thread-b1", name: "Beta thread" }]);
  assert.deepEqual(
    view.field("threadId").querySelectorAll("option").map((option) => option.textContent),
    ["Select a project thread", "Beta thread"],
  );
  assert.equal(view.state.threadsLoading, false);
});

test("a failed obsolete thread list does not report an error for the current project", async () => {
  const { rpc, calls } = deferredRpc();
  const view = mountSetup(rpc);
  const first = chooseProject(view, "project-a");
  const second = chooseProject(view, "project-b");
  calls[0].reject(new Error("project-a went away"));
  await first;
  calls[1].resolve({ data: [] });
  await second;

  assert.equal(view.state.error, null);
});

test("continue and fork cannot open a workspace before a thread is chosen", async () => {
  const { rpc, calls } = deferredRpc();
  const view = mountSetup(rpc);
  type(view.field("goal"), "Pick up where I left off");
  view.card("thread", "continue").dispatch("click");
  const loading = chooseProject(view, "project-a");
  const submit = view.root.querySelector('button[type="submit"]');
  const form = view.root.querySelector("#setup-form");
  assert.equal(submit.disabled, true);

  await Promise.all(form.dispatch("submit").results);
  assert.equal(view.opened.length, 0);
  assert.equal(view.state.error, "Wait for the project's threads to load.");

  calls[0].resolve({ data: [{ id: "thread-a1", name: "Alpha thread" }] });
  await loading;
  assert.equal(submit.disabled, false);
  await Promise.all(form.dispatch("submit").results);
  assert.equal(view.opened.length, 0);
  assert.equal(view.state.error, "Select a thread to continue or fork.");

  const thread = view.field("threadId");
  thread.value = "thread-a1";
  thread.dispatch("change");
  await Promise.all(form.dispatch("submit").results);
  assert.equal(view.opened.length, 1);
  assert.equal(view.opened[0].threadId, "thread-a1");
});

test("submitting opens the workspace once with the captured drafts", async () => {
  const view = mountSetup();
  type(view.field("projectName"), "New project");
  type(view.field("rootPath"), "C:/new");
  type(view.field("goal"), "Ship it");
  const form = view.root.querySelector("#setup-form");
  const event = form.dispatch("submit");
  form.dispatch("submit");
  await Promise.all(event.results);

  assert.equal(event.defaultPrevented, true);
  assert.equal(view.opened.length, 1);
  assert.equal(view.opened[0].goal, "Ship it");
  assert.equal(view.opened[0].rootPath, "C:/new");
  assert.equal(view.root.querySelector('button[type="submit"]').textContent, "Opening…");
});
