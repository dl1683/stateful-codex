// The Ask Answer panel converges to the newest completed answer in authoritative history, through
// the real event reducer, history-page reconciliation, selector and renderer.

import assert from "node:assert/strict";
import test from "node:test";

import { applyHistoryPage, applyWorkspaceEvent } from "../public/workspace-events.mjs";
import { askAnswer, renderWorkspace } from "../public/workspace-view.mjs";
import { workspaceFixture } from "./workspace-fixture.mjs";

const THREAD = "thread-1";
const PAGE_LIMIT = 100;

function askState() {
  const state = workspaceFixture();
  Object.assign(state, {
    threadId: THREAD,
    ask: true,
    run: null,
    obligations: [],
    steering: [],
    liveText: "",
    localAnswer: null,
    requestedPageSeq: 0,
    eventSeq: 0,
    activity: [],
  });
  return state;
}

const answer = (id, text) => ({ turnId: "turn", item: { type: "agentMessage", id, text } });
const tool = (id) => ({
  turnId: "turn",
  item: { type: "commandExecution", id, status: "completed", command: "rg x" },
});
const started = (id) => ({
  method: "item/started",
  params: { threadId: THREAD, turnId: "turn", item: { type: "agentMessage", id, text: "" } },
});
const delta = (id, text) => ({
  method: "item/agentMessage/delta",
  params: { threadId: THREAD, turnId: "turn", itemId: id, delta: text },
});
const completed = (id, text) => ({
  method: "item/completed",
  params: { threadId: THREAD, turnId: "turn", item: { type: "agentMessage", id, text } },
});
const reconnect = { method: "gateway/connected", params: {} };

// What workspace.mjs does: note the event count when the page is requested, apply it on return.
function request(state) {
  state.requestedPageSeq = state.eventSeq ?? 0;
  return state.requestedPageSeq;
}
function respond(state, history, requestSeq) {
  applyHistoryPage(state, history.slice(-PAGE_LIMIT), requestSeq);
}
function page(state, history) {
  respond(state, history, request(state));
}

function shown(state) {
  const panel = renderWorkspace(state).match(/<h2>Answer<\/h2>(.*?)<\/section>/s)[1];
  const copy = panel.match(/<p class="live-copy">(.*?)<\/p>/s);
  return { selected: askAnswer(state), rendered: copy ? copy[1] : "" };
}

function assertShows(state, text) {
  assert.deepEqual(shown(state), { selected: text, rendered: text });
}

test("re-check 3 (1): an old stream kept across a reconnect never hides newer history", () => {
  const state = askState();
  applyWorkspaceEvent(state, delta("old", "The capital of France"));
  // Disconnected: "old" completes and a tool-heavy later turn answers; the page omits "old".
  const history = [
    answer("old", "The capital of France is Paris."),
    ...Array.from({ length: 120 }, (_, index) => tool(`tool-${index}`)),
    answer("new", "The source says Madrid."),
  ];
  applyWorkspaceEvent(state, reconnect);
  // Until the reconnect's page returns, nothing local is shown.
  assertShows(state, "");
  page(state, history);
  assertShows(state, "The source says Madrid.");
  // A stream that starts after the reconnect's page is shown live.
  applyWorkspaceEvent(state, started("next"));
  applyWorkspaceEvent(state, delta("next", "Lisbon"));
  assertShows(state, "Lisbon");
});

test("re-check 3 (2): history proving a newer answer beats the local event order", () => {
  const state = askState();
  const requestSeq = request(state);
  applyWorkspaceEvent(state, completed("A", "Paris."));
  // B completes while disconnected; the outstanding page returns A then B.
  respond(state, [answer("A", "Paris."), answer("B", "Madrid.")], requestSeq);
  assertShows(state, "Madrid.");
});

test("commentary then a final answer whose deltas were all missed", () => {
  const state = askState();
  applyWorkspaceEvent(state, started("comment"));
  applyWorkspaceEvent(state, delta("comment", "Let me check the source."));
  applyWorkspaceEvent(state, completed("comment", "Let me check the source."));
  applyWorkspaceEvent(state, reconnect);
  page(state, [answer("comment", "Let me check the source.")]);
  applyWorkspaceEvent(state, completed("final", "Paris."));
  assertShows(state, "Paris.");
  page(state, [answer("comment", "Let me check the source."), answer("final", "Paris.")]);
  assertShows(state, "Paris.");
});

