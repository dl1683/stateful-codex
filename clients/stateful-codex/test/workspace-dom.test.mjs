import assert from "node:assert/strict";
import test from "node:test";

import { createDraftTracker, createWorkspaceDom } from "../public/workspace-dom.mjs";
import { WORKSPACE_SLOTS } from "../public/workspace-view.mjs";
import { assertSameNode, createDocument, type } from "./mini-dom.mjs";
import { workspaceFixture } from "./workspace-fixture.mjs";

function mountWorkspace(options = {}) {
  const document = createDocument();
  const root = document.createElement("div");
  document.body.append(root);
  const state = { ...workspaceFixture(), liveText: "" };
  const frames = [];
  const counts = { hierarchy: 0 };
  const view = createWorkspaceDom(root, {
    slots: {
      ...WORKSPACE_SLOTS,
      hierarchy(current) {
        counts.hierarchy += 1;
        return WORKSPACE_SLOTS.hierarchy(current);
      },
    },
    schedule: (callback) => frames.push(callback),
    ...options,
  });
  view.update(state);
  const runFrames = () => frames.splice(0).forEach((callback) => callback());
  const $ = (selector) => root.querySelector(selector);
  return { document, root, state, view, frames, runFrames, counts, $ };
}

function formControls({ $ }) {
  return {
    message: $('[name="message"]'),
    steering: $('[name="steering"]'),
    query: $('[name="query"]'),
    mode: $('[name="mode"]'),
    answer: $('[data-request-key] [name="appendix"]'),
  };
}

test("a project refresh replaces only changed slots and keeps forms, drafts and focus", () => {
  const workspace = mountWorkspace();
  const { state, view, root, $, counts } = workspace;
  const controls = formControls(workspace);
  type(controls.message, "half-written instruction");
  type(controls.steering, "steer toward clause 9");
  type(controls.query, "threshold");
  controls.answer.value = "No";
  controls.message.focus();
  const header = $("header");
  const tree = $(".tree");
  tree.scrollTop = 120;
  const strategy = $('[data-slot="strategy"] p');

  Object.assign(state, {
    run: { ...state.run, strategy: "Check the appendix next.", revision: 9 },
    status: { ...state.status },
    hierarchy: state.hierarchy.map((node) => ({ ...node })),
    obligations: [...state.obligations],
  });
  view.update(state);

  for (const [name, control] of Object.entries(controls)) {
    assertSameNode(formControls(workspace)[name], control, `${name} control`);
  }
  assert.deepEqual(
    Object.values(controls).map((control) => control.value),
    ["half-written instruction", "steer toward clause 9", "threshold", "autonomous", "No"],
  );
  assertSameNode(workspace.document.activeElement, controls.message);
  assertSameNode($("header"), header);
  assertSameNode($(".tree"), tree);
  assert.equal(tree.scrollTop, 120);
  assert.equal(counts.hierarchy, 1);
  assert.notEqual($('[data-slot="strategy"] p'), strategy);
  assert.equal($('[data-slot="strategy"] p').textContent, "Check the appendix next.");
  assert.match($('[data-slot="controls-detail"]').textContent, /Run revision9/);
  assert.equal(root.innerHTMLWrites, 1);
});

test("busy and error notices update the status region alone; the page is not a live region", () => {
  const { state, view, root, $ } = mountWorkspace();
  const findings = $(".finding-list");
  state.busyAction = "Applying steering";
  view.update(state, ["notices"]);

  const notices = $('[data-slot="notices"]');
  assert.equal(notices.getAttribute("role"), "status");
  assert.equal(notices.textContent, "Applying steering");
  assertSameNode($(".finding-list"), findings);
  assert.equal(root.querySelectorAll("[aria-live]").length, 0);
  assert.equal(root.hasAttribute("aria-live"), false);
});

test("the hierarchy is rendered again only when its data changes, and selection is an attribute", () => {
  const { state, view, $, counts } = mountWorkspace();
  view.update(state, ["hierarchy"]);
  state.hierarchy = state.hierarchy.map((node) => ({ ...node }));
  view.update(state);
  assert.equal(counts.hierarchy, 1);

  const row = $('[data-node-id="node-file"]');
  state.selectedNodeId = "node-file";
  view.selectNode(state);
  assertSameNode($('[data-node-id="node-file"]'), row);
  assert.equal(row.getAttribute("data-selected"), "true");
  assert.match($('[data-slot="findings"] h2').textContent, /Selected-node understanding/);
  assert.equal(counts.hierarchy, 1);

  state.hierarchy = [
    ...state.hierarchy,
    { id: "node-new", parentId: "node-dir", kind: "file", relativePath: "sources/new.txt", lifecycle: "active" },
  ];
  view.update(state);
  assert.equal(counts.hierarchy, 2);
  assert.equal($('[data-node-id="node-file"]').getAttribute("data-selected"), "true");
});

