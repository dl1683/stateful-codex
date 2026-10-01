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

test("the live tail drops the oldest text first, across interleaved items", () => {
  const { view, runFrames, root, $ } = mountWorkspace({ liveLimit: 100 });
  view.pushDelta({ turnId: "t1", itemId: "a", delta: "a".repeat(50) });
  view.pushDelta({ turnId: "t1", itemId: "b", delta: "b".repeat(50) });
  runFrames();
  view.pushDelta({ turnId: "t1", itemId: "a", delta: "c".repeat(60) });
  runFrames();

  assert.deepEqual(
    root.querySelectorAll("[data-live-item]").map((item) => item.textContent),
    ["c".repeat(60), "b".repeat(40)],
  );
  assert.equal($("[data-live-truncated] a").getAttribute("href"), "#recorded-messages");
});

test("the live tail keeps the newest text when items interleave within one frame", () => {
  const { view, runFrames, root } = mountWorkspace({ liveLimit: 100 });
  view.pushDelta({ turnId: "t1", itemId: "a", delta: "a".repeat(50) });
  view.pushDelta({ turnId: "t1", itemId: "b", delta: "b".repeat(50) });
  view.pushDelta({ turnId: "t1", itemId: "a", delta: "c".repeat(100) });
  runFrames();

  assert.deepEqual(
    root.querySelectorAll("[data-live-item]").map((item) => [
      item.getAttribute("data-live-item"),
      item.textContent,
    ]),
    [["a", "c".repeat(100)]],
  );
  assert.equal(root.querySelector("[data-live-item]").childNodes[0].appendCount, 1);
});

test("refreshing activity keeps focus on a disclosure summary", () => {
  const { state, view, document, $ } = mountWorkspace();
  $('[data-disclosure="recorded-messages"]').focus();
  state.activity = [
    ...state.activity,
    { turnId: "turn-4", item: { type: "agentMessage", id: "m1", text: "Done." } },
  ];
  view.update(state);

  assertSameNode(document.activeElement, $('[data-disclosure="recorded-messages"]'));
  assert.match($("#recorded-messages").textContent, /Done\./);
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

  type(message, "only once");
  const first = drafts.submit(message, operation);
  const second = drafts.submit(message, operation);
  release();
  await Promise.all([first, second]);
  assert.deepEqual(sent, ["first", "first, and more", "only once"]);

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

test("reordering request cards keeps the focused answer focused", () => {
  const { state, view, document, root } = mountWorkspace();
  const approval = {
    id: 7,
    method: "item/fileChange/requestApproval",
    params: { threadId: "thread", reason: "Write the summary" },
  };
  state.pendingRequests = [approval, state.pendingRequests[0]];
  view.update(state, ["requests"]);
  const answer = root.querySelector('[name="appendix"]');
  answer.focus();

  state.pendingRequests = [state.pendingRequests[1], approval];
  view.update(state, ["requests"]);

  assert.deepEqual(
    root.querySelectorAll("[data-request-key]").map((card) => card.getAttribute("data-request-key")),
    ['"request-1"', "7"],
  );
  assertSameNode(root.querySelector('[name="appendix"]'), answer);
  assertSameNode(document.activeElement, answer);
});

test("an unsent mode choice survives a refresh; a clean selector follows the persisted mode", () => {
  const { state, view, $ } = mountWorkspace();
  const mode = $('[name="mode"]');
  assert.equal(mode.value, "autonomous");

  state.run = { ...state.run, mode: "collaborative", revision: 9 };
  view.update(state);
  assertSameNode($('[name="mode"]'), mode);
  assert.equal(mode.value, "collaborative");

  mode.value = "socratic";
  mode.dispatch("change");
  state.run = { ...state.run, mode: "autonomous", revision: 10 };
  view.update(state);
  assertSameNode($('[name="mode"]'), mode);
  assert.equal(mode.value, "socratic");
});

test("a mode edited back while its change is pending is not overwritten, and is sent once", async () => {
  const { state, view, $ } = mountWorkspace();
  const mode = $('[name="mode"]');
  const sent = [];
  let release;
  const setMode = (value) => {
    sent.push(value);
    return new Promise((resolve) => (release = resolve));
  };

  mode.value = "collaborative";
  mode.dispatch("change");
  const pending = view.drafts.submit(mode, setMode, { clear: false });
  await view.drafts.submit(mode, setMode, { clear: false });
  mode.value = "autonomous";
  mode.dispatch("change");
  release();
  await pending;
  state.run = { ...state.run, mode: "collaborative", revision: 9 };
  view.update(state);

  assert.deepEqual(sent, ["collaborative"]);
  assert.equal(mode.value, "autonomous");

  const settled = view.drafts.submit(mode, setMode, { clear: false });
  release();
  await settled;
  state.run = { ...state.run, mode: "autonomous", revision: 10 };
  view.update(state);
  state.run = { ...state.run, mode: "socratic", revision: 11 };
  view.update(state);
  assert.equal(mode.value, "socratic");
});