test("a later turn missed entirely while disconnected", () => {
  const state = askState();
  applyWorkspaceEvent(state, delta("first", "The capital of France"));
  applyWorkspaceEvent(state, reconnect);
  page(state, [
    answer("first", "The capital of France is Paris."),
    answer("second", "A leap year has 366 days."),
  ]);
  assertShows(state, "A leap year has 366 days.");
});

test("an old cached answer outside the 100-item page", () => {
  const state = askState();
  applyWorkspaceEvent(state, completed("old", "Paris."));
  applyWorkspaceEvent(state, reconnect);
  page(state, [
    answer("old", "Paris."),
    ...Array.from({ length: 99 }, (_, index) => tool(`tool-${index}`)),
    answer("new", "The source says Madrid."),
  ]);
  assertShows(state, "The source says Madrid.");
});

test("a newer completion that arrives while a page is in flight", () => {
  const state = askState();
  applyWorkspaceEvent(state, completed("first", "Paris."));
  const requestSeq = request(state);
  applyWorkspaceEvent(state, completed("second", "Madrid."));
  respond(state, [answer("first", "Paris.")], requestSeq);
  assertShows(state, "Madrid.");
  // The completion scheduled a refresh; its page lists the answer itself.
  page(state, [answer("first", "Paris."), answer("second", "Madrid.")]);
  assertShows(state, "Madrid.");
});

test("a page omitting an older local answer still decides", () => {
  // No reconnect: the local answer predates the page request, and the page (bounded) omits it.
  const state = askState();
  applyWorkspaceEvent(state, completed("old", "Paris."));
  page(state, [
    ...Array.from({ length: 99 }, (_, index) => tool(`tool-${index}`)),
    answer("new", "The source says Madrid."),
  ]);
  assertShows(state, "The source says Madrid.");
});

test("a completion always schedules a history refresh", () => {
  const state = askState();
  assert.equal(applyWorkspaceEvent(state, completed("a", "Paris.")).refresh, true);
});

// Deterministic pseudo-random numbers, so a failing interleaving can be replayed by seed.
function random(seed) {
  let value = seed >>> 0;
  return () => {
    value = (Math.imul(value ^ (value >>> 15), 0x2c1b3c6d) + 0x9e3779b9) >>> 0;
    return value / 2 ** 32;
  };
}

test("any interleaving converges to the newest completed answer in history", () => {
  for (let seed = 1; seed <= 400; seed += 1) {
    const next = random(seed);
    const state = askState();
    const history = [];
    let connected = true;
    let streaming = null;
    let counter = 0;
    let inFlight = null;
    const deliver = (message) => {
      if (!connected) return;
      const effect = applyWorkspaceEvent(state, message);
      if (effect.refresh && inFlight === null) inFlight = request(state);
    };
    for (let step = 0; step < 60; step += 1) {
      const roll = next();
      if (roll < 0.15 && streaming === null) {
        streaming = { id: `answer-${(counter += 1)}`, text: "" };
        deliver(started(streaming.id));
      } else if (roll < 0.35 && streaming) {
        const piece = `w${counter}-${step} `;
        streaming.text += piece;
        deliver(delta(streaming.id, piece));
      } else if (roll < 0.5 && streaming) {
        const text = `${streaming.text}done ${streaming.id}`;
        history.push(answer(streaming.id, text));
        deliver(completed(streaming.id, text));
        streaming = null;
      } else if (roll < 0.62) {
        const burst = Math.floor(next() * 60);
        for (let index = 0; index < burst; index += 1) history.push(tool(`tool-${step}-${index}`));
      } else if (roll < 0.72) {
        connected = false;
      } else if (roll < 0.82 && !connected) {
        connected = true;
        deliver(reconnect);
      } else if (roll < 0.92 && inFlight !== null) {
        // The page is read when the server processes it: it holds the history as of now.
        respond(state, history, inFlight);
        inFlight = null;
      } else if (inFlight === null) {
        inFlight = request(state);
      }
    }
    // Finish: the stream completes, the client reconnects and the last page returns.
    if (streaming) {
      const text = `${streaming.text}done ${streaming.id}`;
      history.push(answer(streaming.id, text));
      deliver(completed(streaming.id, text));
    }
    if (!connected) {
      connected = true;
      deliver(reconnect);
    }
    if (inFlight !== null) respond(state, history, inFlight);
    page(state, history);
    const newest = history.filter((entry) => entry.item.type === "agentMessage").at(-1);
    if (!history.slice(-PAGE_LIMIT).some((entry) => entry.item.type === "agentMessage")) continue;
    assert.deepEqual(shown(state), { selected: newest.item.text, rendered: newest.item.text }, `seed ${seed}`);
  }
});