test("a burst of deltas is coalesced into one text append per item per frame", () => {
  const { view, root, frames, runFrames, counts, $ } = mountWorkspace();
  const message = $('[name="message"]');
  const chunks = Array.from({ length: 1000 }, (_, index) => `w${index} `);
  for (const delta of chunks) view.pushDelta({ turnId: "t1", itemId: "i1", delta });

  assert.equal(frames.length, 1);
  assert.equal($("[data-live-copy]").textContent, "");
  runFrames();

  const items = root.querySelectorAll("[data-live-item]");
  assert.equal(items.length, 1);
  assert.equal(items[0].textContent, chunks.join(""));
  assert.equal(items[0].childNodes[0].appendCount, 1);
  assert.equal($("[data-live]").hidden, false);
  assert.equal(counts.hierarchy, 1);
  assert.equal(root.innerHTMLWrites, 1);
  assertSameNode($('[name="message"]'), message);
});

test("streamed text stays isolated by turn and item", () => {
  const { view, runFrames, root, $ } = mountWorkspace();
  const liveItems = () =>
    root.querySelectorAll("[data-live-item]").map((item) => [
      item.getAttribute("data-live-item"),
      item.textContent,
    ]);
  for (const [itemId, delta] of [
    ["a", "first "],
    ["b", "second "],
    ["a", "message"],
    ["b", "message"],
  ]) {
    view.pushDelta({ turnId: "t1", itemId, delta });
  }
  runFrames();
  assert.deepEqual(liveItems(), [
    ["a", "first message"],
    ["b", "second message"],
  ]);

  view.startTurn("t2");
  assert.deepEqual(liveItems(), []);
  assert.equal($("[data-live]").hidden, true);
  view.pushDelta({ turnId: "t2", itemId: "c", delta: "t2 text" });
  view.pushDelta({ turnId: "t1", itemId: "a", delta: "late text from t1" });
  runFrames();
  assert.deepEqual(liveItems(), [["c", "t2 text"]]);

  // A turn whose start was missed (for example across a reconnect) replaces the previous one.
  view.pushDelta({ turnId: "t3", itemId: "d", delta: "t3 text" });
  runFrames();
  assert.deepEqual(liveItems(), [["d", "t3 text"]]);
});

test("the live tail is bounded and says so", () => {
  const { view, runFrames, root, $ } = mountWorkspace({ liveLimit: 100 });
  view.pushDelta({ turnId: "t1", itemId: "a", delta: "a".repeat(80) });
  view.pushDelta({ turnId: "t1", itemId: "b", delta: "b".repeat(170) });
  runFrames();

  assert.equal($("[data-live-copy]").textContent, "b".repeat(100));
  assert.equal(root.querySelectorAll("[data-live-item]").length, 1);
  assert.equal($("[data-live-truncated]").hidden, false);
  view.pushDelta({ turnId: "t1", itemId: "b", delta: "c".repeat(10) });
  runFrames();
  assert.equal($("[data-live-copy]").textContent, `${"b".repeat(90)}${"c".repeat(10)}`);
});

test("a submitted draft is cleared only if it was not edited while the request was pending", async () => {
  const { root, $ } = mountWorkspace();
  const drafts = createDraftTracker(root);
  const message = $('[name="message"]');
  let release;
  const sent = [];
  const operation = (value) => {
    sent.push(value);
    return new Promise((resolve) => (release = resolve));
  };

  type(message, "  first  ");
  let pending = drafts.submit(message, operation);
  type(message, "first, and more");
  release();
  await pending;
  assert.equal(message.value, "first, and more");

  pending = drafts.submit(message, operation);
  release();
  await pending;
  assert.equal(message.value, "");
  assert.deepEqual(sent, ["first", "first, and more"]);

  type(message, "will fail");
  await assert.rejects(
    drafts.submit(message, () => Promise.reject(new Error("offline"))),
  );
  assert.equal(message.value, "will fail");
});

test("request cards are reconciled by typed request ID", () => {
  const { state, view, $, root } = mountWorkspace();
  const cardFor = (key) =>
    root.querySelectorAll("[data-request-key]").find((item) => item.getAttribute("data-request-key") === key);
  const card = cardFor('"request-1"');
  const answer = card.querySelector('[name="appendix"]');
  answer.value = "No";
  answer.focus();
  const approval = (id, command) => ({
    id,
    method: "item/commandExecution/requestApproval",
    params: { threadId: "thread", command },
  });

  state.pendingRequests = [
    approval(11, "numbered request"),
    state.pendingRequests[0],
    approval("11", "string request"),
  ];
  view.update(state, ["requests"]);
  const keys = () =>
    root.querySelectorAll("[data-request-key]").map((item) => item.getAttribute("data-request-key"));
  assert.deepEqual(keys(), ["11", '"request-1"', '"11"']);
  assertSameNode(cardFor('"request-1"'), card);
  assert.equal(answer.value, "No");

  const numbered = cardFor("11");
  state.pendingRequests = state.pendingRequests.filter((request) => request.id !== "11");
  view.update(state, ["requests"]);
  assert.deepEqual(keys(), ["11", '"request-1"']);
  assertSameNode(cardFor("11"), numbered);
  assertSameNode(cardFor('"request-1"'), card);

  state.pendingRequests = [];
  view.update(state, ["requests"]);
  assert.equal($("[data-requests]").hidden, true);
});
